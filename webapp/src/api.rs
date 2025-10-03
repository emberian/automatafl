// Wrapper around automatafl-backend-client for webapp usage
// Re-exports the client with web_sys::window local storage integration

use automatafl_backend_client::{AutomataflClient, ClientError, Result};
use automatafl_api_types::*;
use automatafl_logic::{Coord, Pid};
use uuid::Uuid;

/// Webapp-specific wrapper around the backend client with localStorage integration
#[derive(Clone)]
#[allow(dead_code)]
pub struct ApiClient {
    inner: AutomataflClient,
}

#[allow(dead_code)]
impl ApiClient {
    pub fn new(base_url: String) -> Self {
        // Try to load session from localStorage
        let session_token = if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                storage
                    .get_item("session_token")
                    .ok()
                    .flatten()
                    .and_then(|s| Uuid::parse_str(&s).ok())
            } else {
                None
            }
        } else {
            None
        };

        let inner = if let Some(token) = session_token {
            AutomataflClient::with_session(base_url, token)
        } else {
            AutomataflClient::new(base_url)
        };

        Self { inner }
    }

    pub fn with_session(base_url: String, session_token: Uuid) -> Self {
        Self {
            inner: AutomataflClient::with_session(base_url, session_token),
        }
    }

    pub fn session_token(&self) -> Option<Uuid> {
        self.inner.session_token()
    }

    pub fn set_session_token(&mut self, token: Option<Uuid>) {
        self.inner.set_session_token(token);
        
        // Persist to localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Some(t) = token {
                    let _ = storage.set_item("session_token", &t.to_string());
                } else {
                    let _ = storage.remove_item("session_token");
                }
            }
        }
    }

    // === Health Check ===
    pub async fn health_check(&self) -> Result<HealthResponse> {
        self.inner.health_check().await
    }

    // === Auth ===
    pub async fn register(&self, displayname: String, password: String) -> Result<RegisterResponse> {
        self.inner.register(displayname, password).await
    }

    pub async fn login(&mut self, displayname: String, password: String) -> Result<LoginResponse> {
        let response = self.inner.login(displayname, password).await?;
        // Session token is automatically set in the inner client
        // Persist to localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("session_token", &response.session_id.to_string());
                let _ = storage.set_item("player_id", &response.player_id.to_string());
            }
        }
        Ok(response)
    }

    pub async fn logout(&mut self) -> Result<()> {
        let result = self.inner.logout().await;
        // Clear localStorage regardless of result
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.remove_item("session_token");
                let _ = storage.remove_item("player_id");
            }
        }
        result
    }

    // === Games ===
    pub async fn list_games(&self) -> Result<Vec<GameListItem>> {
        self.inner.list_games().await
    }

    pub async fn create_game(&self, player_count: u8, use_column_rule: bool) -> Result<Uuid> {
        self.inner.create_game(player_count, use_column_rule).await
    }

    pub async fn join_game(&self, game_id: Uuid) -> Result<Pid> {
        self.inner.join_game(game_id).await
    }

    pub async fn get_game_state(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.get_game_state(game_id).await
    }

    pub async fn get_game_state_typed(&self, game_id: Uuid) -> Result<GameStateResponse> {
        let value = self.inner.get_game_state(game_id).await?;
        serde_json::from_value(value).map_err(|e| ClientError::Json(e))
    }

    pub async fn get_goals(&self, game_id: Uuid) -> Result<Vec<(Coord, Pid)>> {
        self.inner.get_goals(game_id).await
    }

    // === Moves ===
    pub async fn get_pending_move(&self, game_id: Uuid) -> Result<Option<automatafl_logic::Move>> {
        self.inner.get_pending_move(game_id).await
    }

    pub async fn perform_move(&self, game_id: Uuid, from: Coord, to: Coord) -> Result<MoveResult> {
        self.inner.perform_move(game_id, from, to).await
    }

    pub async fn complete_round(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.complete_round(game_id).await
    }

    // === Chat ===
    pub async fn send_chat(&self, game_id: Uuid, message: String) -> Result<serde_json::Value> {
        self.inner.send_chat(game_id, message).await
    }

    pub async fn get_chat(&self, game_id: Uuid) -> Result<Vec<ChatMessage>> {
        self.inner.get_chat(game_id).await
    }

    // === Save/Load ===
    pub async fn save_game(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.save_game(game_id).await
    }

    pub async fn list_snapshots(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.list_snapshots(game_id).await
    }

    pub async fn load_game(&self, game_id: Uuid, snapshot_index: usize) -> Result<()> {
        self.inner.load_game(game_id, snapshot_index).await
    }

    // === Admin ===
    pub async fn admin_list_players(&self) -> Result<serde_json::Value> {
        self.inner.admin_list_players().await
    }

    pub async fn admin_list_games(&self) -> Result<serde_json::Value> {
        self.inner.admin_list_games().await
    }

    pub async fn admin_delete_game(&self, game_id: Uuid) -> Result<()> {
        self.inner.admin_delete_game(game_id).await
    }

    pub async fn admin_force_complete_round(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.admin_force_complete_round(game_id).await
    }

    // TODO: Implement when backend supports these
    // === Matchmaking (stubbed) ===
    // pub async fn join_matchmaking(&self, ...) -> Result<()>
    // pub async fn leave_matchmaking(&self) -> Result<()>
    // pub async fn matchmaking_status(&self) -> Result<...>

    // === Leaderboard (stubbed) ===
    // pub async fn get_leaderboard(&self, page: i32) -> Result<...>

    // === User Profile (stubbed) ===
    // pub async fn get_user_profile(&self, user_id: Uuid) -> Result<...>
    // pub async fn get_user_games(&self, user_id: Uuid, page: i32) -> Result<...>
}
