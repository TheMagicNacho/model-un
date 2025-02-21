use std::time::Duration;

use futures::stream::{SplitSink, SplitStream};
use futures::{FutureExt, SinkExt, StreamExt};
use log::debug;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinHandle;
use warp::ws::{Message, WebSocket};
use warp::{Rejection, Reply};

use crate::game::Game;
use crate::structs::{ClientMessage, RoomUpdate, ServerMessage};

pub(crate) struct GameWebSocket;

impl GameWebSocket
{
  pub async fn handle_connection(
    room: String,
    ws: warp::ws::Ws,
    tx: Sender<RoomUpdate>,
  ) -> Result<impl Reply, Rejection>
  {
    debug!("Room: {:?}", room);
    Ok(ws.on_upgrade(move |socket| {
      let tx = tx.clone();
      async move {
        GameWebSocket::manage_client_connection(socket, room, tx).await;
      }
    }))
  }

  pub async fn manage_client_connection(
    websocket: WebSocket,
    room: String,
    tx: Sender<RoomUpdate>,
  )
  {
    let game_state = Game::instance();

    let (mut ws_tx, ws_rx) = websocket.split();
    let rx = tx.subscribe();

    let player_id = game_state.new_player(&room);
    debug!(
      "Room Player State: {:?}",
      game_state.get_room_state(&room).unwrap().players
    );

    // Send initial player assignment
    let msg = serde_json::to_string(&ServerMessage::PlayerAssigned {
      player_id,
    })
    .unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;

    // provide the client with the inital state
    let room_state = game_state.get_room_state(&room).unwrap();
    let msg =
      serde_json::to_string(&ServerMessage::UpdateState(room_state.clone()))
        .unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;

    // Drive the connection
    // Spawn incoming message task
    let rx_task = Self::rx_driver(room.clone(), tx, game_state, ws_rx);

    // Spawn outgoing message task
    let ws_task = Self::tx_driver(room.clone(), ws_tx, rx);
    // Wait for either task to finish
    let _ = tokio::join!(rx_task, ws_task);

    game_state.remove_player(&room, player_id);
  }

  fn rx_driver(
    room: String,
    tx: Sender<RoomUpdate>,
    game_state: &'static Game,
    mut ws_rx: SplitStream<WebSocket>,
  ) -> JoinHandle<()>
  {
    tokio::spawn(async move {
      while let Some(Ok(msg)) = ws_rx.next().await
      {
        if let Ok(text) = msg.to_str()
        {
          if let Ok(client_message) =
            serde_json::from_str::<ClientMessage>(text)
          {
            debug!("Client Message: {:?}", client_message);
            game_state.process_client_message(&room, client_message).await;
            let room_state = game_state.get_room_state(&room).unwrap();
            let _ = tx.send(RoomUpdate {
              room: room.to_string(),
              state: room_state.clone(),
            });
          }
        }
      }
    })
  }

  fn tx_driver(
    room: String,
    mut ws_tx: SplitSink<WebSocket, Message>,
    mut rx: Receiver<RoomUpdate>,
  ) -> JoinHandle<()>
  {
    tokio::spawn(async move {
      let mut interval = tokio::time::interval(Duration::from_secs(5));
      loop
      {
        tokio::select! {
            // Pinging the client to keep the connection alive
            _ = interval.tick().fuse() => {
                let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 })
                  .unwrap();
                if ws_tx.send(Message::text(ping_message)).await.is_err() {
                    break;
                }
            },
            // Forwarding state updates to the client
            Ok(room_update) = rx.recv().fuse() => {
                // Only forward updates for this client's room
                if room_update.room == room {
                    let serialized = serde_json::to_string(&ServerMessage::UpdateState(room_update.state))
                      .unwrap();
                    debug!("State Change for room {}: {:#?}", room, &serialized);
                    if ws_tx.send(Message::text(serialized)).await.is_err() {
                        break;
                    }
                }
            },
            else => break,
        }
      }
    })
  }
}
