use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use warp::ws::{Message, WebSocket};
use warp::{Filter, Rejection, Reply};
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlayerState {
    player_id: usize,
    number: Option<u8>,
    revealed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GameState {
    players: Vec<PlayerState>,
    all_revealed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    ChooseNumber { player_id: usize, number: u8 },
    RevealNumbers { value: bool },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    UpdateState(GameState),
    PlayerAssigned { player_id: usize },
    ErrorMessage(String),
}

type SharedGameState = Arc<Mutex<GameState>>;

#[tokio::main]
async fn main() {
    let game_state = SharedGameState::new(Mutex::new(GameState {
        players: Vec::new(),
        all_revealed: false,
    }));

    let (tx, _rx) = broadcast::channel(32);
    let game_state_filter = warp::any().map(move || game_state.clone());
    let tx_filter = warp::any().map(move || tx.clone());

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(game_state_filter.clone())
        .and(tx_filter.clone())
        .and_then(handle_ws_connection);

    // let static_route = warp::path::end()
    //     .and(warp::fs::file("./src/static/index.html"));

    let static_route = warp::fs::dir("./src/static");

    let routes = ws_route
        .or(static_route)
        .with(warp::cors().allow_any_origin());

    println!("Server running on localhost:3000/");
    warp::serve(routes).run(([127, 0, 0, 1], 3000)).await;
}

async fn handle_ws_connection(
    ws: warp::ws::Ws,
    game_state: SharedGameState,
    tx: broadcast::Sender<GameState>,
) -> Result<impl Reply, Rejection> {
    Ok(ws.on_upgrade(move |socket| client_connected(socket, game_state, tx)))
}

async fn client_connected(
    websocket: WebSocket,
    game_state: SharedGameState,
    tx: broadcast::Sender<GameState>,
) {
    let (mut ws_tx, mut ws_rx) = websocket.split();
    let mut rx = tx.subscribe();

    // Assign a new player ID and add the player to the game state
    let player_id = {
        let mut state = game_state.lock().unwrap();
        let new_id = state.players.len();
        state.players.push(PlayerState {
            player_id: new_id,
            number: None,
            revealed: false,
        });
        new_id
    };

    // Notify the client of their assigned player ID
    let msg = serde_json::to_string(&ServerMessage::PlayerAssigned { player_id }).unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;

    let game_state_clone = game_state.clone();
    tokio::spawn(async move {
        while let Ok(game_state) = rx.recv().await {
            let serialized = serde_json::to_string(&ServerMessage::UpdateState(game_state)).unwrap();
            if ws_tx.send(Message::text(serialized)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        if let Ok(text) = msg.to_str() {
            if let Ok(client_message) = serde_json::from_str::<ClientMessage>(text) {
                handle_client_message(client_message, &game_state_clone, &tx).await;
            }
        }
    }
}

async fn handle_client_message(
    message: ClientMessage,
    game_state: &SharedGameState,
    tx: &broadcast::Sender<GameState>,
) {
    let mut state = game_state.lock().unwrap();

    match message {
        ClientMessage::ChooseNumber { player_id, number } => {
            if let Some(player) = state.players.iter_mut().find(|p| p.player_id == player_id) {
                if !player.revealed {
                    player.number = Some(number);
                }
            }
        }
        ClientMessage::RevealNumbers { value } => {
            state.all_revealed = value;
            
            for player in &mut state.players {
                if player.number.is_some() {
                    player.revealed = value;
                }
            }
        }
    }

    let _ = tx.send(state.clone());
}
