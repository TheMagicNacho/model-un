use std::sync::Arc;
use std::time::Duration;

use futures::stream::{SplitSink, SplitStream};
use futures::{FutureExt, SinkExt, StreamExt};
use log::{debug, error};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::mpsc;
use warp::ws::{Message, WebSocket};
use warp::{Rejection, Reply};

use crate::connection_pool::ConnectionPool;
use crate::game::Game;
use crate::structs::{ClientMessage, RoomUpdate, ServerMessage};

pub(crate) struct GameWebSocket;

impl GameWebSocket
{
  pub async fn handle_connection(
    room: String,
    ws: warp::ws::Ws,
    tx: Sender<RoomUpdate>,
    pool: Arc<ConnectionPool>,
  ) -> Result<impl Reply, Rejection>
  {
    debug!("Room: {:?}", room);
    Ok(ws.on_upgrade(move |socket| {
      let tx = tx.clone();
      async move {
        GameWebSocket::manage_client_connection(socket, room, tx, pool).await;
      }
    }))
  }

  pub async fn manage_client_connection(
    websocket: WebSocket,
    room: String,
    tx: Sender<RoomUpdate>,
    pool: Arc<ConnectionPool>,
  )
  {
    let game_state = Game::instance();

    let (mut ws_tx, ws_rx) = websocket.split();
    let (sender, _) = mpsc::channel::<Message>(32);
    let rx = tx.subscribe();

    pool.add(room.clone(), sender.clone()).await;

    let player_id = game_state.new_player(&room).await;
    debug!(
      "Room Player State: {:?}",
      game_state.get_room_state(&room).await.unwrap().players
    );

    // Send initial player assignment
    let msg = serde_json::to_string(&ServerMessage::PlayerAssigned {
      player_id,
    })
    .unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;

    // provide the client with the initial state
    let room_state =
      (game_state.get_room_state(&room).await).unwrap_or_default();
    let msg =
      serde_json::to_string(&ServerMessage::UpdateState(room_state.clone()))
        .unwrap();
    let _ = ws_tx.send(Message::text(msg)).await;

    // Drive the connection - Combined rx and tx logic in one task
    GameWebSocket::connection_driver(
      room.clone(),
      tx,
      game_state,
      ws_tx,
      ws_rx,
      rx,
    )
    .await;

    pool.remove(&room, &sender).await;
    game_state.remove_player(&room, player_id).await;
  }

  async fn connection_driver(
    room: String,
    tx: Sender<RoomUpdate>,
    game_state: &'static Game,
    mut ws_tx: SplitSink<WebSocket, Message>,
    mut ws_rx: SplitStream<WebSocket>,
    mut rx: Receiver<RoomUpdate>, // Added rx as argument
  )
  {
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop
    {
      tokio::select! {
          // Receiving messages from the client
          msg_result = ws_rx.next() => {
              match msg_result {
                  Some(Ok(msg)) => {
                      if let Ok(text) = msg.to_str() {
                          if let Ok(client_message) =
                              serde_json::from_str::<ClientMessage>(text)
                          {
                              debug!("Client Message: {:?}", client_message);
                              game_state.process_client_message(&room, client_message).await;
                              if let Some(room_state) = game_state.get_room_state(&room).await {
                                  let _ = tx.send(RoomUpdate {
                                      room: room.to_string(),
                                      state: room_state.clone(),
                                  });
                              }

                          }
                      }
                  },
                  // Close the connections on any errors.
                  Some(Err(e)) => {
                      error!("WebSocket receive error for room {}: {:?}", room, e);
                      break;
                  },
                  None => {
                      debug!("WebSocket connection closed for room {}", room);
                      break;
                  }
              }
          },
          // Sending ping messages to the client
          _ = interval.tick().fuse() => {
              let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 })
                  .unwrap();
              if let Err(e) = ws_tx.send(Message::text(ping_message)).await {
                  debug!("WebSocket send (ping) error for room {}: {:?}", room, e);
                  break;
              }
          },
          // Forwarding state updates to the client
          update_result = rx.recv().fuse() => {
              match update_result {
                  Ok(room_update) => {
                      if room_update.room == room {
                          let serialized = serde_json::to_string(&ServerMessage::UpdateState(room_update.state))
                              .unwrap();
                          debug!("State Change for room {}: {:#?}", room, &serialized);
                          if let Err(e) = ws_tx.send(Message::text(serialized)).await {
                              debug!("WebSocket send (state update) error for room {}: {:?}", room, e);
                              break;
                          }
                      }
                  },
                  Err(_e) => {
                      debug!("Broadcast channel receive error for room {}. Likely disconnected.", room);
                      break; // Exit loop if broadcast channel errors (likely closed)
                  }
              }
          },
      }; // tokio::select!
    } // loop
    debug!("Connection driver finished for room: {}", room);
  }
}

