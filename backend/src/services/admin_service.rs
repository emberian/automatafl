use std::{collections::HashMap, sync::Arc};

use automatafl_api_types::{
    CompleteRoundResponse, GameEventData, GameLifecycle, GameListItem, JoinMatchmakingRequest,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    common::{GameLifecycleState, timestamp},
    db::{
        ChatMessageRecord, GameEventRecord, GameRecord, PlayerRecord, PlayerStatsRecord,
        SessionRecord, SnapshotRecord, as_uuid,
    },
    repositories::{GameRepository, MatchmakingRepository, PlayerRepository, SessionRepository},
    services::game_service::ServiceError,
    services::player_service::PlayerServiceError,
};

use super::{GameService, PlayerService};

/// Snapshot of high-level admin metrics for dashboard display
pub struct AdminDashboardStats {
    pub total_players: usize,
    pub total_games: usize,
    pub active_sessions: usize,
    pub queue_size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminQueueEntry {
    pub player_id: Uuid,
    pub player_displayname: Option<String>,
    pub player_elo_rating: Option<i32>,
    pub queued_at: u64,
    pub wait_time_seconds: u64,
    pub game_preferences: JoinMatchmakingRequest,
}

/// Aggregates administrative operations across repositories and services
pub struct AdminService {
    pub(crate) game_repo: GameRepository,
    pub(crate) player_repo: PlayerRepository,
    pub(crate) session_repo: SessionRepository,
    pub(crate) matchmaking_repo: MatchmakingRepository,
    pub(crate) game_service: Arc<GameService>,
    pub(crate) player_service: Arc<PlayerService>,
}

#[derive(Debug)]
pub enum AdminServiceError {
    Database(surrealdb::Error),
    Game(ServiceError),
    Player(PlayerServiceError),
}

impl std::fmt::Display for AdminServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminServiceError::Database(err) => write!(f, "Database error: {}", err),
            AdminServiceError::Game(err) => write!(f, "Game error: {}", err),
            AdminServiceError::Player(err) => write!(f, "Player error: {}", err),
        }
    }
}

impl std::error::Error for AdminServiceError {}

impl From<surrealdb::Error> for AdminServiceError {
    fn from(value: surrealdb::Error) -> Self {
        AdminServiceError::Database(value)
    }
}

impl From<ServiceError> for AdminServiceError {
    fn from(value: ServiceError) -> Self {
        AdminServiceError::Game(value)
    }
}

impl From<PlayerServiceError> for AdminServiceError {
    fn from(value: PlayerServiceError) -> Self {
        AdminServiceError::Player(value)
    }
}

/// Aggregated database statistics for the admin API
pub struct AdminDatabaseStats {
    pub total_players: usize,
    pub total_games: usize,
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub queue_size: usize,
    pub total_chat_messages: usize,
    pub total_snapshots: usize,
}

impl AdminService {
    pub fn new(
        game_repo: GameRepository,
        player_repo: PlayerRepository,
        session_repo: SessionRepository,
        matchmaking_repo: MatchmakingRepository,
        game_service: Arc<GameService>,
        player_service: Arc<PlayerService>,
    ) -> Self {
        Self {
            game_repo,
            player_repo,
            session_repo,
            matchmaking_repo,
            game_service,
            player_service,
        }
    }

    /// Get high-level stats used across admin dashboards
    pub async fn dashboard_stats(&self) -> Result<AdminDashboardStats, surrealdb::Error> {
        let players = self.player_repo.list_all().await?;
        let games: Vec<GameListItem> = self.game_repo.list_with_player_counts().await?;
        let sessions = self.session_repo.list_all().await?;
        let queue = self.matchmaking_repo.list_queue().await?;

        let now = timestamp();
        let active_sessions = sessions.iter().filter(|s| s.expires_at > now).count();

        Ok(AdminDashboardStats {
            total_players: players.len(),
            total_games: games.len(),
            active_sessions,
            queue_size: queue.len(),
        })
    }

    /// List all players for administrative views
    pub async fn list_players(&self) -> Result<Vec<PlayerRecord>, surrealdb::Error> {
        self.player_repo.list_all().await
    }

    /// List all game records
    pub async fn list_game_records(&self) -> Result<Vec<GameRecord>, surrealdb::Error> {
        self.game_repo.list_all().await
    }

    /// Fetch a specific player
    pub async fn get_player(
        &self,
        player_id: Uuid,
    ) -> Result<Option<PlayerRecord>, surrealdb::Error> {
        self.player_repo.get(player_id).await
    }

