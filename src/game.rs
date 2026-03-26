use std::collections::HashMap;
use std::sync::Arc;

use lazy_static::lazy_static;
use log::{debug, error, info};
use tokio::sync::{Mutex, RwLock};

use crate::SharedGameState;
use crate::counter::Counter;
use crate::structs::{ClientMessage, GameState, NotifyChange, PlayerState};

pub(crate) struct Game
{
  game_state: SharedGameState,
  counter: Arc<Mutex<&'static Counter>>, // game_time: Arc<Mutex<SystemTime>>,
}

impl Game
{
  // The Game object is a singleton
  fn new() -> Self
  {
    let game_state: SharedGameState = Arc::new(RwLock::new(HashMap::new()));
    let counter = Arc::new(Mutex::new(Counter::instance()));

    Game {
      game_state,
      counter,
    }
  }

  pub fn instance() -> &'static Game
  {
    lazy_static! {
      static ref GAME: Game = Game::new();
    }
    &GAME
  }

  fn find_player_in_waiting(
    &self,
    players: &[PlayerState],
  ) -> Option<usize>
  {
    let active_count = players.iter().filter(|p| p.player_id < 100).count();
    if active_count < 12
    {
      players
        .iter()
        .find(|player| player.player_id >= 100)
        .map(|player| player.player_id)
    }
    else
    {
      None
    }
  }

  pub(crate) async fn remove_player(
    &self,
    room: &str,
    player_id: usize,
  )
  {
    debug!("remove_player - Room: {}, Player ID: {}", room, player_id);

    let mut state = self.game_state.write().await;
    let room_state = state.entry(room.to_string()).or_insert(GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    });

    if let Some(index) =
      room_state.players.iter().position(|p| p.player_id == player_id)
    {
      room_state.players.remove(index);
      info!("Player {} disconnected.", player_id);

      let player_in_waiting = self.find_player_in_waiting(&room_state.players);

      let vacant_id = player_id;
      match player_in_waiting
      {
        Some(old_id) =>
        {
          self.promote_player(old_id, vacant_id, room_state);

          room_state.notify_change = NotifyChange {
            current_id: old_id,
            new_id: player_id,
          };

          debug!("Player {} promoted to position {}", old_id, player_id);
        },
        None =>
        {
          room_state.notify_change = NotifyChange {
            current_id: 0,
            new_id: 0,
          };
        },
      }
    }

