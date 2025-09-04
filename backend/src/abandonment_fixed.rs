// Fixed version of update_last_move_time with proper transaction handling
use chrono::{Duration, Utc};
use uuid::Uuid;
use crate::{state::AppState, models::{DbGameStatus, GameStatus}};

pub async fn update_last_move_time_fixed(
    state: &AppState,
    game_id: Uuid,
    player_id: Uuid,
) -> Result<(), sqlx::Error> {
    // Start a transaction to ensure atomicity
    let mut tx = state.db.begin().await?;
    
    let now = Utc::now();
    
    // Get current game state within transaction
    let game = sqlx::query!(
        r#"
        SELECT white_player_id as "white_player_id?: Uuid", 
               black_player_id as "black_player_id?: Uuid", 
               white_time_remaining_ms as "white_time_remaining_ms?: i32", 
               black_time_remaining_ms as "black_time_remaining_ms?: i32", 
               last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
        FROM games 
        WHERE id = ?
        FOR UPDATE  -- Lock the row for update
        "#,
        game_id
    )
    .fetch_one(&mut *tx)
    .await?;
    
    // Calculate time spent on this move
    let time_spent_ms = if let Some(last_move) = game.last_move_at {
        (now - last_move).num_milliseconds() as i32
    } else {
        0
    };
    
    // Update time remaining for the player who just moved
    let (white_time, black_time) = if game.white_player_id == Some(player_id) {
        (
            game.white_time_remaining_ms.map(|t| (t - time_spent_ms).max(0)),
            game.black_time_remaining_ms
        )
    } else if game.black_player_id == Some(player_id) {
        (
            game.white_time_remaining_ms,
            game.black_time_remaining_ms.map(|t| (t - time_spent_ms).max(0))
        )
    } else {
        (game.white_time_remaining_ms, game.black_time_remaining_ms)
    };
    
    // Update the database within the same transaction
    sqlx::query!(
        r#"
        UPDATE games 
        SET last_move_at = ?, 
            white_time_remaining_ms = ?,
            black_time_remaining_ms = ?
        WHERE id = ?
        "#,
        now,
        white_time,
        black_time,
        game_id
    )
    .execute(&mut *tx)
    .await?;
    
    // Check if a player ran out of time and update if necessary
    let mut game_ended = false;
    let mut winner_id = None;
    
    if let Some(white_ms) = white_time {
        if white_ms <= 0 && game.white_player_id.is_some() {
            // Black wins on time
            winner_id = game.black_player_id;
            game_ended = true;
        }
    }
    
    if let Some(black_ms) = black_time {
        if black_ms <= 0 && game.black_player_id.is_some() {
            // White wins on time
            winner_id = game.white_player_id;
            game_ended = true;
        }
    }
    
    if game_ended {
        sqlx::query!(
            r#"
            UPDATE games 
            SET status = ?, winner_id = ?, updated_at = ?
            WHERE id = ?
            "#,
            DbGameStatus(GameStatus::Completed),
            winner_id,
            now,
            game_id
        )
        .execute(&mut *tx)
        .await?;
    }
    
    // Commit the transaction
    tx.commit().await?;
    
    // Only update in-memory state after successful commit
    if game_ended {
        state.active_games.remove(&game_id);
        crate::metrics::record_game_completed();
    }
    
    Ok(())
}
