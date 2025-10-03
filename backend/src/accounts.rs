//! Player profile and stats endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use surrealdb::RecordId;
use uuid::Uuid;

use crate::common::{AppError, AuthPlayer, ServerState};
use crate::db;
use automatafl_api_types::*;

pub async fn get_player_profile(
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<PlayerProfile>, AppError> {
    let player = db::get_player(&app_state.db, player_id)
        .await
        .map_err(|e| {
            tracing::error!("Database error getting player {}: {}", player_id, e);
            AppError::NoSuchPlayer(player_id)
        })?
        .ok_or_else(|| {
            tracing::debug!("Player {} not found", player_id);
            AppError::NoSuchPlayer(player_id)
        })?;

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

    // Validate inputs
    if let Some(ref bio) = req.bio {
        crate::validation::validate_bio(bio)?;
    }
    if let Some(ref avatar_url) = req.avatar_url {
        crate::validation::validate_avatar_url(avatar_url)?;
    }

    db::update_player_profile(&app_state.db, player_id, req.bio, req.avatar_url)
        .await
        .map_err(|e| {
            tracing::error!("Failed to update profile for player {}: {}", player_id, e);
            AppError::NoSuchPlayer(player_id)
        })?;

    Ok(StatusCode::OK)
}

pub async fn get_player_stats(
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<PlayerStats>, AppError> {
    let stats = db::get_player_stats(&app_state.db, player_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get player stats for {}: {}", player_id, e);
            AppError::NoSuchPlayer(player_id)
        })?
        .unwrap_or(db::PlayerStatsRecord {
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
