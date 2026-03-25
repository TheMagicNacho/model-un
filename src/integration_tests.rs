//! Integration tests for the WebSocket API.
//!
//! Each test spins up an in-process warp filter (no real TCP socket) using
//! `warp::test::ws()` and exercises the full message flow that a real browser
//! client would experience.
//!
//! Because `Game::instance()` is a process-wide singleton, every test uses a
//! unique room name so that parallel test runs do not share game state.

use tokio::sync::broadcast;
use warp::Filter;

use crate::connection_pool::ConnectionPool;
use crate::interface::GameWebSocket;
use crate::structs::{ClientMessage, GameState, RoomUpdate, ServerMessage};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Builds a warp filter for the `/ws/<room>` route that mirrors the setup in
/// `main()`.  Each call creates an independent broadcast channel and connection
/// pool so tests remain isolated from each other.
fn build_ws_filter(
  tx: broadcast::Sender<RoomUpdate>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
{
  let tx_filter = warp::any().map(move || tx.clone());
  let pool_filter = warp::any().map(ConnectionPool::new);
  warp::path("ws")
    .and(warp::path::param::<String>())
    .and(warp::ws())
    .and(tx_filter)
    .and(pool_filter)
    .and_then(GameWebSocket::handle_connection)
}

/// Reads the next message from the client, discarding any `Ping` frames that
/// arrive due to Tokio's interval firing its first tick immediately.
async fn recv_next_non_ping(
  client: &mut warp::test::WsClient,
) -> ServerMessage
{
  loop
  {
    let msg = client.recv().await.expect("Should receive a WebSocket message");
    let server_msg: ServerMessage =
      serde_json::from_str(msg.to_str().expect("Message should be text"))
        .expect("Should deserialize to ServerMessage");
    match server_msg
    {
      ServerMessage::Ping { .. } => continue,
      other => return other,
    }
  }
}

// ── Connection handshake ──────────────────────────────────────────────────────

/// After the WebSocket handshake the server immediately sends a
/// `PlayerAssigned` message containing the new player's ID.
#[tokio::test]
async fn test_connection_receives_player_assigned()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-player-assigned")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let server_msg = recv_next_non_ping(&mut client).await;
  assert!(
    matches!(server_msg, ServerMessage::PlayerAssigned { .. }),
    "First non-ping message must be PlayerAssigned, got: {server_msg:?}"
  );
}

/// The second message sent on connect is always an `UpdateState` carrying the
/// current room snapshot.
#[tokio::test]
async fn test_connection_receives_initial_state()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-initial-state")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  // Skip PlayerAssigned
  loop
  {
    let msg = recv_next_non_ping(&mut client).await;
    if matches!(msg, ServerMessage::PlayerAssigned { .. })
    {
      break;
    }
  }

  let server_msg = recv_next_non_ping(&mut client).await;
  assert!(
    matches!(server_msg, ServerMessage::UpdateState(_)),
    "Message after PlayerAssigned must be UpdateState, got: {server_msg:?}"
  );
}

/// The initial `UpdateState` includes the connecting player in the players list.
#[tokio::test]
async fn test_initial_state_contains_connecting_player()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-initial-player-list")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  // Capture the assigned player_id
  let player_id = loop
  {
    match recv_next_non_ping(&mut client).await
    {
      ServerMessage::PlayerAssigned { player_id } => break player_id,
      _ => {},
    }
  };

  let state = loop
  {
    match recv_next_non_ping(&mut client).await
    {
      ServerMessage::UpdateState(s) => break s,
      _ => {},
    }
  };

  assert!(
    state.players.iter().any(|p| p.player_id == player_id),
    "Initial state must include the connecting player (id={player_id})"
  );
}

// ── Client messages ───────────────────────────────────────────────────────────

