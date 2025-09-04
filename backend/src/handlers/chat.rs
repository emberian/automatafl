use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, Result},
    models::{ChatMessage, DbChatMessage, SendChatRequest},
    state::AppState,
};

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default = "default_limit")]
    limit: i32,
    #[serde(default)]
    before: Option<chrono::DateTime<chrono::Utc>>,
}

fn default_limit() -> i32 {
    50
}

pub async fn get_messages(
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
    Query(query): Query<ChatQuery>,
) -> Result<Json<Vec<ChatMessage>>> {
    // Validate game exists
    let game_exists = sqlx::query!("SELECT id FROM games WHERE id = ?", game_id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !game_exists {
        return Err(AppError::NotFound("Game not found".to_string()));
    }

    // Get messages with optimized query structure
    let db_messages = if let Some(before) = query.before {
        sqlx::query_as!(
            DbChatMessage,
            r#"SELECT id as "id!: Uuid", game_id as "game_id!: Uuid", user_id as "user_id!: Uuid", 
               message, created_at as "created_at!: chrono::DateTime<chrono::Utc>" 
               FROM chat_messages 
               WHERE game_id = ? AND created_at < ?
               ORDER BY created_at DESC 
               LIMIT ?"#,
            game_id,
            before,
            query.limit
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            DbChatMessage,
            r#"SELECT id as "id!: Uuid", game_id as "game_id!: Uuid", user_id as "user_id!: Uuid", 
               message, created_at as "created_at!: chrono::DateTime<chrono::Utc>" 
               FROM chat_messages 
               WHERE game_id = ?
               ORDER BY created_at DESC 
               LIMIT ?"#,
            game_id,
            query.limit
        )
        .fetch_all(&state.db)
        .await?
    };

    // Convert to API type and reverse to get chronological order
    let mut messages: Vec<ChatMessage> = db_messages
        .into_iter()
        .map(Into::into)
        .collect();
    messages.reverse();

    Ok(Json(messages))
}

pub async fn send_message(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(game_id): Path<Uuid>,
    Json(req): Json<SendChatRequest>,
) -> Result<Json<ChatMessage>> {
    // Validate game exists and get properly typed player IDs
    let game = sqlx::query!(
        r#"SELECT white_player_id as "white_player_id?: Uuid", 
                 black_player_id as "black_player_id?: Uuid" 
           FROM games WHERE id = ?"#,
        game_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    // Check if user is a player or spectator
    let is_player = game.white_player_id == Some(auth_user.id) || 
                    game.black_player_id == Some(auth_user.id);
    let is_spectator = if !is_player {
        sqlx::query!(
            r#"SELECT user_id as "user_id!: Uuid" FROM spectators WHERE game_id = ? AND user_id = ?"#,
            game_id,
            auth_user.id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some()
    } else {
        false
    };

    if !is_player && !is_spectator {
        return Err(AppError::Forbidden("You must be a player or spectator to send messages".to_string()));
    }

    // Validate message
    if req.message.trim().is_empty() {
        return Err(AppError::Validation("Message cannot be empty".to_string()));
    }

    if req.message.len() > 500 {
        return Err(AppError::Validation("Message too long (max 500 characters)".to_string()));
    }

    // Create message
    let message_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    sqlx::query!(
        r#"INSERT INTO chat_messages (id, game_id, user_id, message, created_at)
           VALUES (?, ?, ?, ?, ?)"#,
        message_id,
        game_id,
        auth_user.id,
        req.message,
        now
    )
    .execute(&state.db)
    .await?;

    let message = ChatMessage {
        id: message_id,
        game_id,
        user_id: auth_user.id,
        message: req.message,
        created_at: now,
    };
    
    // Record metric
    crate::metrics::record_chat_message_sent();
    
    // Broadcast chat message to all connected clients
    crate::handlers::websocket::broadcast_chat_message(&state, game_id, message.clone()).await;

    Ok(Json(message))
}
