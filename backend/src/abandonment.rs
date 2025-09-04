use chrono::{Duration, Utc};
use tokio::time::{sleep, Duration as TokioDuration};
use uuid::Uuid;

use crate::{
    models::{DbGameStatus, GameStatus},
    state::AppState,
};

// Default timeout for abandonment (5 minutes)
const DEFAULT_ABANDONMENT_TIMEOUT_SECONDS: i64 = 300;

pub async fn abandonment_worker(state: AppState) {
    loop {
        // Check every 30 seconds
        sleep(TokioDuration::from_secs(30)).await;
        
        // Find games that might be abandoned
        match check_for_abandoned_games(&state).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Marked {} games as abandoned", count);
                }
            }
            Err(e) => {
                tracing::error!("Error checking for abandoned games: {:?}", e);
            }
        }
    }
}

async fn check_for_abandoned_games(state: &AppState) -> Result<usize, sqlx::Error> {
    let cutoff_time = Utc::now() - Duration::seconds(DEFAULT_ABANDONMENT_TIMEOUT_SECONDS);
    
    // Find active games where the last move was too long ago
    let abandoned_games = sqlx::query!(
        r#"
        SELECT id as "id!: Uuid", 
               white_player_id as "white_player_id?: Uuid", 
               black_player_id as "black_player_id?: Uuid", 
               time_control_seconds as "time_control_seconds?: i32", 
               last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
        FROM games 
        WHERE status = 'active' 
          AND last_move_at IS NOT NULL 
          AND last_move_at < ?
        "#,
        cutoff_time
    )
    .fetch_all(&state.db)
    .await?;
    
    let mut count = 0;
    
    for game in abandoned_games {
        // Check if the game has custom time controls
        let timeout_seconds = game.time_control_seconds
            .map(|tc| tc as i64)
            .unwrap_or(DEFAULT_ABANDONMENT_TIMEOUT_SECONDS);
        
        let custom_cutoff = Utc::now() - Duration::seconds(timeout_seconds);
        
        if let Some(last_move) = game.last_move_at {
            if last_move < custom_cutoff {
                // Mark game as abandoned
                mark_game_abandoned(state, game.id).await?;
                count += 1;
            }
        }
    }
    
    Ok(count)
}

async fn mark_game_abandoned(state: &AppState, game_id: uuid::Uuid) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    
    // Update game status
    sqlx::query!(
        r#"
        UPDATE games 
        SET status = ?, updated_at = ?
        WHERE id = ?
        "#,
        DbGameStatus(GameStatus::Abandoned),
        now,
        game_id
    )
    .execute(&state.db)
    .await?;
    
    // Remove from active games
    state.active_games.remove(&game_id);
    
    // Record metric
    crate::metrics::record_game_completed();
    
    // TODO: Send WebSocket notification to players
    
    Ok(())
}

// Helper function to update last move timestamp
pub async fn update_last_move_time(
    state: &AppState,
    game_id: uuid::Uuid,
    player_id: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    
    // Get current game state
    let game = sqlx::query!(
        r#"
        SELECT white_player_id as "white_player_id?: Uuid", 
               black_player_id as "black_player_id?: Uuid", 
               white_time_remaining_ms as "white_time_remaining_ms?: i32", 
               black_time_remaining_ms as "black_time_remaining_ms?: i32", 
               last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
        FROM games 
        WHERE id = ?
        "#,
        game_id
    )
    .fetch_one(&state.db)
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
    
    // Update the database
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
    .execute(&state.db)
    .await?;
    
    // Check if a player ran out of time
    if let Some(white_ms) = white_time {
        if white_ms <= 0 && game.white_player_id.is_some() {
            // White player lost on time
            end_game_on_time(state, game_id, game.black_player_id).await?;
        }
    }
    
    if let Some(black_ms) = black_time {
        if black_ms <= 0 && game.black_player_id.is_some() {
            // Black player lost on time
            end_game_on_time(state, game_id, game.white_player_id).await?;
        }
    }
    
    Ok(())
}

async fn end_game_on_time(
    state: &AppState,
    game_id: uuid::Uuid,
    winner_id: Option<uuid::Uuid>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    
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
    .execute(&state.db)
    .await?;
    
    // Remove from active games
    state.active_games.remove(&game_id);
    
    // Record metric
    crate::metrics::record_game_completed();
    
    // TODO: Send WebSocket notification
    
    Ok(())
}