    debug!(
      "remove_player - Room: {}, Player ID: {} - finished",
      room, player_id
    );
  }

  fn promote_player(
    &self,
    old_id: usize,
    new_id: usize,
    state: &mut GameState,
  )
  {
    if let Some(player_index) =
      state.players.iter().position(|p| p.player_id == old_id)
    {
      let cloned_player = state.players[player_index].clone();

      let promoted_player = PlayerState {
        player_id: new_id,
        player_name: cloned_player.player_name,
        value: None,
      };

      state.players.remove(player_index);
      state.players.push(promoted_player);
    }

    debug!(
      "promote_player - Old ID: {}, New ID: {} - finished",
      old_id, new_id
    );
  }

  pub(crate) async fn generate_new_room(
    &self,
    room: Option<&str>,
  ) -> String
  {
    let room_name = match room
    {
      Some(room) => room.to_string(),
      None => self.random_name_generator().await,
    };

    let mut game_state = self.game_state.write().await;
    game_state.insert(
      room_name.clone(),
      GameState {
        players: Vec::new(),
        all_revealed: false,
        notify_change: NotifyChange::default(),
      },
    );
    debug!("generate_new_room - Room Name: {} - finished", room_name);
    room_name
  }

  pub async fn random_name_generator(&self) -> String
  {
    debug!("random_name_generator - entry");

    let adjectives = &[
      "Swift", "Mighty", "Clever", "Silent", "Fierce", "Gentle", "Wild",
      "Brave", "Wise", "Nimble", "Proud", "Noble", "Sleepy", "Cunning",
      "Playful",
    ];

    let animals = &[
      "Fox", "Bear", "Wolf", "Eagle", "Owl", "Lion", "Tiger", "Dolphin",
      "Elephant", "Panther", "Hawk", "Deer", "Rabbit", "Raccoon", "Penguin",
    ];

    let room_name = {
      let c = self.counter.lock().await;
      let ani_index = c.get_fast_index(animals.len());
      let adj_index = c.get_slow_index(adjectives.len(), animals.len());
      format!("{}{}", adjectives[adj_index], animals[ani_index])
    };

    debug!("random_name_generator - Room Name: {} - finished", room_name);

    room_name
  }

  async fn update_room_state(
    &self,
    room: String,
    state: GameState,
  ) -> Option<GameState>
  {
    debug!("update_room_state - Room: {} - finished", room);
    self.game_state.write().await.insert(room.clone(), state)
  }

  pub(crate) async fn get_room_state(
    &self,
    room: &str,
  ) -> Option<GameState>
  {
    debug!("get_room_state - Room: {}", room);

    self.game_state.read().await.get(room).cloned()
  }

  async fn calculate_player_id(
    &self,
    room: &str,
  ) -> usize
  {
    debug!("calculate_player_id - Room: {}", room);
    let max_room_size = 12;
    let overflow_index = 100;

    let player_id = match self.get_room_state(room).await
    {
      Some(state) =>
      {
        if state.players.len() >= max_room_size
        {
          overflow_index + state.players.len()
        }
        else
        {
          // Find the lowest unused player ID
          (0..state.players.len())
            .find(|&i| state.players.iter().all(|p| p.player_id != i))
            .unwrap_or(state.players.len())
        }
      },
      None =>
      {
        // if we had to generate a new room, then we know that the new player id
        // will be 0
        self.generate_new_room(Some(room)).await;
        return 0;
      },
    };

    debug!(
      "calculate_player_id - Room: {} - Player ID: {} - finished",
      room, player_id
    );
    player_id
  }

  pub(crate) async fn new_player(
    &self,
    room: &str,
  ) -> usize
  {
    debug!("new_player - Room: {}", room);
    let player_id = self.calculate_player_id(room).await;

    match self.get_room_state(room).await
    {
      Some(mut room_state) =>
      {
        room_state.players.push(PlayerState {
          player_id,
          player_name: "Delegate Unknown".to_string(),
          value: None,
        });
        match self.update_room_state(room.to_string(), room_state).await
        {
          None => error!("Could not update room state."),
          Some(room_state) => debug!("State updated: {:?}", room_state),
        }
      },
      None =>
      {
        error!("Room state not found in new_player for room: {}", room);
      },
    }

    info!("Player {} joined the room.", player_id);
    debug!("new_player - Room: {}, Player ID: {} - finished", room, player_id);
    player_id
  }

  pub async fn process_client_message(
    &self,
    room: &str,
    message: ClientMessage,
  )
  {
    debug!("process_client_message - Room: {}, Message: {:?}", room, message);

    let mut state = self.game_state.write().await;
    let room_state = state.entry(room.to_string()).or_insert(GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    });

    match message
    {
      ClientMessage::Pong {
        player_id,
      } =>
      {
        debug!("Player {} ponged.", player_id);
      },
      ClientMessage::ChangeValue {
        player_id,
        value,
      } =>
      {
        if let Some(player) =
          room_state.players.iter_mut().find(|p| p.player_id == player_id)
        {
          player.value = Some(value);
        }
      },
      ClientMessage::ChangeName {
        player_id,
        name,
      } =>
      {
        if let Some(player) =
          room_state.players.iter_mut().find(|p| p.player_id == player_id)
        {
          player.player_name = name;
        }
      },
      ClientMessage::RevealNumbers {
        value,
      } =>
      {
        // Only zero out the values if the user wants to reset and the
        // previous state was revealed.
        if !value && room_state.all_revealed
        {
          for player in &mut room_state.players
          {
            if player.value.is_some()
            {
              player.value = Some(0);
            }
          }
        }
        // Update the state
        room_state.all_revealed = value;
      },
    }
  }
}

#[cfg(test)]
mod tests
{
  use super::*;
  use crate::structs::ClientMessage;

  fn new_game() -> Game
  {
    Game::new()
  }

  // ── Room management ──────────────────────────────────────────────────────

  /// Generating a room with a given name stores an empty GameState for that
  /// name.
  #[tokio::test]
  async fn test_generate_new_room_creates_empty_room()
  {
    let game = new_game();
    let name = game.generate_new_room(Some("g-room-1")).await;
    assert_eq!(name, "g-room-1");
    let state = game.get_room_state("g-room-1").await.unwrap();
    assert!(state.players.is_empty());
    assert!(!state.all_revealed);
  }

  /// Calling get_room_state for a room that was never created returns None.
  #[tokio::test]
  async fn test_get_room_state_returns_none_for_nonexistent_room()
  {
    let game = new_game();
    assert!(game.get_room_state("does-not-exist").await.is_none());
  }

