//! Database models that map directly to database tables

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::DbGameStatus;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub rating: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Game {
    pub id: Uuid,
    pub white_player_id: Option<Uuid>,
    pub black_player_id: Option<Uuid>,
    pub current_state: String, // JSON serialized game state
    pub status: DbGameStatus,
    pub winner_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub time_control_seconds: Option<i32>,
    pub white_time_remaining_ms: Option<i32>,
    pub black_time_remaining_ms: Option<i32>,
    pub last_move_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GameMove {
    pub id: Uuid,
    pub game_id: Uuid,
    pub player_id: Uuid,
    pub move_number: i32,
    pub from_x: i32,
    pub from_y: i32,
    pub to_x: i32,
    pub to_y: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChatMessage {
    pub id: Uuid,
    pub game_id: Uuid,
    pub user_id: Uuid,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Spectator {
    pub game_id: Uuid,
    pub user_id: Uuid,
    pub joined_at: DateTime<Utc>,
}

// Game state stored in database (internal use only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGameState {
    pub game: automatafl_logic::Game,
    pub white_player_id: Option<Uuid>,
    pub black_player_id: Option<Uuid>,
    pub move_count: u32,
}
