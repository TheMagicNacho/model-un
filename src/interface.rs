use std::sync::Arc;
use std::time::Duration;

use futures::{FutureExt, SinkExt, StreamExt};
use log::{debug, error};
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc;
use warp::ws::{Message, WebSocket};
use warp::{Rejection, Reply};

use crate::connection_pool::ConnectionPool;
use crate::game::Game;
use crate::structs::{
  ClientMessage,
  ConnectionContext,
  RoomUpdate,
  ServerMessage,
};

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

    let connection_context = ConnectionContext {
      tx,
      rx,
      ws_tx,
      ws_rx,
    };

    GameWebSocket::connection_driver(
      connection_context,
      room.clone(),
      game_state,
      pool,
      sender,
      player_id,
    )
    .await;
  }

  async fn connection_driver(
    connection_context: ConnectionContext,
    room: String,
    game_state: &'static Game,
    pool: Arc<ConnectionPool>,
    sender: mpsc::Sender<Message>,
    player_id: usize,
  )
  {
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    let tx = connection_context.tx;
    let mut rx = connection_context.rx;
    let mut ws_tx = connection_context.ws_tx;
    let mut ws_rx = connection_context.ws_rx;

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
                      game_state.remove_player(&room, player_id).await;
                      break;
                  },
                  None => {
                      debug!("WebSocket connection closed for room {}", room);
                      game_state.remove_player(&room, player_id).await;
                      break;
                  }
              }
          },
          // Sending ping messages to the client
          _ = interval.tick().fuse() => {
              let ping_message = serde_json::to_string(&ServerMessage::Ping { data: 0 })
                  .unwrap();
              // TODO: Handle if a client goes stale and does not reply to a ping.
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
                              game_state.remove_player(&room, player_id).await;
                              break;
                          }
                      }
                  },
                  Err(_e) => {
                      debug!("Broadcast channel receive error for room {}. Likely disconnected.", room);
                      game_state.remove_player(&room, player_id).await;
                      break;
                  }
              }
          },
      }; // tokio::select!
    } // loop
    debug!("Connection driver finished for room: {}", room);

    pool.remove(&room, &sender).await;
    game_state.remove_player(&room, player_id).await;
  }
}
