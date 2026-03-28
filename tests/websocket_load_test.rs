//! WebSocket load / stress integration tests.
//!
//! These tests validate that the Model UN server can handle
//! many concurrent WebSocket connections across multiple rooms
//! while clients actively change values and toggle reveal.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use model_un::build_ws_route;
use model_un::structs::{ClientMessage, GameState, ServerMessage};
use tokio::net::TcpStream;
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

/// Start a warp server on an OS-assigned port and return the address.
async fn start_server() -> SocketAddr {
    let (ws_route, _tx) = build_ws_route();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0u16))
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().expect("failed to get local addr");
    tokio::spawn(warp::serve(ws_route).incoming(listener).run());
    // Give the server a moment to start accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// Open a WebSocket connection to `room` at the given server
/// address. Returns the WebSocket stream and the player_id
/// assigned by the server.
async fn connect_client(
    addr: SocketAddr,
    room: &str,
) -> (WebSocketStream<MaybeTlsStream<TcpStream>>, usize) {
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
    for _ in 0..2 {
        match timeout(Duration::from_secs(5), stream.next()).await {
            Ok(Some(Ok(msg))) => {
                if let Ok(text) = msg.into_text()
                    && let Ok(ServerMessage::PlayerAssigned { player_id: pid }) =
                        serde_json::from_str::<ServerMessage>(&text)
                {
                    player_id = Some(pid);
                }
            }
            _ => break,
        }
    }

    let ws = sink.reunite(stream).expect("reunite");
    let pid = player_id.expect("server should assign player_id");
    (ws, pid)
}

