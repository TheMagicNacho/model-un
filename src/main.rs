use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio::time::Duration;
use uuid::Uuid;
use warp::ws::{Message, WebSocket};
use warp::{http::Uri, Filter, Rejection, Reply};
use futures::{SinkExt, StreamExt};

// Constants
const ROOM_EXPIRATION_HOURS: u64 = 2;
const PING_INTERVAL_SECS: u64 = 5;

// Type aliases for clarity
type RoomId = String;
type SharedGameState = Arc<Mutex<GameState>>;
type Rooms = Arc<Mutex<HashMap<RoomId, RoomData>>>;

// Structs for game state
#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlayerState {
    player_id: usize,
    player_name: String,
    value: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GameState {
    players: Vec<PlayerState>,
    all_revealed: bool,
    notify_change: NotifyChange,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
struct NotifyChange {
    current_id: usize,
    new_id: usize,
}

struct RoomData {
    game_state: SharedGameState,
    tx: broadcast::Sender<GameState>,
    created_at: SystemTime,
}

// Message enums for client-server communication
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    ChangeValue { player_id: usize, value: u8 },
    ChangeName { player_id: usize, name: String },
    RevealNumbers { value: bool },
    Pong { player_id: usize },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    UpdateState(GameState),
    PlayerAssigned { player_id: usize },
    ErrorMessage(String),
    Ping { data: usize },
}

#[tokio::main]
async fn main() {
    let rooms: Rooms = Arc::new(Mutex::new(HashMap::new()));
    let rooms_filter = warp::any().map(move || rooms.clone());

    // Route handlers
    let create_room = warp::path::end()
        .and(warp::get())
        .and(rooms_filter.clone())
        .and_then(handle_create_room);

    let join_room = warp::path!(String)
        .and(warp::get())
        .and(rooms_filter.clone())
        .and_then(handle_join_room);

    let ws_route = warp::path!("ws" / String)
        .and(warp::ws())
        .and(rooms_filter.clone())
        .and_then(handle_ws_connection);

    let static_route = warp::path("static").and(warp::fs::dir("./src/static"));

    let routes = create_room
        .or(join_room)
        .or(ws_route)
        .or(static_route)
        .with(warp::cors().allow_any_origin());

    // Start room cleanup task
    let cleanup_rooms = rooms.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            cleanup_expired_rooms(&cleanup_rooms).await;
        }
    });

    println!("Server running on localhost:3000/");
    warp::serve(routes).run(([127, 0, 0, 1], 3000)).await;
}

async fn handle_create_room(rooms: Rooms) -> Result<impl Reply, Rejection> {
    let room_id = Uuid::new_v4().to_string()[..8].to_string();

    let game_state = SharedGameState::new(Mutex::new(GameState {
        players: Vec::new(),
        all_revealed: false,
        notify_change: NotifyChange::default(),
    }));

    let (tx, _) = broadcast::channel(32);

    let room_data = RoomData {
        game_state,
        tx,
        created_at: SystemTime::now(),
    };

    rooms.lock().unwrap().insert(room_id.clone(), room_data);

    Ok(warp::redirect::redirect(
        Uri::builder()
            .path_and_query(format!("/{}", room_id))
            .build()
            .unwrap(),
    ))
}

