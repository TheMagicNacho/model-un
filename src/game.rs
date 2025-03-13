use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use lazy_static::lazy_static;
use log::{debug, error, info};

use crate::SharedGameState;
use crate::structs::{ClientMessage, GameState, NotifyChange, PlayerState};


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
        // No logging here, as this is a very frequent update.
        // Error handling for lock in this background task is less critical
        // as it's not directly in the request path, but could be added if needed.
        if let Ok(mut time) = GAME.game_time.lock() {
          *time = SystemTime::now();
        }
      }
    });

    &GAME
  }

  fn find_player_in_waiting(
    &self,
    // Allowing a vec reference because I do not want an array.
    #[allow(clippy::ptr_arg)] players: &Vec<PlayerState>,
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
    debug!("remove_player - Room: {}, Player ID: {}", room, player_id); // Entry log
    match self.game_state.lock() {
      Ok(mut state) => {
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
      },
      Err(poison_err) => {
        error!("Mutex poisoned in remove_player for room: {}, player_id: {}. Error: {}", room, player_id, poison_err);
        // Consider how to handle mutex poisoning. For now, logging the error.
      }
    }
    debug!("remove_player - Room: {}, Player ID: {} - finished", room, player_id); // Exit log
  }

  fn promote_player(
    &self,
    old_id: usize,
    new_id: usize,
    state: &mut GameState,
  )
  {
    debug!("promote_player - Old ID: {}, New ID: {}", old_id, new_id); // Entry log
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
    debug!("promote_player - Old ID: {}, New ID: {} - finished", old_id, new_id); // Exit log
  }
  pub(crate) fn generate_new_room(
    &self,
    room: Option<&str>,
  ) -> String
  {
    let room_name = match room
    {
      Some(room) => room.to_string().to_owned(),
      None => self.random_name_generator(),
    };
    debug!("generate_new_room - Room Name: {}", room_name); // Entry log


    // add the room to the state.
    match self.game_state.lock() {
      Ok(mut game_state) => {
        game_state.insert(room_name.clone(), GameState {
          players: Vec::new(),
          all_revealed: false,
          notify_change: NotifyChange::default(),
        });
      },
      Err(poison_err) => {
        error!("Mutex poisoned in generate_new_room for room_name: {}. Error: {}", room_name, poison_err);
      }
    }
    debug!("generate_new_room - Room Name: {} - finished", room_name); // Exit log
    room_name
  }

  fn random_name_generator(&self) -> String
  {
    debug!("random_name_generator - entry"); // Entry log

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
    let mut time_adj_millis = 0;
    match self.game_time.lock() {
      Ok(time_lock) => {
        match time_lock.duration_since(SystemTime::UNIX_EPOCH) {
          Ok(duration) => {
            time_adj_millis = duration.as_millis();
          },
          Err(e) => {
            error!("Error getting duration since epoch in random_name_generator (adjectives): {}", e);
          }
        }
      },
      Err(poison_err) => {
        error!("Mutex poisoned in random_name_generator (adjectives). Error: {}", poison_err);
      }
    }


    let time_adj = time_adj_millis - 6857; // A non-repeating prime number as a seed.


    let adj = adjectives[(time_adj % adjectives.len() as u128) as usize];


    let mut time_animal_millis = 0;
    match self.game_time.lock() {
      Ok(time_lock) => {
        match time_lock.duration_since(SystemTime::UNIX_EPOCH) {
          Ok(duration) => {
            time_animal_millis = duration.as_millis();
          },
          Err(e) => {
            error!("Error getting duration since epoch in random_name_generator (animals): {}", e);
          }
        }
      },
      Err(poison_err) => {
        error!("Mutex poisoned in random_name_generator (animals). Error: {}", poison_err);
      }
    }
    let time_animal = time_animal_millis - 2039; // A non-repeating prime number as a seed.
    let animal = animals[(time_animal % animals.len() as u128) as usize];

    let room_name = format!("{}{}", adj, animal);
    debug!("random_name_generator - Room Name: {} - finished", room_name);
    room_name
  }

  fn update_room_state(
    &self,
    room: String,
    state: GameState,
  )
  {
    debug!("update_room_state - Room: {}", room); // Entry log
    match self.game_state.lock() {
      Ok(mut game_state) => {
        game_state.insert(room.clone(), state);
      },
      Err(poison_err) => {
        error!("Mutex poisoned in update_room_state for room: {}. Error: {}", room, poison_err);
      }
    }
    debug!("update_room_state - Room: {} - finished", room); // Exit log
  }

  // get room state
  pub(crate) fn get_room_state(
    &self,
    room: &str,
  ) -> Option<GameState>
  {
    debug!("get_room_state - Room: {}", room);
    let room_state_option = match self.game_state.lock() {
      Ok(game_state) => {
        game_state.get(room).cloned()
      },
      Err(poison_err) => {
        error!("Mutex poisoned in get_room_state for room: {}. Error: {}", room, poison_err);
        None 
      }
    };
    debug!("get_room_state - Room: {} - finished", room); 
    room_state_option
  }

  fn calculate_player_id(
    &self,
    room: &str,
  ) -> usize
  {
    debug!("calculate_player_id - Room: {}", room); // Entry log
    let player_id = match self.get_room_state(room)
    {
      Some(state) => {

        if state.players.len() >= 6
        {
          10 + state.players.len()
        } else {

          // Otherwise, find the lowest unused player ID
          for i in 0..state.players.len()
          {
            if state.players.iter().all(|p| p.player_id != i)
            {
              debug!("calculate_player_id - Room: {} - Player ID: {} - finished (reusing id)", room, i);
              return i;
            }
          }
          debug!("calculate_player_id - Room: {} - Player ID: {} - finished (new id)", room, state.players.len());
          state.players.len()
        }
      },
      None =>
        {
          self.generate_new_room(Some(room));
          self.calculate_player_id(room) 
        },
    };
    debug!("calculate_player_id - Room: {} - Player ID: {} - finished", room, player_id);
    player_id
  }

  pub(crate) fn new_player(
    &self,
    room: &str,
  ) -> usize
  {
    debug!("new_player - Room: {}", room); // Entry log
    let player_id = self.calculate_player_id(room);

    match self.get_room_state(room) {
      Some(mut room_state) => {
        room_state.players.push(PlayerState {
          player_id,
          player_name: "Delegate Unknown".to_string(),
          value: None,
        });
        self.update_room_state(room.to_string(), room_state);

      },
      None => {
        error!("Room state not found in new_player for room: {}", room);
      }
    }


    info!("Player {} joined the room.", player_id);
    debug!("new_player - Room: {}, Player ID: {} - finished", room, player_id); // Exit log
    player_id
  }

  pub async fn process_client_message(
    &self,
    room: &str,
    message: ClientMessage,
  )
  {
    debug!("process_client_message - Room: {}, Message: {:?}", room, message); // Entry log
    
    match self.game_state.lock() {
      Ok(mut state) => {
        let room_state = state.entry(room.to_string()).or_insert(GameState {
          players: Vec::new(),
          all_revealed: false,
          notify_change: NotifyChange::default(),
        });

        match message.clone()
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
      },
      Err(poison_err) => {
        error!("Mutex poisoned in process_client_message for room: {}, message: {:?}. Error: {}", room, message, poison_err);
      }
    }
    debug!("process_client_message - Room: {}, Message: {:?} - finished", room, message); // Exit log
  }
}

