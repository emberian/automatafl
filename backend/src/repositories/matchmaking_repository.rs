use crate::db::{Db, MatchmakingQueueRecord};
use surrealdb::RecordId;
use uuid::Uuid;

/// Repository for matchmaking queue operations
pub struct MatchmakingRepository {
    db: Db,
}

impl MatchmakingRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Add a player to the matchmaking queue
    pub async fn join_queue(
        &self,
        player_id: Uuid,
        queued_at: u64,
        preferences: String,
    ) -> Result<(), surrealdb::Error> {
        let record = MatchmakingQueueRecord {
            player_id: RecordId::from_table_key("players", player_id),
            queued_at,
            game_preferences: preferences,
        };

        self.db
            .create::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id))
            .content(record)
            .await?;

        Ok(())
    }

    /// Remove a player from the matchmaking queue
    pub async fn leave_queue(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db
            .delete::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id))
            .await?;
        Ok(())
    }

    /// Fetch matchmaking queue record for a specific player
    pub async fn get_status(
        &self,
        player_id: Uuid,
    ) -> Result<Option<MatchmakingQueueRecord>, surrealdb::Error> {
        self.db.select(("matchmaking_queue", player_id)).await
    }

    /// Retrieve the entire matchmaking queue
    pub async fn list_queue(&self) -> Result<Vec<MatchmakingQueueRecord>, surrealdb::Error> {
        self.db.select("matchmaking_queue").await
    }

    /// Remove a batch of players from the queue
    pub async fn remove_players(&self, player_ids: &[Uuid]) -> Result<(), surrealdb::Error> {
        for player_id in player_ids {
            let _ = self
                .db
                .delete::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", *player_id))
                .await;
        }
        Ok(())
    }

    /// Remove a single player without ignoring errors
    pub async fn remove_player(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db
            .delete::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id))
            .await?;
        Ok(())
    }
}
