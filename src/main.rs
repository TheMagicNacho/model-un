use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};
use warp::ws::{Message, WebSocket};
use warp::{Filter, Rejection, Reply};
use futures::{SinkExt, StreamExt};
use futures::stream::SplitSink;
use include_dir::{include_dir, Dir};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, Instant};


#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlayerState {
    player_id: usize,
    player_name: String,
    value: Option<u8>,
    // revealed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GameState {
    players: Vec<PlayerState>,
    all_revealed: bool,
    notify_change: NotifyChange,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[derive(PartialEq)]
struct NotifyChange {
    current_id: usize,
    new_id: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    ChangeValue { player_id: usize, value: u8 },
    ChangeName    { player_id: usize, name: String },
    RevealNumbers { value: bool },
    Pong { player_id: usize },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    UpdateState(GameState),
    PlayerAssigned { player_id: usize },
    ErrorMessage(String),
    Ping{data: usize},
}

type SharedGameState = Arc<Mutex<GameState>>;

#[tokio::main]
async fn main() {
    let game_state = SharedGameState::new(Mutex::new(GameState {
        players: Vec::new(),
        all_revealed: false,
        notify_change: NotifyChange::default(),
    }));

    let (tx, _rx) = broadcast::channel(32);
    let game_state_filter = warp::any().map(move || game_state.clone());
    let tx_filter = warp::any().map(move || tx.clone());

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(game_state_filter.clone())
        .and(tx_filter.clone())
        .and_then(handle_ws_connection);


    let static_route = warp::fs::dir("./src/client");
    
    let img_route = warp::path("img").and(
        warp::path("portraits.png")
            .and(warp::fs::file("./client/img/portraits.png"))
            .or(warp::path("atlas.png")
                .and(warp::fs::file("./client/img/atlas.png"))),
    );

    
    let client_code = warp::path("game.js")
        .and(warp::fs::file("./client/game.js"));
    
    let client_style = warp::path("style.css")
        .and(warp::fs::file("./client/style.css"));
    
    let client_html =  warp::path("index.html")
        .and(warp::fs::file("./client/index.html"));
    

    
    let routes = ws_route
        // .or(static_route)
        // .or(atlas_route)
        // .or(sprite_route)
        .or(img_route)
        .or(client_code)
        .or(client_style)
        .or(client_html)
        .with(warp::cors().allow_any_origin());

    println!("Server running on localhost:3000/");
    warp::serve(routes).run(([127, 0, 0, 1], 3000)).await;
    // warp::serve(routes).run(([0, 0, 0, 0], 3000)).await;
}

async fn handle_ws_connection(
    ws: warp::ws::Ws,
    game_state: SharedGameState,
    tx: broadcast::Sender<GameState>,
) -> Result<impl Reply, Rejection> {
    Ok(ws.on_upgrade(move |socket| client_connected(socket, game_state, tx)))
}

