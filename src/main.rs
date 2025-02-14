use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use futures::{FutureExt, SinkExt, StreamExt};
use log::{debug, error, info, Level};
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, Instant};
use warp::ws::{Message, WebSocket};
use warp::{Filter, Rejection, Reply};

#[derive(Clone)]
struct RoomUpdate
{
  room: String,
  state: GameState,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlayerState
{
  player_id: usize,
  player_name: String,
  value: Option<u8>,
  // revealed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct GameState
{
  players: Vec<PlayerState>,
  all_revealed: bool,
  notify_change: NotifyChange,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
struct NotifyChange
{
  current_id: usize,
  new_id: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage
{
  ChangeValue
  {
    player_id: usize, value: u8
  },
  ChangeName
  {
    player_id: usize, name: String
  },
  RevealNumbers
  {
    value: bool
  },
  Pong
  {
    player_id: usize
  },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage
{
  UpdateState(GameState),
  PlayerAssigned
  {
    player_id: usize,
  },
  ErrorMessage(String),
  Ping
  {
    data: usize,
  },
}

// TODO: Make everythign OOPy
type SharedGameState = Arc<Mutex<HashMap<String, GameState>>>;

static PORT: u16 = 3000;
static BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

#[tokio::main]
async fn main()
{
  env_logger::init();

  let game_state = Arc::new(Mutex::new(HashMap::new()));
  
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(32);
  let game_state_filter = warp::any().map(move || game_state.clone());
  let tx_filter = warp::any().map(move || tx.clone());

  // if the user goes to the root, generate a room name and redirect them to
  // index.html with the parameter of the room name
  let index_route = warp::path::end().map(move || {
    let room_name = generate_room_name();
    warp::redirect(
      warp::http::Uri::from_maybe_shared(format!(
        "/index.html?room={}",
        room_name
      ))
      .unwrap(),
    )
  });
  // the client is going to send a paremeter after the /ws/ route. That
  // parameter is the room name. We need to filter out the room nae and group
  // all connections with the same room name together.
  let ws_route = warp::path("ws")
    .and(warp::path::param::<String>())
    .and(warp::ws())
    .and(game_state_filter.clone())
    .and(tx_filter.clone())
    .and_then(handle_ws_connection);

  let img_route = warp::path("img").and(
    warp::path("portraits.png")
      .and(warp::fs::file("./client/img/portraits.png"))
      .or(
        warp::path("atlas.png").and(warp::fs::file("./client/img/atlas.png")),
      ),
  );

  let client_code =
    warp::path("game.js").and(warp::fs::file("./client/game.js"));

  let client_style =
    warp::path("style.css").and(warp::fs::file("./client/style.css"));

  let client_html =
    warp::path("index.html").and(warp::fs::file("./client/index.html"));

  let routes = index_route
    .or(ws_route)
    .or(img_route)
    .or(client_code)
    .or(client_style)
    .or(client_html)
    .with(warp::cors().allow_any_origin());

  info!("Model UN Server Running.");
  // info!("Bind Address: {:?}:{:#?}", BIND_ADDRESS, PORT);
  warp::serve(routes).run((BIND_ADDRESS, PORT)).await;
}

async fn handle_ws_connection(
  room: String,
  ws: warp::ws::Ws,
  game_state: SharedGameState,
  tx: broadcast::Sender<RoomUpdate>,
) -> Result<impl Reply, Rejection>
{
  debug!("Room: {:?}", room);
  Ok(
    ws.on_upgrade(move |socket| client_connected(socket, room, game_state, tx)),
  )
}

fn generate_room_name() -> String
{
  let adjectives = vec![
    "Swift", "Mighty", "Clever", "Silent", "Fierce", "Gentle", "Wild", "Brave",
    "Wise", "Nimble", "Proud", "Noble", "Sleepy", "Cunning", "Playful",
  ];

  let animals = vec![
    "Fox", "Bear", "Wolf", "Eagle", "Owl", "Lion", "Tiger", "Dolphin",
    "Elephant", "Panther", "Hawk", "Deer", "Rabbit", "Raccoon", "Penguin",
  ];

  let mut rng = rand::thread_rng();
  let adj = adjectives[rng.gen_range(0..adjectives.len())];
  let animal = animals[rng.gen_range(0..animals.len())];

  format!("{}{}", adj, animal)
}

fn calculate_player_id(state: &GameState) -> usize
{
  // if the players array is 6, then the player id should be increment from 10
  if state.players.len() >= 6
  {
    return 10 + state.players.len();
  }

  // Otherwise, find the lowest unused player ID
  for i in 0..state.players.len()
  {
    if state.players.iter().all(|p| p.player_id != i)
    {
      return i;
    }
  }
  state.players.len()
}
async fn client_connected(
  websocket: WebSocket,
  room: String,
  game_state: SharedGameState,
  tx: broadcast::Sender<RoomUpdate>,
)
{
  let room_clone = room.clone();
  let (mut ws_tx, mut ws_rx) = websocket.split();
  let mut rx = tx.subscribe();

  // Assign a new player ID and add the player to the game state
  let outgoing_id = {
    let mut state = game_state.lock().unwrap();
    let room_state = state.entry(room.clone()).or_insert(GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    });

    let new_id = calculate_player_id(&room_state);
    room_state.players.push(PlayerState {
      player_id: new_id,
      player_name: "Delegate Unknown".to_string(),
      value: None,
    });

    info!("New Player ID: {}", new_id);
    new_id
  };

  // Send initial player assignment
  let msg = serde_json::to_string(&ServerMessage::PlayerAssigned {
    player_id: outgoing_id,
  })
  .unwrap();
  let _ = ws_tx.send(Message::text(msg)).await;

  // Send initial state
  // let room_number = room.clone();
  if let Ok(state) = game_state.lock()
  {
    if let Some(room_state) = state.get(&room)
    {
      let _ = tx.send(RoomUpdate {
        room: room.clone(),
        state: room_state.clone(),
      });
    }
  }

  let game_state_clone = game_state.clone();

  let room_for_loop = room.clone();
  let ws_task = tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop
    {
      tokio::select! {
          _ = interval.tick().fuse() => {
              let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 })
                .unwrap();
              if ws_tx.send(Message::text(ping_message)).await.is_err() {
                  break;
              }
          },
          Ok(room_update) = rx.recv().fuse() => {
              // Only forward updates for this client's room

              if room_update.room == room_for_loop {
                  let serialized = serde_json::to_string(&ServerMessage::UpdateState(room_update.state))
                    .unwrap();
                  debug!("State Change for room {}: {:#?}", room_for_loop, &serialized);
                  if ws_tx.send(Message::text(serialized)).await.is_err() {
                      break;
                  }
              }
          },
          else => break,
      }
    }
  });

