//! Authentication endpoints (register, login, logout)

use axum::{Json, extract::State, http::StatusCode};
use uuid::Uuid;

use crate::common::{AppError, AuthPlayer, ServerState, timestamp};
use crate::{db, validation};
use automatafl_api_types::*;

// ============================================================================
// Password Validation
// ============================================================================

/// Validate password following NIST SP 800-63B guidelines
/// Requirements:
/// - Minimum 8 characters (NIST recommends 8+)
/// - Maximum 64 characters (allow long passphrases)
/// - All printable ASCII and Unicode characters allowed
/// - No complexity requirements (no forced uppercase/lowercase/digits)
///
/// Note: In production, also check against breached password databases (e.g., haveibeenpwned)
fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters long");
    }
    if password.len() > 64 {
        return Err("Password must not exceed 64 characters");
    }
    // Allow all printable characters - no complexity requirements
    if !password.chars().all(|c| !c.is_control()) {
        return Err("Password cannot contain control characters");
    }
    Ok(())
}

#[tracing::instrument(skip(state, payload), fields(displayname = %payload.displayname))]
pub async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    // Validate displayname using central validation module
    validation::validate_displayname(&payload.displayname)?;

    // Validate password strength
    validate_password(&payload.password)
        .map_err(|msg| AppError::ValidationError(msg.to_string()))?;

    // Check for duplicate displayname
    if db::find_player_by_displayname(&state.db, payload.displayname.clone())
        .await
        .map_err(|e| {
            tracing::error!(
                "Database error checking displayname '{}': {}",
                payload.displayname,
                e
            );
            AppError::InvalidCredentials
        })?
        .is_some()
    {
        return Err(AppError::DisplaynameTaken(payload.displayname));
    }

    // Hash password with argon2
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(payload.password.as_bytes(), &salt)
        .map_err(|e| {
            tracing::error!("Password hashing failed: {}", e);
            AppError::InvalidCredentials
        })?
        .to_string();

    let new_uuid = Uuid::new_v4();
    db::create_player(
        &state.db,
        new_uuid,
        payload.displayname.clone(),
        password_hash,
        false,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create player '{}': {}", payload.displayname, e);
        AppError::InvalidCredentials
    })?;

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
        .map_err(|e| {
            tracing::error!("Database error finding player '{}': {}", req.displayname, e);
            AppError::InvalidCredentials
        })?
        .ok_or_else(|| {
            tracing::debug!("Login failed: player '{}' not found", req.displayname);
            AppError::InvalidCredentials
        })?;

    // Verify password with argon2
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };

    let parsed_hash = PasswordHash::new(&player.password_hash).map_err(|e| {
        tracing::error!(
            "Failed to parse password hash for '{}': {}",
            req.displayname,
            e
        );
        AppError::InvalidCredentials
    })?;

    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| {
            tracing::debug!("Password verification failed for '{}'", req.displayname);
            AppError::InvalidCredentials
        })?;

    let player_id = Uuid::parse_str(&player.id).map_err(|e| {
        tracing::error!("Failed to parse player ID for '{}': {}", req.displayname, e);
        AppError::InvalidCredentials
    })?;

    // Create session with expiration
    let session_id = Uuid::new_v4();
    db::create_session(
        &state.db,
        session_id,
        player_id,
        timestamp() + state.config.session_duration,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create session for '{}': {}", req.displayname, e);
        AppError::InvalidCredentials
    })?;

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
