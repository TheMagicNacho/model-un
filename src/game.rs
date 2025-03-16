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
    if players.len() >= 6
    {
      players
        .iter()
        .find(|player| player.player_id > 10)
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
    game_state.insert(room_name.clone(), GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    });
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

    let player_id = match self.get_room_state(room).await
    {
      Some(state) =>
      {
        if state.players.len() >= 6
        {
          10 + state.players.len()
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
