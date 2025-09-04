use automatafl_api::{UserInfo, GameStateResponse, WebSocketMessage};
use leptos::prelude::*;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct AppState {
    pub current_user: RwSignal<Option<UserInfo>>,
    pub auth_token: RwSignal<Option<String>>,
    pub games: RwSignal<HashMap<Uuid, GameStateResponse>>,
    pub websocket_connected: RwSignal<bool>,
    pub active_game_id: RwSignal<Option<Uuid>>,
    pub matchmaking_status: RwSignal<MatchmakingState>,
    pub api_base_url: String,
    pub ws_base_url: String,
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

impl AppState {
    pub fn new() -> Self {
        // Get base URLs from environment or use defaults
        let api_base_url = option_env!("API_BASE_URL")
            .unwrap_or("http://localhost:3000/api")
            .to_string();
        
        let ws_base_url = option_env!("WS_BASE_URL")
            .unwrap_or("ws://localhost:3000/api/ws")
            .to_string();

        // Check localStorage for saved auth
        let (current_user, auth_token) = if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let token = storage.get_item("auth_token").ok().flatten();
                let user = storage.get_item("user_info")
                    .ok()
                    .flatten()
                    .and_then(|json| serde_json::from_str(&json).ok());
                (user, token)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Self {
            current_user: create_rw_signal(current_user),
            auth_token: create_rw_signal(auth_token),
            games: create_rw_signal(HashMap::new()),
            websocket_connected: create_rw_signal(false),
            active_game_id: create_rw_signal(None),
            matchmaking_status: create_rw_signal(MatchmakingState::default()),
            api_base_url,
            ws_base_url,
        }
    }

    pub fn login(&self, token: String, user: UserInfo) {
        self.auth_token.set(Some(token.clone()));
        self.current_user.set(Some(user.clone()));
        
        // Save to localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("auth_token", &token);
                if let Ok(user_json) = serde_json::to_string(&user) {
                    let _ = storage.set_item("user_info", &user_json);
                }
            }
        }
    }

    pub fn logout(&self) {
        self.auth_token.set(None);
        self.current_user.set(None);
        self.active_game_id.set(None);
        self.matchmaking_status.set(MatchmakingState::NotInQueue);
        
        // Clear localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.remove_item("auth_token");
                let _ = storage.remove_item("user_info");
            }
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth_token.get().is_some()
    }

    pub fn update_game(&self, game: GameStateResponse) {
        self.games.update(|games| {
            games.insert(game.id, game);
        });
    }

    pub fn remove_game(&self, game_id: Uuid) {
        self.games.update(|games| {
            games.remove(&game_id);
        });
    }

    pub fn set_active_game(&self, game_id: Option<Uuid>) {
        self.active_game_id.set(game_id);
    }

    pub fn handle_websocket_message(&self, msg: WebSocketMessage) {
        match msg {
            WebSocketMessage::GameUpdate { game_state } => {
                self.update_game(game_state);
            }
            WebSocketMessage::Error { error } => {
                // Log error or show notification
                web_sys::console::error_1(&format!("WebSocket error: {}", error).into());
            }
            WebSocketMessage::Subscribed { game_id } => {
                web_sys::console::log_1(&format!("Subscribed to game: {}", game_id).into());
            }
            WebSocketMessage::Unsubscribed { game_id } => {
                web_sys::console::log_1(&format!("Unsubscribed from game: {}", game_id).into());
            }
            _ => {}
        }
    }
}
