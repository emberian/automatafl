//! Player profile and stats endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use surrealdb::RecordId;
use uuid::Uuid;

use crate::common::{AppError, AuthPlayer, ServerState};
use crate::db::PlayerStatsRecord;
use crate::services::PlayerServiceError;
use automatafl_api_types::*;

pub async fn get_player_profile(
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<PlayerProfile>, AppError> {
    let player = app_state
        .player_service
        .get_player_or_error(player_id)
        .await
        .map_err(|err| map_player_error("get_profile", player_id, err))?;

    Ok(Json(PlayerProfile {
        id: player_id,
        displayname: player.displayname,
        bio: player.bio,
        avatar_url: player.avatar_url,
        elo_rating: player.elo_rating,
        created_at: player.created_at,
    }))
}

pub async fn update_player_profile(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<StatusCode, AppError> {
    // Only allow updating own profile
    if auth.player_id != player_id {
        return Err(AppError::Forbidden);
    }

    app_state
        .player_service
        .update_profile(player_id, req.bio, req.avatar_url)
        .await
        .map_err(|err| map_player_error("update_profile", player_id, err))?;

    Ok(StatusCode::OK)
}

pub async fn get_player_stats(
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<PlayerStats>, AppError> {
    let stats = app_state
        .player_service
        .get_stats(player_id)
        .await
        .map_err(|err| map_player_error("get_stats", player_id, err))?
        .unwrap_or(PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        });

    let win_rate = if stats.games_played > 0 {
        (stats.games_won as f64 / stats.games_played as f64) * 100.0
    } else {
        0.0
    };

    Ok(Json(PlayerStats {
        games_played: stats.games_played,
        games_won: stats.games_won,
        total_playtime: stats.total_playtime,
        win_rate,
    }))
}

fn map_player_error(action: &str, player_id: Uuid, err: PlayerServiceError) -> AppError {
    match err {
        PlayerServiceError::NotFound(_) => AppError::NoSuchPlayer(player_id),
        PlayerServiceError::Validation(msg) => AppError::ValidationError(msg),
        PlayerServiceError::Database(e) => {
            tracing::error!(player_id = %player_id, action = action, error = %e, "Player service database error");
            AppError::NoSuchPlayer(player_id)
        }
    }
}