/// Simulate a single client performing a series of actions:
///   - Change value several times (cycling through Fibonacci values)
///   - Change name once
///   - Toggle reveal on/off
///
/// `vote_values` is the set of Fibonacci values to cycle
/// through for each round.
///
/// Returns a tuple of:
///   - `bool`: whether the client operated without fatal error
///   - `Option<GameState>`: the last room state observed from server broadcasts
///     (used to validate state homogeneity across room members)
async fn simulate_client_activity(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    player_id: usize,
    rounds: usize,
    vote_values: &[u8],
) -> (bool, Option<GameState>) {
    let mut last_state: Option<GameState> = None;

    for round in 0..rounds {
        // Pick a Fibonacci value to vote.
        let value = vote_values[round % vote_values.len()];

        let change_value =
            serde_json::to_string(&ClientMessage::ChangeValue { player_id, value }).unwrap();
        if ws.send(Message::Text(change_value.into())).await.is_err() {
            return (false, last_state);
        }

        // Change name occasionally.
        if round % 3 == 0 {
            let change_name = serde_json::to_string(&ClientMessage::ChangeName {
                player_id,
                name: format!("Player_{player_id}_r{round}"),
            })
            .unwrap();
            if ws.send(Message::Text(change_name.into())).await.is_err() {
                return (false, last_state);
            }
        }

        // Toggle reveal periodically.
        if round % 5 == 0 {
            let reveal =
                serde_json::to_string(&ClientMessage::RevealNumbers { value: true }).unwrap();
            if ws.send(Message::Text(reveal.into())).await.is_err() {
                return (false, last_state);
            }

            let reset =
                serde_json::to_string(&ClientMessage::RevealNumbers { value: false }).unwrap();
            if ws.send(Message::Text(reset.into())).await.is_err() {
                return (false, last_state);
            }
        }

        // Drain any pending server messages so the receiver
        // buffer does not fill up. Track the last
        // UpdateState to validate room state consistency.
        while let Ok(Some(Ok(msg))) = timeout(Duration::from_millis(5), ws.next()).await {
            if let Ok(text) = msg.into_text()
                && let Ok(ServerMessage::UpdateState(state)) =
                    serde_json::from_str::<ServerMessage>(&text)
            {
                last_state = Some(state);
            }
        }

        // Small delay between rounds to prevent broadcast
        // channel overflow when many clients are active
        // concurrently in the same room.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    (true, last_state)
}

/// Drain remaining server messages and return the last
/// `GameState` observed. Used after a synchronization
/// barrier so all clients collect state while every
/// connection is still open.
async fn drain_final_state(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Option<GameState> {
    let mut last_state: Option<GameState> = None;
    while let Ok(Some(Ok(msg))) = timeout(Duration::from_millis(50), ws.next()).await {
        if let Ok(text) = msg.into_text()
            && let Ok(ServerMessage::UpdateState(state)) =
                serde_json::from_str::<ServerMessage>(&text)
        {
            last_state = Some(state);
        }
    }
    last_state
}

/// Assert that all observed final states within a single
/// room are structurally consistent: same player count and
/// same set of player IDs.
///
/// This catches caching or concurrency bugs where one
/// client might see a stale or divergent snapshot of the
/// room (e.g. missing players, duplicate IDs, or corrupted
/// player lists).
fn assert_room_state_homogeneity(states: &[GameState], room: &str, expected_player_count: usize) {
    assert!(!states.is_empty(), "No states collected for room {room}");

    let reference = &states[0];
    let mut ref_ids: Vec<usize> = reference.players.iter().map(|p| p.player_id).collect();
    ref_ids.sort();

    // Every observed state must have the expected number
    // of players and the same set of player IDs.
    for (i, state) in states.iter().enumerate() {
        assert_eq!(
            state.players.len(),
            expected_player_count,
            "Room {room}: client {i} saw {} players, \
       expected {expected_player_count}",
            state.players.len(),
        );

        let mut ids: Vec<usize> = state.players.iter().map(|p| p.player_id).collect();
        ids.sort();

        assert_eq!(
            ref_ids, ids,
            "Room {room}: client 0 and client {i} \
       observed different player ID sets"
        );
    }
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

/// Validates that 24 unique WebSocket connections can operate
/// simultaneously across two rooms that are filled to capacity.
///
/// Room capacity: 12 delegates (ids 0-11) + spectators (ids
/// 100+). We use 2 rooms with 12 connections each (24 total).
///
/// The test runs in three phases:
///   1. **Connect** – all 24 clients establish WebSocket connections before any
///      activity begins.
///   2. **Activity** – all clients concurrently change values, rename, and
///      toggle reveal.
///   3. **Barrier + state snapshot** – a `tokio::sync::Barrier` ensures every
///      client finishes activity and keeps its connection open while all
///      clients drain remaining server messages. The last `UpdateState` each
///      client sees is collected.
///
/// After the barrier we assert:
///   1. All 24 clients finished without errors.
///   2. Room state homogeneity: every client within a room observed the same
///      player count and the same set of player IDs. This catches caching or
///      concurrency bugs that might cause divergent views under load.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_minimum_24_concurrent_connections() {
    let addr = start_server().await;
    let activity_rounds: usize = 20;

    let room_a = "LoadTestRoomAlpha";
    let room_b = "LoadTestRoomBeta";
    let clients_per_room: usize = 12;
    let total_clients = clients_per_room * 2;

    // Phase 1: Connect all clients before any activity
    // starts.
    let mut connections: Vec<(WebSocketStream<MaybeTlsStream<TcpStream>>, usize, String)> =
        Vec::with_capacity(total_clients);

    for i in 0..total_clients {
        let room = if i < clients_per_room {
            room_a.to_string()
        } else {
            room_b.to_string()
        };
        let (ws, pid) = connect_client(addr, &room).await;
        connections.push((ws, pid, room));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Phase 2 + 3: Run activity, then barrier + drain.
    // The barrier keeps every connection alive until ALL
    // clients have finished activity so that no player is
    // removed from the room before the state snapshot.
    let barrier = Arc::new(Barrier::new(total_clients));
    let mut handles: Vec<JoinHandle<(bool, Option<GameState>, String)>> =
        Vec::with_capacity(total_clients);

    for (ws, pid, room) in connections {
        let b = barrier.clone();
        let handle = tokio::spawn(async move {
            let mut ws = ws;
            let vote_values: &[u8] = &[1, 2, 3, 5, 8, 13, 21];
            let (ok, _) =
                simulate_client_activity(&mut ws, pid, activity_rounds, vote_values).await;

            // Wait for every client to finish activity
            // before draining the final state.
            b.wait().await;
            let final_state = drain_final_state(&mut ws).await;

            (ok, final_state, room)
        });
        handles.push(handle);
    }

    // Collect results and validate.
    let mut success_count = 0usize;
    let mut room_a_states: Vec<GameState> = Vec::new();
    let mut room_b_states: Vec<GameState> = Vec::new();

    for handle in handles {
        match timeout(Duration::from_secs(5), handle).await {
            Ok(Ok((true, last_state, room))) => {
                success_count += 1;
                if let Some(state) = last_state {
                    if room == room_a {
                        room_a_states.push(state);
                    } else {
                        room_b_states.push(state);
                    }
                }
            }
            Ok(Ok((false, _, _))) => {
                panic!(
                    "A client encountered a send error – \
           server may have dropped the connection."
                );
            }
            Ok(Err(e)) => {
                panic!("Client task panicked: {e}");
            }
            Err(_) => {
                panic!("Client task timed out after 30 s");
            }
        }
    }

    assert_eq!(
        success_count, total_clients,
        "All {total_clients} clients should complete \
     successfully"
    );

    // Validate room state homogeneity: all clients within
    // a room must have observed the same player count and
    // player ID set.
    assert_room_state_homogeneity(&room_a_states, room_a, clients_per_room);
    assert_room_state_homogeneity(&room_b_states, room_b, clients_per_room);
}

/// Progressively opens WebSocket connections to find the
/// maximum number the server can handle before it starts
/// rejecting or erroring.
///
/// Strategy:
///   - Distribute connections across rooms (12 per room) to match real room
///     capacity.
///   - Open connections in batches of 12.
///   - Each client sends a quick vote + reveal cycle to prove the connection is
///     functional.
///   - Stop when a connection or activity fails, or after reaching a hard cap
///     (5000).
///   - Assert we maintained concurrent connections up to the hard cap value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_find_maximum_connections() {
    let addr = start_server().await;
    let vote_values: &[u8] = &[1, 2, 3, 5, 8, 13, 21];

    let clients_per_room: usize = 12;
    let batch_size: usize = 12;
    // We have tested up to 5000 connections. But that takes a long time to run in
    // the pipeline. So we are keeping the hard cap for this at 100.
    let hard_cap: usize = 100;

    let mut current_count: usize = 0;
    let mut room_index: usize = 0;

    // Hold all WebSocket streams so connections stay open
    // while we keep adding more.
    let mut live_connections: Vec<(WebSocketStream<MaybeTlsStream<TcpStream>>, usize)> = Vec::new();

    let mut hit_limit = false;

    while current_count < hard_cap && !hit_limit {
        let room = format!("MaxRoom_{room_index}");

        for _ in 0..batch_size {
            if current_count >= hard_cap {
                break;
            }

            // Connect with a short timeout.
            let result = timeout(Duration::from_secs(1), connect_client(addr, &room)).await;

            match result {
                Ok((ws, player_id)) => {
                    live_connections.push((ws, player_id));
                    current_count += 1;
                }
                Err(_) => {
                    hit_limit = true;
                    break;
                }
            }
        }

        if hit_limit {
            break;
        }

        // After each batch, exercise the newest connections
        // with a quick activity cycle.
        let start = current_count.saturating_sub(batch_size);
        for (ws, pid) in &mut live_connections[start..current_count] {
            let (ok, _state) = simulate_client_activity(ws, *pid, 5, vote_values).await;
            if !ok {
                hit_limit = true;
                break;
            }
        }

        // Move to next room every `clients_per_room`
        // connections to spread the load.
        if current_count % clients_per_room == 0 {
            room_index += 1;
        }
    }

    eprintln!(
        "\n=== Maximum concurrent WebSocket connections \
     sustained: {current_count} ===\n"
    );

    assert_eq!(
        current_count, hard_cap,
        "Server should maintain {hard_cap} concurrent \
     connections, but only managed {current_count}"
    );
}