/// Reads messages, skipping Pings, until an `UpdateState` is found.
async fn recv_update_state(client: &mut warp::test::WsClient) -> GameState
{
  loop
  {
    match recv_next_non_ping(client).await
    {
      ServerMessage::UpdateState(s) => return s,
      _ => {},
    }
  }
}

/// Reads messages until `PlayerAssigned` is found and returns the player_id.
async fn recv_player_assigned(client: &mut warp::test::WsClient) -> usize
{
  loop
  {
    if let ServerMessage::PlayerAssigned { player_id } =
      recv_next_non_ping(client).await
    {
      return player_id;
    }
  }
}

/// Sending `ChangeName` causes the server to broadcast an `UpdateState` where
/// the player's name reflects the requested change.
#[tokio::test]
async fn test_change_name_updates_state()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-change-name")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let player_id = recv_player_assigned(&mut client).await;
  let _ = recv_update_state(&mut client).await; // discard initial UpdateState

  client
    .send_text(
      serde_json::to_string(&ClientMessage::ChangeName {
        player_id,
        name: "Test Delegate".to_string(),
      })
      .unwrap(),
    )
    .await;

  let state = recv_update_state(&mut client).await;
  let player = state
    .players
    .iter()
    .find(|p| p.player_id == player_id)
    .expect("Player must be in state");
  assert_eq!(player.player_name, "Test Delegate");
}

/// Sending `ChangeValue` causes the server to broadcast an `UpdateState` where
/// the player's vote value reflects the requested change.
#[tokio::test]
async fn test_change_value_updates_state()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-change-value")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let player_id = recv_player_assigned(&mut client).await;
  let _ = recv_update_state(&mut client).await; // discard initial UpdateState

  client
    .send_text(
      serde_json::to_string(&ClientMessage::ChangeValue {
        player_id,
        value: 5,
      })
      .unwrap(),
    )
    .await;

  let state = recv_update_state(&mut client).await;
  let player = state
    .players
    .iter()
    .find(|p| p.player_id == player_id)
    .expect("Player must be in state");
  assert_eq!(player.value, Some(5));
}

/// Sending `RevealNumbers { true }` sets `all_revealed` to true in the
/// broadcast state.
#[tokio::test]
async fn test_reveal_numbers_sets_all_revealed_flag()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-reveal")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let _ = recv_player_assigned(&mut client).await;
  let _ = recv_update_state(&mut client).await;

  client
    .send_text(
      serde_json::to_string(&ClientMessage::RevealNumbers { value: true })
        .unwrap(),
    )
    .await;

  let state = recv_update_state(&mut client).await;
  assert!(state.all_revealed, "all_revealed must be true after reveal");
}

/// Sending `RevealNumbers { false }` after a reveal resets every player's value
/// to `Some(0)` and clears `all_revealed`.
#[tokio::test]
async fn test_hide_numbers_resets_values()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-hide")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let player_id = recv_player_assigned(&mut client).await;
  let _ = recv_update_state(&mut client).await; // initial UpdateState

  // Set a value
  client
    .send_text(
      serde_json::to_string(&ClientMessage::ChangeValue {
        player_id,
        value: 8,
      })
      .unwrap(),
    )
    .await;
  let _ = recv_update_state(&mut client).await; // UpdateState with value

  // Reveal
  client
    .send_text(
      serde_json::to_string(&ClientMessage::RevealNumbers { value: true })
        .unwrap(),
    )
    .await;
  let _ = recv_update_state(&mut client).await; // UpdateState revealed

  // Hide – values must be zeroed
  client
    .send_text(
      serde_json::to_string(&ClientMessage::RevealNumbers { value: false })
        .unwrap(),
    )
    .await;
  let state = recv_update_state(&mut client).await;

  assert!(!state.all_revealed, "all_revealed must be false after hide");
  if let Some(player) = state.players.iter().find(|p| p.player_id == player_id)
  {
    assert_eq!(
      player.value,
      Some(0),
      "Player value must be reset to 0 after hide"
    );
  }
}

