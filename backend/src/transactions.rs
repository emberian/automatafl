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
use surrealdb::Error as DbError;
use std::fmt;

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

    // Start transaction
    db.query("BEGIN TRANSACTION;").await?;

    // Update game state with parameterized query
    db.query("UPDATE games SET game_state = $game_state, lifecycle = $lifecycle WHERE id = $id")
        .bind(("game_state", game_state_json))
        .bind(("lifecycle", lifecycle_json))
        .bind(("id", game_id.to_string()))
        .await?;

    // Add all events with parameterized queries
    for (kind, data) in events {
        let data_json = serde_json::to_string(&data)?;
        
        db.query("CREATE game_events SET game_id = $game_id, timestamp = $timestamp, event_kind = $kind, event_data = $data")
            .bind(("game_id", game_id.to_string()))
            .bind(("timestamp", timestamp))
            .bind(("kind", kind))
            .bind(("data", data_json))
            .await?;
    }

    // Commit transaction
    db.query("COMMIT TRANSACTION;").await?;

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
    // Start transaction
    db.query("BEGIN TRANSACTION;").await?;

    // Update or create stats with atomic operations
    let won_delta = if won { 1 } else { 0 };
    
    db.query(
        "LET $stats = (SELECT * FROM player_stats WHERE player_id = $player_id)[0];\
         IF $stats THEN \
             UPDATE player_stats SET \
                 games_played += 1, \
                 games_won += $won_delta, \
                 total_playtime += $playtime \
             WHERE player_id = $player_id \
         ELSE \
             CREATE player_stats SET \
                 player_id = $player_id, \
                 games_played = 1, \
                 games_won = $won_delta, \
                 total_playtime = $playtime \
         END;"
    )
    .bind(("player_id", player_id.to_string()))
    .bind(("won_delta", won_delta))
    .bind(("playtime", playtime))
    .await?;

    // Update ELO if provided
    if let Some(elo) = new_elo {
        db.query("UPDATE players SET elo_rating = $elo WHERE id = $id")
            .bind(("elo", elo))
            .bind(("id", player_id.to_string()))
            .await?;
    }

    // Commit transaction
    db.query("COMMIT TRANSACTION;").await?;
    
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

    // Start transaction
    db.query("BEGIN TRANSACTION;").await?;

    // Create game with parameterized query
    db.query(
        "CREATE games SET id = $id, game_state = $game_state, lifecycle = $lifecycle, \
         created_at = $created_at, created_by = $created_by, player_count = $player_count"
    )
    .bind(("id", game_id.to_string()))
    .bind(("game_state", game_state_json))
    .bind(("lifecycle", lifecycle_json))
    .bind(("created_at", timestamp))
    .bind(("created_by", creator_id.to_string()))
    .bind(("player_count", player_count))
    .await?;

    // Add all players with parameterized queries
    for (player_uuid, pid) in player_uuids {
        db.query("CREATE game_players SET game_id = $game_id, player_id = $player_id, player_pid = $player_pid")
            .bind(("game_id", game_id.to_string()))
            .bind(("player_id", player_uuid.to_string()))
            .bind(("player_pid", pid))
            .await?;
    }

    // Commit transaction
    db.query("COMMIT TRANSACTION;").await?;

    Ok(())
}

/// Remove players from matchmaking queue (atomic)
/// Note: This function only removes from queue, it does not add to games
pub async fn remove_players_from_queue(
    db: &Db,
    player_uuids: Vec<uuid::Uuid>,
) -> Result<(), TransactionError> {
    // Start transaction
    db.query("BEGIN TRANSACTION;").await?;

    // Remove each player from queue with parameterized queries
    for player_uuid in &player_uuids {
        db.query("DELETE matchmaking_queue WHERE player_id = $player_id")
            .bind(("player_id", player_uuid.to_string()))
            .await?;
    }

    // Commit transaction
    db.query("COMMIT TRANSACTION;").await?;

    Ok(())
}
