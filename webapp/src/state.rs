use automatafl_api_types::{ChatMessage, GameEvent, GameStateResponse};
use automatafl_logic::Coord;
use leptos::prelude::*;
use std::collections::HashMap;
use uuid::Uuid;

/// Per-game reactive state - components subscribe directly to these signals
#[derive(Clone, Debug)]
pub struct GameReactiveState {
    /// Full game state (board, players, lifecycle) - updated incrementally
    pub state: RwSignal<Option<GameStateResponse>>,
    /// Chat messages - append-only, updated from Chat events
    pub chat: RwSignal<Vec<ChatMessage>>,
    /// Visual events for animations (moves, conflicts)
    pub move_events: RwSignal<Vec<MoveEvent>>,
    pub conflict_events: RwSignal<Vec<ConflictEvent>>,
    /// Complete event log - ALL GameEvents are stored here for history panel
    /// Components read from this signal instead of fetching via HTTP
    pub event_log: RwSignal<Vec<GameEvent>>,
    /// Per-game WebSocket connection status
    pub websocket_connected: RwSignal<bool>,
}

impl Default for GameReactiveState {
    fn default() -> Self {
        Self {
            state: RwSignal::new(None),
            chat: RwSignal::new(Vec::new()),
            move_events: RwSignal::new(Vec::new()),
            conflict_events: RwSignal::new(Vec::new()),
            event_log: RwSignal::new(Vec::new()),
            websocket_connected: RwSignal::new(false),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub current_player_id: RwSignal<Option<Uuid>>,
    pub session_token: RwSignal<Option<Uuid>>,
    /// Per-game reactive state - eliminates need for triggers!
    pub games: RwSignal<HashMap<Uuid, GameReactiveState>>,
    pub active_game_id: RwSignal<Option<Uuid>>,
    pub matchmaking_status: RwSignal<MatchmakingState>,
    pub api_base_url: String,
    pub ws_base_url: String,
    pub session_validating: RwSignal<bool>,
    /// Cached API client (eliminates repeated localStorage reads)
    pub api_client: StoredValue<crate::api::ApiClient>,
    /// Admin status for authorization checks
    pub is_admin: RwSignal<bool>,
    /// Derived signal for authentication status (computed from session_token)
    pub is_authenticated: Signal<bool>,
}

#[derive(Clone, Debug, Default)]
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

        // Check localStorage for saved session and admin status
        let (current_player_id, session_token, is_admin_stored) =
            if let Some(window) = web_sys::window() {
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
                    let is_admin = storage
                        .get_item("is_admin")
                        .ok()
                        .flatten()
                        .and_then(|s| s.parse::<bool>().ok())
                        .unwrap_or(false);
                    (player, session, is_admin)
                } else {
                    (None, None, false)
                }
            } else {
                (None, None, false)
            };

        let api_client = crate::api::ApiClient::new(api_base_url.clone(), session_token);

        // Create reactive signals
        let session_token_signal = RwSignal::new(session_token);
        let is_admin_signal = RwSignal::new(is_admin_stored);

        // Derived signal for authentication (automatically updates when session_token changes)
        let is_authenticated = Signal::derive(move || session_token_signal.get().is_some());

        let app_state = Self {
            current_player_id: RwSignal::new(current_player_id),
            session_token: session_token_signal,
            games: RwSignal::new(HashMap::new()),
            active_game_id: RwSignal::new(None),
            matchmaking_status: RwSignal::new(MatchmakingState::default()),
            api_base_url,
            ws_base_url,
            session_validating: RwSignal::new(false),
            api_client: StoredValue::new(api_client),
            is_admin: is_admin_signal,
            is_authenticated,
        };

        // Validate session on startup if we have one
        if session_token.is_some() {
            app_state.validate_session();
        }

