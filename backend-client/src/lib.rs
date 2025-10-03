use automatafl_api_types::*;
use automatafl_logic::{Pid, Coord, Move};

use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("Not authenticated")]
    NotAuthenticated,
    #[error("Deserialized error: {0}")]
    Deserialized(String),
}

// these impls are needed for leptos to work

impl serde::Serialize for ClientError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ClientError {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(ClientError::Deserialized(s))
    }
}

impl Clone for ClientError {
    fn clone(&self) -> Self {
        ClientError::Deserialized(self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

// ============================================================================
// Client
// ============================================================================

#[derive(Clone)]
pub struct AutomataflClient {
    base_url: String,
    client: reqwest::Client,
    session_token: Option<Uuid>,
}

impl AutomataflClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            session_token: None,
        }
    }

    pub fn with_session(base_url: impl Into<String>, session_token: Uuid) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
            session_token: Some(session_token),
        }
    }

    pub fn session_token(&self) -> Option<Uuid> {
        self.session_token
    }

    pub fn set_session_token(&mut self, token: Option<Uuid>) {
        self.session_token = token;
    }

    fn auth_header(&self) -> Result<String> {
        self.session_token
            .map(|t| format!("Bearer {}", t))
            .ok_or(ClientError::NotAuthenticated)
    }

    // ========================================================================
    // Health Check
    // ========================================================================

    pub async fn health_check(&self) -> Result<HealthResponse> {
        let url = format!("{}/api/health", self.base_url);
        let response = self.client.get(&url).send().await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Auth
    // ========================================================================

    pub async fn register(&self, displayname: String, password: String) -> Result<RegisterResponse> {
        let url = format!("{}/api/v1/register", self.base_url);
        let request = RegisterRequest { displayname, password };
        let response = self.client.post(&url).json(&request).send().await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn login(&mut self, displayname: String, password: String) -> Result<LoginResponse> {
        let url = format!("{}/api/v1/login", self.base_url);
        let request = LoginRequest { displayname, password };
        let response = self.client.post(&url).json(&request).send().await?;

        if response.status().is_success() {
            let login_response: LoginResponse = response.json().await?;
            self.session_token = Some(login_response.session_id);
            Ok(login_response)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn logout(&mut self) -> Result<()> {
        let url = format!("{}/api/v1/logout", self.base_url);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            self.session_token = None;
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Games
    // ========================================================================

    pub async fn list_games(&self) -> Result<Vec<GameListItem>> {
        let url = format!("{}/api/v1/games", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn create_game(&self, player_count: u8, use_column_rule: bool) -> Result<Uuid> {
        let url = format!("{}/api/v1/games", self.base_url);
        let request = CreateGameRequest { player_count, use_column_rule };
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn join_game(&self, game_id: Uuid) -> Result<Pid> {
        let url = format!("{}/api/v1/games/{}", self.base_url, game_id);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_game_state(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/games/{}", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_goals(&self, game_id: Uuid) -> Result<Vec<(Coord, Pid)>> {
        let url = format!("{}/api/v1/games/{}/goals", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Moves
    // ========================================================================

    pub async fn get_pending_move(&self, game_id: Uuid) -> Result<Option<Move>> {
        let url = format!("{}/api/v1/games/{}/move", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn perform_move(&self, game_id: Uuid, from: Coord, to: Coord) -> Result<MoveResultResponse> {
        let url = format!("{}/api/v1/games/{}/move", self.base_url, game_id);
        let request = PerformMove { from, to };
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn complete_round(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/games/{}/complete", self.base_url, game_id);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Chat
    // ========================================================================

    pub async fn send_chat(&self, game_id: Uuid, message: String) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/games/{}/chat", self.base_url, game_id);
        let request = PostChatRequest { message };
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_chat(&self, game_id: Uuid) -> Result<Vec<ChatMessage>> {
        let url = format!("{}/api/v1/games/{}/chat", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_game_history(&self, game_id: Uuid) -> Result<Vec<GameEvent>> {
        let url = format!("{}/api/v1/games/{}/history", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_game_history_filtered(
        &self,
        game_id: Uuid,
        since: Option<u64>,
        until: Option<u64>,
        event_kind: Option<String>,
    ) -> Result<Vec<GameEvent>> {
        let mut url = format!("{}/api/v1/games/{}/history?", self.base_url, game_id);
        let mut params = vec![];
        if let Some(s) = since {
            params.push(format!("since={}", s));
        }
        if let Some(u) = until {
            params.push(format!("until={}", u));
        }
        if let Some(k) = event_kind {
            params.push(format!("event_kind={}", k));
        }
        url.push_str(&params.join("&"));

        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Save/Load
    // ========================================================================

    pub async fn save_game(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/games/{}/save", self.base_url, game_id);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn list_snapshots(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/games/{}/snapshots", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn load_game(&self, game_id: Uuid, snapshot_index: usize) -> Result<()> {
        let url = format!("{}/api/v1/games/{}/load/{}", self.base_url, game_id, snapshot_index);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Admin - Player Management
    // ========================================================================

    pub async fn admin_list_players(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/players", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_get_player(&self, player_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/players/{}", self.base_url, player_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_update_player(&self, player_id: Uuid, data: serde_json::Value) -> Result<()> {
        let url = format!("{}/api/v1/admin/players/{}", self.base_url, player_id);
        let response = self.client
            .put(&url)
            .header("Authorization", self.auth_header()?)
            .json(&data)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_delete_player(&self, player_id: Uuid) -> Result<()> {
        let url = format!("{}/api/v1/admin/players/{}", self.base_url, player_id);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_get_player_stats(&self, player_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/players/{}/stats", self.base_url, player_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_update_player_stats(&self, player_id: Uuid, data: serde_json::Value) -> Result<()> {
        let url = format!("{}/api/v1/admin/players/{}/stats", self.base_url, player_id);
        let response = self.client
            .put(&url)
            .header("Authorization", self.auth_header()?)
            .json(&data)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Admin - Game Management
    // ========================================================================

    pub async fn admin_list_games(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/games", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_get_game(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/games/{}", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_delete_game(&self, game_id: Uuid) -> Result<()> {
        let url = format!("{}/api/v1/admin/games/{}", self.base_url, game_id);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_force_complete_round(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/games/{}/force-complete", self.base_url, game_id);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_set_game_lifecycle(&self, game_id: Uuid, lifecycle: serde_json::Value) -> Result<()> {
        let url = format!("{}/api/v1/admin/games/{}/lifecycle", self.base_url, game_id);
        let response = self.client
            .put(&url)
            .header("Authorization", self.auth_header()?)
            .json(&lifecycle)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_get_game_events(&self, game_id: Uuid) -> Result<Vec<GameEvent>> {
        let url = format!("{}/api/v1/admin/games/{}/events", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_get_game_chat(&self, game_id: Uuid) -> Result<Vec<ChatMessage>> {
        let url = format!("{}/api/v1/admin/games/{}/chat", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_delete_chat_message(&self, game_id: Uuid, timestamp: u64) -> Result<()> {
        let url = format!("{}/api/v1/admin/games/{}/chat/{}", self.base_url, game_id, timestamp);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_list_snapshots(&self, game_id: Uuid) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/games/{}/snapshots", self.base_url, game_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_delete_snapshot(&self, game_id: Uuid, snapshot_index: usize) -> Result<()> {
        let url = format!("{}/api/v1/admin/games/{}/snapshots/{}", self.base_url, game_id, snapshot_index);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Admin - Session Management
    // ========================================================================

    pub async fn admin_list_sessions(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/sessions", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_delete_session(&self, session_id: Uuid) -> Result<()> {
        let url = format!("{}/api/v1/admin/sessions/{}", self.base_url, session_id);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_cleanup_expired_sessions(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/sessions/cleanup", self.base_url);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Admin - Matchmaking
    // ========================================================================

    pub async fn admin_list_matchmaking_queue(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/matchmaking/queue", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_remove_from_matchmaking(&self, player_id: Uuid) -> Result<()> {
        let url = format!("{}/api/v1/admin/matchmaking/queue/{}", self.base_url, player_id);
        let response = self.client
            .delete(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Admin - Database Introspection
    // ========================================================================

    pub async fn admin_get_database_stats(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/stats", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn admin_list_tables(&self) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/admin/tables", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Profile Endpoints
    // ========================================================================

    pub async fn get_player_profile(&self, player_id: Uuid) -> Result<PlayerProfile> {
        let url = format!("{}/api/v1/players/{}", self.base_url, player_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn update_player_profile(&self, player_id: Uuid, bio: Option<String>, avatar_url: Option<String>) -> Result<()> {
        let url = format!("{}/api/v1/players/{}", self.base_url, player_id);
        let request = UpdateProfileRequest { bio, avatar_url };
        let response = self.client
            .put(&url)
            .header("Authorization", self.auth_header()?)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_player_stats(&self, player_id: Uuid) -> Result<PlayerStats> {
        let url = format!("{}/api/v1/players/{}/stats", self.base_url, player_id);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Leaderboard Endpoints
    // ========================================================================

    pub async fn get_leaderboard_elo(&self) -> Result<LeaderboardResponse> {
        let url = format!("{}/api/v1/leaderboard/elo", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_leaderboard_wins(&self) -> Result<LeaderboardResponse> {
        let url = format!("{}/api/v1/leaderboard/wins", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_leaderboard_games(&self) -> Result<LeaderboardResponse> {
        let url = format!("{}/api/v1/leaderboard/games", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    // ========================================================================
    // Matchmaking Endpoints
    // ========================================================================

    pub async fn join_matchmaking(&self, player_count: u8, use_column_rule: bool) -> Result<()> {
        let url = format!("{}/api/v1/matchmaking/join", self.base_url);
        let request = JoinMatchmakingRequest { player_count, use_column_rule };
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn leave_matchmaking(&self) -> Result<()> {
        let url = format!("{}/api/v1/matchmaking/leave", self.base_url);
        let response = self.client
            .post(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }

    pub async fn get_matchmaking_status(&self) -> Result<MatchmakingStatus> {
        let url = format!("{}/api/v1/matchmaking/status", self.base_url);
        let response = self.client
            .get(&url)
            .header("Authorization", self.auth_header()?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            Err(ClientError::Api(response.text().await?))
        }
    }
}
