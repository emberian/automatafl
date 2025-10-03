//! Authentication endpoints (register, login, logout)

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use automatafl_api_types::*;
use crate::common::{AppError, AuthPlayer, ServerState, timestamp};
use crate::db;

#[tracing::instrument(skip(state, payload), fields(displayname = %payload.displayname))]
pub async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    // Check for duplicate displayname
    if db::find_player_by_displayname(&state.db, payload.displayname.clone())
        .await
        .map_err(|_| AppError::InvalidCredentials)?
        .is_some()
    {
        return Err(AppError::DisplaynameTaken(payload.displayname));
    }

    // Hash password with argon2
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(payload.password.as_bytes(), &salt)
        .map_err(|_| AppError::InvalidCredentials)?
        .to_string();

    let new_uuid = Uuid::new_v4();
    db::create_player(&state.db, new_uuid, payload.displayname, password_hash, false)
        .await
        .map_err(|_| AppError::InvalidCredentials)?;

    Ok(Json(RegisterResponse {
        player_id: new_uuid,
    }))
}

#[tracing::instrument(skip(state, req), fields(displayname = %req.displayname))]
pub async fn login(
    State(state): ServerState,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    // Find player by displayname
    let player = db::find_player_by_displayname(&state.db, req.displayname.clone())
        .await
        .map_err(|_| AppError::InvalidCredentials)?
        .ok_or(AppError::InvalidCredentials)?;

    // Verify password with argon2
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };

    let parsed_hash = PasswordHash::new(&player.password_hash)
        .map_err(|_| AppError::InvalidCredentials)?;

    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::InvalidCredentials)?;

    let player_id = Uuid::parse_str(&player.id).map_err(|_| AppError::InvalidCredentials)?;

    // Create session with expiration
    let session_id = Uuid::new_v4();
    db::create_session(&state.db, session_id, player_id, timestamp() + state.config.session_duration)
        .await
        .map_err(|_| AppError::InvalidCredentials)?;

    Ok(Json(LoginResponse {
        session_id,
        player_id,
    }))
}

#[tracing::instrument(skip(auth, state), fields(player_id = %auth.player_id))]
pub async fn logout(auth: AuthPlayer, State(state): ServerState) -> StatusCode {
    let _ = db::delete_session(&state.db, auth.session_id).await;
    StatusCode::OK
}
