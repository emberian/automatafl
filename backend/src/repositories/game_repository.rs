use crate::db::{Db, GameEventRecord, GamePlayerRecord, GameRecord, SnapshotRecord, as_uuid};
use automatafl_api_types::{GameEventData, GameLifecycle, GameListItem};
use automatafl_logic::{Game, Pid};
use surrealdb::RecordId;
use std::collections::HashMap;
use uuid::Uuid;

/// Repository for game-related database operations
pub struct GameRepository {
    pub(crate) db: Db, // Accessible within backend crate for services
}

/// Persisted event with metadata returned after save
pub struct PersistedEvent {
    pub timestamp: u64,
    pub event: GameEventData,
}

impl GameRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Load game state, lifecycle, and player mappings
    pub async fn load(
        &self,
        game_id: Uuid,
    ) -> Result<Option<(Game, GameLifecycle, HashMap<Uuid, Pid>)>, surrealdb::Error> {
        // Get game record
        let game_record: Option<GameRecord> =
            self.db.select(("games", game_id.to_string())).await?;

        let game_record = match game_record {
            Some(g) => g,
            None => return Ok(None),
        };

        // Deserialize game state
        let game_state: Game = serde_json::from_str(&game_record.game_state)
            .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;

        let lifecycle: GameLifecycle = serde_json::from_str(&game_record.lifecycle)
            .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;

        // Get players
        let mut result = self
            .db
            .query("SELECT * FROM game_players WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        let game_players: Vec<GamePlayerRecord> = result.take(0)?;
        let player_ids: HashMap<Uuid, Pid> = game_players
            .iter()
            .filter_map(|gp| {
                Some(as_uuid(&gp.player_id))
                    .map(|id| (id, Pid(gp.player_pid)))
            })
            .collect();

        Ok(Some((game_state, lifecycle, player_ids)))
    }

    /// Atomically save game state and events in a single transaction
    /// Returns the persisted events for broadcasting
    pub async fn save_with_events(
        &self,
        game_id: Uuid,
        game: &Game,
        lifecycle: &GameLifecycle,
        events: &[GameEventData],
    ) -> Result<Vec<PersistedEvent>, surrealdb::Error> {
        use serde::Serialize;

        let game_state_json = serde_json::to_string(game)
            .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;
        let lifecycle_json = serde_json::to_string(lifecycle)
            .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;
        let timestamp = crate::common::timestamp();

        // Create typed event records
        #[derive(Serialize)]
        struct EventRecord {
            game_id: String,
            timestamp: u64,
            event: GameEventData,
        }

        let event_records: Vec<EventRecord> = events
            .iter()
            .map(|event| EventRecord {
                game_id: game_id.to_string(),
                timestamp,
                event: event.clone(),
            })
            .collect();

        // Execute atomic transaction
        let query = r#"
            BEGIN TRANSACTION;
            UPDATE games SET game_state = $game_state, lifecycle = $lifecycle WHERE id = $id;
            IF array::len($events) > 0 THEN
                INSERT INTO game_events $events;
            END;
            COMMIT TRANSACTION;
        "#;

        self.db
            .query(query)
            .bind(("game_state", game_state_json))
            .bind(("lifecycle", lifecycle_json))
            .bind(("id", game_id.to_string()))
            .bind(("events", event_records))
            .await?;

        // Return persisted events
        Ok(events
            .iter()
            .map(|e| PersistedEvent {
                timestamp,
                event: e.clone(),
            })
            .collect())
    }

    /// List all games with player counts in a single query (fixes N+1)
    pub async fn list_with_player_counts(&self) -> Result<Vec<GameListItem>, surrealdb::Error> {
        // Use subquery to get player count in single query
        let query = r#"
            SELECT
                id,
                lifecycle,
                player_count as max_players,
                created_at,
                created_by,
                (SELECT count() FROM game_players WHERE game_id = $parent.id GROUP ALL)[0].count as current_players
            FROM games
            ORDER BY created_at DESC
        "#;

        let mut result = self.db.query(query).await?;

        #[derive(serde::Deserialize)]
        struct GameRow {
            id: RecordId,
            lifecycle: String,
            max_players: u8,
            created_at: u64,
            created_by: RecordId,
            current_players: Option<i64>,
        }

        let rows: Vec<GameRow> = result.take(0)?;

        let games: Vec<GameListItem> = rows
            .into_iter()
            .filter_map(|row| {
                let game_id = as_uuid(&row.id);
                let lifecycle = serde_json::from_str(&row.lifecycle).ok()?;
                let created_by = as_uuid(&row.created_by);

                Some(GameListItem {
                    id: game_id,
                    lifecycle,
                    player_count: row.current_players.unwrap_or(0) as usize,
                    max_players: row.max_players,
                    created_at: row.created_at,
                    created_by,
                })
            })
            .collect();

        Ok(games)
    }

    /// Get game history with optional filtering
    pub async fn get_history(
        &self,
        game_id: Uuid,
        since: Option<u64>,
        until: Option<u64>,
        event_kind: Option<String>,
    ) -> Result<Vec<GameEventRecord>, surrealdb::Error> {
        let mut query = "SELECT * FROM game_events WHERE game_id = $game_id".to_string();

        if since.is_some() {
            query.push_str(" AND timestamp >= $since");
        }
        if until.is_some() {
            query.push_str(" AND timestamp <= $until");
        }
        if event_kind.is_some() {
            query.push_str(" AND event.kind = $event_kind");
        }

        query.push_str(" ORDER BY timestamp ASC");

        let mut result = self
            .db
            .query(&query)
            .bind(("game_id", game_id.to_string()))
            .bind(("since", since))
            .bind(("until", until))
            .bind(("event_kind", event_kind))
            .await?;

        let events: Vec<GameEventRecord> = result.take(0)?;
        Ok(events)
    }

    /// Save a snapshot
    pub async fn save_snapshot(
        &self,
        game_id: Uuid,
        index: usize,
        timestamp: u64,
        snapshot_data: &str,
    ) -> Result<(), surrealdb::Error> {
        let record = SnapshotRecord {
            game_id: RecordId::from_table_key("games", game_id),
            index,
            timestamp,
            snapshot_data: snapshot_data.to_string(),
        };

        let record_id = format!("{}:{}", game_id, index);

        self.db
            .create::<Option<SnapshotRecord>>(("snapshots", record_id))
            .content(record)
            .await?;

        Ok(())
    }

    /// List snapshots for a game
    pub async fn list_snapshots(&self, game_id: Uuid) -> Result<Vec<SnapshotRecord>, surrealdb::Error> {
        let mut result = self
            .db
            .query("SELECT * FROM snapshots WHERE game_id = $game_id ORDER BY index ASC")
            .bind(("game_id", game_id.to_string()))
            .await?;

        let snapshots: Vec<SnapshotRecord> = result.take(0)?;
        Ok(snapshots)
    }

    /// Get a specific snapshot
    pub async fn get_snapshot(
        &self,
        game_id: Uuid,
        index: usize,
    ) -> Result<Option<SnapshotRecord>, surrealdb::Error> {
        let record_id = format!("{}:{}", game_id, index);
        self.db.select(("snapshots", record_id)).await
    }
}