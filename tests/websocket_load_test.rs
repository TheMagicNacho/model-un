//! WebSocket load / stress integration tests.
//!
//! These tests validate that the Model UN server can handle
//! many concurrent WebSocket connections across multiple rooms
//! while clients actively change values and toggle reveal.

use std::net::SocketAddr;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use model_un::build_ws_route;
use model_un::structs::{ClientMessage, ServerMessage};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
  MaybeTlsStream, WebSocketStream, connect_async,
};

/// Fibonacci values used by the game UI.
const VOTE_VALUES: &[u8] = &[1, 2, 3, 5, 8, 13, 21];

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

/// Start a warp server on an OS-assigned port and return the address.
async fn start_server() -> SocketAddr
{
  let (ws_route, _tx) = build_ws_route();
  let (addr, server_future) = warp::serve(ws_route)
    .bind_ephemeral(([127, 0, 0, 1], 0u16));
  tokio::spawn(server_future);
  // Give the server a moment to bind.
  tokio::time::sleep(Duration::from_millis(50)).await;
  addr
}

/// Open a WebSocket connection to `room` at the given server
/// address. Returns the WebSocket stream and the player_id
/// assigned by the server.
async fn connect_client(
  addr: SocketAddr,
  room: &str,
) -> (
  WebSocketStream<MaybeTlsStream<TcpStream>>,
  usize,
)
{
  let url = format!("ws://{addr}/ws/{room}");

  let (ws, _resp) = connect_async(&url)
    .await
    .expect("WebSocket handshake failed");

  // The server sends two messages on connect:
  //   1. PlayerAssigned { player_id }
  //   2. UpdateState(GameState)
  // We need to read at least the first one to get the
  // player_id.
  let (sink, mut stream) = ws.split();

  let mut player_id: Option<usize> = None;

  // Read up to 2 initial messages within a timeout.
  for _ in 0..2
  {
    match timeout(
      Duration::from_secs(5),
      stream.next(),
    )
    .await
    {
      Ok(Some(Ok(msg))) =>
      {
        if let Ok(text) = msg.into_text()
          && let Ok(ServerMessage::PlayerAssigned {
            player_id: pid,
          }) =
            serde_json::from_str::<ServerMessage>(&text)
        {
          player_id = Some(pid);
        }
      },
      _ => break,
    }
  }

  let ws = sink.reunite(stream).expect("reunite");
  let pid = player_id.expect("server should assign player_id");
  (ws, pid)
}

