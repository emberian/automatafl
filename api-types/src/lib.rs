use std::collections::HashMap;

use automatafl_logic::{MoveFeedback, MoveResult, Coord, Pid};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: Uuid,
    pub displayname: String,
    pub password_hash: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// ============================================================================
// Game Event Types
// ============================================================================

/// Properly typed game event data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum GameEventData {
    /// Player joined the game
    #[serde(rename = "PLAYER_JOINED")]
    PlayerJoined {
        player_id: Uuid,
        player_pid: Pid,
        displayname: String,
    },
    
    /// Game has started
    #[serde(rename = "GAME_STARTED")]
    GameStarted,
    
    /// Player's move was acknowledged
    #[serde(rename = "MOVE_ACK")]
    MoveAcknowledged {
        player_pid: Pid,
        from: Coord,
        to: Coord,
    },
    
    /// Player's move was invalid
    #[serde(rename = "MOVE_INVALID")]
    MoveInvalid {
        player_pid: Pid,
        feedback: MoveFeedback,
    },
    
    /// Move was executed
    #[serde(rename = "MOVE")]
    Move {
        player_pid: Pid,
        from: Coord,
        to: Coord,
        result: MoveResult,
    },
    
    /// Automaton stepped to new location
    #[serde(rename = "AUTOMATON_STEP")]
    AutomatonStep {
        location: Coord,
    },
    
    /// Game has ended
    #[serde(rename = "GAME_OVER")]
    GameOver {
        winner: Pid,
    },
    
    /// ELO ratings updated
    #[serde(rename = "ELO_UPDATE")]
    EloUpdate {
        changes: Vec<EloChange>,
    },
    
    /// Round completed successfully
    #[serde(rename = "ROUND_COMPLETE")]
    RoundComplete,
    
    /// Conflicts occurred during round
    #[serde(rename = "CONFLICTS")]
    Conflicts {
        locked_players: Vec<Pid>,
        conflict_coords: Vec<Coord>,
    },
    
    /// Chat message
    #[serde(rename = "CHAT")]
    Chat {
        timestamp: u64,
        player_id: Uuid,
        displayname: String,
        message: String,
    },
    
    /// Game loaded from snapshot
    #[serde(rename = "GAME_LOADED")]
    GameLoaded {
        snapshot_index: usize,
    },
    
    /// Full game state (WebSocket initial message)
    #[serde(rename = "STATE")]
    State {
        lifecycle: GameLifecycle,
        game: automatafl_logic::Game,
        player_ids: HashMap<Uuid, Pid>,
    },
}

/// ELO rating change for a player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EloChange {
    pub player_id: Uuid,
    pub old_elo: i32,
    pub new_elo: i32,
    pub change: i32,
}

/// Game event with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEvent {
    #[serde(flatten)]
    pub data: GameEventData,
    #[serde(default)]
    pub timestamp: Option<u64>,
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
pub struct MoveResultResponse {
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

// ============================================================================
// Profile Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
    pub id: Uuid,
    pub displayname: String,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub elo_rating: i32,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub games_played: u32,
    pub games_won: u32,
    pub total_playtime: u64,
    pub win_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
}

// ============================================================================
// Leaderboard Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: usize,
    pub player_id: Uuid,
    pub displayname: String,
    pub value: i64,  // Primary sort value (ELO, wins, or games played depending on leaderboard type)
    // Additional stats for display
    #[serde(default)]
    pub elo_rating: Option<i32>,
    #[serde(default)]
    pub games_played: Option<u32>,
    #[serde(default)]
    pub games_won: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardResponse {
    pub entries: Vec<LeaderboardEntry>,
    pub total_players: usize,
}

// ============================================================================
// Matchmaking Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinMatchmakingRequest {
    pub player_count: u8,
    pub use_column_rule: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingStatus {
    pub in_queue: bool,
    pub queued_at: Option<u64>,
    pub estimated_wait_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchFoundNotification {
    pub game_id: Uuid,
    pub players: Vec<Uuid>,
}
