use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use automatafl_api::{LeaderboardEntry, LeaderboardResponse, UserInfo};

use crate::{
    error::Result,
    state::AppState,
};

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    #[serde(default = "default_page")]
    page: i32,
    #[serde(default = "default_per_page")]
    per_page: i32,
}

fn default_page() -> i32 {
    1
}

fn default_per_page() -> i32 {
    20
}

pub async fn get_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<LeaderboardResponse>> {
    // Validate pagination
    let page = query.page.max(1);
    let per_page = query.per_page.clamp(1, 100);
    let offset = (page - 1) * per_page;
    
    // Get total count of users
    let total = sqlx::query!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users"#
    )
    .fetch_one(&state.db)
    .await?
    .count;
    
    // Get leaderboard entries with win/loss statistics
    let entries = sqlx::query!(
        r#"
        SELECT 
            u.id as "user_id!: uuid::Uuid",
            u.username,
            u.rating,
            u.created_at as "created_at!: chrono::DateTime<chrono::Utc>",
            COALESCE(
                SUM(CASE 
                    WHEN g.winner_id = u.id THEN 1 
                    ELSE 0 
                END), 0
            ) as "wins!: i64",
            COALESCE(
                SUM(CASE 
                    WHEN g.status = 'completed' AND g.winner_id != u.id 
                         AND (g.white_player_id = u.id OR g.black_player_id = u.id) 
                    THEN 1 
                    ELSE 0 
                END), 0
            ) as "losses!: i64",
            COALESCE(
                SUM(CASE 
                    WHEN g.status = 'completed' AND g.winner_id IS NULL 
                         AND (g.white_player_id = u.id OR g.black_player_id = u.id)
                    THEN 1 
                    ELSE 0 
                END), 0
            ) as "draws!: i64"
        FROM users u
        LEFT JOIN games g ON (g.white_player_id = u.id OR g.black_player_id = u.id)
        GROUP BY u.id, u.username, u.rating, u.created_at
        ORDER BY u.rating DESC, u.username ASC
        LIMIT ? OFFSET ?
        "#,
        per_page,
        offset
    )
    .fetch_all(&state.db)
    .await?;
    
    // Convert to API response
    let mut leaderboard_entries = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        leaderboard_entries.push(LeaderboardEntry {
            rank: (offset as u32) + (index as u32) + 1,
            user: UserInfo {
                id: entry.user_id,
                username: entry.username,
                rating: entry.rating as i32,
                created_at: Some(entry.created_at),
            },
            wins: entry.wins as u32,
            losses: entry.losses as u32,
            draws: entry.draws as u32,
        });
    }
    
    Ok(Json(LeaderboardResponse {
        entries: leaderboard_entries,
        page,
        per_page,
        total,
    }))
}
