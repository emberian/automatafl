use std::collections::HashMap;

use automatafl_logic::MoveFeedback;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: Uuid,
    pub displayname: String,
    pub password_hash: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameLifecycle {
    Waiting,
    InProgress,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub timestamp: u64,
    pub player_id: Uuid,
    pub displayname: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEvent {
    pub kind: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub api_version: String,
    pub cargo_package_version: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub displayname: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub player_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub displayname: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub session_id: Uuid,
    pub player_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameListItem {
    pub id: Uuid,
    pub lifecycle: GameLifecycle,
    pub player_count: usize,
    pub max_players: u8,
    pub created_at: u64,
    pub created_by: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    pub player_count: u8,
    pub use_column_rule: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameStateResponse {
    pub lifecycle: GameLifecycle,
    pub game: automatafl_logic::Game,
    pub player_ids: HashMap<Uuid, automatafl_logic::Pid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformMove {
    pub from: automatafl_logic::Coord,
    pub to: automatafl_logic::Coord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveResult {
    pub feedback: MoveFeedback,
    pub ready_to_complete: bool,
    pub auto_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRoundResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostChatRequest {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostChatResponse {
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGameResponse {
    pub snapshot_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSnapshotsResponse {
    pub snapshots: Vec<SnapshotInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub index: usize,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerListItem {
    pub id: Uuid,
    pub displayname: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub player_id: Uuid,
    pub expires_at: u64,  // Unix timestamp
}