  /// random_name_generator produces a non-empty string.
  #[tokio::test]
  async fn test_random_name_generator_returns_nonempty_string()
  {
    let game = new_game();
    let name = game.random_name_generator().await;
    assert!(!name.is_empty());
  }

  // ── Player ID assignment ─────────────────────────────────────────────────

  /// The very first player in a brand-new room receives ID 0.
  #[tokio::test]
  async fn test_new_player_in_new_room_gets_id_zero()
  {
    let game = new_game();
    let id = game.new_player("p-room-first").await;
    assert_eq!(id, 0);
    let state = game.get_room_state("p-room-first").await.unwrap();
    assert_eq!(state.players.len(), 1);
    assert_eq!(state.players[0].player_id, 0);
  }

  /// Subsequent players receive sequential IDs.
  #[tokio::test]
  async fn test_new_player_assigns_sequential_ids()
  {
    let game = new_game();
    game.generate_new_room(Some("p-room-seq")).await;
    assert_eq!(game.new_player("p-room-seq").await, 0);
    assert_eq!(game.new_player("p-room-seq").await, 1);
    assert_eq!(game.new_player("p-room-seq").await, 2);
  }

  /// After a player is removed the lowest vacant ID is reused.
  #[tokio::test]
  async fn test_new_player_reuses_lowest_available_id()
  {
    let game = new_game();
    game.generate_new_room(Some("p-room-reuse")).await;
    game.new_player("p-room-reuse").await; // id 0
    game.new_player("p-room-reuse").await; // id 1
    game.new_player("p-room-reuse").await; // id 2
    game.remove_player("p-room-reuse", 1).await;
    // ID 1 is now the lowest vacant slot
    assert_eq!(game.new_player("p-room-reuse").await, 1);
  }

  /// New players get an overflow ID (≥ 100) when the room already has 12
  /// players.
  #[tokio::test]
  async fn test_new_player_overflow_id_when_room_full()
  {
    let game = new_game();
    game.generate_new_room(Some("p-room-full")).await;
    for _ in 0..12
    {
      game.new_player("p-room-full").await;
    }
    let overflow_id = game.new_player("p-room-full").await;
    assert!(
      overflow_id >= 100,
      "Expected overflow ID ≥ 100, got {overflow_id}"
    );
  }

  /// A new player starts with the default name and no value.
  #[tokio::test]
  async fn test_new_player_has_default_name_and_no_value()
  {
    let game = new_game();
    game.generate_new_room(Some("p-room-defaults")).await;
    game.new_player("p-room-defaults").await;
    let state = game.get_room_state("p-room-defaults").await.unwrap();
    assert_eq!(state.players[0].player_name, "Delegate Unknown");
    assert_eq!(state.players[0].value, None);
  }

  // ── Player removal ───────────────────────────────────────────────────────

  /// Removing a player by ID reduces the player list by one.
  #[tokio::test]
  async fn test_remove_player_removes_player()
  {
    let game = new_game();
    game.generate_new_room(Some("r-room-1")).await;
    game.new_player("r-room-1").await; // id 0
    game.new_player("r-room-1").await; // id 1
    game.remove_player("r-room-1", 0).await;
    let state = game.get_room_state("r-room-1").await.unwrap();
    assert_eq!(state.players.len(), 1);
    assert!(state.players.iter().all(|p| p.player_id != 0));
  }

  /// When 12 active players are present plus a spectating player (ID ≥ 100),
  /// removing an active player promotes the spectator into the vacant slot.
  #[tokio::test]
  async fn test_remove_player_promotes_waiting_player()
  {
    let game = new_game();
    game.generate_new_room(Some("r-room-promote")).await;
    // Fill 12 active players (IDs 0–11)
    for _ in 0..12
    {
      game.new_player("r-room-promote").await;
    }
    // 13th join → spectator with ID ≥ 100
    let spectator_id = game.new_player("r-room-promote").await;
    assert!(spectator_id >= 100, "13th player must be a spectator");

    // Remove player 0 (vacancy); spectator should be promoted into slot 0
    game.remove_player("r-room-promote", 0).await;
    let state = game.get_room_state("r-room-promote").await.unwrap();

    // Spectator no longer retains its original spectator ID
    assert!(state.players.iter().all(|p| p.player_id != spectator_id));
    // Slot 0 is now filled by the promoted player
    assert!(state.players.iter().any(|p| p.player_id == 0));
    // notify_change records the promotion
    assert_eq!(state.notify_change.current_id, spectator_id);
    assert_eq!(state.notify_change.new_id, 0);
  }

