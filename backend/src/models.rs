//! Re-export API types and define backend-specific conversions

// Re-export all API types
pub use automatafl_api::*;

// Re-export database models that are used in handlers
pub use crate::db_models::{User, Game, ChatMessage as DbChatMessage, StoredGameState};

use crate::db_models;

// Newtype wrapper for GameStatus to allow sqlx trait implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DbGameStatus(pub GameStatus);

// Conversion from DB models to API types

impl From<db_models::User> for UserInfo {
    fn from(user: db_models::User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            rating: user.rating,
            created_at: Some(user.created_at),
        }
    }
}

impl From<db_models::ChatMessage> for ChatMessage {
    fn from(msg: db_models::ChatMessage) -> Self {
        Self {
            id: msg.id,
            game_id: msg.game_id,
            user_id: msg.user_id,
            message: msg.message,
            created_at: msg.created_at,
        }
    }
}

// Custom sqlx type implementation for DbGameStatus
impl sqlx::Type<sqlx::Sqlite> for DbGameStatus {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
    
    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for DbGameStatus {
    fn encode_by_ref(&self, buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let s = match self.0 {
            GameStatus::Waiting => "waiting",
            GameStatus::Active => "active",
            GameStatus::Completed => "completed",
            GameStatus::Abandoned => "abandoned",
        };
        <&str as sqlx::Encode<'q, sqlx::Sqlite>>::encode_by_ref(&s, buf)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for DbGameStatus {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<'r, sqlx::Sqlite>>::decode(value)?;
        match s.as_str() {
            "waiting" => Ok(DbGameStatus(GameStatus::Waiting)),
            "active" => Ok(DbGameStatus(GameStatus::Active)),
            "completed" => Ok(DbGameStatus(GameStatus::Completed)),
            "abandoned" => Ok(DbGameStatus(GameStatus::Abandoned)),
            _ => Err("Invalid game status".into()),
        }
    }
}

// Conversion helpers
impl From<GameStatus> for DbGameStatus {
    fn from(status: GameStatus) -> Self {
        DbGameStatus(status)
    }
}

impl From<DbGameStatus> for GameStatus {
    fn from(status: DbGameStatus) -> Self {
        status.0
    }
}