//
//
//
// pub(crate) struct GameWebSocket;
//
// impl GameWebSocket
// {
//   pub async fn handle_connection(
//     room: String,
//     ws: warp
// ::ws::Ws,
//     tx: Sender<RoomUpdate>,
//   ) -> Result<impl Reply, Rejection>
//   {
//     debug!("Room: {:?}", room);
//     Ok(ws.on_upgrade(move |socket| {
//       let tx = tx.clone();
//       async move {
//         GameWebSocket::manage_client_connection(socket, room, tx).await;
//       }
//     }))
//   }
//
//   pub async fn manage_client_connection(
//     websocket: WebSocket,
//     room: String,
//     tx: Sender<RoomUpdate>,
//   )
//   {
//     let game_state = Game::instance();
//
//     let (mut ws_tx, ws_rx) = websocket.split();
//     let rx = tx.subscribe();
//
//     let player_id = game_state.new_player(&room);
//     debug!(
//       "Room Player State: {:?}",
//       game_state.get_room_state(&room).unwrap().players
//     );
//
//     // Send initial player assignment
//     let msg = serde_json::to_string(&ServerMessage::PlayerAssigned {
//       player_id,
//     })
//     .unwrap();
//     let _ = ws_tx.send(Message::text(msg)).await;
//
//     // provide the client with the inital state
//     let room_state = game_state.get_room_state(&room).unwrap();
//     let msg =
//       serde_json::to_string(&ServerMessage::UpdateState(room_state.clone()))
//         .unwrap();
//     let _ = ws_tx.send(Message::text(msg)).await;
//
//     // Drive the connection
//     // Spawn incoming message task
//     let rx_task = Self::rx_driver(room.clone(), tx, game_state, ws_rx);
//
//     // Spawn outgoing message task
//     let ws_task = Self::tx_driver(room.clone(), ws_tx, rx);
//     // Wait for either task to finish
//     let _ = tokio::join!(rx_task, ws_task);
//
//     game_state.remove_player(&room, player_id);
//   }
//
//   fn rx_driver(
//     room: String,
//     tx: Sender<RoomUpdate>,
//     game_state: &'static Game,
//     mut ws_rx: SplitStream<WebSocket>,
//   ) -> JoinHandle<()>
//   {
//     tokio::spawn(async move {
//       while let Some(Ok(msg)) = ws_rx.next().await
//       {
//         if let Ok(text) = msg.to_str()
//         {
//           if let Ok(client_message) =
//             serde_json::from_str::<ClientMessage>(text)
//           {
//             debug!("Client Message: {:?}", client_message);
//             game_state.process_client_message(&room, client_message).await;
//             let room_state = game_state.get_room_state(&room).unwrap();
//             let _ = tx.send(RoomUpdate {
//               room: room.to_string(),
//               state: room_state.clone(),
//             });
//           }
//         }
//       }
//     })
//   }
//
//   fn tx_driver(
//     room: String,
//     mut ws_tx: SplitSink<WebSocket, Message>,
//     mut rx: Receiver<RoomUpdate>,
//   ) -> JoinHandle<()>
//   {
//     tokio::spawn(async move {
//       let mut interval = tokio::time::interval(Duration::from_secs(5));
//       loop
//       {
//         tokio::select! {
//             // Pinging the client to keep the connection alive
//             _ = interval.tick().fuse() => {
//                 let ping_message = serde_json::to_string(&ServerMessage::Ping
// { data: 0 })                   .unwrap();
//                 if ws_tx.send(Message::text(ping_message)).await.is_err() {
//                     break;
//                 }
//             },
//             // Forwarding state updates to the client
//             Ok(room_update) = rx.recv().fuse() => {
//                 // Only forward updates for this client's room
//                 if room_update.room == room {
//                     let serialized =
// serde_json::to_string(&ServerMessage::UpdateState(room_update.state))
//                       .unwrap();
//                     debug!("State Change for room {}: {:#?}", room,
// &serialized);                     if
// ws_tx.send(Message::text(serialized)).await.is_err() {
// break;                     }
//                 }
//             },
//             else => break,
//         }
//       }
//     })
//   }
// }
