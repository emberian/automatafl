// Wrapper around automatafl-backend-client for webapp usage
// Re-exports the client with web_sys::window local storage integration

#![allow(dead_code)] // Many methods are part of the public API but not yet used

use automatafl_api_types::*;
use automatafl_backend_client::{AutomataflClient, ClientError, Result};
use automatafl_logic::{Coord, Pid};
use uuid::Uuid;

/// Webapp-specific wrapper around the backend client with localStorage integration
#[derive(Clone)]
pub struct ApiClient {
    inner: AutomataflClient,
}

impl ApiClient {
    pub fn new(base_url: String, session_token: Option<Uuid>) -> Self {
        let inner = if let Some(token) = session_token {
            AutomataflClient::with_session(base_url, token)
        } else {
            AutomataflClient::new(base_url)
        };

        Self { inner }
    }

    pub fn with_session(base_url: String, session_token: Uuid) -> Self {
        Self::new(base_url, Some(session_token))
    }

    pub fn session_token(&self) -> Option<Uuid> {
        self.inner.session_token()
    }

    pub fn set_session_token(&mut self, token: Option<Uuid>) {
        self.inner.set_session_token(token);
    }

    // === Health Check ===
    pub async fn health_check(&self) -> Result<HealthResponse> {
        self.inner.health_check().await
    }

    // === Auth ===
    pub async fn register(
        &self,
        displayname: String,
        password: String,
    ) -> Result<RegisterResponse> {
        self.inner.register(displayname, password).await
    }

    /// Login and return response - caller should create a new client with session token
    pub async fn login(&mut self, displayname: String, password: String) -> Result<LoginResponse> {
        self.inner.login(displayname, password).await
    }

