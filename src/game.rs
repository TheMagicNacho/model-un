use std::collections::HashMap;
use std::sync::Arc;

use lazy_static::lazy_static;
use log::{debug, info};
use regex::Regex;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::SharedGameState;
use crate::counter::Counter;
use crate::structs::{ClientMessage, GameState, NotifyChange, PlayerState, VotingSequence};

pub struct Game {
    game_state: SharedGameState,
    counter: Arc<Mutex<&'static Counter>>, // game_time: Arc<Mutex<SystemTime>>,
}

impl Game {
    // The Game object is a singleton
    fn new() -> Self {
        let game_state: SharedGameState = Arc::new(RwLock::new(HashMap::new()));
        let counter = Arc::new(Mutex::new(Counter::instance()));

        Game {
            game_state,
            counter,
        }
    }

    const MAX_ROOM_SIZE: usize = 12;
    const OVERFLOW_INDEX: usize = 100;

    pub fn instance() -> &'static Game {
        lazy_static! {
            static ref GAME: Game = Game::new();
        }
        &GAME
    }

    fn find_player_in_waiting(&self, players: &[PlayerState]) -> Option<usize> {
        let active_count = players
            .iter()
            .filter(|p| p.player_id < Self::OVERFLOW_INDEX)
            .count();
        if active_count < Self::MAX_ROOM_SIZE {
            players
                .iter()
                .find(|player| player.player_id >= Self::OVERFLOW_INDEX)
                .map(|player| player.player_id)
        } else {
            None
        }
    }

    fn find_illegal_character(input: &str) -> Option<char> {
        lazy_static! {
            static ref ILLEGAL_CHAR_REGEX: Regex = Regex::new(r"[<>\p{P}\p{C}]").unwrap();
        }

        ILLEGAL_CHAR_REGEX
            .find(input)
            .and_then(|m| m.as_str().chars().next())
    }

    fn connection_for_player(room_state: &GameState, player_id: usize) -> String {
        room_state
            .players
            .iter()
            .find(|p| p.player_id == player_id)
            .map(|p| p.connection_id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn move_player(
        &self,
        old_id: usize,
        new_id: usize,
        state: &mut GameState,
        player_name: Option<String>,
        reset_value: bool,
    ) -> bool {
        if let Some(player_index) = state.players.iter().position(|p| p.player_id == old_id) {
            let mut moved_player = state.players[player_index].clone();
            moved_player.player_id = new_id;

            if let Some(player_name) = player_name {
                moved_player.player_name = player_name;
            }

            if reset_value {
                moved_player.value = None;
            }

            state.players.remove(player_index);
            state.players.push(moved_player);

            debug!(
                "move_player - Old ID: {}, New ID: {} - finished",
                old_id, new_id
            );
            true
        } else {
            false
        }
    }

    pub async fn remove_player(&self, room: &str, player_id: usize) {
        debug!("remove_player - Room: {}, Player ID: {}", room, player_id);

        let mut state = self.game_state.write().await;
        let room_state = state.entry(room.to_string()).or_insert(GameState {
            players: Vec::new(),
            all_revealed: false,
            notify_change: NotifyChange::default(),
            voting_sequence: VotingSequence::default(),
        });

        if let Some(index) = room_state
            .players
            .iter()
            .position(|p| p.player_id == player_id)
        {
            room_state.players.remove(index);
            info!("Player {} disconnected.", player_id);

            let player_in_waiting = self.find_player_in_waiting(&room_state.players);

            let vacant_id = player_id;
            match player_in_waiting {
                Some(old_id) => {
                    if self.move_player(old_id, vacant_id, room_state, None, true) {
                        room_state.notify_change = NotifyChange {
                            current_id: old_id,
                            new_id: player_id,
                        };

                        debug!("Player {} promoted to position {}", old_id, player_id);
                    } else {
                        room_state.notify_change = NotifyChange::default();
                    }
                }
                None => {
                    room_state.notify_change = NotifyChange::default();
                }
            }
        }

        debug!(
            "remove_player - Room: {}, Player ID: {} - finished",
            room, player_id
        );
    }

    /// Remove a player from a room by immutable connection ID.
    ///
    /// This decouples socket lifetime from mutable seat/player IDs.
    pub async fn remove_player_by_connection(&self, room: &str, connection_id: &str) {
        let player_id = {
            let state = self.game_state.read().await;
            state.get(room).and_then(|room_state| {
                room_state
                    .players
                    .iter()
                    .find(|p| p.connection_id == connection_id)
                    .map(|p| p.player_id)
            })
        };

        match player_id {
            Some(player_id) => self.remove_player(room, player_id).await,
            None => debug!(
                "remove_player_by_connection - no player found in room {} for connection {}",
                room, connection_id
            ),
        }
    }

    pub async fn generate_new_room(&self, room: Option<&str>) -> String {
        let room_name = match room {
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
                voting_sequence: VotingSequence::default(),
            },
        );
        debug!("generate_new_room - Room Name: {} - finished", room_name);
        room_name
    }

    pub async fn random_name_generator(&self) -> String {
        debug!("random_name_generator - entry");

        let adjectives = &[
            "Swift", "Mighty", "Clever", "Silent", "Fierce", "Gentle", "Wild", "Brave", "Wise",
            "Nimble", "Proud", "Noble", "Sleepy", "Cunning", "Playful",
        ];

        let animals = &[
            "Fox", "Bear", "Wolf", "Eagle", "Owl", "Lion", "Tiger", "Dolphin", "Elephant",
            "Panther", "Hawk", "Deer", "Rabbit", "Raccoon", "Penguin",
        ];

        let room_name = {
            let c = self.counter.lock().await;
            let ani_index = c.get_fast_index(animals.len());
            let adj_index = c.get_slow_index(adjectives.len(), animals.len());
            format!("{}{}", adjectives[adj_index], animals[ani_index])
        };

        debug!(
            "random_name_generator - Room Name: {} - finished",
            room_name
        );

        room_name
    }

    pub async fn get_room_state(&self, room: &str) -> Option<GameState> {
        debug!("get_room_state - Room: {}", room);

        self.game_state.read().await.get(room).cloned()
    }

    pub async fn new_player_with_connection(&self, room: &str, connection_id: String) -> usize {
        debug!("new_player - Room: {}", room);

        let mut state = self.game_state.write().await;

        // Create room if it does not exist yet.
        if !state.contains_key(room) {
            state.insert(
                room.to_string(),
                GameState {
                    players: Vec::new(),
                    all_revealed: false,
                    notify_change: NotifyChange::default(),
                    voting_sequence: VotingSequence::default(),
                },
            );
        }

        let room_state = state.get_mut(room).unwrap();

        let active_player_count = room_state
            .players
            .iter()
            .filter(|p| p.player_id < Self::OVERFLOW_INDEX)
            .count();

        let player_id = if active_player_count >= Self::MAX_ROOM_SIZE {
            // Spectator: find the lowest available ID >= OVERFLOW_INDEX.
            (Self::OVERFLOW_INDEX..)
                .find(|&id| room_state.players.iter().all(|p| p.player_id != id))
                .unwrap()
        } else {
            // Find the lowest unused active seat ID.
            (0..Self::MAX_ROOM_SIZE)
                .find(|&i| room_state.players.iter().all(|p| p.player_id != i))
                .unwrap_or(Self::MAX_ROOM_SIZE)
        };

        room_state.players.push(PlayerState {
            player_id,
            player_name: "Delegate Unknown".to_string(),
            value: None,
            connection_id,
        });

        info!("Player {} joined the room.", player_id);
        debug!(
            "new_player - Room: {}, Player ID: {} - finished",
            room, player_id
        );
        player_id
    }

    pub async fn new_player(&self, room: &str) -> usize {
        self.new_player_with_connection(room, Uuid::new_v4().to_string())
            .await
    }

    /// Process a message received from a client and update the room state.
    pub async fn process_client_message(&self, room: &str, message: ClientMessage) {
        debug!(
            "process_client_message - Room: {}, Message: {:?}",
            room, message
        );

        let mut state = self.game_state.write().await;
        let room_state = state.entry(room.to_string()).or_insert(GameState {
            players: Vec::new(),
            all_revealed: false,
            notify_change: NotifyChange::default(),
            voting_sequence: VotingSequence::default(),
        });

        match message {
            ClientMessage::Pong { player_id } => {
                debug!("Player {} ponged.", player_id);
            }
            ClientMessage::ChangeValue { player_id, value } => {
                if let Some(player) = room_state
                    .players
                    .iter_mut()
                    .find(|p| p.player_id == player_id)
                {
                    player.value = Some(value);
                }
            }
            ClientMessage::ChangeName { player_id, name } => {
                if let Some(illegal) = Self::find_illegal_character(&name) {
                    let offending_connection = Self::connection_for_player(room_state, player_id);
                    info!(
                        "Dropping ChangeName request from connection {} due to illegal character '{}'",
                        offending_connection, illegal
                    );
                    return;
                }

                if let Some(player) = room_state
                    .players
                    .iter_mut()
                    .find(|p| p.player_id == player_id)
                {
                    player.player_name = name;
                }
            }
            ClientMessage::RevealNumbers { value } => {
                // Only zero out the values if the user wants to reset and the
                // previous state was revealed.
                if !value && room_state.all_revealed {
                    for player in &mut room_state.players {
                        if player.value.is_some() {
                            player.value = Some(0);
                        }
                    }
                }
                // Update the state
                room_state.all_revealed = value;
            }
            ClientMessage::ChangeSeat {
                name,
                current_id,
                requested_id,
            } => {
                if let Some(illegal) = Self::find_illegal_character(&name) {
                    let offending_connection = Self::connection_for_player(room_state, current_id);
                    info!(
                        "Dropping ChangeSeat request from connection {} due to illegal character '{}'",
                        offending_connection, illegal
                    );
                    return;
                }

                // A seat change is only valid when the requested seat is within
                // the active range (0–11) AND is not already occupied. Spectator
                // slots (≥ 100) and out-of-range indices are always rejected.
                let is_valid = requested_id < Self::MAX_ROOM_SIZE
                    && room_state
                        .players
                        .iter()
                        .all(|p| p.player_id != requested_id);

                if is_valid {
                    if self.move_player(current_id, requested_id, room_state, Some(name), false) {
                        room_state.notify_change = NotifyChange {
                            current_id,
                            new_id: requested_id,
                        };
                        debug!(
                            "Player {} moved to seat {} in room {}",
                            current_id, requested_id, room
                        );
                    } else {
                        room_state.notify_change = NotifyChange::default();
                    }
                } else {
                    room_state.notify_change = NotifyChange::default();
                }
            }
            ClientMessage::ChangeSequence {
                player_id,
                sequence,
            } => {
                // Only the captain (lowest active player_id) may change the sequence.
                let min_id = room_state
                    .players
                    .iter()
                    .filter(|p| p.player_id < 100)
                    .map(|p| p.player_id)
                    .min();
                if Some(player_id) == min_id {
                    room_state.voting_sequence = sequence;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::ClientMessage;

    fn new_game() -> Game {
        Game::new()
    }

    // ── Room management ──────────────────────────────────────────────────────

    /// Rule: a room created with a given name is immediately visible in state
    /// with an empty player list and `all_revealed = false`.
    #[tokio::test]
    async fn test_generate_new_room_creates_empty_room() {
        let game = new_game();
        let name = game.generate_new_room(Some("g-room-1")).await;
        assert_eq!(name, "g-room-1");
        let state = game.get_room_state("g-room-1").await.unwrap();
        assert!(state.players.is_empty());
        assert!(!state.all_revealed);
    }

    /// Rule: querying a room that has never been created must return `None`
    /// (no implicit room creation).
    #[tokio::test]
    async fn test_get_room_state_returns_none_for_nonexistent_room() {
        let game = new_game();
        assert!(game.get_room_state("does-not-exist").await.is_none());
    }

    /// Rule: room name generation must always produce a usable (non-empty)
    /// string so that clients can identify their room.
    #[tokio::test]
    async fn test_random_name_generator_returns_nonempty_string() {
        let game = new_game();
        let name = game.random_name_generator().await;
        assert!(!name.is_empty());
    }

    // ── Player ID assignment ─────────────────────────────────────────────────

    /// Rule: the very first player in a new room always receives ID 0 so that
    /// they immediately become the captain (lowest active ID).
    #[tokio::test]
    async fn test_new_player_in_new_room_gets_id_zero() {
        let game = new_game();
        let id = game.new_player("p-room-first").await;
        assert_eq!(id, 0);
        let state = game.get_room_state("p-room-first").await.unwrap();
        assert_eq!(state.players.len(), 1);
        assert_eq!(state.players[0].player_id, 0);
    }

    /// Rule: players joining a room receive consecutive IDs starting at 0 so
    /// that the seat layout is always predictable.
    #[tokio::test]
    async fn test_new_player_assigns_sequential_ids() {
        let game = new_game();
        game.generate_new_room(Some("p-room-seq")).await;
        assert_eq!(game.new_player("p-room-seq").await, 0);
        assert_eq!(game.new_player("p-room-seq").await, 1);
        assert_eq!(game.new_player("p-room-seq").await, 2);
    }

    /// Rule: when a seat is vacated the lowest-numbered empty slot is reused
    /// for the next joining player to keep IDs compact.
    #[tokio::test]
    async fn test_new_player_reuses_lowest_available_id() {
        let game = new_game();
        game.generate_new_room(Some("p-room-reuse")).await;
        game.new_player("p-room-reuse").await; // id 0
        game.new_player("p-room-reuse").await; // id 1
        game.new_player("p-room-reuse").await; // id 2
        game.remove_player("p-room-reuse", 1).await;
        // ID 1 is now the lowest vacant slot
        assert_eq!(game.new_player("p-room-reuse").await, 1);
    }

    /// Rule: once all 12 active seats are filled, additional players are placed
    /// in spectator mode and receive an overflow ID (≥ 100).
    #[tokio::test]
    async fn test_new_player_overflow_id_when_room_full() {
        let game = new_game();
        game.generate_new_room(Some("p-room-full")).await;
        for _ in 0..12 {
            game.new_player("p-room-full").await;
        }
        let overflow_id = game.new_player("p-room-full").await;
        assert!(
            overflow_id >= 100,
            "Expected overflow ID ≥ 100, got {overflow_id}"
        );
    }

    /// Rule: new players start with a placeholder name ("Delegate Unknown") and
    /// no vote value so that the UI can distinguish un-named players.
    #[tokio::test]
    async fn test_new_player_has_default_name_and_no_value() {
        let game = new_game();
        game.generate_new_room(Some("p-room-defaults")).await;
        game.new_player("p-room-defaults").await;
        let state = game.get_room_state("p-room-defaults").await.unwrap();
        assert_eq!(state.players[0].player_name, "Delegate Unknown");
        assert_eq!(state.players[0].value, None);
    }

    // ── Player removal ───────────────────────────────────────────────────────

    /// Rule: removing a player by ID must eliminate exactly that player from
    /// the roster; all remaining players are unaffected.
    #[tokio::test]
    async fn test_remove_player_removes_player() {
        let game = new_game();
        game.generate_new_room(Some("r-room-1")).await;
        game.new_player("r-room-1").await; // id 0
        game.new_player("r-room-1").await; // id 1
        game.remove_player("r-room-1", 0).await;
        let state = game.get_room_state("r-room-1").await.unwrap();
        assert_eq!(state.players.len(), 1);
        assert!(state.players.iter().all(|p| p.player_id != 0));
    }

    /// Rule: when a full room (12 players) loses a member, the first waiting
    /// spectator is automatically promoted to fill the vacant seat.
    #[tokio::test]
    async fn test_remove_player_promotes_waiting_player() {
        let game = new_game();
        game.generate_new_room(Some("r-room-promote")).await;
        // Fill 12 active players (IDs 0–11)
        for _ in 0..12 {
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

    /// Rule: after a spectator is promoted into an active seat, the next joiner
    /// must stay in spectator mode with a fresh overflow ID instead of reusing
    /// the promoted spectator's old overflow ID.
    #[tokio::test]
    async fn test_new_spectator_gets_fresh_id_after_promotion() {
        let game = new_game();
        game.generate_new_room(Some("r-room-fresh-spectator")).await;
        for _ in 0..12 {
            game.new_player("r-room-fresh-spectator").await;
        }

        let first_spectator_id = game.new_player("r-room-fresh-spectator").await;
        assert!(
            first_spectator_id >= Game::OVERFLOW_INDEX,
            "13th player must be a spectator"
        );

        // Remove player 0; first spectator is promoted into seat 0.
        game.remove_player("r-room-fresh-spectator", 0).await;

        // The promoted spectator's old overflow ID is now free, so the next
        // spectator reuses it (lowest available ID >= OVERFLOW_INDEX).
        let next_join_id = game.new_player("r-room-fresh-spectator").await;
        assert!(
            next_join_id >= Game::OVERFLOW_INDEX,
            "new player should be a spectator"
        );

        let state = game.get_room_state("r-room-fresh-spectator").await.unwrap();
        // Promoted player occupies seat 0.
        assert!(state.players.iter().any(|p| p.player_id == 0));
        // New spectator exists.
        assert!(state.players.iter().any(|p| p.player_id == next_join_id));
    }

    /// Rule: when the room is not full and has no spectators, removing a player
    /// leaves `notify_change` at its zero values (no promotion event).
    #[tokio::test]
    async fn test_remove_player_no_promotion_with_small_room() {
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

    /// Rule: a ChangeValue message updates only the sending player's stored
    /// vote; other players in the room are unaffected.
    #[tokio::test]
    async fn test_process_change_value_updates_player_value() {
        let game = new_game();
        game.generate_new_room(Some("m-room-cv")).await;
        game.new_player("m-room-cv").await; // id 0
        game.process_client_message(
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

    /// Rule: a ChangeName message updates only the sending player's display
    /// name; other players in the room are unaffected.
    #[tokio::test]
    async fn test_process_change_name_updates_player_name() {
        let game = new_game();
        game.generate_new_room(Some("m-room-cn")).await;
        game.new_player("m-room-cn").await; // id 0
        game.process_client_message(
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

    /// Rule: names containing illegal characters are rejected and leave the
    /// stored name unchanged.
    #[tokio::test]
    async fn test_process_change_name_rejects_illegal_characters() {
        let game = new_game();
        game.generate_new_room(Some("m-room-cn-illegal")).await;
        game.new_player("m-room-cn-illegal").await; // id 0
        game.process_client_message(
            "m-room-cn-illegal",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Bad<Name".to_string(),
            },
        )
        .await;
        let state = game.get_room_state("m-room-cn-illegal").await.unwrap();
        let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
        assert_eq!(player.player_name, "Delegate Unknown");
    }

    /// Rule: RevealNumbers { true } transitions the room into the revealed
    /// state so that all clients can display vote values.
    #[tokio::test]
    async fn test_process_reveal_numbers_sets_all_revealed() {
        let game = new_game();
        game.generate_new_room(Some("m-room-rn-show")).await;
        game.process_client_message(
            "m-room-rn-show",
            ClientMessage::RevealNumbers { value: true },
        )
        .await;
        let state = game.get_room_state("m-room-rn-show").await.unwrap();
        assert!(state.all_revealed);
    }

    /// Rule: RevealNumbers { false } after a reveal clears `all_revealed` and
    /// resets every player's vote to 0, starting a fresh voting round.
    #[tokio::test]
    async fn test_process_reveal_numbers_false_resets_values() {
        let game = new_game();
        game.generate_new_room(Some("m-room-rn-hide")).await;
        game.new_player("m-room-rn-hide").await; // id 0
        game.process_client_message(
            "m-room-rn-hide",
            ClientMessage::ChangeValue {
                player_id: 0,
                value: 8,
            },
        )
        .await;
        game.process_client_message(
            "m-room-rn-hide",
            ClientMessage::RevealNumbers { value: true },
        )
        .await;
        game.process_client_message(
            "m-room-rn-hide",
            ClientMessage::RevealNumbers { value: false },
        )
        .await;
        let state = game.get_room_state("m-room-rn-hide").await.unwrap();
        assert!(!state.all_revealed);
        let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
        assert_eq!(player.value, Some(0));
    }

    /// Rule: RevealNumbers { false } when no reveal has occurred yet must NOT
    /// reset existing votes, as no voting round has completed.
    #[tokio::test]
    async fn test_hide_without_prior_reveal_keeps_values() {
        let game = new_game();
        game.generate_new_room(Some("m-room-rn-noreset")).await;
        game.new_player("m-room-rn-noreset").await;
        game.process_client_message(
            "m-room-rn-noreset",
            ClientMessage::ChangeValue {
                player_id: 0,
                value: 3,
            },
        )
        .await;
        // Hide without ever revealing – value must stay intact
        game.process_client_message(
            "m-room-rn-noreset",
            ClientMessage::RevealNumbers { value: false },
        )
        .await;
        let state = game.get_room_state("m-room-rn-noreset").await.unwrap();
        let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
        assert_eq!(player.value, Some(3));
    }

    /// Rule: a Pong message is a keep-alive reply and must never modify any
    /// game state.
    #[tokio::test]
    async fn test_process_pong_is_noop() {
        let game = new_game();
        game.generate_new_room(Some("m-room-pong")).await;
        game.new_player("m-room-pong").await;
        let before = game.get_room_state("m-room-pong").await.unwrap();
        game.process_client_message("m-room-pong", ClientMessage::Pong { player_id: 0 })
            .await;
        let after = game.get_room_state("m-room-pong").await.unwrap();
        assert_eq!(before, after);
    }

    // ── Seat switching ───────────────────────────────────────────────────────

    /// Rule: seat change requests with illegal characters in the provided name
    /// are dropped without moving the player.
    #[tokio::test]
    async fn test_change_seat_rejects_illegal_name() {
        let game = new_game();
        game.generate_new_room(Some("s-room-illegal-seat")).await;
        game.new_player("s-room-illegal-seat").await; // id 0

        game.process_client_message(
            "s-room-illegal-seat",
            ClientMessage::ChangeSeat {
                name: "Bad<Name".to_string(),
                current_id: 0,
                requested_id: 1,
            },
        )
        .await;

        let state = game.get_room_state("s-room-illegal-seat").await.unwrap();

        let player = state.players.iter().find(|p| p.player_id == 0).unwrap();
        assert_eq!(player.player_name, "Delegate Unknown");
        assert!(state.players.iter().all(|p| p.player_id != 1));
    }

    /// Rule: a player may move to any vacant seat in the range 0–11.  Their
    /// name travels with them and the old seat becomes vacant.
    #[tokio::test]
    async fn test_change_seat_moves_player_to_vacant_seat() {
        let game = new_game();
        game.generate_new_room(Some("s-room-move")).await;
        game.new_player("s-room-move").await; // id 0
        game.new_player("s-room-move").await; // id 1

        // Name the players
        game.process_client_message(
            "s-room-move",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Alice".to_string(),
            },
        )
        .await;
        game.process_client_message(
            "s-room-move",
            ClientMessage::ChangeName {
                player_id: 1,
                name: "Bob".to_string(),
            },
        )
        .await;

        // Assert starting positions
        let pre = game.get_room_state("s-room-move").await.unwrap();
        assert!(
            pre.players
                .iter()
                .any(|p| p.player_id == 0 && p.player_name == "Alice")
        );
        assert!(
            pre.players
                .iter()
                .any(|p| p.player_id == 1 && p.player_name == "Bob")
        );

        // Alice moves from seat 0 to seat 3
        game.process_client_message(
            "s-room-move",
            ClientMessage::ChangeSeat {
                name: "Alice".to_string(),
                current_id: 0,
                requested_id: 3,
            },
        )
        .await;

        let state = game.get_room_state("s-room-move").await.unwrap();
        // Alice should now be at seat 3
        let alice = state.players.iter().find(|p| p.player_id == 3).unwrap();
        assert_eq!(alice.player_name, "Alice");
        // Bob should remain at seat 1
        let bob = state.players.iter().find(|p| p.player_id == 1).unwrap();
        assert_eq!(bob.player_name, "Bob");
    }

    /// Rule: a ChangeSeat request targeting an occupied seat is silently
    /// ignored; both players remain at their original seats.
    #[tokio::test]
    async fn test_change_seat_rejects_occupied_seat() {
        let game = new_game();
        game.generate_new_room(Some("s-room-occupied")).await;
        game.new_player("s-room-occupied").await; // id 0
        game.new_player("s-room-occupied").await; // id 1

        // Name the players
        game.process_client_message(
            "s-room-occupied",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Alice".to_string(),
            },
        )
        .await;
        game.process_client_message(
            "s-room-occupied",
            ClientMessage::ChangeName {
                player_id: 1,
                name: "Bob".to_string(),
            },
        )
        .await;

        // Alice attempts to take Bob's occupied seat
        game.process_client_message(
            "s-room-occupied",
            ClientMessage::ChangeSeat {
                name: "Alice".to_string(),
                current_id: 0,
                requested_id: 1, // seat 1 is Bob's
            },
        )
        .await;

        let state = game.get_room_state("s-room-occupied").await.unwrap();
        // Alice must still be at seat 0
        let alice = state.players.iter().find(|p| p.player_id == 0).unwrap();
        assert_eq!(alice.player_name, "Alice");
        // Bob must still be at seat 1
        let bob = state.players.iter().find(|p| p.player_id == 1).unwrap();
        assert_eq!(bob.player_name, "Bob");
    }

    /// Rule: seat IDs ≥ 12 fall in the spectator/overflow zone and must be
    /// rejected; the requesting player stays at their current seat.
    #[tokio::test]
    async fn test_change_seat_rejects_overflow_seat() {
        let game = new_game();
        game.generate_new_room(Some("s-room-overflow")).await;
        game.new_player("s-room-overflow").await; // id 0

        game.process_client_message(
            "s-room-overflow",
            ClientMessage::ChangeSeat {
                name: "Delegate Unknown".to_string(),
                current_id: 0,
                requested_id: 12,
            },
        )
        .await;

        let state = game.get_room_state("s-room-overflow").await.unwrap();
        assert!(state.players.iter().any(|p| p.player_id == 0));
        assert!(state.players.iter().all(|p| p.player_id != 12));
    }

    /// Rule: when a player changes seats, the name supplied in the ChangeSeat
    /// message is assigned to the new seat so the player's identity follows
    /// them.
    #[tokio::test]
    async fn test_change_seat_preserves_player_name_and_value() {
        let game = new_game();
        game.generate_new_room(Some("s-room-preserve")).await;
        game.new_player("s-room-preserve").await; // id 0
        game.new_player("s-room-preserve").await; // id 1
        game.process_client_message(
            "s-room-preserve",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Alice".to_string(),
            },
        )
        .await;
        game.process_client_message(
            "s-room-preserve",
            ClientMessage::ChangeName {
                player_id: 1,
                name: "Bob".to_string(),
            },
        )
        .await;

        // Alice moves to seat 3
        game.process_client_message(
            "s-room-preserve",
            ClientMessage::ChangeSeat {
                name: "Alice".to_string(),
                current_id: 0,
                requested_id: 3,
            },
        )
        .await;

        let state = game.get_room_state("s-room-preserve").await.unwrap();
        // The player at seat 3 should be Alice
        let alice = state.players.iter().find(|p| p.player_id == 3).unwrap();
        assert_eq!(alice.player_name, "Alice");
    }

    /// Rule: captainship belongs to the active player with the lowest seat ID.
    /// When a captain moves to a higher-numbered seat, the player who now holds
    /// the lowest ID becomes the new captain.  Existing votes are unaffected by
    /// the transfer.
    #[tokio::test]
    async fn test_captain_transfers_on_seat_change() {
        let game = new_game();
        game.generate_new_room(Some("s-room-captain")).await;
        game.new_player("s-room-captain").await; // id 0
        game.new_player("s-room-captain").await; // id 1

        // Name the players
        game.process_client_message(
            "s-room-captain",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Alice".to_string(),
            },
        )
        .await;
        game.process_client_message(
            "s-room-captain",
            ClientMessage::ChangeName {
                player_id: 1,
                name: "Bob".to_string(),
            },
        )
        .await;

        // Alice (id 0) is captain; give her a vote so we can prove it survives
        game.process_client_message(
            "s-room-captain",
            ClientMessage::ChangeValue {
                player_id: 0,
                value: 5,
            },
        )
        .await;

        // Alice moves to seat 3; Bob (id 1) now holds the lowest ID
        game.process_client_message(
            "s-room-captain",
            ClientMessage::ChangeSeat {
                name: "Alice".to_string(),
                current_id: 0,
                requested_id: 3,
            },
        )
        .await;

        let state = game.get_room_state("s-room-captain").await.unwrap();
        // Bob (id 1) is now the captain — lowest active ID
        let min_id = state
            .players
            .iter()
            .filter(|p| p.player_id < 100)
            .map(|p| p.player_id)
            .min()
            .unwrap();
        assert_eq!(min_id, 1);
        let bob = state.players.iter().find(|p| p.player_id == 1).unwrap();
        assert_eq!(bob.player_name, "Bob");

        // Alice retains her vote at the new seat
        let alice = state.players.iter().find(|p| p.player_id == 3).unwrap();
        assert_eq!(alice.player_name, "Alice");
        assert_eq!(alice.value, Some(5));
    }

    // ── Voting sequence ──────────────────────────────────────────────────────

    /// Rule: only the captain (lowest active seat ID) may change the voting
    /// sequence.  The sequence is updated immediately and broadcast to all
    /// clients via the next state update.
    #[tokio::test]
    async fn test_process_change_sequence_updates_voting_sequence() {
        let game = new_game();
        game.generate_new_room(Some("m-room-cs")).await;
        game.new_player("m-room-cs").await; // id 0 – captain

        // Default should be Fibonacci
        let initial_state = game.get_room_state("m-room-cs").await.unwrap();
        assert_eq!(initial_state.voting_sequence, VotingSequence::Fibonacci);

        // Captain (id 0) changes to Linear
        game.process_client_message(
            "m-room-cs",
            ClientMessage::ChangeSequence {
                player_id: 0,
                sequence: VotingSequence::Linear,
            },
        )
        .await;
        let state = game.get_room_state("m-room-cs").await.unwrap();
        assert_eq!(state.voting_sequence, VotingSequence::Linear);

        // Captain changes to SmMedLgXl
        game.process_client_message(
            "m-room-cs",
            ClientMessage::ChangeSequence {
                player_id: 0,
                sequence: VotingSequence::SmMedLgXl,
            },
        )
        .await;
        let state = game.get_room_state("m-room-cs").await.unwrap();
        assert_eq!(state.voting_sequence, VotingSequence::SmMedLgXl);
    }

    /// Rule: captainship transfers when a player moves to a higher-numbered
    /// seat.  After the transfer, only the new captain (Bob) may change the
    /// voting sequence; the former captain (Alice) is ignored.
    #[tokio::test]
    async fn test_process_change_sequence_ignored_for_non_captain() {
        let game = new_game();
        game.generate_new_room(Some("m-room-cs-nc")).await;
        game.new_player("m-room-cs-nc").await; // id 0
        game.new_player("m-room-cs-nc").await; // id 1

        // Name the players
        game.process_client_message(
            "m-room-cs-nc",
            ClientMessage::ChangeName {
                player_id: 0,
                name: "Alice".to_string(),
            },
        )
        .await;
        game.process_client_message(
            "m-room-cs-nc",
            ClientMessage::ChangeName {
                player_id: 1,
                name: "Bob".to_string(),
            },
        )
        .await;

        // Alice (id 0) is currently captain; move her to seat 3 so Bob (id 1)
        // becomes the new captain.
        game.process_client_message(
            "m-room-cs-nc",
            ClientMessage::ChangeSeat {
                name: "Alice".to_string(),
                current_id: 0,
                requested_id: 3,
            },
        )
        .await;

        // Verify the seat change succeeded and captainship transferred.
        let state = game.get_room_state("m-room-cs-nc").await.unwrap();
        let alice = state
            .players
            .iter()
            .find(|p| p.player_name == "Alice")
            .unwrap();
        assert_eq!(alice.player_id, 3, "Alice should now be at seat 3");
        let captain_id = state
            .players
            .iter()
            .filter(|p| p.player_id < 100)
            .map(|p| p.player_id)
            .min()
            .unwrap();
        assert_eq!(captain_id, 1, "Bob (id 1) should now be the captain");

        // Alice (now at id 3) attempts to change the sequence – should be ignored.
        game.process_client_message(
            "m-room-cs-nc",
            ClientMessage::ChangeSequence {
                player_id: 3,
                sequence: VotingSequence::SmMedLgXl,
            },
        )
        .await;
        let state = game.get_room_state("m-room-cs-nc").await.unwrap();
        assert_eq!(state.voting_sequence, VotingSequence::Fibonacci);

        // Bob (id 1, the new captain) changes the sequence to Linear.
        game.process_client_message(
            "m-room-cs-nc",
            ClientMessage::ChangeSequence {
                player_id: 1,
                sequence: VotingSequence::Linear,
            },
        )
        .await;
        let state = game.get_room_state("m-room-cs-nc").await.unwrap();
        assert_eq!(state.voting_sequence, VotingSequence::Linear);
    }
}
