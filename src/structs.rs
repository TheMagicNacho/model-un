use futures::stream::{SplitSink, SplitStream};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{Receiver, Sender};
use warp::ws::{Message, WebSocket};

#[derive(Clone, Debug)]
pub struct RoomUpdate
{
  pub room: String,
  pub state: GameState,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct PlayerState
{
  pub player_id: usize,
  pub player_name: String,
  pub value: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct GameState
{
  pub players: Vec<PlayerState>,
  pub all_revealed: bool,
  pub notify_change: NotifyChange,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct NotifyChange
{
  pub current_id: usize,
  pub new_id: usize,
}

// The JSON from the client to the server.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ClientMessage
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

// The JSON from the server to the client.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage
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

// A simple structure to help tidy the connections between functions.
pub struct ConnectionContext
{
  pub tx: Sender<RoomUpdate>,
  pub rx: Receiver<RoomUpdate>,
  pub ws_tx: SplitSink<WebSocket, Message>,
  pub ws_rx: SplitStream<WebSocket>,
}