        app_state
    }

    /// Validate the current session by checking with server
    pub fn validate_session(&self) {
        let app_state = self.clone();

        app_state.session_validating.set(true);

        leptos::task::spawn_local(async move {
            let client = app_state.get_api_client();

            // Try a simple authenticated request
            match client.get_matchmaking_status().await {
                Ok(_) => {
                    // Session is valid
                    web_sys::console::log_1(&"✅ Session validated successfully".into());
                }
                Err(_) => {
                    // Session is invalid, clear it
                    web_sys::console::warn_1(&"❌ Session validation failed, logging out".into());
                    app_state.logout();
                }
            }

            app_state.session_validating.set(false);
        });
    }

    pub fn login(&self, session_token: Uuid, player_id: Uuid, is_admin: bool) {
        self.session_token.set(Some(session_token));
        self.current_player_id.set(Some(player_id));
        self.is_admin.set(is_admin);

        // Update cached API client with new session token
        let new_client = crate::api::ApiClient::new(self.api_base_url.clone(), Some(session_token));
        self.api_client.set_value(new_client);

        // Save to localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("session_token", &session_token.to_string());
                let _ = storage.set_item("player_id", &player_id.to_string());
                let _ = storage.set_item("is_admin", &is_admin.to_string());
            }
        }
    }

    pub fn logout(&self) {
        self.session_token.set(None);
        self.current_player_id.set(None);
        self.active_game_id.set(None);
        self.matchmaking_status.set(MatchmakingState::NotInQueue);

        // Update cached API client to unauthenticated state
        let new_client = crate::api::ApiClient::new(self.api_base_url.clone(), None);
        self.api_client.set_value(new_client);

        // Clear localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.remove_item("session_token");
                let _ = storage.remove_item("player_id");
            }
        }
    }

    /// Check if the current user has admin privileges
    pub fn is_admin(&self) -> bool {
        self.is_admin.get()
    }

    /// Get the cached API client (avoids repeated localStorage reads)
    pub fn get_api_client(&self) -> crate::api::ApiClient {
        self.api_client.get_value()
    }

    pub fn set_active_game(&self, game_id: Option<Uuid>) {
        self.active_game_id.set(game_id);
    }

    /// Ensure game has reactive state initialized
    pub fn ensure_game_state(&self, game_id: Uuid) {
        self.games.update(|games| {
            games
                .entry(game_id)
                .or_insert_with(GameReactiveState::default);
        });
    }

    /// Remove game state when leaving
    pub fn remove_game_state(&self, game_id: Uuid) {
        self.games.update(|games| {
            games.remove(&game_id);
        });
    }

    /// Get game state signal for reactive subscriptions
    pub fn get_game_signal(&self, game_id: Uuid) -> Option<RwSignal<Option<GameStateResponse>>> {
        self.games.get().get(&game_id).map(|g| g.state)
    }

    /// Get chat signal for reactive subscriptions
    pub fn get_chat_signal(&self, game_id: Uuid) -> Option<RwSignal<Vec<ChatMessage>>> {
        self.games.get().get(&game_id).map(|g| g.chat)
    }

    /// Get event log signal for reactive subscriptions
    /// Components read from this signal instead of making HTTP API calls
    pub fn get_event_log_signal(&self, game_id: Uuid) -> Option<RwSignal<Vec<GameEvent>>> {
        self.games.get().get(&game_id).map(|g| g.event_log)
    }

    /// Get WebSocket connection signal for a specific game
    pub fn get_websocket_connected_signal(&self, game_id: Uuid) -> Option<RwSignal<bool>> {
        self.games
            .get()
            .get(&game_id)
            .map(|g| g.websocket_connected)
    }

    /// Set WebSocket connection status for a specific game
    pub fn set_websocket_connected(&self, game_id: Uuid, connected: bool) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.websocket_connected.set(connected);
            }
        });
    }

    /// Update game state directly from event data
    pub fn update_game_state(&self, game_id: Uuid, state: GameStateResponse) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.state.set(Some(state));
            }
        });
    }

    /// Helper to apply a mutation to a specific game's state
    /// Reduces boilerplate in event handlers
    fn with_game_state_mut<F>(&self, game_id: Uuid, mut f: F)
    where
        F: FnMut(&mut GameStateResponse),
    {
        self.games.update(|games| {
            if let Some(game_state) = games.get_mut(&game_id) {
                if let Some(mut state) = game_state.state.get_untracked() {
                    f(&mut state);
                    game_state.state.set(Some(state));
                }
            }
        });
    }

    /// Update specific board position (for incremental Move updates)
    pub fn update_board_position(&self, game_id: Uuid, from: Coord, to: Coord) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                if let Some(state) = game.state.get_untracked() {
                    let mut new_state = state.clone();
                    // Apply move to board using correct ndarray indexing
                    let piece = new_state.game.board.particles[from.ix()];
                    new_state.game.board.particles[from.ix()].what =
                        automatafl_logic::Particle::Vacuum;
                    new_state.game.board.particles[to.ix()] = piece;
                    game.state.set(Some(new_state));
                }
            }
        });
    }

    /// Add chat message directly
    pub fn add_chat_message(&self, game_id: Uuid, message: ChatMessage) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.chat.update(|chat| chat.push(message));
            }
        });
    }

    pub fn add_move_event(&self, game_id: Uuid, event: MoveEvent) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.move_events.update(|events| {
                    events.push(event);
                    // Keep only recent events (last 10)
                    if events.len() > 10 {
                        events.remove(0);
                    }
                });
            }
        });
    }

    pub fn add_conflict_event(&self, game_id: Uuid, event: ConflictEvent) {
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.conflict_events.update(|events| {
                    events.push(event);
                    // Keep only recent events (last 5)
                    if events.len() > 5 {
                        events.remove(0);
                    }
                });
            }
        });
    }

    pub fn get_move_events(&self, game_id: Uuid) -> Vec<MoveEvent> {
        self.games
            .get()
            .get(&game_id)
            .map(|g| g.move_events.get())
            .unwrap_or_default()
    }

    pub fn get_conflict_events(&self, game_id: Uuid) -> Vec<ConflictEvent> {
        self.games
            .get()
            .get(&game_id)
            .map(|g| g.conflict_events.get())
            .unwrap_or_default()
    }

    pub fn handle_game_event(&self, game_id: Uuid, event: GameEvent) {
        use automatafl_api_types::GameEventData;
        use automatafl_logic::MoveResult;

        // FIRST: Store the event in the log before processing
        // This ensures the history panel always has access to all events
        self.games.update(|games| {
            if let Some(game) = games.get_mut(&game_id) {
                game.event_log.update(|log| {
                    log.push(event.clone());
                    // Optional: Keep only last 500 events to prevent memory bloat
                    // For most games this won't be needed, but prevents issues in very long games
                    if log.len() > 500 {
                        log.drain(0..100); // Remove oldest 100 events
                    }
                });
            }
        });

        // THEN: Process the event for state updates
        match &event.data {
            GameEventData::PlayerJoined {
                displayname,
                player_ids,
                ..
            } => {
                web_sys::console::log_1(&format!("Player {} joined", displayname).into());
                // DIRECT UPDATE: Update player_ids map from enriched event
                self.with_game_state_mut(game_id, |state| {
                    state.player_ids = player_ids.clone();
                });
            }
            GameEventData::GameStarted { new_lifecycle } => {
                web_sys::console::log_1(&"Game started event".into());
                // DIRECT UPDATE: Update lifecycle from enriched event
                self.with_game_state_mut(game_id, |state| {
                    state.lifecycle = new_lifecycle.clone();
                });
            }
            GameEventData::MoveAcknowledged {
                player_pid,
                pending_move,
                ..
            } => {
                web_sys::console::log_1(
                    &format!("Player {} move acknowledged", player_pid.0).into(),
                );
                // DIRECT UPDATE: Add pending move from enriched event
                self.with_game_state_mut(game_id, |state| {
                    state.game.pending_moves.push(pending_move.clone());
                });
            }
            GameEventData::MoveInvalid {
                player_pid,
                feedback,
            } => {
                web_sys::console::warn_1(
                    &format!("Invalid move from player {}: {:?}", player_pid.0, feedback).into(),
                );
                // No state change needed - just a feedback message
            }
            GameEventData::Move {
                player_pid,
                from,
                to,
                result,
            } => {
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

                // Update board incrementally (reactive!) - instant visual feedback
                if success {
                    self.update_board_position(game_id, *from, *to);
                }

                // No bump_history needed! Board state is updated directly above
            }
            GameEventData::Conflicts {
                locked_players,
                conflict_coords,
            } => {
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
                // No bump_history needed - conflict events are stored directly
            }
            GameEventData::AutomatonStep { location } => {
                web_sys::console::log_1(&format!("Automaton at: {:?}", location).into());
                // Update automaton position directly
                self.games.update(|games| {
                    if let Some(game) = games.get_mut(&game_id) {
                        if let Some(state) = game.state.get_untracked() {
                            let mut new_state = state.clone();
                            new_state.game.board.automaton_location = *location;
                            game.state.set(Some(new_state));
                        }
                    }
                });
                // No bump_history needed - automaton position updated directly above
            }
            GameEventData::RoundComplete { new_lifecycle } => {
                web_sys::console::log_1(&"Round completed".into());
                // DIRECT UPDATE: Update lifecycle from enriched event
                self.with_game_state_mut(game_id, |state| {
                    state.lifecycle = new_lifecycle.clone();
                    state.game.pending_moves.clear(); // Round complete clears pending moves
                });
            }
            GameEventData::GameOver {
                winner,
                new_lifecycle,
            } => {
                web_sys::console::log_1(&format!("Game over! Winner: {}", winner.0).into());
                // DIRECT UPDATE: Update lifecycle from enriched event
                self.with_game_state_mut(game_id, |state| {
                    state.lifecycle = new_lifecycle.clone();
                });
            }
            GameEventData::EloUpdate { .. } => {
                web_sys::console::log_1(&"ELO ratings updated".into());
                // Stats updated server-side, no local state change needed
            }
            GameEventData::Chat {
                displayname,
                message,
                player_id,
                timestamp,
            } => {
                web_sys::console::log_1(&format!("{}: {}", displayname, message).into());
                // DIRECT UPDATE: Add chat message, component auto-updates from signal!
                let chat_msg = ChatMessage {
                    timestamp: *timestamp,
                    player_id: *player_id,
                    displayname: displayname.clone(),
                    message: message.clone(),
                };
                self.add_chat_message(game_id, chat_msg);
                // No bump_history needed - chat is updated directly
            }
            GameEventData::State {
                lifecycle,
                game,
                player_ids,
            } => {
                web_sys::console::log_1(&"Full game state update".into());
                // DIRECT UPDATE: We have the full state, use it directly!
                let state = GameStateResponse {
                    lifecycle: lifecycle.clone(),
                    game: (**game).clone(),
                    player_ids: player_ids.clone(),
                };
                self.update_game_state(game_id, state);
                // No bump_history needed - full state is updated directly
            }
            GameEventData::GameLoaded { snapshot_index } => {
                web_sys::console::log_1(
                    &format!("Game loaded from snapshot {}", snapshot_index).into(),
                );
                // Full reset - the State event that follows will update everything automatically
                // No manual refresh needed - event_log is already updated above
            }
        }
    }
}
