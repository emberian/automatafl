use std::{fmt, sync::Arc};

use crate::{
    db::{MatchmakingQueueRecord, as_uuid},
    repositories::{MatchmakingRepository, PlayerRepository},
};
use uuid::Uuid;

use super::GameService;

pub struct MatchmakingService {
    matchmaking_repo: MatchmakingRepository,
    player_repo: PlayerRepository,
    game_service: Arc<GameService>,
}

#[derive(Debug)]
pub enum MatchmakingServiceError {
    Database(surrealdb::Error),
    PlayerNotFound(Uuid),
}

#[derive(Clone)]
pub struct QueueEntry {
    pub player_id: Uuid,
    pub queued_at: u64,
    pub game_preferences: String,
}

impl From<MatchmakingQueueRecord> for QueueEntry {
    fn from(record: MatchmakingQueueRecord) -> Self {
        Self {
            player_id: as_uuid(&record.player_id),
            queued_at: record.queued_at,
            game_preferences: record.game_preferences,
        }
    }
}

impl From<surrealdb::Error> for MatchmakingServiceError {
    fn from(value: surrealdb::Error) -> Self {
        Self::Database(value)
    }
}

impl fmt::Display for MatchmakingServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatchmakingServiceError::Database(err) => write!(f, "Database error: {}", err),
            MatchmakingServiceError::PlayerNotFound(id) => write!(f, "Player not found: {}", id),
        }
    }
}

impl std::error::Error for MatchmakingServiceError {}

impl MatchmakingService {
    pub fn new(
        matchmaking_repo: MatchmakingRepository,
        player_repo: PlayerRepository,
        game_service: Arc<GameService>,
    ) -> Self {
        Self {
            matchmaking_repo,
            player_repo,
            game_service,
        }
    }

    pub async fn join_queue(
        &self,
        player_id: Uuid,
        queued_at: u64,
        preferences: String,
    ) -> Result<(), MatchmakingServiceError> {
        // Ensure player exists before queueing
        if self.player_repo.get(player_id).await?.is_none() {
            return Err(MatchmakingServiceError::PlayerNotFound(player_id));
        }

        self.matchmaking_repo
            .join_queue(player_id, queued_at, preferences)
            .await?;
        Ok(())
    }

    pub async fn leave_queue(&self, player_id: Uuid) -> Result<(), MatchmakingServiceError> {
        self.matchmaking_repo.leave_queue(player_id).await?;
        Ok(())
    }

    pub async fn get_status(
        &self,
        player_id: Uuid,
    ) -> Result<Option<QueueEntry>, MatchmakingServiceError> {
        let record = self.matchmaking_repo.get_status(player_id).await?;
        Ok(record.map(QueueEntry::from))
    }

    pub async fn list_queue(&self) -> Result<Vec<QueueEntry>, MatchmakingServiceError> {
        let records = self.matchmaking_repo.list_queue().await?;
        Ok(records.into_iter().map(QueueEntry::from).collect())
    }

    pub async fn remove_players(&self, player_ids: &[Uuid]) -> Result<(), MatchmakingServiceError> {
        self.matchmaking_repo.remove_players(player_ids).await?;
        Ok(())
    }

    pub fn game_service(&self) -> Arc<GameService> {
        Arc::clone(&self.game_service)
    }
}
