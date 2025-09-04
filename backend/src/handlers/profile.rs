use axum::{
    extract::{Path, State},
    Json,
};
use uuid::Uuid;

use automatafl_api::{UserProfileResponse, UserInfo, UserStats};

use crate::{
    error::{AppError, Result},
    models::GameSummary,
    state::AppState,
};

pub async fn get_user_profile(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserProfileResponse>> {
    // Get user information
    let user_row = sqlx::query!(
        r#"SELECT 
            id as "id!: Uuid", 
            username, 
            rating, 
            created_at as "created_at!: chrono::DateTime<chrono::Utc>" 
        FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;
    
    let user = UserInfo {
        id: user_row.id,
        username: user_row.username,
        rating: user_row.rating as i32,
        created_at: Some(user_row.created_at),
    };
    
    // Get user statistics
    let stats_row = sqlx::query!(
        r#"
        SELECT 
            COUNT(DISTINCT g.id) as "total_games!: i64",
            COALESCE(SUM(CASE WHEN g.winner_id = ? THEN 1 ELSE 0 END), 0) as "wins!: i64",
            COALESCE(SUM(CASE 
                WHEN g.status = 'completed' AND g.winner_id != ? 
                     AND (g.white_player_id = ? OR g.black_player_id = ?) 
                THEN 1 ELSE 0 
            END), 0) as "losses!: i64",
            COALESCE(SUM(CASE 
                WHEN g.status = 'completed' AND g.winner_id IS NULL 
                     AND (g.white_player_id = ? OR g.black_player_id = ?)
                THEN 1 ELSE 0 
            END), 0) as "draws!: i64"
        FROM games g
        WHERE (g.white_player_id = ? OR g.black_player_id = ?)
        "#,
        user_id, user_id, user_id, user_id, user_id, user_id, user_id, user_id
    )
    .fetch_one(&state.db)
    .await?;
    
    // Calculate additional statistics
    let total_decided_games = stats_row.wins + stats_row.losses;
    let win_rate = if total_decided_games > 0 {
        (stats_row.wins as f32 / total_decided_games as f32) * 100.0
    } else {
        0.0
    };
    
    // Get rating history for highest/lowest rating
    let rating_stats = sqlx::query!(
        r#"
        SELECT 
            MAX(rating_after) as "highest_rating?: i32",
            MIN(rating_after) as "lowest_rating?: i32"
        FROM rating_history
        WHERE user_id = ?
        "#,
        user_id
    )
    .fetch_one(&state.db)
    .await?;
    
    // Get current streak
    let recent_games = sqlx::query!(
        r#"
        SELECT 
            g.id,
            g.winner_id as "winner_id?: Uuid",
            g.status,
            g.created_at
        FROM games g
        WHERE (g.white_player_id = ? OR g.black_player_id = ?) 
              AND g.status = 'completed'
        ORDER BY g.created_at DESC
        LIMIT 20
        "#,
        user_id, user_id
    )
    .fetch_all(&state.db)
    .await?;
    
    let mut current_streak = 0;
    for game in recent_games {
        if let Some(winner_id) = game.winner_id {
            if winner_id == user_id {
                if current_streak >= 0 {
                    current_streak += 1;
                } else {
                    break;
                }
            } else {
                if current_streak <= 0 {
                    current_streak -= 1;
                } else {
                    break;
                }
            }
        } else {
            // Draw breaks streak
            break;
        }
    }
    
    let stats = UserStats {
        total_games: stats_row.total_games as u32,
        wins: stats_row.wins as u32,
        losses: stats_row.losses as u32,
        draws: stats_row.draws as u32,
        win_rate,
        highest_rating: rating_stats.highest_rating.unwrap_or(user.rating),
        lowest_rating: rating_stats.lowest_rating.unwrap_or(user.rating),
        current_streak,
    };
    
    // Get recent games
    let recent_games = sqlx::query!(
        r#"
        SELECT 
            g.id as "id!: Uuid",
            g.white_player_id as "white_player_id?: Uuid",
            g.black_player_id as "black_player_id?: Uuid",
            g.status,
            g.winner_id as "winner_id?: Uuid",
            g.created_at as "created_at!: chrono::DateTime<chrono::Utc>",
            g.updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
            w.username as "white_username?: String",
            b.username as "black_username?: String",
            winner.username as "winner_username?: String",
            COALESCE((SELECT COUNT(*) FROM game_moves WHERE game_id = g.id), 0) as "move_count!: i64"
        FROM games g
        LEFT JOIN users w ON g.white_player_id = w.id
        LEFT JOIN users b ON g.black_player_id = b.id
        LEFT JOIN users winner ON g.winner_id = winner.id
        WHERE (g.white_player_id = ? OR g.black_player_id = ?)
        ORDER BY g.created_at DESC
        LIMIT 10
        "#,
        user_id, user_id
    )
    .fetch_all(&state.db)
    .await?;
    
    let recent_games_summary: Vec<GameSummary> = recent_games
        .into_iter()
        .map(|g| {
            use automatafl_api::{GameStatus, PlayerInfo};
            
            let status = match g.status.as_str() {
                "waiting" => GameStatus::Waiting,
                "active" => GameStatus::Active,
                "completed" => GameStatus::Completed,
                "abandoned" => GameStatus::Abandoned,
                _ => GameStatus::Abandoned,
            };
            
            GameSummary {
                id: g.id,
                white_player: g.white_player_id.zip(g.white_username).map(|(id, username)| PlayerInfo { id, username }),
                black_player: g.black_player_id.zip(g.black_username).map(|(id, username)| PlayerInfo { id, username }),
                status,
                winner: g.winner_id.zip(g.winner_username).map(|(id, username)| PlayerInfo { id, username }),
                created_at: g.created_at,
                updated_at: g.updated_at,
                total_moves: g.move_count as i32,
            }
        })
        .collect();
    
    Ok(Json(UserProfileResponse {
        user,
        stats,
        recent_games: recent_games_summary,
    }))
}
