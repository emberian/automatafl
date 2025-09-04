use axum::{extract::State, Json};
use bcrypt::{hash, verify, DEFAULT_COST};
use uuid::Uuid;

use crate::{
    auth::{create_jwt, AuthUser},
    error::{AppError, Result},
    models::{AuthResponse, LoginRequest, RegisterRequest, User},
    state::AppState,
};

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>> {
    // Validate input
    if req.username.len() < 3 || req.username.len() > 20 {
        return Err(AppError::Validation(
            "Username must be between 3 and 20 characters".to_string(),
        ));
    }
    
    if req.password.len() < 6 {
        return Err(AppError::Validation(
            "Password must be at least 6 characters".to_string(),
        ));
    }

    // Check if username already exists
    let existing_user = sqlx::query!(
        "SELECT id FROM users WHERE username = ?",
        req.username
    )
    .fetch_optional(&state.db)
    .await?;

    if existing_user.is_some() {
        return Err(AppError::Validation("Username already taken".to_string()));
    }

    // Hash password
    let password_hash = hash(req.password, DEFAULT_COST)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to hash password")))?;

    // Create user
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    
    sqlx::query!(
        "INSERT INTO users (id, username, password_hash, rating, created_at) VALUES (?, ?, ?, ?, ?)",
        user_id,
        req.username,
        password_hash,
        1200, // Starting ELO rating
        now
    )
    .execute(&state.db)
    .await?;

    let user = User {
        id: user_id,
        username: req.username.clone(),
        password_hash,
        rating: 1200,
        created_at: now,
    };

    // Create JWT
    let token = create_jwt(user_id, req.username, &state.jwt_secret)?;

    Ok(Json(AuthResponse { 
        token, 
        user: user.into() 
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>> {
    // Find user
    let user_row = sqlx::query!(
        r#"SELECT id as "id!: Uuid", username, password_hash, rating, created_at as "created_at!: chrono::DateTime<chrono::Utc>" FROM users WHERE username = ?"#,
        req.username
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Auth("Invalid username or password".to_string()))?;

    let user = User {
        id: user_row.id,
        username: user_row.username,
        password_hash: user_row.password_hash,
        rating: user_row.rating as i32,
        created_at: user_row.created_at,
    };

    // Verify password
    let valid = verify(req.password, &user.password_hash)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to verify password")))?;

    if !valid {
        return Err(AppError::Auth("Invalid username or password".to_string()));
    }

    // Create JWT
    let token = create_jwt(user.id, user.username.clone(), &state.jwt_secret)?;

    Ok(Json(AuthResponse { 
        token, 
        user: user.into() 
    }))
}

pub async fn me(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<User>> {
    let user_row = sqlx::query!(
        r#"SELECT id as "id!: Uuid", username, password_hash, rating, created_at as "created_at!: chrono::DateTime<chrono::Utc>" FROM users WHERE id = ?"#,
        auth_user.id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let user = User {
        id: user_row.id,
        username: user_row.username,
        password_hash: user_row.password_hash,
        rating: user_row.rating as i32,
        created_at: user_row.created_at,
    };

    Ok(Json(user))
}