async fn handle_join_room(room_id: String, rooms: Rooms) -> Result<impl Reply, Rejection> {
    if rooms.lock().unwrap().contains_key(&room_id) {
        // Serve the static file directly
        Ok(warp::fs::file("./src/static/index.html"))
    } else {
        // Return a 404 status
        Ok(warp::reply::with_status(
            "Room not found",
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

async fn handle_ws_connection(
    room_id: String,
    ws: warp::ws::Ws,
    rooms: Rooms,
) -> Result<impl Reply, Rejection> {
    let rooms_lock = rooms.lock().unwrap();

    if let Some(room_data) = rooms_lock.get(&room_id) {
        let game_state = room_data.game_state.clone();
        let tx = room_data.tx.clone();
        Ok(ws.on_upgrade(move |socket| client_connected(socket, game_state, tx)))
    } else {
        Err(warp::reject::not_found())
    }
}

fn calculate_player_id(state: &GameState) -> usize {
    if state.players.len() >= 6 {
        return 10 + state.players.len();
    }

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

    let outgoing_id = {
        let mut state = game_state.lock().unwrap();
        let new_id = calculate_player_id(&state);
        state.players.push(PlayerState {
            player_id: new_id,
            player_name: "Connecting...".to_string(),
            value: None,
        });
        println!("New client connected: {:?}", new_id);
        new_id
    };

    let msg = serde_json::to_string(&ServerMessage::PlayerAssigned { player_id: outgoing_id }).unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;
    let _ = tx.send(game_state.lock().unwrap().clone());

    let ws_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 }).unwrap();
                    if ws_tx.send(Message::text(ping_message)).await.is_err() {
                        break;
                    }
                },
                Ok(game_state) = rx.recv() => {
                    let serialized = serde_json::to_string(&ServerMessage::UpdateState(game_state)).unwrap();
                    if ws_tx.send(Message::text(serialized)).await.is_err() {
                        break;
                    }
                },
                else => break,
            }
        }
    });

    let game_state_clone = game_state.clone();
    let rx_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let Ok(text) = msg.to_str() {
                if let Ok(client_message) = serde_json::from_str::<ClientMessage>(text) {
                    handle_client_message(client_message, &game_state_clone, &tx).await;
                }
            }
        }
    });

    let _ = tokio::join!(ws_task, rx_task);

    // Handle disconnection
    let mut state = game_state.lock().unwrap();
    if let Some(index) = state.players.iter().position(|p| p.player_id == outgoing_id) {
        state.players.remove(index);
        println!("Player {} disconnected and removed.", outgoing_id);

        if let Some(waiting_player_id) = find_player_in_waiting(&state) {
            let promoting_player = state.players
                .iter()
                .find(|p| p.player_id == waiting_player_id)
                .unwrap()
                .clone();

            let promoted_player_index = state.players
                .iter()
                .position(|p| p.player_id == waiting_player_id)
                .unwrap();

            state.players.remove(promoted_player_index);
            state.players.push(PlayerState {
                player_id: outgoing_id,
                player_name: promoting_player.player_name,
                value: None,
            });

            state.notify_change = NotifyChange {
                current_id: waiting_player_id,
                new_id: outgoing_id,
            };
        } else {
            state.notify_change = NotifyChange::default();
        }
    }
}

fn find_player_in_waiting(state: &GameState) -> Option<usize> {
    if state.players.len() >= 6 {
        state.players.iter()
            .find(|p| p.player_id > 10)
            .map(|p| p.player_id)
    } else {
        None
    }
}

async fn handle_client_message(
    message: ClientMessage,
    game_state: &SharedGameState,
    tx: &broadcast::Sender<GameState>,
) {
    println!("Client message: {:?}", message);
    let mut state = game_state.lock().unwrap();

    match message {
        ClientMessage::Pong { player_id: _ } => (),
        ClientMessage::ChangeValue { player_id, value } => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                player.value = Some(value);
            }
        }
        ClientMessage::ChangeName { player_id, name } => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                player.player_name = name;
            }
        }
        ClientMessage::RevealNumbers { value } => {
            if !value && state.all_revealed {
                for player in &mut state.players {
                    if player.value.is_some() {
                        player.value = Some(0);
                    }
                }
            }
            state.all_revealed = value;
        }
    }

    let _ = tx.send(state.clone());
}

async fn cleanup_expired_rooms(rooms: &Rooms) {
    let mut rooms = rooms.lock().unwrap();
    let now = SystemTime::now();

    rooms.retain(|_, room_data| {
        match now.duration_since(room_data.created_at) {
            Ok(duration) => duration.as_secs() < ROOM_EXPIRATION_HOURS * 3600,
            Err(_) => true,
        }
    });
}