    /// Logout - caller should create a new client without session token after
    pub async fn logout(&mut self) -> Result<()> {
        self.inner.logout().await
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

    pub async fn perform_move(
        &self,
        game_id: Uuid,
        from: Coord,
        to: Coord,
    ) -> Result<MoveResultResponse> {
        self.inner.perform_move(game_id, from, to).await
    }

    pub async fn complete_round(&self, game_id: Uuid) -> Result<CompleteRoundResponse> {
        let value = self.inner.complete_round(game_id).await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    // === Chat ===
    pub async fn send_chat(&self, game_id: Uuid, message: String) -> Result<PostChatResponse> {
        let value = self.inner.send_chat(game_id, message).await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn get_chat(&self, game_id: Uuid) -> Result<Vec<ChatMessage>> {
        self.inner.get_chat(game_id).await
    }

    // === Save/Load ===
    pub async fn save_game(&self, game_id: Uuid) -> Result<SaveGameResponse> {
        let value = self.inner.save_game(game_id).await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn list_snapshots(&self, game_id: Uuid) -> Result<ListSnapshotsResponse> {
        let value = self.inner.list_snapshots(game_id).await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn load_game(&self, game_id: Uuid, snapshot_index: usize) -> Result<()> {
        self.inner.load_game(game_id, snapshot_index).await
    }

    // === Admin - Player Management ===
    pub async fn admin_list_players(&self) -> Result<Vec<PlayerListItem>> {
        let value = self.inner.admin_list_players().await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn admin_get_player(&self, player_id: Uuid) -> Result<serde_json::Value> {
        self.inner.admin_get_player(player_id).await
    }

    pub async fn admin_update_player(
        &self,
        player_id: Uuid,
        data: serde_json::Value,
    ) -> Result<()> {
        self.inner.admin_update_player(player_id, data).await
    }

    pub async fn admin_delete_player(&self, player_id: Uuid) -> Result<()> {
        self.inner.admin_delete_player(player_id).await
    }

    pub async fn admin_get_player_stats(&self, player_id: Uuid) -> Result<serde_json::Value> {
        self.inner.admin_get_player_stats(player_id).await
    }

    pub async fn admin_update_player_stats(
        &self,
        player_id: Uuid,
        data: serde_json::Value,
    ) -> Result<()> {
        self.inner.admin_update_player_stats(player_id, data).await
    }

    // === Admin - Game Management ===
    pub async fn admin_list_games(&self) -> Result<Vec<GameListItem>> {
        let value = self.inner.admin_list_games().await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn admin_get_game(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.admin_get_game(game_id).await
    }

    pub async fn admin_delete_game(&self, game_id: Uuid) -> Result<()> {
        self.inner.admin_delete_game(game_id).await
    }

    pub async fn admin_force_complete_round(&self, game_id: Uuid) -> Result<CompleteRoundResponse> {
        let value = self.inner.admin_force_complete_round(game_id).await?;
        serde_json::from_value(value).map_err(ClientError::Json)
    }

    pub async fn admin_set_game_lifecycle(
        &self,
        game_id: Uuid,
        lifecycle: serde_json::Value,
    ) -> Result<()> {
        self.inner
            .admin_set_game_lifecycle(game_id, lifecycle)
            .await
    }

    pub async fn admin_get_game_events(&self, game_id: Uuid) -> Result<Vec<GameEvent>> {
        self.inner.admin_get_game_events(game_id).await
    }

    pub async fn admin_get_game_chat(&self, game_id: Uuid) -> Result<Vec<ChatMessage>> {
        self.inner.admin_get_game_chat(game_id).await
    }

    pub async fn admin_delete_chat_message(&self, game_id: Uuid, timestamp: u64) -> Result<()> {
        self.inner
            .admin_delete_chat_message(game_id, timestamp)
            .await
    }

    // === Admin - Snapshots ===
    pub async fn admin_list_snapshots(&self, game_id: Uuid) -> Result<serde_json::Value> {
        self.inner.admin_list_snapshots(game_id).await
    }

    pub async fn admin_delete_snapshot(&self, game_id: Uuid, snapshot_index: usize) -> Result<()> {
        self.inner
            .admin_delete_snapshot(game_id, snapshot_index)
            .await
    }

    // === Admin - Session Management ===
    pub async fn admin_list_sessions(&self) -> Result<serde_json::Value> {
        self.inner.admin_list_sessions().await
    }

    pub async fn admin_delete_session(&self, session_id: Uuid) -> Result<()> {
        self.inner.admin_delete_session(session_id).await
    }

    pub async fn admin_cleanup_expired_sessions(&self) -> Result<serde_json::Value> {
        self.inner.admin_cleanup_expired_sessions().await
    }

    // === Admin - Matchmaking ===
    pub async fn admin_list_matchmaking_queue(&self) -> Result<serde_json::Value> {
        self.inner.admin_list_matchmaking_queue().await
    }

    pub async fn admin_remove_from_matchmaking(&self, player_id: Uuid) -> Result<()> {
        self.inner.admin_remove_from_matchmaking(player_id).await
    }

    // === Admin - Database Introspection ===
    pub async fn admin_get_database_stats(&self) -> Result<serde_json::Value> {
        self.inner.admin_get_database_stats().await
    }

    pub async fn admin_list_tables(&self) -> Result<serde_json::Value> {
        self.inner.admin_list_tables().await
    }

    // === Matchmaking ===
    pub async fn join_matchmaking(&self, player_count: u8, use_column_rule: bool) -> Result<()> {
        self.inner
            .join_matchmaking(player_count, use_column_rule)
            .await
    }

    pub async fn leave_matchmaking(&self) -> Result<()> {
        self.inner.leave_matchmaking().await
    }

    pub async fn get_matchmaking_status(&self) -> Result<MatchmakingStatus> {
        self.inner.get_matchmaking_status().await
    }

    // === Leaderboard ===
    pub async fn get_leaderboard_elo(&self) -> Result<LeaderboardResponse> {
        self.inner.get_leaderboard_elo().await
    }

    pub async fn get_leaderboard_wins(&self) -> Result<LeaderboardResponse> {
        self.inner.get_leaderboard_wins().await
    }

    pub async fn get_leaderboard_games(&self) -> Result<LeaderboardResponse> {
        self.inner.get_leaderboard_games().await
    }

    // === User Profile ===
    pub async fn get_player_profile(&self, player_id: Uuid) -> Result<PlayerProfile> {
        self.inner.get_player_profile(player_id).await
    }

    pub async fn update_player_profile(
        &self,
        player_id: Uuid,
        bio: Option<String>,
        avatar_url: Option<String>,
    ) -> Result<()> {
        self.inner
            .update_player_profile(player_id, bio, avatar_url)
            .await
    }

    pub async fn get_player_stats(&self, player_id: Uuid) -> Result<PlayerStats> {
        self.inner.get_player_stats(player_id).await
    }

    // === Game History ===
    pub async fn get_game_history(&self, game_id: Uuid) -> Result<Vec<GameEvent>> {
        self.inner.get_game_history(game_id).await
    }

    pub async fn get_game_history_filtered(
        &self,
        game_id: Uuid,
        since: Option<u64>,
        until: Option<u64>,
        event_kind: Option<String>,
    ) -> Result<Vec<GameEvent>> {
        self.inner
            .get_game_history_filtered(game_id, since, until, event_kind)
            .await
    }
}
