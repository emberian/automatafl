//! Authentication endpoints (register, login, logout)

use axum::{Json, extract::State, http::StatusCode};

use crate::common::{AppError, AuthPlayer, ServerState};
use crate::services::{AuthServiceError, LoginResult};
use automatafl_api_types::*;

#[tracing::instrument(skip(state, payload), fields(displayname = %payload.displayname))]
pub async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    let service = &state.auth_service;
    let response = service
        .register(payload.displayname.clone(), payload.password.clone())
        .await
        .map_err(|err| map_auth_error("register", &payload.displayname, err))?;

    Ok(Json(RegisterResponse {
        player_id: response.player_id,
    }))
}

#[tracing::instrument(skip(state, req), fields(displayname = %req.displayname))]
pub async fn login(
    State(state): ServerState,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let service = &state.auth_service;
    let LoginResult {
        session_id,
        player_id,
        is_admin,
    } = service
        .login(req.displayname.clone(), req.password.clone())
        .await
        .map_err(|err| map_auth_error("login", &req.displayname, err))?;

    Ok(Json(LoginResponse {
        session_id,
        player_id,
        is_admin,
    }))
}

#[tracing::instrument(skip(auth, state), fields(player_id = %auth.player_id))]
pub async fn logout(auth: AuthPlayer, State(state): ServerState) -> StatusCode {
    if let Err(err) = state.auth_service.logout(auth.session_id).await {
        tracing::warn!(
            session_id = %auth.session_id,
            "Failed to delete session during logout: {}",
            err
        );
    }
    StatusCode::OK
}

fn map_auth_error(action: &str, displayname: &str, error: AuthServiceError) -> AppError {
    match error {
        AuthServiceError::DisplaynameTaken(name) => AppError::DisplaynameTaken(name),
        AuthServiceError::InvalidCredentials => {
            tracing::debug!(
                displayname = displayname,
                action = action,
                "Auth invalid credentials"
            );
            AppError::InvalidCredentials
        }
        AuthServiceError::ValidationError(msg) => AppError::ValidationError(msg),
        AuthServiceError::Database(err) => {
            tracing::error!(displayname = displayname, action = action, error = %err, "Auth database error");
            AppError::InvalidCredentials
        }
        AuthServiceError::PasswordHash(msg) => {
            tracing::error!(displayname = displayname, action = action, message = %msg, "Password hashing error");
            AppError::InvalidCredentials
        }
    }
}
