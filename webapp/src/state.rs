use automatafl_api_types::{GameStateResponse, GameEvent};
use automatafl_logic::Coord;
use leptos::prelude::*;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AppState {
    pub current_player_id: RwSignal<Option<Uuid>>,
    pub session_token: RwSignal<Option<Uuid>>,
    pub games: RwSignal<HashMap<Uuid, GameStateResponse>>,
    pub websocket_connected: RwSignal<bool>,
    pub active_game_id: RwSignal<Option<Uuid>>,
    pub matchmaking_status: RwSignal<MatchmakingState>,
    pub api_base_url: String,
    pub ws_base_url: String,
    pub game_refresh_trigger: RwSignal<HashMap<Uuid, u32>>,
    pub move_events: RwSignal<HashMap<Uuid, Vec<MoveEvent>>>,
    pub conflict_events: RwSignal<HashMap<Uuid, Vec<ConflictEvent>>>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub enum MatchmakingState {
    #[default]
    NotInQueue,
    InQueue {
        estimated_wait: Option<u32>,
        players_in_queue: u32,
    },
    MatchFound {
        game_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveEvent {
    pub from: Coord,
    pub to: Coord,
    pub player_id: u8,
    pub success: bool,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConflictEvent {
    pub coord: Coord,
    pub players: Vec<u8>,
    pub timestamp: u64,
}

impl AppState {
    pub fn new() -> Self {
        // Get base URLs from environment or use defaults
        let api_base_url = option_env!("API_BASE_URL")
            .unwrap_or("http://localhost:3000")
            .to_string();
        
        let ws_base_url = option_env!("WS_BASE_URL")
            .unwrap_or("ws://localhost:3000")
            .to_string();

        // Check localStorage for saved session
        let (current_player_id, session_token) = if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let session = storage
                    .get_item("session_token")
                    .ok()
                    .flatten()
                    .and_then(|s| Uuid::parse_str(&s).ok());
                let player = storage
                    .get_item("player_id")
                    .ok()
                    .flatten()
                    .and_then(|s| Uuid::parse_str(&s).ok());
                (player, session)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Self {
            current_player_id: RwSignal::new(current_player_id),
            session_token: RwSignal::new(session_token),
            games: RwSignal::new(HashMap::new()),
            websocket_connected: RwSignal::new(false),
            active_game_id: RwSignal::new(None),
            matchmaking_status: RwSignal::new(MatchmakingState::default()),
            api_base_url,
            ws_base_url,
            game_refresh_trigger: RwSignal::new(HashMap::new()),
            move_events: RwSignal::new(HashMap::new()),
            conflict_events: RwSignal::new(HashMap::new()),
        }
    }

    pub fn login(&self, session_token: Uuid, player_id: Uuid) {
        self.session_token.set(Some(session_token));
        self.current_player_id.set(Some(player_id));
        
        // Save to localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("session_token", &session_token.to_string());
                let _ = storage.set_item("player_id", &player_id.to_string());
            }
        }
    }

    pub fn logout(&self) {
        self.session_token.set(None);
        self.current_player_id.set(None);
        self.active_game_id.set(None);
        self.matchmaking_status.set(MatchmakingState::NotInQueue);
        
        // Clear localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.remove_item("session_token");
                let _ = storage.remove_item("player_id");
            }
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.session_token.get().is_some()
    }

    #[allow(dead_code)]
    pub fn update_game(&self, _game: GameStateResponse) {
        // Extract game ID from the first player or create a placeholder
        // Note: In the new API, games don't have their own ID in GameStateResponse
        // We need to track this differently - through the active_game_id
        // For now, we'll use a workaround by checking player_ids keys
        
        // Since GameStateResponse doesn't have an ID field, we need to handle this differently
        // The calling code should manage game IDs
        // This is a limitation we'll need to address
        web_sys::console::warn_1(&"update_game called but GameStateResponse has no ID field".into());
    }

    #[allow(dead_code)]
    pub fn set_active_game(&self, game_id: Option<Uuid>) {
        self.active_game_id.set(game_id);
    }

    pub fn register_game_refresh(&self, game_id: Uuid) {
        self.game_refresh_trigger.update(|triggers| {
            triggers.insert(game_id, 0);
        });
    }

    #[allow(dead_code)]
    pub fn unregister_game_refresh(&self, game_id: Uuid) {
        self.game_refresh_trigger.update(|triggers| {
            triggers.remove(&game_id);
        });
    }

    pub fn trigger_game_refresh(&self, game_id: Uuid) {
        self.game_refresh_trigger.update(|triggers| {
            if let Some(count) = triggers.get_mut(&game_id) {
                *count += 1;
            }
        });
    }

    pub fn get_game_refresh_trigger(&self, game_id: Uuid) -> u32 {
        self.game_refresh_trigger.get().get(&game_id).copied().unwrap_or(0)
    }

    pub fn add_move_event(&self, game_id: Uuid, event: MoveEvent) {
        self.move_events.update(|events| {
            let game_events = events.entry(game_id).or_insert_with(Vec::new);
            game_events.push(event);
            // Keep only recent events (last 10)
            if game_events.len() > 10 {
                game_events.remove(0);
            }
        });
    }

    pub fn add_conflict_event(&self, game_id: Uuid, event: ConflictEvent) {
        self.conflict_events.update(|events| {
            let game_events = events.entry(game_id).or_insert_with(Vec::new);
            game_events.push(event);
            // Keep only recent events (last 5)
            if game_events.len() > 5 {
                game_events.remove(0);
            }
        });
    }

    pub fn get_move_events(&self, game_id: Uuid) -> Vec<MoveEvent> {
        self.move_events.get().get(&game_id).cloned().unwrap_or_default()
    }

    pub fn get_conflict_events(&self, game_id: Uuid) -> Vec<ConflictEvent> {
        self.conflict_events.get().get(&game_id).cloned().unwrap_or_default()
    }

    pub fn handle_game_event(&self, game_id: Uuid, event: GameEvent) {
        use automatafl_api_types::GameEventData;
        use automatafl_logic::MoveResult;
        
        // Handle different event types
        match &event.data {
            GameEventData::PlayerJoined { displayname, .. } => {
                web_sys::console::log_1(&format!("Player {} joined", displayname).into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::GameStarted => {
                web_sys::console::log_1(&"Game started event".into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::MoveAcknowledged { player_pid, .. } => {
                web_sys::console::log_1(&format!("Player {} move acknowledged", player_pid.0).into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::MoveInvalid { player_pid, feedback } => {
                web_sys::console::warn_1(&format!("Invalid move from player {}: {:?}", player_pid.0, feedback).into());
            }
            GameEventData::Move { player_pid, from, to, result } => {
                web_sys::console::log_1(&"Move executed".into());
                
                let success = matches!(result, MoveResult::Applied);
                let move_event = MoveEvent {
                    from: *from,
                    to: *to,
                    player_id: player_pid.0,
                    success,
                    timestamp: js_sys::Date::now() as u64,
                };
                self.add_move_event(game_id, move_event);
                self.trigger_game_refresh(game_id);
            }
            GameEventData::Conflicts { locked_players, conflict_coords } => {
                web_sys::console::warn_1(&"Move conflicts detected".into());
                
                // Create conflict events for visualization
                for coord in conflict_coords {
                    let conflict_event = ConflictEvent {
                        coord: *coord,
                        players: locked_players.iter().map(|p| p.0).collect(),
                        timestamp: js_sys::Date::now() as u64,
                    };
                    self.add_conflict_event(game_id, conflict_event);
                }
                self.trigger_game_refresh(game_id);
            }
            GameEventData::AutomatonStep { location } => {
                web_sys::console::log_1(&format!("Automaton at: {:?}", location).into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::RoundComplete => {
                web_sys::console::log_1(&"Round completed".into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::GameOver { winner } => {
                web_sys::console::log_1(&format!("Game over! Winner: {}", winner.0).into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::EloUpdate { changes } => {
                web_sys::console::log_1(&format!("ELO ratings updated: {} players", changes.len()).into());
            }
            GameEventData::Chat { displayname, message, .. } => {
                web_sys::console::log_1(&format!("{}: {}", displayname, message).into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::State { .. } => {
                web_sys::console::log_1(&"Full game state update".into());
                self.trigger_game_refresh(game_id);
            }
            GameEventData::GameLoaded { snapshot_index } => {
                web_sys::console::log_1(&format!("Game loaded from snapshot {}", snapshot_index).into());
                self.trigger_game_refresh(game_id);
            }
        }
    }
}