fn calculate_player_id(state: &GameState) -> usize {
    // if the players array is 6, then the player id should be increment from 10
    if state.players.len() >= 6 {
        return 10 + state.players.len();
    }

    // Otherwise, find the lowest unused player ID
    for i in 0..state.players.len() {
        if state.players.iter().all(|p| p.player_id != i) {
            return i;
        }
    }
    state.players.len()
}
async fn client_connected(
    websocket: WebSocket,
    game_state: SharedGameState,
    tx: broadcast::Sender<GameState>,
) {
    let (mut ws_tx, mut ws_rx) = websocket.split();
    let mut rx = tx.subscribe();

    // Assign a new player ID and add the player to the game state
    let outgoing_id = {
        let mut state = game_state.lock().unwrap();
        let new_id = calculate_player_id(&state);
        state.players.push(PlayerState {
            player_id: new_id,
            player_name: "Connecting...".to_string(),
            value: None,
            // revealed: false,
            // missed_checkins: 0,
        });
        println!("New client connected: {:?}", new_id);
        new_id
    };

    let msg = serde_json::to_string(&ServerMessage::PlayerAssigned { player_id: outgoing_id }).unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;
    let game_state_clone = game_state.clone();
    let _ = tx.send(game_state.lock().unwrap().clone());

    let ws_task = tokio::spawn(async move {
        // Regular WebSocket task
        let mut interval = tokio::time::interval(Duration::from_secs(5)); // Ping every second

        loop {
            tokio::select! {
                _ = interval.tick().fuse() => {
                    let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 }).unwrap();

                    if ws_tx.send(Message::text(ping_message)).await.is_err() {
                        break; // Exit loop if sending fails (client disconnected)
                    }
                },

                Ok(game_state) = rx.recv().fuse() => {
                    let serialized = serde_json::to_string(&ServerMessage::UpdateState(game_state)).unwrap();
                    if ws_tx.send(Message::text(serialized)).await.is_err() {
                        break;
                    }
                },
                else => break,
            }
        }
    });

    let rx_task = tokio::spawn(async move {
        // Listen for incoming WebSocket messages
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let Ok(text) = msg.to_str() {
                if let Ok(client_message) = serde_json::from_str::<ClientMessage>(text) {
                    handle_client_message(client_message, &game_state_clone, &tx).await;
                }
            }
        }
    });

    // Wait for either task to finish, ignoring shutdown ordering
    let _ = tokio::join!(ws_task, rx_task);

    // Clean up after client disconnection
    // missed_checkins_task.abort(); // Stop the missed check-ins task
    let mut state = game_state.lock().unwrap();

    if let Some(index) = state.players.iter().position(|p| p.player_id == outgoing_id) {
        state.players.remove(index);
        println!("Player {} disconnected and removed.", outgoing_id);
        let player_in_waiting = find_player_in_waiting(&mut state);


        match player_in_waiting {
            Some(player_id) => {
                // get promoting player
                let promoting_player = state.players.iter().find(|p| p.player_id == player_id).unwrap().clone();
                let promoted_player_index = state.players.iter().position(|p| p.player_id == player_id).unwrap();
                // delete record of promoting player
                state.players.remove(promoted_player_index);
                // promote the player
                state.players.push(PlayerState {
                    player_id: outgoing_id,
                    player_name: promoting_player.player_name.clone(),
                    value: None,
                });
                state.notify_change = NotifyChange {
                    current_id: player_id,
                    new_id: outgoing_id,
                };
                println!("Notify Change: {:?}", state);
            }
            None => {
                state.notify_change = NotifyChange {
                    current_id: 0,
                    new_id: 0,
                };
            }
        }
    }
}

fn find_player_in_waiting(state: &mut MutexGuard<GameState>) -> Option<usize> {
    if state.players.len() >= 6 {
        for player in &state.players {
            if player.player_id > 10 {
                return Some(player.player_id);
            }
        }
    }
    None
}

async fn handle_client_message(
    message: ClientMessage,
    game_state: &SharedGameState,
    tx: &broadcast::Sender<GameState>,
) {
    println!("Client message: {:?}", message);
    let mut state = game_state.lock().unwrap();

    match message {
        ClientMessage::Pong { player_id } => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                // player.missed_checkins = 0;
            }
        }
        ClientMessage::ChangeValue { player_id, value } => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                player.value = Some(value);
            }
        }
        ClientMessage::ChangeName {player_id, name} => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                player.player_name = name;
            }
        }
        ClientMessage::RevealNumbers { value } => {
            // Only zero out the values if the user wants to reset and the previous state was reviealed.
            if value == false && state.all_revealed == true {
                for player in &mut state.players {
                    if player.value.is_some() {
                        player.value = Some(0);
                    }
                }
            }
            // Update the stateddddddd
            state.all_revealed = value;
        }
    }

    let _ = tx.send(state.clone());
}
