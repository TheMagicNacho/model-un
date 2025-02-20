use serde::{Deserialize, Serialize};

#[derive(Clone)]
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