/////

// 
// pub(crate) struct Game
// {
//   game_state: SharedGameState,
//   game_time: Arc<Mutex<SystemTime>>,
// }
// 
// impl Game
// {
//   // The Game object is a singleton
//   fn new() -> Self
//   {
//     let game_state = Arc::new(Mutex::new(HashMap::new()));
//     let game_time = Arc::new(Mutex::new(SystemTime::now()));
//     Game {
//       game_state,
//       game_time,
//     }
//   }
// 
//   pub fn instance() -> &'static Game
//   {
//     lazy_static! {
//       static ref GAME: Game = Game::new();
//     }
//     tokio::spawn(async move {
//       loop
//       {
//         let mut time = GAME.game_time.lock().unwrap();
//         *time = SystemTime::now();
//       }
//     });
// 
//     &GAME
//   }
// 
//   fn find_player_in_waiting(
//     &self,
//     // Allowing a vec reference because I do not want an array.
//     #[allow(clippy::ptr_arg)] players: &Vec<PlayerState>,
//   ) -> Option<usize>
//   {
//     if players.len() >= 6
//     {
//       for player in players.iter()
//       {
//         if player.player_id > 10
//         {
//           return Some(player.player_id);
//         }
//       }
//     }
//     None
//   }
// 
//   pub(crate) fn remove_player(
//     &self,
//     room: &str,
//     player_id: usize,
//   )
//   {
//     let mut state = self.game_state.lock().unwrap();
//     let room_state = state.entry(room.to_string()).or_insert(GameState {
//       players: Vec::new(),
//       all_revealed: false,
//       notify_change: NotifyChange::default(),
//     });
// 
//     if let Some(index) =
//       room_state.players.iter().position(|p| p.player_id == player_id)
//     {
//       room_state.players.remove(index);
//       info!("Player {} disconnected.", player_id);
// 
//       let player_in_waiting = self.find_player_in_waiting(&room_state.players);
// 
//       // I'm renaming the variable for comprehension.
//       let vacant_id = player_id;
//       match player_in_waiting
//       {
//         Some(old_id) =>
//         {
//           self.promote_player(old_id, vacant_id, room_state);
// 
//           room_state.notify_change = NotifyChange {
//             current_id: player_in_waiting.unwrap(),
//             new_id: player_id,
//           };
// 
//           debug!("Player {} promoted to position {}", player_id, player_id);
//         },
//         None =>
//         {
//           room_state.notify_change = NotifyChange {
//             current_id: 0,
//             new_id: 0,
//           };
//         },
//       }
//     }
//   }
// 
//   fn promote_player(
//     &self,
//     old_id: usize,
//     new_id: usize,
//     state: &mut GameState,
//   )
//   {
//     let player_index =
//       state.players.iter().position(|p| p.player_id == old_id).unwrap();
// 
//     let cloned_player = state.players[player_index].clone();
// 
//     let promoted_player = PlayerState {
//       player_id: new_id,
//       player_name: cloned_player.player_name,
//       value: None,
//     };
// 
//     state.players.remove(player_index);
// 
//     state.players.push(promoted_player);
//   }
//   pub(crate) fn generate_new_room(
//     &self,
//     room: Option<&str>,
//   ) -> String
//   {
//     let room = match room
//     {
//       Some(room) => room.to_string().to_owned(),
//       None => self.random_name_generator(),
//     };
// 
//     // add the room to the state.
//     let mut game_state = self.game_state.lock().unwrap();
//     game_state.insert(room.clone(), GameState {
//       players: Vec::new(),
//       all_revealed: false,
//       notify_change: NotifyChange::default(),
//     });
//     room
//   }
// 
//   fn random_name_generator(&self) -> String
//   {
//     let adjectives = vec![
//       "Swift", "Mighty", "Clever", "Silent", "Fierce", "Gentle", "Wild",
//       "Brave", "Wise", "Nimble", "Proud", "Noble", "Sleepy", "Cunning",
//       "Playful",
//     ];
// 
//     let animals = vec![
//       "Fox", "Bear", "Wolf", "Eagle", "Owl", "Lion", "Tiger", "Dolphin",
//       "Elephant", "Panther", "Hawk", "Deer", "Rabbit", "Raccoon", "Penguin",
//     ];
// 
//     // We use this method instead of the rand crate because we noticed that the
//     // rand crate would not constantly update over time. This method allows the
//     // system to update constantly since the time is always getting re-written
//     // in a separate thread.
//     let time_adj = self
//       .game_time
//       .lock()
//       .unwrap()
//       .duration_since(SystemTime::UNIX_EPOCH)
//       .unwrap()
//       .as_millis()
//       - 6857; // A non-repeating prime number as a seed.
//     let adj = adjectives[(time_adj % adjectives.len() as u128) as usize];
//     println!("time_adj: {}", time_adj);
// 
//     let time_animal = self
//       .game_time
//       .lock()
//       .unwrap()
//       .duration_since(SystemTime::UNIX_EPOCH)
//       .unwrap()
//       .as_millis()
//       - 2039; // A non-repeating prime number as a seed.
//     println!("time_adj: {}", time_animal);
//     let animal = animals[(time_animal % animals.len() as u128) as usize];
// 
//     format!("{}{}", adj, animal)
//   }
// 
//   fn update_room_state(
//     &self,
//     room: String,
//     state: GameState,
//   )
//   {
//     let mut game_state = self.game_state.lock().unwrap();
//     game_state.insert(room, state);
//   }
// 
//   // get room state
//   pub(crate) fn get_room_state(
//     &self,
//     room: &str,
//   ) -> Option<GameState>
//   {
//     let game_state = self.game_state.lock().unwrap();
//     game_state.get(room).cloned()
//   }
// 
//   fn calculate_player_id(
//     &self,
//     room: &str,
//   ) -> usize
//   {
//     let state = match self.get_room_state(room)
//     {
//       Some(state) => state,
//       None =>
//       {
//         self.generate_new_room(Some(room));
//         self.get_room_state(room).unwrap()
//       },
//     };
// 
//     if state.players.len() >= 6
//     {
//       return 10 + state.players.len();
//     }
// 
//     // Otherwise, find the lowest unused player ID
//     for i in 0..state.players.len()
//     {
//       if state.players.iter().all(|p| p.player_id != i)
//       {
//         return i;
//       }
//     }
//     state.players.len()
//   }
// 
//   pub(crate) fn new_player(
//     &self,
//     room: &str,
//   ) -> usize
//   {
//     let player_id = self.calculate_player_id(room);
//     let mut room_state = self.get_room_state(room).unwrap();
//     room_state.players.push(PlayerState {
//       player_id,
//       player_name: "Delegate Unknown".to_string(),
//       value: None,
//     });
// 
//     self.update_room_state(room.to_string(), room_state);
// 
//     info!("Player {} joined the room.", player_id);
// 
//     player_id
//   }
// 
//   pub async fn process_client_message(
//     &self,
//     room: &str,
//     message: ClientMessage,
//   )
//   {
//     // We are locking and getting the room state within this scope to avoid
//     // having to deal with lifetimes between the lock and to send.
//     let mut state = self.game_state.lock().unwrap();
//     let room_state = state.entry(room.to_string()).or_insert(GameState {
//       players: Vec::new(),
//       all_revealed: false,
//       notify_change: NotifyChange::default(),
//     });
// 
//     match message
//     {
//       ClientMessage::Pong {
//         player_id,
//       } =>
//       {
//         if let Some(_player) =
//           room_state.players.iter_mut().find(|p| p.player_id == player_id)
//         {
//           debug!("Player {} ponged.", player_id);
//         }
//         // TODO: Handle the pong. Currently, nothing happens if a ping is
//         // ignored.
//       },
//       ClientMessage::ChangeValue {
//         player_id,
//         value,
//       } =>
//       {
//         if let Some(player) =
//           room_state.players.iter_mut().find(|p| p.player_id == player_id)
//         {
//           player.value = Some(value);
//         }
//       },
//       ClientMessage::ChangeName {
//         player_id,
//         name,
//       } =>
//       {
//         if let Some(player) =
//           room_state.players.iter_mut().find(|p| p.player_id == player_id)
//         {
//           player.player_name = name;
//         }
//       },
//       ClientMessage::RevealNumbers {
//         value,
//       } =>
//       {
//         // Only zero out the values if the user wants to reset and the previous
//         // state was revealed.
//         if !value && room_state.all_revealed
//         {
//           for player in &mut room_state.players
//           {
//             if player.value.is_some()
//             {
//               player.value = Some(0);
//             }
//           }
//         }
//         // Update the state
//         room_state.all_revealed = value;
//       },
//     }
//   }
// }
// 
// #[cfg(test)]
// mod test
// {
//   use super::*;
// 
//   fn setup() -> &'static Game
//   {
//     let game = Game::instance();
//     let room_name = "TestRoom";
//     game.generate_new_room(Some("TestRoom"));
//     load_players(game, room_name);
//     game
//   }
// 
//   fn load_players(
//     game: &Game,
//     room: &str,
//   )
//   {
//     game.new_player(room);
//     game.new_player(room);
//     game.new_player(room);
//     game.new_player(room);
//     game.new_player(room);
//     game.new_player(room);
//     game.new_player(room);
//   }
// 
//   #[test]
//   fn test_generate_new_room()
//   {
//     let game = Game::instance();
//     let room = game.generate_new_room(None);
// 
//     info!("New Room Generated: {:?}", room);
//     assert!(room.len() > 6); // the room name is larger than 5 characters
// 
//     let room_state = game.get_room_state(&room).unwrap();
// 
//     let empty_room = GameState {
//       players: Vec::new(),
//       all_revealed: false,
//       notify_change: NotifyChange::default(),
//     };
// 
//     assert_eq!(room_state, empty_room);
//   }
// 
//   #[test]
//   fn test_create_new_room_with_name()
//   {
//     let game = Game::instance();
//     let new_room_name = "NewRoom32";
//     let room = game.generate_new_room(Some(new_room_name));
//     let room_state = game.get_room_state(&room).unwrap();
//     assert_eq!(room, new_room_name);
// 
//     let empty_room = GameState {
//       players: Vec::new(),
//       all_revealed: false,
//       notify_change: NotifyChange::default(),
//     };
// 
//     assert_eq!(room_state, empty_room);
//   }
// 
//   #[test]
//   fn test_new_players()
//   {
//     let game = setup();
//     let room = "LoadingRoom";
//     load_players(game, room);
//     let room_state = game.get_room_state(room).unwrap();
// 
//     assert_eq!(room_state.players.len(), 7);
// 
//     for i in 0..5
//     {
//       let player =
//         room_state.players.iter().find(|p| p.player_id == i).unwrap();
//       assert_eq!(player.player_name, "Delegate Unknown");
//     }
//   }
// 
//   #[tokio::test]
//   async fn process_name_change()
//   {
//     let game = setup();
//     let room = "TestRoom";
//     load_players(game, room);
// 
//     let name_change = ClientMessage::ChangeName {
//       player_id: 0,
//       name: "Test Name".to_string(),
//     };
// 
//     game.process_client_message(room, name_change).await;
// 
//     let room_state = game.get_room_state(room).unwrap();
//     let player = room_state.players.iter().find(|p| p.player_id == 0).unwrap();
//     assert_eq!(player.player_name, "Test Name");
//   }
// 
//   #[tokio::test]
//   async fn player_changes_value()
//   {
//     let game = setup();
//     let room = "TestRoom";
//     load_players(game, room);
// 
//     let value_change = ClientMessage::ChangeValue {
//       player_id: 3,
//       value: 5,
//     };
// 
//     game.process_client_message(room, value_change).await;
// 
//     let room_state = game.get_room_state(room).unwrap();
//     let player = room_state.players.iter().find(|p| p.player_id == 3).unwrap();
//     assert_eq!(player.value, Some(5));
//   }
// 
//   #[tokio::test]
//   async fn player_reveals_numbers()
//   {
//     let game = setup();
//     let room = "TestRoom";
//     load_players(game, room);
// 
//     let reveal_numbers = ClientMessage::RevealNumbers {
//       value: true,
//     };
// 
//     game.process_client_message(room, reveal_numbers).await;
// 
//     let room_state = game.get_room_state(room).unwrap();
//     assert_eq!(room_state.all_revealed, true);
// 
//     let reveal_numbers = ClientMessage::RevealNumbers {
//       value: false,
//     };
// 
//     game.process_client_message(room, reveal_numbers).await;
// 
//     let room_state = game.get_room_state(room).unwrap();
//     assert_eq!(room_state.all_revealed, false);
//   }
// 
//   #[tokio::test]
//   async fn player_leaves_room()
//   {
//     let name_of_player = "PlayerInWaiting";
//     let leaving_id: usize = 3;
//     let room = "PlayersLeavingRoom";
// 
//     let game = setup();
//     load_players(game, room);
// 
//     // Change the name of a player in waiting
//     game
//       .process_client_message(room, ClientMessage::ChangeName {
//         player_id: 16,
//         name: name_of_player.to_string(),
//       })
//       .await;
// 
//     // Remove a player
//     game.remove_player(room, leaving_id);
// 
//     let room_state = game.get_room_state(room).unwrap();
//     assert_eq!(room_state.players.len(), 6);
// 
//     for player in room_state.players.iter()
//     {
//       println!("Player: {:?}", player);
//     }
//     let player = room_state.players.iter().find(|p| p.player_id == leaving_id);
//     println!("Player: {:?}", player);
//     assert_eq!(player.unwrap().player_name, name_of_player);
//   }
// }
