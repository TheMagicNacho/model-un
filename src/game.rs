use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use lazy_static::lazy_static;
use log::{debug, info};

use crate::structs::{ClientMessage, GameState, NotifyChange, PlayerState};
use crate::SharedGameState;

pub(crate) struct Game
{
  game_state: SharedGameState,
  game_time: Arc<Mutex<SystemTime>>,
}

impl Game
{
  // The Game object is a singleton
  fn new() -> Self
  {
    let game_state = Arc::new(Mutex::new(HashMap::new()));
    let game_time = Arc::new(Mutex::new(SystemTime::now()));
    Game {
      game_state,
      game_time,
    }
  }

  pub fn instance() -> &'static Game
  {
    lazy_static! {
      static ref GAME: Game = Game::new();
    }
    tokio::spawn(async move {
      loop
      {
        let mut time = GAME.game_time.lock().unwrap();
        *time = SystemTime::now();
      }
    });

    &GAME
  }

  fn find_player_in_waiting(
    &self,
    players: &Vec<PlayerState>,
  ) -> Option<usize>
  {
    if players.len() >= 6
    {
      for player in players.iter()
      {
        if player.player_id > 10
        {
          return Some(player.player_id);
        }
      }
    }
    None
  }

  pub(crate) fn remove_player(
    &self,
    room: &str,
    player_id: usize,
  )
  {
    let mut state = self.game_state.lock().unwrap();
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

      // I'm renaming the variable for comprehension.
      let vacant_id = player_id;
      match player_in_waiting
      {
        Some(old_id) =>
        {
          self.promote_player(old_id, vacant_id, room_state);

          room_state.notify_change = NotifyChange {
            current_id: player_in_waiting.unwrap(),
            new_id: player_id,
          };

          debug!("Player {} promoted to position {}", player_id, player_id);
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
  }

  fn promote_player(
    &self,
    old_id: usize,
    new_id: usize,
    state: &mut GameState,
  )
  {
    let player_index =
      state.players.iter().position(|p| p.player_id == old_id).unwrap();

    let cloned_player = state.players[player_index].clone();

    let promoted_player = PlayerState {
      player_id: new_id,
      player_name: cloned_player.player_name,
      value: None,
    };

    state.players.remove(player_index);

    state.players.push(promoted_player);
  }
  pub(crate) fn generate_new_room(
    &self,
    room: Option<&str>,
  ) -> String
  {
    let room = match room
    {
      Some(room) => room.to_string().to_owned(),
      None => self.random_name_generator(),
    };

    // add the room to the state.
    let mut game_state = self.game_state.lock().unwrap();
    game_state.insert(
      room.clone(),
      GameState {
        players: Vec::new(),
        all_revealed: false,
        notify_change: NotifyChange::default(),
      },
    );
    room
  }

  fn random_name_generator(&self) -> String
  {
    let adjectives = vec![
      "Swift", "Mighty", "Clever", "Silent", "Fierce", "Gentle", "Wild",
      "Brave", "Wise", "Nimble", "Proud", "Noble", "Sleepy", "Cunning",
      "Playful",
    ];

    let animals = vec![
      "Fox", "Bear", "Wolf", "Eagle", "Owl", "Lion", "Tiger", "Dolphin",
      "Elephant", "Panther", "Hawk", "Deer", "Rabbit", "Raccoon", "Penguin",
    ];

    // We use this method instead of the rand crate because we noticed that the
    // rand crate would not constantly update over time. This method allows the
    // system to update constantly since the time is always getting re-written
    // in a separate thread.
    let time_adj = self
      .game_time
      .lock()
      .unwrap()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_millis()
      - 6857; // A non-repeating prime number as a seed.
    let adj = adjectives[(time_adj % adjectives.len() as u128) as usize];
    println!("time_adj: {}", time_adj);

    let time_animal = self
      .game_time
      .lock()
      .unwrap()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_millis()
      - 2039; // A non-repeating prime number as a seed.
    println!("time_adj: {}", time_animal);
    let animal = animals[(time_animal % animals.len() as u128) as usize];

    format!("{}{}", adj, animal)
  }

  // update room state
  fn update_room_state(
    &self,
    room: String,
    state: GameState,
  )
  {
    let mut game_state = self.game_state.lock().unwrap();
    game_state.insert(room, state);
  }

  // get room state
  pub(crate) fn get_room_state(
    &self,
    room: &str,
  ) -> Option<GameState>
  {
    let game_state = self.game_state.lock().unwrap();
    game_state.get(room).cloned()
  }

  // calculate player id
  fn calculate_player_id(
    &self,
    room: &str,
  ) -> usize
  {
    let state = match self.get_room_state(room)
    {
      Some(state) => state,
      None =>
      {
        self.generate_new_room(Some(room));
        self.get_room_state(room).unwrap()
      },
    };

    // if the players array is 6, then the player id should be increment from 10
    if state.players.len() >= 6
    {
      return 10 + state.players.len();
    }

    // Otherwise, find the lowest unused player ID
    for i in 0..state.players.len()
    {
      if state.players.iter().all(|p| p.player_id != i)
      {
        return i;
      }
    }
    state.players.len()
  }

  pub(crate) fn new_player(
    &self,
    room: &str,
  ) -> usize
  {
    let player_id = self.calculate_player_id(room);
    let mut room_state = self.get_room_state(room).unwrap();
    room_state.players.push(PlayerState {
      player_id: player_id,
      player_name: "Delegate Unknown".to_string(),
      value: None,
    });

    self.update_room_state(room.to_string(), room_state);

    info!("Player {} joined the room.", player_id);

    player_id
  }

  // Process the client messege, by appropriately updating the game state
  pub async fn process_client_message(
    &self,
    room: &str,
    message: ClientMessage,
  )
  {
    // We are locking and getting the room state within this scope to avoid
    // having to deal with lifetimes between the lock and to send.
    let mut state = self.game_state.lock().unwrap();
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
        if let Some(_player) =
          room_state.players.iter_mut().find(|p| p.player_id == player_id)
        {
          debug!("Player {} ponged.", player_id);
        }
        // TODO: Handle the pong. Currently, nothing happens if a ping is
        // ignored.
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
        // Only zero out the values if the user wants to reset and the previous
        // state was revealed.
        if value == false && room_state.all_revealed == true
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
mod test
{
  use super::*;

  fn setup() -> &'static Game
  {
    let game = Game::instance();
    let room_name = "TestRoom";
    game.generate_new_room(Some("TestRoom"));
    load_players(game, room_name);
    game
  }

  fn load_players(
    game: &Game,
    room: &str,
  )
  {
    game.new_player(room);
    game.new_player(room);
    game.new_player(room);
    game.new_player(room);
    game.new_player(room);
    game.new_player(room);
    game.new_player(room);
  }

  #[test]
  fn test_generate_new_room()
  {
    let game = Game::instance();
    let room = game.generate_new_room(None);

    println!("Room: {:?}", room);
    assert!(room.len() > 6); // the room name is larger than 5 characters

    let room_state = game.get_room_state(&room).unwrap();

    let empty_room = GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    };

    assert_eq!(room_state, empty_room);
  }

  #[test]
  fn test_create_new_room_with_name()
  {
    let game = Game::instance();
    let new_room_name = "NewRoom32";
    let room = game.generate_new_room(Some(new_room_name));
    let room_state = game.get_room_state(&room).unwrap();
    assert_eq!(room, new_room_name);

    let empty_room = GameState {
      players: Vec::new(),
      all_revealed: false,
      notify_change: NotifyChange::default(),
    };

    assert_eq!(room_state, empty_room);
  }

  #[test]
  fn test_new_players()
  {
    let game = setup();
    let room = "LoadingRoom";
    load_players(game, room);
    let room_state = game.get_room_state(room).unwrap();

    assert_eq!(room_state.players.len(), 7);

    for i in 0..5
    {
      let player =
        room_state.players.iter().find(|p| p.player_id == i).unwrap();
      assert_eq!(player.player_name, "Delegate Unknown");
    }
  }

  #[tokio::test]
  async fn process_name_change()
  {
    let game = setup();
    let room = "TestRoom";
    load_players(game, room);

    let name_change = ClientMessage::ChangeName {
      player_id: 0,
      name: "Test Name".to_string(),
    };

    game.process_client_message(room, name_change).await;

    let room_state = game.get_room_state(room).unwrap();
    let player = room_state.players.iter().find(|p| p.player_id == 0).unwrap();
    assert_eq!(player.player_name, "Test Name");
  }

  #[tokio::test]
  async fn player_changes_value()
  {
    let game = setup();
    let room = "TestRoom";
    load_players(game, room);

    let value_change = ClientMessage::ChangeValue {
      player_id: 3,
      value: 5,
    };

    game.process_client_message(room, value_change).await;

    let room_state = game.get_room_state(room).unwrap();
    let player = room_state.players.iter().find(|p| p.player_id == 3).unwrap();
    assert_eq!(player.value, Some(5));
  }

  #[tokio::test]
  async fn player_reveals_numbers()
  {
    let game = setup();
    let room = "TestRoom";
    load_players(game, room);

    let reveal_numbers = ClientMessage::RevealNumbers {
      value: true,
    };

    game.process_client_message(room, reveal_numbers).await;

    let room_state = game.get_room_state(room).unwrap();
    assert_eq!(room_state.all_revealed, true);

    let reveal_numbers = ClientMessage::RevealNumbers {
      value: false,
    };

    game.process_client_message(room, reveal_numbers).await;

    let room_state = game.get_room_state(room).unwrap();
    assert_eq!(room_state.all_revealed, false);
  }

  #[tokio::test]
  async fn player_leaves_room()
  {
    let name_of_player = "PlayerInWaiting";
    let leaving_id: usize = 3;
    let room = "PlayersLeavingRoom";

    let game = setup();
    load_players(game, room);

    // Change the name of a player in waiting
    game
      .process_client_message(
        room,
        ClientMessage::ChangeName {
          player_id: 16,
          name: name_of_player.to_string(),
        },
      )
      .await;

    // Remove a player
    game.remove_player(room, leaving_id);

    let room_state = game.get_room_state(room).unwrap();
    assert_eq!(room_state.players.len(), 6);

    for player in room_state.players.iter()
    {
      println!("Player: {:?}", player);
    }
    let player = room_state.players.iter().find(|p| p.player_id == leaving_id);
    println!("Player: {:?}", player);
    assert_eq!(player.unwrap().player_name, name_of_player);
  }
}
