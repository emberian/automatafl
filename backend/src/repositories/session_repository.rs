use crate::db::{Db, SessionRecord};
use surrealdb::RecordId;
use uuid::Uuid;

/// Repository for session-related database operations
pub struct SessionRepository {
    db: Db,
}

impl SessionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Create a new session with expiration metadata
    pub async fn create(
        &self,
        session_id: Uuid,
        player_id: Uuid,
        expires_at: u64,
    ) -> Result<(), surrealdb::Error> {
        let record = SessionRecord {
            id: RecordId::from_table_key("sessions", session_id),
            player_id: RecordId::from_table_key("players", player_id),
            expires_at,
        };

        self.db
            .create::<Option<SessionRecord>>(("sessions", session_id))
            .content(record)
            .await?;

        Ok(())
    }

    /// Fetch a session by identifier
    pub async fn get(&self, session_id: Uuid) -> Result<Option<SessionRecord>, surrealdb::Error> {
        self.db.select(("sessions", session_id)).await
    }

    /// Delete a session by identifier
    pub async fn delete(&self, session_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db
            .delete::<Option<SessionRecord>>(("sessions", session_id))
            .await?;
        Ok(())
    }

    /// Delete all sessions belonging to a player
    pub async fn delete_by_player(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        // FIXED: Use RecordId instead of string
        self.db
            .query("DELETE sessions WHERE player_id = $player_id")
            .bind(("player_id", RecordId::from_table_key("players", player_id)))
            .await?;
        Ok(())
    }

    /// List all sessions (admin use case)
    pub async fn list_all(&self) -> Result<Vec<SessionRecord>, surrealdb::Error> {
        self.db.select("sessions").await
    }

    /// Remove expired sessions, returning the number of deleted rows
    pub async fn cleanup_expired(&self, now: u64) -> Result<u64, surrealdb::Error> {
        let mut result = self
            .db
            .query("DELETE sessions WHERE expires_at < $now RETURN BEFORE")
            .bind(("now", now))
            .await?;

        let deleted: Vec<SessionRecord> = result.take(0).unwrap_or_default();
        Ok(deleted.len() as u64)
    }
}
