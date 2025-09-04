use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{Game, GameStatus, DbGameStatus, GameHistoryResponse, GameSummary, PlayerInfo, MoveWithPlayer, UserGamesResponse},
    state::AppState,
};

#[derive(Deserialize)]
pub struct HistoryQuery {
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

pub async fn get_game_history(
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameHistoryResponse>> {
    // Get game details
    let game = sqlx::query_as!(
        Game,
        r#"SELECT id as "id!: Uuid", 
           white_player_id as "white_player_id?: Uuid", 
           black_player_id as "black_player_id?: Uuid", 
           current_state, 
           status as "status: DbGameStatus", 
           winner_id as "winner_id?: Uuid", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
           updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
           time_control_seconds as "time_control_seconds?: i32",
           white_time_remaining_ms as "white_time_remaining_ms?: i32",
           black_time_remaining_ms as "black_time_remaining_ms?: i32",
           last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
           FROM games WHERE id = ?"#,
        game_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    // Get player information
    let white_player = if let Some(id) = game.white_player_id {
        sqlx::query!(r#"SELECT id as "id!: Uuid", username FROM users WHERE id = ?"#, id)
            .fetch_optional(&state.db)
            .await?
            .map(|u| PlayerInfo {
                id: u.id,
                username: u.username,
            })
    } else {
        None
    };

    let black_player = if let Some(id) = game.black_player_id {
        sqlx::query!(r#"SELECT id as "id!: Uuid", username FROM users WHERE id = ?"#, id)
            .fetch_optional(&state.db)
            .await?
            .map(|u| PlayerInfo {
                id: u.id,
                username: u.username,
            })
    } else {
        None
    };

    let winner = if let Some(winner_id) = game.winner_id {
        sqlx::query!(r#"SELECT id as "id!: Uuid", username FROM users WHERE id = ?"#, winner_id)
            .fetch_optional(&state.db)
            .await?
            .map(|u| PlayerInfo {
                id: u.id,
                username: u.username,
            })
    } else {
        None
    };

    // Get all moves
    let moves = sqlx::query!(
        r#"SELECT gm.id as "id!: Uuid", 
           gm.game_id as "game_id!: Uuid", 
           gm.player_id as "player_id!: Uuid", 
           gm.move_number, 
           gm.from_x, gm.from_y, gm.to_x, gm.to_y, 
           gm.created_at as "created_at!: chrono::DateTime<chrono::Utc>",
           u.username
           FROM game_moves gm
           JOIN users u ON gm.player_id = u.id
           WHERE gm.game_id = ?
           ORDER BY gm.move_number ASC"#,
        game_id
    )
    .fetch_all(&state.db)
    .await?;

    let move_count = moves.len() as i32;

    let moves_with_players: Vec<MoveWithPlayer> = moves
        .into_iter()
        .map(|m| MoveWithPlayer {
            move_number: m.move_number as i32,
            player: PlayerInfo {
                id: m.player_id,
                username: m.username,
            },
            from_x: m.from_x as i32,
            from_y: m.from_y as i32,
            to_x: m.to_x as i32,
            to_y: m.to_y as i32,
            created_at: m.created_at,
        })
        .collect();

    let game_summary = GameSummary {
        id: game.id,
        white_player,
        black_player,
        status: game.status.into(),
        winner,
        created_at: game.created_at,
        updated_at: game.updated_at,
        total_moves: move_count,
    };

    Ok(Json(GameHistoryResponse {
        game: game_summary,
        moves: moves_with_players,
    }))
}

pub async fn get_user_games(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<UserGamesResponse>> {
    // Validate pagination
    if query.page < 1 || query.per_page < 1 || query.per_page > 100 {
        return Err(AppError::Validation("Invalid pagination parameters".to_string()));
    }

    let offset = (query.page - 1) * query.per_page;

    // Get total count
    let total_games = sqlx::query!(
        r#"SELECT COUNT(*) as "count!: i64" 
           FROM games 
           WHERE white_player_id = ? OR black_player_id = ?"#,
        user_id,
        user_id
    )
    .fetch_one(&state.db)
    .await?
    .count;

    // Get games
    let games = sqlx::query!(
        r#"SELECT g.id as "id!: Uuid", 
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
           WHERE g.white_player_id = ? OR g.black_player_id = ?
           ORDER BY g.updated_at DESC
           LIMIT ? OFFSET ?"#,
        user_id,
        user_id,
        query.per_page,
        offset
    )
    .fetch_all(&state.db)
    .await?;

    let game_summaries: Vec<GameSummary> = games
        .into_iter()
        .map(|g| {
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

    Ok(Json(UserGamesResponse {
        games: game_summaries,
        total_games,
        page: query.page,
        per_page: query.per_page,
    }))
}
