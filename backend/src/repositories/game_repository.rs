use crate::db::{
    ChatMessageRecord, Db, GameEventRecord, GamePlayerRecord, GameRecord, SnapshotRecord, as_uuid,
};
use automatafl_api_types::{GameEventData, GameLifecycle, GameListItem};
use automatafl_logic::{Game, Pid};
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::{collections::HashMap, pin::Pin};
use surrealdb::RecordId;
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

    /// Create a new game record
    pub async fn create(
        &self,
        game_id: Uuid,
        game_state: &Game,
        lifecycle: &GameLifecycle,
        created_by: Uuid,
        player_count: u8,
    ) -> Result<(), surrealdb::Error> {
        let game_record = GameRecord {
            id: RecordId::from_table_key("games", game_id),
            game_state: serde_json::to_string(game_state)
                .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?,
            lifecycle: serde_json::to_string(lifecycle)
                .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?,
            created_at: crate::common::timestamp(),
            created_by: RecordId::from_table_key("players", created_by),
            player_count,
        };

        let _: Option<GameRecord> = self
            .db
            .create(("games", game_id))
            .content(game_record)
            .await?;

        Ok(())
    }

    /// Fetch a raw game record
    pub async fn get(&self, game_id: Uuid) -> Result<Option<GameRecord>, surrealdb::Error> {
        self.db.select(("games", game_id)).await
    }

    /// List all game records
    pub async fn list_all(&self) -> Result<Vec<GameRecord>, surrealdb::Error> {
        self.db.select("games").await
    }

    /// Add a player association to a game
    pub async fn add_player(
        &self,
        game_id: Uuid,
        player_id: Uuid,
        player_pid: Pid,
    ) -> Result<(), surrealdb::Error> {
        let record = GamePlayerRecord {
            game_id: RecordId::from_table_key("games", game_id),
            player_id: RecordId::from_table_key("players", player_id),
            player_pid: player_pid.0,
        };

        let record_id = format!("{}:{}", game_id, player_id);

        self.db
            .create::<Option<GamePlayerRecord>>(("game_players", record_id))
            .content(record)
            .await?;

        Ok(())
    }

    /// Update only the lifecycle field for a game
    pub async fn update_lifecycle(
        &self,
        game_id: Uuid,
        lifecycle: &GameLifecycle,
    ) -> Result<(), surrealdb::Error> {
        let lifecycle_json = serde_json::to_string(lifecycle)
            .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;

        let _: Option<GameRecord> = self
            .db
            .update(("games", game_id))
            .merge(serde_json::json!({ "lifecycle": lifecycle_json }))
            .await?;

        Ok(())
    }

    /// Load game state, lifecycle, and player mappings
    pub async fn load(
        &self,
        game_id: Uuid,
    ) -> Result<Option<(Game, GameLifecycle, HashMap<Uuid, Pid>)>, surrealdb::Error> {
        // Get game record
        let game_record: Option<GameRecord> = self.db.select(("games", game_id)).await?;

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
            .bind(("game_id", RecordId::from_table_key("games", game_id)))
            .await?;

        let game_players: Vec<GamePlayerRecord> = result.take(0)?;
        let player_ids: HashMap<Uuid, Pid> = game_players
            .iter()
            .filter_map(|gp| Some(as_uuid(&gp.player_id)).map(|id| (id, Pid(gp.player_pid))))
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
        #[derive(Serialize, Clone)]
        struct EventRecord {
            game_id: RecordId,
            timestamp: u64,
            event: Value,
        }

        let event_records: Vec<EventRecord> = events
            .iter()
            .map(|event| {
                let value = serde_json::to_value(event).map_err(|e| {
                    surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string()))
                })?;
                Ok(EventRecord {
                    game_id: RecordId::from_table_key("games", game_id),
                    timestamp,
                    event: value,
                })
            })
            .collect::<Result<Vec<_>, surrealdb::Error>>()?;

        // Execute atomic transaction with explicit error handling
        self.db.query("BEGIN TRANSACTION;").await?;

        let outcome = async {
            self.db
                .query("UPDATE games SET game_state = $game_state, lifecycle = $lifecycle WHERE id = $id")
                .bind(("game_state", game_state_json.clone()))
                .bind(("lifecycle", lifecycle_json.clone()))
                .bind(("id", RecordId::from_table_key("games", game_id)))
                .await?;

            if !event_records.is_empty() {
                self.db
                    .query("INSERT INTO game_events $events")
                    .bind(("events", event_records.clone()))
                    .await?;
            }

            Ok::<(), surrealdb::Error>(())
        }
        .await;

        match outcome {
            Ok(()) => {
                self.db.query("COMMIT TRANSACTION;").await?;
            }
            Err(err) => {
                let _ = self.db.query("ROLLBACK TRANSACTION;").await;
                return Err(err);
            }
        }

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

    /// List recent games a player has participated in
    pub async fn list_recent_for_player(
        &self,
        player_id: Uuid,
        limit: usize,
    ) -> Result<Vec<GameListItem>, surrealdb::Error> {
        // Single query to fetch all games for a player with player counts
        // This eliminates the N+1 query problem
        // FIXED: Removed SQL-style AS aliasing - SurrealDB doesn't support it
        let query = r#"
            SELECT
                id, lifecycle, created_at, created_by,
                player_count as max_players,
                (SELECT count() FROM game_players WHERE game_id = $parent.id GROUP ALL)[0].count as current_players
            FROM games
            WHERE id IN (SELECT game_id FROM game_players WHERE player_id = $player_id)
            ORDER BY created_at DESC
            LIMIT $limit;
        "#;

        #[derive(serde::Deserialize)]
        struct QueryRow {
            id: RecordId,
            lifecycle: String,
            created_at: u64,
            created_by: RecordId,
            max_players: u8,
            current_players: Option<usize>,
        }

        let mut result = self
            .db
            .query(query)
            .bind(("player_id", RecordId::from_table_key("players", player_id)))
            .bind(("limit", limit))
            .await?;

        let rows: Vec<QueryRow> = result.take(0)?;

        let games = rows
            .into_iter()
            .filter_map(|row| {
                let lifecycle: GameLifecycle = serde_json::from_str(&row.lifecycle).ok()?;
                Some(GameListItem {
                    id: as_uuid(&row.id),
                    lifecycle,
                    player_count: row.current_players.unwrap_or(0),
                    max_players: row.max_players,
                    created_at: row.created_at,
                    created_by: as_uuid(&row.created_by),
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
            .bind(("game_id", RecordId::from_table_key("games", game_id)))
            .bind(("since", since))
            .bind(("until", until))
            .bind(("event_kind", event_kind))
            .await?;

        let events: Vec<GameEventRecord> = result.take(0)?;
        Ok(events)
    }

    /// Fetch recent game events with optional game filter
    pub async fn recent_events(
        &self,
        game_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<GameEventRecord>, surrealdb::Error> {
        let mut query = String::from("SELECT * FROM game_events");

        if game_id.is_some() {
            query.push_str(" WHERE game_id = $game_id");
        }

        query.push_str(" ORDER BY timestamp DESC LIMIT $limit");

        let builder = self.db.query(&query).bind(("limit", limit as i64));

        let mut response = if let Some(id) = game_id {
            builder
                .bind(("game_id", RecordId::from_table_key("games", id)))
                .await?
        } else {
            builder.await?
        };

        let events: Vec<GameEventRecord> = response.take(0)?;
        Ok(events)
    }

    /// Append a chat message to the game chat log
    pub async fn add_chat_message(
        &self,
        game_id: Uuid,
        timestamp: u64,
        player_id: Uuid,
        displayname: String,
        message: String,
    ) -> Result<(), surrealdb::Error> {
        let record = ChatMessageRecord {
            game_id: RecordId::from_table_key("games", game_id),
            timestamp,
            player_id: RecordId::from_table_key("players", player_id),
            displayname,
            message,
        };

        self.db
            .create::<Option<ChatMessageRecord>>("chat_messages")
            .content(record)
            .await?;

        Ok(())
    }

    /// Retrieve chat messages for a game
    pub async fn get_chat_messages(
        &self,
        game_id: Uuid,
    ) -> Result<Vec<ChatMessageRecord>, surrealdb::Error> {
        let mut result = self
            .db
            .query("SELECT * FROM chat_messages WHERE game_id = $game_id ORDER BY timestamp ASC")
            .bind(("game_id", RecordId::from_table_key("games", game_id)))
            .await?;

        let messages: Vec<ChatMessageRecord> = result.take(0)?;
        Ok(messages)
    }

    /// Append a single game event record
    pub async fn append_event(
        &self,
        game_id: Uuid,
        timestamp: u64,
        event: GameEventData,
    ) -> Result<(), surrealdb::Error> {
        let record = GameEventRecord {
            game_id: RecordId::from_table_key("games", game_id),
            timestamp,
            event: serde_json::to_value(event)
                .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?,
        };

        self.db
            .create::<Option<GameEventRecord>>("game_events")
            .content(record)
            .await?;

        Ok(())
    }

    /// Stream live game events as they are inserted
    pub async fn live_event_stream(
        &self,
        game_id: Uuid,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<GameEventRecord, surrealdb::Error>> + Send>>,
        surrealdb::Error,
    > {
        let mut result = self
            .db
            .query("LIVE SELECT * FROM game_events WHERE game_id = $game_id")
            .bind(("game_id", RecordId::from_table_key("games", game_id)))
            .await?;

        let stream = result.stream::<surrealdb::Notification<GameEventRecord>>(0)?;
        let mapped = stream.map(|notification| notification.map(|entry| entry.data));
        Ok(Box::pin(mapped))
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
    pub async fn list_snapshots(
        &self,
        game_id: Uuid,
    ) -> Result<Vec<SnapshotRecord>, surrealdb::Error> {
        let mut result = self
            .db
            .query("SELECT * FROM snapshots WHERE game_id = $game_id ORDER BY index ASC")
            .bind(("game_id", RecordId::from_table_key("games", game_id)))
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

    /// Delete a snapshot for a game by index
    pub async fn delete_snapshot(
        &self,
        game_id: Uuid,
        index: usize,
    ) -> Result<(), surrealdb::Error> {
        let record_id = format!("{}:{}", game_id, index);
        self.db
            .delete::<Option<SnapshotRecord>>(("snapshots", record_id))
            .await?;
        Ok(())
    }

    /// Delete a chat message for a game by timestamp
    pub async fn delete_chat_message(
        &self,
        game_id: Uuid,
        timestamp: u64,
    ) -> Result<(), surrealdb::Error> {
        let record_id = format!("{}:{}", game_id, timestamp);
        self.db
            .delete::<Option<ChatMessageRecord>>(("chat_messages", record_id))
            .await?;
        Ok(())
    }

    /// Remove all references of a player from game rosters
    pub async fn remove_player_from_all_games(
        &self,
        player_id: Uuid,
    ) -> Result<(), surrealdb::Error> {
        self.db
            .query("DELETE game_players WHERE player_id = $player_id")
            .bind(("player_id", RecordId::from_table_key("players", player_id)))
            .await?;
        Ok(())
    }

    /// Delete an entire game and related records atomically
    pub async fn delete_game(&self, game_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db.query("BEGIN TRANSACTION;").await?;

        let outcome = async {
            self.db
                .delete::<Option<GameRecord>>(("games", game_id))
                .await?;

            self.db
                .query("DELETE game_players WHERE game_id = $game_id")
                .bind(("game_id", RecordId::from_table_key("games", game_id)))
                .await?;

            self.db
                .query("DELETE game_events WHERE game_id = $game_id")
                .bind(("game_id", RecordId::from_table_key("games", game_id)))
                .await?;

            self.db
                .query("DELETE chat_messages WHERE game_id = $game_id")
                .bind(("game_id", RecordId::from_table_key("games", game_id)))
                .await?;

            self.db
                .query("DELETE snapshots WHERE game_id = $game_id")
                .bind(("game_id", RecordId::from_table_key("games", game_id)))
                .await?;

            Ok::<(), surrealdb::Error>(())
        }
        .await;

        match outcome {
            Ok(()) => {
                self.db.query("COMMIT TRANSACTION;").await?;
                Ok(())
            }
            Err(err) => {
                let _ = self.db.query("ROLLBACK TRANSACTION;").await;
                Err(err)
            }
        }
    }

    /// Count all chat messages across games
    pub async fn count_chat_messages(&self) -> Result<usize, surrealdb::Error> {
        #[derive(serde::Deserialize)]
        struct CountRow {
            count: i64,
        }

        let mut result = self
            .db
            .query("SELECT count() as count FROM chat_messages GROUP ALL")
            .await?;

        let rows: Vec<CountRow> = result.take(0)?;
        Ok(rows
            .first()
            .map(|row| row.count.max(0) as usize)
            .unwrap_or(0))
    }

    /// Count all snapshots across games
    pub async fn count_snapshots(&self) -> Result<usize, surrealdb::Error> {
        #[derive(serde::Deserialize)]
        struct CountRow {
            count: i64,
        }

        let mut result = self
            .db
            .query("SELECT count() as count FROM snapshots GROUP ALL")
            .await?;

        let rows: Vec<CountRow> = result.take(0)?;
        Ok(rows
            .first()
            .map(|row| row.count.max(0) as usize)
            .unwrap_or(0))
    }
}