  let rx_task = tokio::spawn(async move {
    // Listen for incoming WebSocket messages
    while let Some(Ok(msg)) = ws_rx.next().await
    {
      if let Ok(text) = msg.to_str()
      {
        if let Ok(client_message) = serde_json::from_str::<ClientMessage>(text)
        {
          handle_client_message(
            &room_clone,
            client_message,
            &game_state_clone,
            &tx,
          )
          .await;
        }
      }
    }
  });

  // Wait for either task to finish, ignoring shutdown ordering
  let _ = tokio::join!(ws_task, rx_task);

  // Clean up after client disconnection
  let mut state = game_state.lock().unwrap();
  if let Some(room_state) = state.get_mut(&room)
  {
    if let Some(index) =
      room_state.players.iter().position(|p| p.player_id == outgoing_id)
    {
      room_state.players.remove(index);
      info!("Player {} disconnected.", outgoing_id);

      let player_in_waiting = find_player_in_waiting(room_state);

      match player_in_waiting
      {
        Some(player_id) =>
        {
          // Find and clone the promoting player's data
          if let Some(promoting_player) = room_state
            .players
            .iter()
            .find(|p| p.player_id == player_id)
            .cloned()
          {
            // Remove the promoting player
            if let Some(promoted_index) =
              room_state.players.iter().position(|p| p.player_id == player_id)
            {
              room_state.players.remove(promoted_index);

              // Add the promoted player with the new ID
              room_state.players.push(PlayerState {
                player_id: outgoing_id,
                player_name: promoting_player.player_name,
                value: None,
              });

              room_state.notify_change = NotifyChange {
                current_id: player_id,
                new_id: outgoing_id,
              };

              debug!(
                "Player {} promoted to position {}",
                player_id, outgoing_id
              );
            }
          }
        },
        None =>
        {
          room_state.notify_change = NotifyChange {
            current_id: 0,
            new_id: 0,
          };
        },
      }
    }
  }

  // Clean up empty rooms
  if let Some(room_state) = state.get(&room)
  {
    if room_state.players.is_empty()
    {
      state.remove(&room);
      info!("Removed empty room: {}", room);
    }
  }
}

fn find_player_in_waiting(state: &mut GameState) -> Option<usize>
{
  if state.players.len() >= 6
  {
    for player in &state.players
    {
      if player.player_id > 10
      {
        return Some(player.player_id);
      }
    }
  }
  None
}

async fn handle_client_message(
  room: &str,
  message: ClientMessage,
  game_state: &SharedGameState,
  tx: &broadcast::Sender<RoomUpdate>,
)
{
  // println!("Client message: {:?}", message);
  debug!("Client message: {:?}", message);
  let mut state = game_state.lock().unwrap();

  let room_state = state.entry(room.to_string()).or_insert(GameState {
    players: Vec::new(),
    all_revealed: false,
    notify_change: NotifyChange::default(),
  });

  match message
  {
    ClientMessage::Pong {
      player_id,
    } =>
    {
      if let Some(player) =
        room_state.players.iter_mut().find(|p| p.player_id == player_id)
      {
        // player.missed_checkins = 0;
      }
    },
    ClientMessage::ChangeValue {
      player_id,
      value,
    } =>
    {
      if let Some(player) =
        room_state.players.iter_mut().find(|p| p.player_id == player_id)
      {
        player.value = Some(value);
      }
    },
    ClientMessage::ChangeName {
      player_id,
      name,
    } =>
    {
      if let Some(player) =
        room_state.players.iter_mut().find(|p| p.player_id == player_id)
      {
        player.player_name = name;
      }
    },
    ClientMessage::RevealNumbers {
      value,
    } =>
    {
      // Only zero out the values if the user wants to reset and the previous
      // state was reviealed.
      if value == false && room_state.all_revealed == true
      {
        for player in &mut room_state.players
        {
          if player.value.is_some()
          {
            player.value = Some(0);
          }
        }
      }
      // Update the state
      room_state.all_revealed = value;
    },
  }
  // Broadcast the update with room information
  let _ = tx.send(RoomUpdate {
    room: room.to_string(),
    state: room_state.clone(),
  });
}