/// Sending `Pong` does not change the game state; the server broadcasts the
/// same state back to confirm the Pong was processed.
#[tokio::test]
async fn test_pong_does_not_change_game_state()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  let mut client = warp::test::ws()
    .path("/ws/it-pong")
    .handshake(filter)
    .await
    .expect("WebSocket handshake should succeed");

  let player_id = recv_player_assigned(&mut client).await;
  let initial_state = recv_update_state(&mut client).await;

  client
    .send_text(
      serde_json::to_string(&ClientMessage::Pong { player_id }).unwrap(),
    )
    .await;

  // The server broadcasts state after every valid message including Pong.
  // The state must be identical to the initial state (Pong is a no-op).
  let after_pong_state = recv_update_state(&mut client).await;
  assert_eq!(
    initial_state, after_pong_state,
    "Pong must not change game state"
  );
}

// ── Multi-client broadcast ────────────────────────────────────────────────────

/// When one client sends a message, all clients in the same room receive the
/// resulting `UpdateState` broadcast.
#[tokio::test]
async fn test_multiple_clients_receive_state_updates()
{
  let (tx, _rx) = broadcast::channel::<RoomUpdate>(255);
  let filter = build_ws_filter(tx);

  // Connect client 1
  let mut client1 = warp::test::ws()
    .path("/ws/it-multi")
    .handshake(filter.clone())
    .await
    .expect("Client 1 handshake should succeed");

  let player_id1 = recv_player_assigned(&mut client1).await;
  let _ = recv_update_state(&mut client1).await; // initial UpdateState for client 1

  // Connect client 2
  let mut client2 = warp::test::ws()
    .path("/ws/it-multi")
    .handshake(filter.clone())
    .await
    .expect("Client 2 handshake should succeed");

  let _ = recv_player_assigned(&mut client2).await;
  let _ = recv_update_state(&mut client2).await; // initial UpdateState for client 2

  // Client 1 changes their name – both clients should receive the broadcast.
  client1
    .send_text(
      serde_json::to_string(&ClientMessage::ChangeName {
        player_id: player_id1,
        name: "Broadcaster".to_string(),
      })
      .unwrap(),
    )
    .await;

  // Client 1 receives its own broadcast
  let state1 = recv_update_state(&mut client1).await;
  let p1 = state1
    .players
    .iter()
    .find(|p| p.player_id == player_id1)
    .expect("Player 1 must be in state");
  assert_eq!(p1.player_name, "Broadcaster");

  // Client 2 also receives the broadcast
  let state2 = recv_update_state(&mut client2).await;
  let p2 = state2
    .players
    .iter()
    .find(|p| p.player_id == player_id1)
    .expect("Player 1 must also be visible to client 2");
  assert_eq!(p2.player_name, "Broadcaster");
}

// ── HTTP routes ───────────────────────────────────────────────────────────────

/// `GET /` redirects to `/index.html?room=<name>`.  `warp::redirect` uses
/// HTTP 301 (Moved Permanently).
#[tokio::test]
async fn test_index_route_redirects_to_room()
{
  let game_state = crate::game::Game::instance();
  let index_route = warp::path::end().and_then(async move || {
    let room_name = game_state.random_name_generator().await;
    Ok::<_, warp::Rejection>(warp::redirect(
      warp::http::Uri::from_maybe_shared(format!("/index.html?room={room_name}"))
        .unwrap(),
    ))
  });
  let routes = warp::get().and(index_route);

  let response = warp::test::request()
    .method("GET")
    .path("/")
    .reply(&routes)
    .await;

  // warp::redirect returns 301 (Moved Permanently)
  assert_eq!(response.status(), 301, "Root path must redirect");
  let location = response
    .headers()
    .get("location")
    .expect("Redirect must include Location header")
    .to_str()
    .expect("Location header must be valid UTF-8");
  assert!(
    location.starts_with("/index.html?room="),
    "Location must point to /index.html?room=…, got: {location}"
  );
}
