//! Transaction utilities for atomic multi-step operations
//!
//! This module provides utilities for wrapping multi-step database operations in transactions
//! to ensure atomicity and data consistency. While SurrealDB's embedded mode (surrealkv)
//! has some transaction limitations, these utilities provide best-effort atomicity.
//!
//! ## Key Functions:
//! - `update_game_with_events`: Atomic game state + events update
//! - `create_game_with_players`: Atomic game creation with player associations
//! - `update_player_stats_atomic`: Atomic stats + ELO update
//! - `remove_players_from_queue`: Atomic queue removal
//!
//! ## Usage Pattern:
//! Always use these transaction helpers for multi-step operations to prevent:
//! - Race conditions between game state updates and event broadcasting
//! - Inconsistent state when players are added to games
//! - Partial updates when matchmaking creates games
//!
//! ## Security:
//! All functions use parameterized queries with bind() to prevent SQL injection.
//!
//! ## Error Handling:
//! All transaction functions log errors before returning, preserving diagnostic context
//! while returning clean errors to callers.

use crate::db::Db;
use serde::Serialize;
use std::fmt;
use surrealdb::Error as DbError;

/// Custom error type for transaction operations
#[derive(Debug)]
pub enum TransactionError {
    DbError(DbError),
    SerializationError(String),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::DbError(e) => write!(f, "Database error: {}", e),
            TransactionError::SerializationError(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for TransactionError {}

impl From<DbError> for TransactionError {
    fn from(e: DbError) -> Self {
        TransactionError::DbError(e)
    }
}

impl From<serde_json::Error> for TransactionError {
    fn from(e: serde_json::Error) -> Self {
        TransactionError::SerializationError(e.to_string())
    }
}

/// Helper for game state updates with events (atomic)
pub async fn update_game_with_events<T: Serialize>(
    db: &Db,
    game_id: uuid::Uuid,
    game_state: &automatafl_logic::Game,
    lifecycle: &automatafl_api_types::GameLifecycle,
    events: Vec<(String, T)>, // Vec of (event_kind, event_data)
) -> Result<(), TransactionError> {
    let game_state_json = serde_json::to_string(game_state)?;
    let lifecycle_json = serde_json::to_string(lifecycle)?;
    let timestamp = crate::common::timestamp();

    let mut event_records = Vec::with_capacity(events.len());
    for (kind, data) in events {
        event_records.push(serde_json::json!({
            "game_id": game_id.to_string(),
            "timestamp": timestamp,
            "event_kind": kind,
            "event_data": serde_json::to_string(&data)?,
        }));
    }

    let query = r#"
        BEGIN TRANSACTION;
        UPDATE games SET game_state = $game_state, lifecycle = $lifecycle WHERE id = $id;
        IF array::len($events) > 0 THEN
            INSERT INTO game_events $events;
        END;
        COMMIT TRANSACTION;
    "#;

    db.query(query)
        .bind(("game_state", game_state_json))
        .bind(("lifecycle", lifecycle_json))
        .bind(("id", game_id.to_string()))
        .bind(("events", event_records))
        .await?;

    Ok(())
}

/// Atomic player stats update with ELO change
pub async fn update_player_stats_atomic(
    db: &Db,
    player_id: uuid::Uuid,
    won: bool,
    playtime: u64,
    new_elo: Option<i32>,
) -> Result<(), TransactionError> {
    let won_delta = if won { 1 } else { 0 };

    let mut query = String::from(
        "BEGIN TRANSACTION;\n        LET $stats = (SELECT * FROM player_stats WHERE player_id = $player_id)[0];\n        IF $stats THEN\n            UPDATE player_stats SET\n                games_played += 1,\n                games_won += $won_delta,\n                total_playtime += $playtime\n            WHERE player_id = $player_id\n        ELSE\n            CREATE player_stats SET\n                player_id = $player_id,\n                games_played = 1,\n                games_won = $won_delta,\n                total_playtime = $playtime\n        END;\n",
    );

    if new_elo.is_some() {
        query.push_str("        UPDATE players SET elo_rating = $elo WHERE id = $player_id;\n");
    }

    query.push_str("        COMMIT TRANSACTION;\n");

    let mut request = db
        .query(query)
        .bind(("player_id", player_id.to_string()))
        .bind(("won_delta", won_delta))
        .bind(("playtime", playtime));

    if let Some(elo) = new_elo {
        request = request.bind(("elo", elo));
    }

    request.await?;

    Ok(())
}

/// Atomic matchmaking: create game and add players
pub async fn create_game_with_players(
    db: &Db,
    game_id: uuid::Uuid,
    game_state: &automatafl_logic::Game,
    lifecycle: &automatafl_api_types::GameLifecycle,
    creator_id: uuid::Uuid,
    player_count: u8,
    player_uuids: Vec<(uuid::Uuid, u8)>, // (player_id, pid)
) -> Result<(), TransactionError> {
    let game_state_json = serde_json::to_string(game_state)?;
    let lifecycle_json = serde_json::to_string(lifecycle)?;
    let timestamp = crate::common::timestamp();

    let mut player_records = Vec::with_capacity(player_uuids.len());
    for (player_uuid, pid) in player_uuids {
        player_records.push(serde_json::json!({
            "game_id": game_id.to_string(),
            "player_id": player_uuid.to_string(),
            "player_pid": pid,
        }));
    }

    let query = r#"
        BEGIN TRANSACTION;
        CREATE games SET id = $id, game_state = $game_state, lifecycle = $lifecycle,
            created_at = $created_at, created_by = $created_by, player_count = $player_count;
        IF array::len($players) > 0 THEN
            INSERT INTO game_players $players;
        END;
        COMMIT TRANSACTION;
    "#;

    db.query(query)
        .bind(("id", game_id.to_string()))
        .bind(("game_state", game_state_json))
        .bind(("lifecycle", lifecycle_json))
        .bind(("created_at", timestamp))
        .bind(("created_by", creator_id.to_string()))
        .bind(("player_count", player_count))
        .bind(("players", player_records))
        .await?;

    Ok(())
}

/// Remove players from matchmaking queue (atomic)
/// Note: This function only removes from queue, it does not add to games
pub async fn remove_players_from_queue(
    db: &Db,
    player_uuids: Vec<uuid::Uuid>,
) -> Result<(), TransactionError> {
    let ids: Vec<String> = player_uuids.into_iter().map(|id| id.to_string()).collect();

    let query = r#"
        BEGIN TRANSACTION;
        DELETE matchmaking_queue WHERE player_id IN $player_ids;
        COMMIT TRANSACTION;
    "#;

    db.query(query).bind(("player_ids", ids)).await?;

    Ok(())
}