/// Simulate a single client performing a series of actions:
///   - Change value several times (cycling through Fibonacci
///     values)
///   - Change name once
///   - Toggle reveal on/off
///
/// Returns `true` if the client operated without fatal error.
async fn simulate_client_activity(
  ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
  player_id: usize,
  rounds: usize,
) -> bool
{
  for round in 0..rounds
  {
    // Pick a Fibonacci value to vote.
    let value =
      VOTE_VALUES[round % VOTE_VALUES.len()];

    let change_value = serde_json::to_string(
      &ClientMessage::ChangeValue {
        player_id,
        value,
      },
    )
    .unwrap();
    if ws.send(Message::Text(change_value.into())).await.is_err()
    {
      return false;
    }

    // Change name occasionally.
    if round % 3 == 0
    {
      let change_name = serde_json::to_string(
        &ClientMessage::ChangeName {
          player_id,
          name: format!("Player_{player_id}_r{round}"),
        },
      )
      .unwrap();
      if ws.send(Message::Text(change_name.into())).await.is_err()
      {
        return false;
      }
    }

    // Toggle reveal periodically.
    if round % 5 == 0
    {
      let reveal = serde_json::to_string(
        &ClientMessage::RevealNumbers {
          value: true,
        },
      )
      .unwrap();
      if ws.send(Message::Text(reveal.into())).await.is_err()
      {
        return false;
      }

      let reset = serde_json::to_string(
        &ClientMessage::RevealNumbers {
          value: false,
        },
      )
      .unwrap();
      if ws.send(Message::Text(reset.into())).await.is_err()
      {
        return false;
      }
    }

    // Drain any pending server messages so the receiver
    // buffer does not fill up. We do not block long.
    while let Ok(Some(Ok(_msg))) = timeout(
      Duration::from_millis(5),
      ws.next(),
    )
    .await
    {
      // consumed a message – keep draining
    }
  }

  true
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

/// Validates that at least 20 unique WebSocket connections can
/// operate simultaneously across multiple rooms that are filled
/// to capacity.
///
/// Room capacity: 12 delegates (ids 0-11) + spectators (ids
/// 100+). To exercise "rooms filled up" we use 2 rooms with
/// 12 connections each (24 total ≥ 20 minimum).
///
/// Each client constantly changes its value, changes its name,
/// and toggles reveal, then we assert the server did not crash.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_minimum_20_concurrent_connections()
{
  let addr = start_server().await;

  let room_a = "LoadTestRoomAlpha";
  let room_b = "LoadTestRoomBeta";
  let clients_per_room: usize = 12;
  let total_clients = clients_per_room * 2;
  assert!(total_clients >= 20);

  // Connect all clients. We store handles so we can
  // join them later.
  let mut handles: Vec<JoinHandle<bool>> =
    Vec::with_capacity(total_clients);

  for i in 0..total_clients
  {
    let room = if i < clients_per_room
    {
      room_a.to_string()
    }
    else
    {
      room_b.to_string()
    };

    let handle: JoinHandle<bool> = tokio::spawn(async move {
      let (mut ws, player_id) =
        connect_client(addr, &room).await;
      simulate_client_activity(&mut ws, player_id, 20).await
    });

    handles.push(handle);
    // Stagger connections slightly to avoid
    // overwhelming the accept queue.
    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  // Wait for every client to finish.
  let mut success_count = 0usize;
  for handle in handles
  {
    match timeout(Duration::from_secs(30), handle).await
    {
      Ok(Ok(true)) => success_count += 1,
      Ok(Ok(false)) =>
      {
        panic!(
          "A client encountered a send error – \
           server may have dropped the connection."
        );
      },
      Ok(Err(e)) =>
      {
        panic!("Client task panicked: {e}");
      },
      Err(_) =>
      {
        panic!("Client task timed out after 30 s");
      },
    }
  }

  assert_eq!(
    success_count, total_clients,
    "All {total_clients} clients should complete \
     successfully"
  );
}

/// Progressively opens WebSocket connections to find the
/// maximum number the server can handle before it starts
/// rejecting or erroring.
///
/// Strategy:
///   - Distribute connections across rooms (20 per room) to
///     avoid broadcast-channel saturation.
///   - Open connections in batches of 20.
///   - Each client sends a quick vote + reveal cycle to prove
///     the connection is functional.
///   - Stop when a connection or activity fails, or after
///     reaching a hard cap (200).
///   - Assert we reached at least 20 (the minimum
///     requirement).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_find_maximum_connections()
{
  let addr = start_server().await;

  let clients_per_room: usize = 20;
  let batch_size: usize = 20;
  let hard_cap: usize = 200;

  let mut current_count: usize = 0;
  let mut room_index: usize = 0;

  // Hold all WebSocket streams so connections stay open
  // while we keep adding more.
  let mut live_connections: Vec<(
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    usize,
  )> = Vec::new();

  let mut hit_limit = false;

  while current_count < hard_cap && !hit_limit
  {
    let room =
      format!("MaxRoom_{room_index}");

    for _ in 0..batch_size
    {
      // Connect with a short timeout.
      let result = timeout(
        Duration::from_secs(5),
        connect_client(addr, &room),
      )
      .await;

      match result
      {
        Ok((ws, player_id)) =>
        {
          live_connections.push((ws, player_id));
          current_count += 1;
        },
        Err(_) =>
        {
          hit_limit = true;
          break;
        },
      }
    }

    if hit_limit
    {
      break;
    }

    // After each batch, exercise the newest connections
    // with a quick activity cycle.
    let start = current_count.saturating_sub(batch_size);
    for (ws, pid) in
      &mut live_connections[start..current_count]
    {
      if !simulate_client_activity(ws, *pid, 5).await
      {
        hit_limit = true;
        break;
      }
    }

    // Move to next room every `clients_per_room`
    // connections to spread the load.
    if current_count % clients_per_room == 0
    {
      room_index += 1;
    }
  }

  eprintln!(
    "\n=== Maximum concurrent WebSocket connections \
     sustained: {current_count} ===\n"
  );

  assert!(
    current_count >= 20,
    "Server should handle at least 20 concurrent \
     connections, but only managed {current_count}"
  );
}