    /// Update admin-managed player fields
    pub async fn update_player_fields(
        &self,
        player_id: Uuid,
        displayname: Option<String>,
        is_admin: Option<bool>,
        bio: Option<String>,
        avatar_url: Option<String>,
        elo_rating: Option<i32>,
    ) -> Result<(), surrealdb::Error> {
        self.player_repo
            .update_admin_fields(
                player_id,
                displayname,
                is_admin,
                bio,
                avatar_url,
                elo_rating,
            )
            .await
    }

    /// Delete player and related records
    pub async fn delete_player(&self, player_id: Uuid) -> Result<(), AdminServiceError> {
        self.session_repo.delete_by_player(player_id).await?;
        self.matchmaking_repo.remove_player(player_id).await?;
        self.game_repo
            .remove_player_from_all_games(player_id)
            .await?;
        self.player_repo.delete_stats(player_id).await?;
        self.player_repo.delete(player_id).await?;
        Ok(())
    }

    /// Fetch all session records for monitoring
    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, surrealdb::Error> {
        self.session_repo.list_all().await
    }

    /// Delete a specific session
    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), surrealdb::Error> {
        self.session_repo.delete(session_id).await
    }

    /// Cleanup expired sessions and return number of deletions
    pub async fn cleanup_expired_sessions(&self, now: u64) -> Result<u64, surrealdb::Error> {
        self.session_repo.cleanup_expired(now).await
    }

    /// Retrieve the current matchmaking queue
    pub async fn queue_entries(&self) -> Result<Vec<AdminQueueEntry>, AdminServiceError> {
        let queue = self.matchmaking_repo.list_queue().await?;
        let player_ids: Vec<Uuid> = queue
            .iter()
            .map(|record| as_uuid(&record.player_id))
            .collect();

        let players_by_id = self.players_by_ids(&player_ids).await?;
        let now = timestamp();

        let entries = queue
            .into_iter()
            .map(|record| {
                let player_id = as_uuid(&record.player_id);
                let preferences = serde_json::from_str(&record.game_preferences).unwrap_or(
                    JoinMatchmakingRequest {
                        player_count: 2,
                        use_column_rule: false,
                    },
                );

                AdminQueueEntry {
                    player_id,
                    player_displayname: players_by_id
                        .get(&player_id)
                        .map(|player| player.displayname.clone()),
                    player_elo_rating: players_by_id
                        .get(&player_id)
                        .map(|player| player.elo_rating),
                    queued_at: record.queued_at,
                    wait_time_seconds: now.saturating_sub(record.queued_at),
                    game_preferences: preferences,
                }
            })
            .collect();

        Ok(entries)
    }

    /// Remove a specific player from the matchmaking queue
    pub async fn remove_from_queue(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        self.matchmaking_repo.remove_player(player_id).await
    }

    /// Helper to resolve player records in bulk by ID
    pub async fn players_by_ids(
        &self,
        player_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, PlayerRecord>, surrealdb::Error> {
        let mut result = HashMap::new();
        for &player_id in player_ids {
            if let Some(record) = self.player_repo.get(player_id).await? {
                result.insert(player_id, record);
            }
        }
        Ok(result)
    }

    /// Fetch player stats record
    pub async fn player_stats(
        &self,
        player_id: Uuid,
    ) -> Result<Option<PlayerStatsRecord>, surrealdb::Error> {
        let start = std::time::Instant::now();
        let result = self.player_repo.get_stats(player_id).await;
        let duration = start.elapsed();

        metrics::histogram!(
            "database_query_duration_seconds",
            &[("operation", "player_stats"), ("table", "player_stats"),]
        )
        .record(duration.as_secs_f64());

        if result.is_err() {
            metrics::counter!("database_errors_total", &[("operation", "player_stats")])
                .increment(1);
        }

        result
    }

    /// Update specific player stats fields
    pub async fn update_player_stats_fields(
        &self,
        player_id: Uuid,
        games_played: Option<u32>,
        games_won: Option<u32>,
        total_playtime: Option<u64>,
    ) -> Result<(), surrealdb::Error> {
        self.player_repo
            .update_stats_fields(player_id, games_played, games_won, total_playtime)
            .await
    }

    /// Fetch a specific game record
    pub async fn get_game_record(
        &self,
        game_id: Uuid,
    ) -> Result<Option<GameRecord>, surrealdb::Error> {
        self.game_repo.get(game_id).await
    }

    /// Delete an entire game and related records
    pub async fn delete_game(&self, game_id: Uuid) -> Result<(), surrealdb::Error> {
        self.game_repo.delete_game(game_id).await
    }

    /// Update a game's lifecycle
    pub async fn update_game_lifecycle(
        &self,
        game_id: Uuid,
        lifecycle: GameLifecycle,
    ) -> Result<(), surrealdb::Error> {
        self.game_repo.update_lifecycle(game_id, &lifecycle).await
    }

    /// Fetch raw game history records
    pub async fn game_events(
        &self,
        game_id: Uuid,
        since: Option<u64>,
        until: Option<u64>,
        event_kind: Option<String>,
    ) -> Result<Vec<GameEventRecord>, surrealdb::Error> {
        self.game_repo
            .get_history(game_id, since, until, event_kind)
            .await
    }

    /// Fetch recent game events with optional filtering
    pub async fn recent_game_events(
        &self,
        game_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<GameEventRecord>, surrealdb::Error> {
        self.game_repo.recent_events(game_id, limit).await
    }

    /// Fetch raw chat messages for a game
    pub async fn game_chat(
        &self,
        game_id: Uuid,
    ) -> Result<Vec<ChatMessageRecord>, surrealdb::Error> {
        self.game_repo.get_chat_messages(game_id).await
    }

    /// Delete a specific chat message
    pub async fn delete_chat_message(
        &self,
        game_id: Uuid,
        timestamp: u64,
    ) -> Result<(), surrealdb::Error> {
        self.game_repo.delete_chat_message(game_id, timestamp).await
    }

    /// List snapshot records for a game
    pub async fn list_snapshots(
        &self,
        game_id: Uuid,
    ) -> Result<Vec<SnapshotRecord>, surrealdb::Error> {
        self.game_repo.list_snapshots(game_id).await
    }

    /// Delete a snapshot record for a game
    pub async fn delete_snapshot(
        &self,
        game_id: Uuid,
        index: usize,
    ) -> Result<(), surrealdb::Error> {
        self.game_repo.delete_snapshot(game_id, index).await
    }

    /// Collect aggregate database statistics for admin API
    pub async fn database_stats(&self) -> Result<AdminDatabaseStats, surrealdb::Error> {
        let players = self.player_repo.list_all().await?;
        let games = self.game_repo.list_with_player_counts().await?;
        let sessions = self.session_repo.list_all().await?;
        let queue = self.matchmaking_repo.list_queue().await?;
        let chat_messages = self.game_repo.count_chat_messages().await?;
        let snapshots = self.game_repo.count_snapshots().await?;

        let now = timestamp();
        let active_sessions = sessions.iter().filter(|s| s.expires_at > now).count();

        Ok(AdminDatabaseStats {
            total_players: players.len(),
            total_games: games.len(),
            total_sessions: sessions.len(),
            active_sessions,
            queue_size: queue.len(),
            total_chat_messages: chat_messages,
            total_snapshots: snapshots,
        })
    }

    /// Count records in a set of tables
    pub async fn table_counts(
        &self,
        tables: &[&str],
    ) -> Result<Vec<(String, usize)>, surrealdb::Error> {
        #[derive(serde::Deserialize)]
        struct CountRow {
            count: i64,
        }

        let mut info = Vec::new();
        for table in tables {
            let query = format!("SELECT count() AS count FROM {} GROUP ALL", table);
            let mut response = self.game_repo.db.query(&query).await?;
            let rows: Vec<CountRow> = response.take(0)?;
            let count = rows
                .first()
                .map(|row| row.count.max(0) as usize)
                .unwrap_or(0);
            info.push((table.to_string(), count));
        }
        Ok(info)
    }

    /// Run an arbitrary query (admin-only tooling)
    pub async fn run_query(&self, query: &str) -> Result<surrealdb::Response, surrealdb::Error> {
        self.game_repo.db.query(query).await
    }

    /// Force complete a round through the game service
    pub async fn force_complete_round(
        &self,
        game_id: Uuid,
    ) -> Result<(CompleteRoundResponse, Vec<GameEventData>), AdminServiceError> {
        let started = self.game_service.get_game_created_at(game_id).await?;
        let result = self
            .game_service
            .force_complete_round(game_id, started)
            .await?;
        Ok(result)
    }

    /// Set game lifecycle using the service layer
    pub async fn set_game_lifecycle(
        &self,
        game_id: Uuid,
        lifecycle: GameLifecycle,
    ) -> Result<(), AdminServiceError> {
        self.game_service.set_lifecycle(game_id, lifecycle).await?;
        Ok(())
    }
}
