//! Shared API types between the backend and client

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// User-related types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub username: String,
    pub rating: i32,
    pub created_at: Option<DateTime<Utc>>,
}

// Game-related types

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameStatus {
    Waiting,
    Active,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    pub time_control: Option<String>, // For future time control implementation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameStateResponse {
    pub id: Uuid,
    pub white_player: Option<UserInfo>,
    pub black_player: Option<UserInfo>,
    pub board: BoardState,
    pub round_state: String,
    pub winner: Option<Uuid>,
    pub current_player_turn: Option<u8>,
    pub spectator_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub width: u8,
    pub height: u8,
    pub cells: Vec<Vec<CellState>>,
    pub automaton_position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellState {
    pub particle: Option<String>, // "repulsor", "attractor", "automaton", null for vacuum
    pub is_goal: bool,
    pub goal_player: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitMoveRequest {
    pub from_x: u8,
    pub from_y: u8,
    pub to_x: u8,
    pub to_y: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitMoveResponse {
    pub feedback: String,
    pub game_state: GameStateResponse,
}

// Chat-related types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendChatRequest {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub game_id: Uuid,
    pub user_id: Uuid,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

// WebSocket message types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSocketMessage {
    // Client -> Server
    Subscribe { game_id: Uuid },
    Unsubscribe { game_id: Uuid },
    SubmitMove { game_id: Uuid, move_data: SubmitMoveRequest },
    SendChat { game_id: Uuid, message: String },
    
    // Server -> Client
    GameUpdate { game_state: GameStateResponse },
    ChatMessage { message: ChatMessage },
    Error { error: String },
    Subscribed { game_id: Uuid },
    Unsubscribed { game_id: Uuid },
    
    // Ping/Pong for keepalive
    Ping,
    Pong,
}

// History-related types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameHistoryResponse {
    pub game: GameSummary,
    pub moves: Vec<MoveWithPlayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSummary {
    pub id: Uuid,
    pub white_player: Option<PlayerInfo>,
    pub black_player: Option<PlayerInfo>,
    pub status: GameStatus,
    pub winner: Option<PlayerInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub total_moves: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: Uuid,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveWithPlayer {
    pub move_number: i32,
    pub player: PlayerInfo,
    pub from_x: i32,
    pub from_y: i32,
    pub to_x: i32,
    pub to_y: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserGamesResponse {
    pub games: Vec<GameSummary>,
    pub total_games: i64,
    pub page: i32,
    pub per_page: i32,
}

// Error response type

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

// Leaderboard types (for future implementation)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user: UserInfo,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardResponse {
    pub entries: Vec<LeaderboardEntry>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

// Matchmaking types (for future implementation)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinMatchmakingRequest {
    pub time_control: Option<String>,
    pub rating_range: Option<(i32, i32)>, // Min and max acceptable opponent rating
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingStatusResponse {
    pub in_queue: bool,
    pub estimated_wait_time_seconds: Option<u32>,
    pub players_in_queue: u32,
}

// User profile types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileResponse {
    pub user: UserInfo,
    pub stats: UserStats,
    pub recent_games: Vec<GameSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub total_games: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub win_rate: f32,
    pub highest_rating: i32,
    pub lowest_rating: i32,
    pub current_streak: i32, // positive for wins, negative for losses
}

// Health check types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub database: DatabaseHealth,
    pub uptime_seconds: u64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub connected: bool,
    pub latency_ms: Option<u32>,
}
