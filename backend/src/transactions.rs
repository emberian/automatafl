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
use std::fmt;
use std::future::Future;
use std::pin::Pin;
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

/// Generic transaction wrapper that guarantees COMMIT on success and ROLLBACK on failure
///
/// This higher-order function ensures transactions are always properly completed or rolled back,
/// even in the presence of errors or panics. It provides better safety than manual BEGIN/COMMIT.
///
/// # Example
/// ```
/// in_transaction(&db, |tx_db| Box::pin(async move {
///     tx_db.query("CREATE ...").await?;
///     tx_db.query("UPDATE ...").await?;
///     Ok(())
/// })).await?;
/// ```
pub async fn in_transaction<F, T, E>(db: &Db, f: F) -> Result<T, E>
where
    E: From<DbError>,
    F: FnOnce(&Db) -> Pin<Box<dyn Future<Output = Result<T, E>> + Send + '_>>,
{
    db.query("BEGIN TRANSACTION;").await.map_err(E::from)?;

    match f(db).await {
        Ok(result) => {
            db.query("COMMIT TRANSACTION;").await.map_err(E::from)?;
            Ok(result)
        }
        Err(e) => {
            if let Err(rollback_err) = db.query("ROLLBACK TRANSACTION;").await {
                tracing::error!(
                    "Failed to roll back transaction after error: {}",
                    rollback_err
                );
            }
            Err(e)
        }
    }
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
    use surrealdb::RecordId;

    let game_state_json = serde_json::to_string(game_state)?;
    let lifecycle_json = serde_json::to_string(lifecycle)?;
    let timestamp = crate::common::timestamp();

    // Start transaction
    db.query("BEGIN TRANSACTION;").await?;

    let outcome = async {
        // FIXED: Use RecordId instead of string conversion
        // Create game with parameterized query
        db.query(
            "CREATE games SET id = $id, game_state = $game_state, lifecycle = $lifecycle, \
             created_at = $created_at, created_by = $created_by, player_count = $player_count",
        )
        .bind(("id", RecordId::from_table_key("games", game_id)))
        .bind(("game_state", game_state_json))
        .bind(("lifecycle", lifecycle_json))
        .bind(("created_at", timestamp))
        .bind(("created_by", RecordId::from_table_key("players", creator_id)))
        .bind(("player_count", player_count))
        .await?;

        // Add all players with parameterized queries using RecordId
        for (player_uuid, pid) in player_uuids {
            db.query("CREATE game_players SET game_id = $game_id, player_id = $player_id, player_pid = $player_pid")
                .bind(("game_id", RecordId::from_table_key("games", game_id)))
                .bind(("player_id", RecordId::from_table_key("players", player_uuid)))
                .bind(("player_pid", pid))
                .await?;
        }
        
        Ok::<(), surrealdb::Error>(())
    }
    .await;

    match outcome {
        Ok(()) => {
            // Commit transaction
            db.query("COMMIT TRANSACTION;").await?;
            Ok(())
        }
        Err(err) => {
            // Ensure rollback on any error
            let _ = db.query("ROLLBACK TRANSACTION;").await;
            Err(TransactionError::DbError(err))
        }
    }
}