  /// When fewer than 12 players remain but no spectator exists, no promotion
  /// occurs.
  #[tokio::test]
  async fn test_remove_player_no_promotion_with_small_room()
  {
    let game = new_game();
    game.generate_new_room(Some("r-room-small")).await;
    game.new_player("r-room-small").await; // id 0
    game.new_player("r-room-small").await; // id 1
    game.remove_player("r-room-small", 0).await;
    let state = game.get_room_state("r-room-small").await.unwrap();
    // No promotion expected; notify_change should be zeroed
    assert_eq!(state.notify_change.current_id, 0);
    assert_eq!(state.notify_change.new_id, 0);
  }

  // ── Message processing ───────────────────────────────────────────────────

  /// ChangeValue updates the player's stored value.
  #[tokio::test]
  async fn test_process_change_value_updates_player_value()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-cv")).await;
    game.new_player("m-room-cv").await; // id 0
    game
      .process_client_message(
        "m-room-cv",
        ClientMessage::ChangeValue {
          player_id: 0,
          value: 5,
        },
      )
      .await;
    let state = game.get_room_state("m-room-cv").await.unwrap();
    let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
    assert_eq!(player.value, Some(5));
  }

  /// ChangeName updates the player's display name.
  #[tokio::test]
  async fn test_process_change_name_updates_player_name()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-cn")).await;
    game.new_player("m-room-cn").await; // id 0
    game
      .process_client_message(
        "m-room-cn",
        ClientMessage::ChangeName {
          player_id: 0,
          name: "Alice".to_string(),
        },
      )
      .await;
    let state = game.get_room_state("m-room-cn").await.unwrap();
    let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
    assert_eq!(player.player_name, "Alice");
  }

  /// RevealNumbers { true } sets all_revealed to true.
  #[tokio::test]
  async fn test_process_reveal_numbers_sets_all_revealed()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-rn-show")).await;
    game
      .process_client_message(
        "m-room-rn-show",
        ClientMessage::RevealNumbers {
          value: true,
        },
      )
      .await;
    let state = game.get_room_state("m-room-rn-show").await.unwrap();
    assert!(state.all_revealed);
  }

  /// RevealNumbers { false } after a reveal resets every player's value to 0
  /// and clears all_revealed.
  #[tokio::test]
  async fn test_process_reveal_numbers_false_resets_values()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-rn-hide")).await;
    game.new_player("m-room-rn-hide").await; // id 0
    game
      .process_client_message(
        "m-room-rn-hide",
        ClientMessage::ChangeValue {
          player_id: 0,
          value: 8,
        },
      )
      .await;
    game
      .process_client_message(
        "m-room-rn-hide",
        ClientMessage::RevealNumbers {
          value: true,
        },
      )
      .await;
    game
      .process_client_message(
        "m-room-rn-hide",
        ClientMessage::RevealNumbers {
          value: false,
        },
      )
      .await;
    let state = game.get_room_state("m-room-rn-hide").await.unwrap();
    assert!(!state.all_revealed);
    let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
    assert_eq!(player.value, Some(0));
  }

  /// RevealNumbers { false } when numbers were never revealed does not reset
  /// values.
  #[tokio::test]
  async fn test_hide_without_prior_reveal_keeps_values()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-rn-noreset")).await;
    game.new_player("m-room-rn-noreset").await;
    game
      .process_client_message(
        "m-room-rn-noreset",
        ClientMessage::ChangeValue {
          player_id: 0,
          value: 3,
        },
      )
      .await;
    // Hide without ever revealing – value must stay intact
    game
      .process_client_message(
        "m-room-rn-noreset",
        ClientMessage::RevealNumbers {
          value: false,
        },
      )
      .await;
    let state = game.get_room_state("m-room-rn-noreset").await.unwrap();
    let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
    assert_eq!(player.value, Some(3));
  }

  /// Pong does not alter game state.
  #[tokio::test]
  async fn test_process_pong_is_noop()
  {
    let game = new_game();
    game.generate_new_room(Some("m-room-pong")).await;
    game.new_player("m-room-pong").await;
    let before = game.get_room_state("m-room-pong").await.unwrap();
    game
      .process_client_message(
        "m-room-pong",
        ClientMessage::Pong {
          player_id: 0,
        },
      )
      .await;
    let after = game.get_room_state("m-room-pong").await.unwrap();
    assert_eq!(before, after);
  }
}
