//! Common types, extractors, and utilities shared across all modules

use std::{collections::HashMap, fmt, sync::Arc, time::SystemTime};

use automatafl_api_types::*;
use automatafl_logic::Pid;
use uuid::Uuid;

use axum::{
    extract::{
        FromRequestParts,
        ws::{Message, WebSocket},
    },
    http::{Response, StatusCode, request::Parts},
    response::IntoResponse,
};

use std::time::Duration;

use crate::{
    db::{self, as_uuid},
    services,
};

// ============================================================================
// Constants & Types
// ============================================================================

/// ELO K-factor for rating calculations
pub const ELO_K_FACTOR: f64 = 32.0;

/// Default starting ELO rating
pub const DEFAULT_ELO: i32 = 1200;

/// WebSocket heartbeat interval
pub const WEBSOCKET_PING_INTERVAL: Duration = Duration::from_secs(30);

/// Game lifecycle states
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameLifecycleState {
    /// Game is waiting for players to join
    Waiting,
    /// Game is in progress
    InProgress,
    /// Game has finished
    Finished,
}

impl From<GameLifecycleState> for String {
    fn from(state: GameLifecycleState) -> String {
        match state {
            GameLifecycleState::Waiting => "Waiting".to_string(),
            GameLifecycleState::InProgress => "InProgress".to_string(),
            GameLifecycleState::Finished => "Finished".to_string(),
        }
    }
}

impl TryFrom<String> for GameLifecycleState {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "Waiting" => Ok(GameLifecycleState::Waiting),
            "InProgress" => Ok(GameLifecycleState::InProgress),
            "Finished" => Ok(GameLifecycleState::Finished),
            _ => Err(format!("Invalid game lifecycle state: {}", s)),
        }
    }
}

/// Move feedback types
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MoveFeedbackType {
    /// Move was accepted and applied
    Committed,
    /// Move was invalid for some reason
    Invalid(String),
    /// Move conflicts with another player's move
    Conflict,
}

impl From<automatafl_logic::MoveFeedback> for MoveFeedbackType {
    fn from(feedback: automatafl_logic::MoveFeedback) -> Self {
        use automatafl_logic::MoveFeedback;

        match feedback {
            MoveFeedback::Committed => MoveFeedbackType::Committed,
            MoveFeedback::SeeCoords(details) => {
                MoveFeedbackType::Invalid(format!("Invalid move: {}", details))
            }
            MoveFeedback::MustMove => MoveFeedbackType::Invalid(
                "Move must change source and destination squares".to_string(),
            ),
            MoveFeedback::AxisAlignedOnly => MoveFeedbackType::Invalid(
                "Move must stay within a single row or column".to_string(),
            ),
            MoveFeedback::WaitYourTurn => MoveFeedbackType::Conflict,
            MoveFeedback::GameOver => MoveFeedbackType::Invalid("Game is already over".to_string()),
        }
    }
}

/// Game event types for better type safety
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameEventType {
    PlayerJoined,
    GameStarted,
    MoveAcknowledged,
    MoveInvalid,
    Move,
    AutomatonStep,
    GameOver,
    EloUpdate,
    RoundComplete,
    Conflicts,
    Chat,
    GameLoaded,
    State,
}

impl From<&str> for GameEventType {
    fn from(s: &str) -> Self {
        match s {
            "PLAYER_JOINED" => GameEventType::PlayerJoined,
            "GAME_STARTED" => GameEventType::GameStarted,
            "MOVE_ACK" => GameEventType::MoveAcknowledged,
            "MOVE_INVALID" => GameEventType::MoveInvalid,
            "MOVE" => GameEventType::Move,
            "AUTOMATON_STEP" => GameEventType::AutomatonStep,
            "GAME_OVER" => GameEventType::GameOver,
            "ELO_UPDATE" => GameEventType::EloUpdate,
            "ROUND_COMPLETE" => GameEventType::RoundComplete,
            "CONFLICTS" => GameEventType::Conflicts,
            "CHAT" => GameEventType::Chat,
            "GAME_LOADED" => GameEventType::GameLoaded,
            "STATE" => GameEventType::State,
            _ => GameEventType::State, // Default fallback
        }
    }
}

/// Database table names for type safety
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseTable {
    Players,
    Sessions,
    Games,
    GamePlayers,
    GameEvents,
    ChatMessages,
    Snapshots,
    MatchmakingQueue,
    PlayerStats,
}

impl From<DatabaseTable> for &'static str {
    fn from(table: DatabaseTable) -> &'static str {
        match table {
            DatabaseTable::Players => "players",
            DatabaseTable::Sessions => "sessions",
            DatabaseTable::Games => "games",
            DatabaseTable::GamePlayers => "game_players",
            DatabaseTable::GameEvents => "game_events",
            DatabaseTable::ChatMessages => "chat_messages",
            DatabaseTable::Snapshots => "snapshots",
            DatabaseTable::MatchmakingQueue => "matchmaking_queue",
            DatabaseTable::PlayerStats => "player_stats",
        }
    }
}

/// Leaderboard types for type safety
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LeaderboardType {
    Elo,
    Wins,
    GamesPlayed,
}

impl From<&str> for LeaderboardType {
    fn from(s: &str) -> Self {
        match s {
            "elo" => LeaderboardType::Elo,
            "wins" => LeaderboardType::Wins,
            "games" => LeaderboardType::GamesPlayed,
            _ => LeaderboardType::Elo, // Default fallback
        }
    }
}

impl From<LeaderboardType> for String {
    fn from(leaderboard_type: LeaderboardType) -> String {
        match leaderboard_type {
            LeaderboardType::Elo => "ELO Rating".to_string(),
            LeaderboardType::Wins => "Wins".to_string(),
            LeaderboardType::GamesPlayed => "Games Played".to_string(),
        }
    }
}

/// API version for better versioning
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApiVersion {
    V1,
}

impl From<ApiVersion> for String {
    fn from(version: ApiVersion) -> String {
        match version {
            ApiVersion::V1 => "v1".to_string(),
        }
    }
}

/// WebSocket message types for better type safety
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WebSocketMessage {
    /// Initial game state
    GameState {
        lifecycle: String,
        game: Box<automatafl_logic::Game>,
        player_ids: std::collections::HashMap<Uuid, automatafl_logic::Pid>,
    },
    /// Game event
    GameEvent {
        event: automatafl_api_types::GameEventData,
        timestamp: u64,
    },
    /// Chat message
    ChatMessage {
        timestamp: u64,
        player_id: Uuid,
        displayname: String,
        message: String,
    },
    /// Player joined/left
    PlayerUpdate {
        player_id: Uuid,
        displayname: String,
        action: PlayerAction,
    },
    /// Heartbeat ping
    Ping,
    /// Heartbeat pong response
    Pong,
}

/// Player actions in WebSocket messages
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlayerAction {
    Joined,
    Left,
    Disconnected,
}

// ============================================================================
// State Types
// ============================================================================

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GameSnapshot {
    pub core: automatafl_logic::Game,
    pub player_ids: HashMap<Uuid, automatafl_logic::Pid>,
    pub lifecycle: GameLifecycle,
    pub timestamp: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    /// Application configuration
    pub config: Arc<crate::config::Config>,
    /// Game service for business logic
    pub game_service: Arc<services::GameService>,
    /// Authentication service for identity workflow
    pub auth_service: Arc<services::AuthService>,
    /// Player service for profile & stats operations
    pub player_service: Arc<services::PlayerService>,
    /// Matchmaking service orchestrating queue / background tasks
    pub matchmaking_service: Arc<services::MatchmakingService>,
    /// Administrative facade aggregating operations
    pub admin_service: Arc<services::AdminService>,
}

pub type ServerState = axum::extract::State<Arc<AppState>>;

// ============================================================================
// Custom Extractors
// ============================================================================

/// Authenticated player extractor
pub struct AuthPlayer {
    pub player_id: Uuid,
    pub session_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for AuthPlayer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let session_id = Uuid::parse_str(auth_header).map_err(|_| AppError::Unauthorized)?;

        let session = state
            .auth_service
            .get_session(session_id)
            .await
            .map_err(|err| {
                tracing::error!(session_id = %session_id, "Failed to fetch session: {}", err);
                AppError::Unauthorized
            })?
            .ok_or(AppError::Unauthorized)?;

        // Check if session has expired
        if session.expires_at < timestamp() {
            return Err(AppError::SessionExpired);
        }

        let player_id = as_uuid(&session.player_id);

        Ok(AuthPlayer {
            player_id,
            session_id,
        })
    }
}

/// Admin player extractor
pub struct AdminPlayer {
    #[allow(unused)]
    pub player_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for AdminPlayer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_player = AuthPlayer::from_request_parts(parts, state).await?;

        let player = state
            .player_service
            .get_player_or_error(auth_player.player_id)
            .await
            .map_err(|err| {
                tracing::error!(player_id = %auth_player.player_id, "Failed to load player: {}", err);
                match err {
                    crate::services::PlayerServiceError::NotFound(id) => AppError::NoSuchPlayer(id),
                    crate::services::PlayerServiceError::Validation(msg) => AppError::ValidationError(msg),
                    crate::services::PlayerServiceError::Database(_) => AppError::NoSuchPlayer(auth_player.player_id),
                }
            })?;

        if !player.is_admin {
            return Err(AppError::Forbidden);
        }

        Ok(AdminPlayer {
            player_id: auth_player.player_id,
        })
    }
}

/// Extractor for authenticated player + game they're in
pub struct PlayerInGame {
    pub player_uuid: Uuid,
    pub player_pid: Pid,
    pub game_uuid: Uuid,
}

impl PlayerInGame {
    pub async fn extract(
        auth: AuthPlayer,
        game_uuid: Uuid,
        state: &Arc<AppState>,
    ) -> Result<Self, AppError> {
        let (_, _, player_map) = state
            .game_service
            .load_game(game_uuid)
            .await
            .map_err(|_| AppError::NoSuchGame(game_uuid))?;

        let player_pid = player_map
            .get(&auth.player_id)
            .copied()
            .ok_or_else(|| AppError::NotInGame(auth.player_id, game_uuid))?;

        Ok(PlayerInGame {
            player_uuid: auth.player_id,
            player_pid,
            game_uuid,
        })
    }
}

// ============================================================================
// Enhanced Error Types
// ============================================================================

/// Core error types for the application
#[derive(Debug, Clone)]
pub enum AppError {
    // Resource not found errors
    NoSuchGame(Uuid),
    NoSuchPlayer(Uuid),
    NoSuchSnapshot(Uuid),
    NoSuchSession(Uuid),

    // Resource state errors
    PlayerAlreadyExists(Uuid),
    GameFull(Uuid),
    GameNotInProgress,
    NotInGame(Uuid, Uuid),

    // Authentication errors
    Unauthorized,
    SessionExpired,
    Forbidden,
    InvalidCredentials,
    DisplaynameTaken(String),

    // Validation errors
    ValidationError(String),
    InvalidInput(String),

    // System errors
    DatabaseError(String),
    SerializationError(String),
    ConfigurationError(String),

    // Rate limiting
    RateLimited(String),

    // WebSocket errors
    WebSocketError(String),

    // Matchmaking errors
    MatchmakingError(String),
}

impl AppError {
    /// Get the HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::NoSuchGame(_)
            | AppError::NoSuchPlayer(_)
            | AppError::NoSuchSnapshot(_)
            | AppError::NoSuchSession(_) => StatusCode::NOT_FOUND,

            AppError::PlayerAlreadyExists(_)
            | AppError::GameFull(_)
            | AppError::GameNotInProgress
            | AppError::NotInGame(_, _)
            | AppError::ValidationError(_)
            | AppError::InvalidInput(_)
            | AppError::DisplaynameTaken(_) => StatusCode::BAD_REQUEST,

            AppError::Unauthorized | AppError::SessionExpired | AppError::InvalidCredentials => {
                StatusCode::UNAUTHORIZED
            }

            AppError::Forbidden => StatusCode::FORBIDDEN,

            AppError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,

            AppError::DatabaseError(_)
            | AppError::SerializationError(_)
            | AppError::ConfigurationError(_) => StatusCode::INTERNAL_SERVER_ERROR,

            AppError::WebSocketError(_) | AppError::MatchmakingError(_) => StatusCode::BAD_GATEWAY,
        }
    }

    /// Get the error code for client applications
    pub fn error_code(&self) -> &'static str {
        match self {
            AppError::NoSuchGame(_) => "GAME_NOT_FOUND",
            AppError::NoSuchPlayer(_) => "PLAYER_NOT_FOUND",
            AppError::NoSuchSnapshot(_) => "SNAPSHOT_NOT_FOUND",
            AppError::NoSuchSession(_) => "SESSION_NOT_FOUND",

            AppError::PlayerAlreadyExists(_) => "PLAYER_ALREADY_EXISTS",
            AppError::GameFull(_) => "GAME_FULL",
            AppError::GameNotInProgress => "GAME_NOT_IN_PROGRESS",
            AppError::NotInGame(_, _) => "NOT_IN_GAME",

            AppError::Unauthorized => "UNAUTHORIZED",
            AppError::SessionExpired => "SESSION_EXPIRED",
            AppError::Forbidden => "FORBIDDEN",
            AppError::InvalidCredentials => "INVALID_CREDENTIALS",
            AppError::DisplaynameTaken(_) => "DISPLAYNAME_TAKEN",

            AppError::ValidationError(_) => "VALIDATION_ERROR",
            AppError::InvalidInput(_) => "INVALID_INPUT",

            AppError::DatabaseError(_) => "DATABASE_ERROR",
            AppError::SerializationError(_) => "SERIALIZATION_ERROR",
            AppError::ConfigurationError(_) => "CONFIGURATION_ERROR",

            AppError::RateLimited(_) => "RATE_LIMITED",

            AppError::WebSocketError(_) => "WEBSOCKET_ERROR",

            AppError::MatchmakingError(_) => "MATCHMAKING_ERROR",
        }
    }

    /// Check if this is a client error (4xx) vs server error (5xx)
    pub fn is_client_error(&self) -> bool {
        matches!(self.status_code().as_u16() / 100, 4)
    }

    /// Check if this is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        matches!(self.status_code().as_u16() / 100, 5)
    }

    /// Log the error with appropriate level and context
    pub fn log_error(&self, context: Option<&str>) {
        let context = context.unwrap_or("unknown");
        match self {
            AppError::NoSuchGame(uuid)
            | AppError::NoSuchPlayer(uuid)
            | AppError::NoSuchSnapshot(uuid)
            | AppError::NoSuchSession(uuid) => {
                tracing::debug!(error = %self, context, resource_id = %uuid, "Resource not found");
            }

            AppError::Unauthorized | AppError::SessionExpired | AppError::InvalidCredentials => {
                tracing::info!(error = %self, context, "Authentication error");
            }

            AppError::Forbidden => {
                tracing::warn!(error = %self, context, "Authorization error");
            }

            AppError::ValidationError(msg) | AppError::InvalidInput(msg) => {
                tracing::info!(error = %self, context, message = %msg, "Input validation error");
            }

            AppError::RateLimited(msg) => {
                tracing::warn!(error = %self, context, message = %msg, "Rate limit exceeded");
            }

            AppError::DatabaseError(msg)
            | AppError::SerializationError(msg)
            | AppError::ConfigurationError(msg) => {
                tracing::error!(error = %self, context, message = %msg, "System error");
            }

            AppError::WebSocketError(msg) | AppError::MatchmakingError(msg) => {
                tracing::warn!(error = %self, context, message = %msg, "Service error");
            }

            _ => {
                tracing::debug!(error = %self, context, "Application error");
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        self.log_error(None);

        let status = self.status_code();
        let error_code = self.error_code();

        // Create structured error response for API endpoints
        if self.is_client_error() {
            let error_response = serde_json::json!({
                "error": {
                    "code": error_code,
                    "message": self.to_string(),
                    "status": status.as_u16()
                }
            });

            match serde_json::to_string(&error_response) {
                Ok(body) => Response::builder()
                    .status(status)
                    .header("Content-Type", "application/json")
                    .body(body.into())
                    .unwrap(),
                Err(_) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body("Internal server error".into())
                    .unwrap(),
            }
        } else {
            // For server errors, don't expose internal details to clients
            Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "error": {
                            "code": error_code,
                            "message": "Internal server error",
                            "status": status.as_u16()
                        }
                    })
                    .to_string()
                    .into(),
                )
                .unwrap()
        }
    }
}

impl std::error::Error for AppError {}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NoSuchGame(uuid) => write!(f, "No such game: {}", uuid),
            AppError::NoSuchPlayer(uuid) => write!(f, "No such player: {}", uuid),
            AppError::NoSuchSnapshot(uuid) => write!(f, "No such snapshot for game: {}", uuid),
            AppError::NoSuchSession(uuid) => write!(f, "No such session: {}", uuid),

            AppError::PlayerAlreadyExists(uuid) => write!(f, "Player already exists: {}", uuid),
            AppError::GameFull(uuid) => write!(f, "Game already full: {}", uuid),
            AppError::GameNotInProgress => write!(f, "Game is not in progress"),
            AppError::NotInGame(player, game) => {
                write!(f, "Player {} not in game {}", player, game)
            }

            AppError::Unauthorized => write!(f, "Unauthorized"),
            AppError::SessionExpired => write!(f, "Session expired"),
            AppError::Forbidden => write!(f, "Forbidden"),
            AppError::InvalidCredentials => write!(f, "Invalid credentials"),
            AppError::DisplaynameTaken(name) => write!(f, "Displayname already taken: {}", name),

            AppError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            AppError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),

            AppError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            AppError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            AppError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),

            AppError::RateLimited(msg) => write!(f, "Rate limited: {}", msg),

            AppError::WebSocketError(msg) => write!(f, "WebSocket error: {}", msg),

            AppError::MatchmakingError(msg) => write!(f, "Matchmaking error: {}", msg),
        }
    }
}

// Convert ServiceError to AppError automatically
impl From<services::game_service::ServiceError> for AppError {
    fn from(err: services::game_service::ServiceError) -> Self {
        match err {
            services::game_service::ServiceError::GameNotFound(id) => AppError::NoSuchGame(id),
            services::game_service::ServiceError::GameNotInProgress => AppError::GameNotInProgress,
            services::game_service::ServiceError::GameFull(id) => AppError::GameFull(id),
            services::game_service::ServiceError::PlayerAlreadyInGame(id) => {
                tracing::error!("Player {} already in a game", id);
                AppError::InvalidInput("Player already in a game".to_string())
            }
            services::game_service::ServiceError::PlayerNotFound(id) => AppError::NoSuchPlayer(id),
            services::game_service::ServiceError::ValidationFailed(msg) => {
                AppError::ValidationError(msg)
            }
            services::game_service::ServiceError::SnapshotNotFound(game_id, _) => {
                AppError::NoSuchSnapshot(game_id)
            }
            services::game_service::ServiceError::DatabaseError(e) => {
                AppError::DatabaseError(e.to_string())
            }
            services::game_service::ServiceError::SerializationError(e) => {
                AppError::SerializationError(e.to_string())
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Persist an event to the database
/// Live query subscribers will automatically receive the event
pub async fn broadcast_event(
    state: &AppState,
    game_id: Uuid,
    event_data: GameEventData,
) -> Result<(), AppError> {
    // Persist event through the service layer so live subscribers receive it
    state
        .game_service
        .append_event(game_id, event_data)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_id, error = %err, "Failed to broadcast game event");
            match err {
                services::game_service::ServiceError::GameNotFound(_) => {
                    AppError::NoSuchGame(game_id)
                }
                _ => AppError::ValidationError("Unable to broadcast game event".to_string()),
            }
        })?;

    Ok(())
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// Enhanced WebSocket handler with better error handling and resource management
pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>, game_uuid: Uuid) {
    use futures::{SinkExt, StreamExt};
    use tokio::{
        sync::Mutex,
        time::{Duration, timeout},
    };

    // Set up connection timeout and limits
    let connection_timeout = state.config.websocket.timeout;
    let ping_interval = state.config.websocket.ping_interval;
    let max_message_size = state.config.websocket.max_message_size;

    // Record WebSocket connection start
    metrics::gauge!(
        "websocket_active_connections",
        &[("game_id", game_uuid.to_string())]
    )
    .increment(1.0);

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Send initial game state with timeout
    if let Ok(snapshot_result) = timeout(
        Duration::from_secs(5),
        state.game_service.state_snapshot_event(game_uuid),
    )
    .await
    {
        match snapshot_result {
            Ok(Some(event)) => {
                if let Ok(msg) = serde_json::to_string(&event) {
                    match timeout(Duration::from_secs(2), async {
                        let mut guard = sender.lock().await;
                        guard.send(Message::Text(msg.into())).await
                    })
                    .await
                    {
                        Ok(Ok(())) => {
                            tracing::debug!("Sent initial game state for game {}", game_uuid);
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                "Failed to send initial game state for game {}: {}",
                                game_uuid,
                                e
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Timeout sending initial game state for game {}: {}",
                                game_uuid,
                                e
                            );
                        }
                    }
                }
            }
            Ok(None) => {
                tracing::warn!("No initial state available for game {}", game_uuid);
            }
            Err(e) => {
                tracing::error!("Failed to get initial state for game {}: {}", game_uuid, e);
            }
        }
    } else {
        tracing::warn!("Timeout getting initial state for game {}", game_uuid);
    }

    // Subscribe to live events with timeout
    let event_stream = match timeout(
        Duration::from_secs(5),
        state.game_service.live_event_stream(game_uuid),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            tracing::error!(
                "Failed to subscribe to live events for game {}: {}",
                game_uuid,
                e
            );
            return;
        }
        Err(_) => {
            tracing::error!("Timeout subscribing to live events for game {}", game_uuid);
            return;
        }
    };

    let sender_for_heartbeat = Arc::clone(&sender);
    let sender_for_events = Arc::clone(&sender);
    let sender_for_messages = Arc::clone(&sender);

    // Spawn heartbeat task
    let mut heartbeat_task = tokio::spawn(async move {
        let mut heartbeat_interval = tokio::time::interval(ping_interval);
        let mut consecutive_failures = 0;
        const MAX_FAILURES: u32 = 3;

        loop {
            heartbeat_interval.tick().await;

            match timeout(Duration::from_secs(5), async {
                let mut guard = sender_for_heartbeat.lock().await;
                guard.send(Message::Ping(vec![].into())).await
            })
            .await
            {
                Ok(Ok(())) => {
                    consecutive_failures = 0;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to send ping for game {}: {}", game_uuid, e);
                    consecutive_failures += 1;
                }
                Err(_) => {
                    tracing::warn!("Timeout sending ping for game {}", game_uuid);
                    consecutive_failures += 1;
                }
            }

            if consecutive_failures >= MAX_FAILURES {
                tracing::info!(
                    "Too many ping issues for game {}, closing connection",
                    game_uuid
                );
                break;
            }
        }
    });

    // Spawn event forwarding task
    let mut event_task = tokio::spawn(async move {
        let mut event_stream = event_stream;

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if json.len() > max_message_size {
                            tracing::warn!(
                                "Event message too large for game {}: {} bytes",
                                game_uuid,
                                json.len()
                            );
                            continue;
                        }

                        match timeout(Duration::from_secs(2), async {
                            let mut guard = sender_for_events.lock().await;
                            guard.send(Message::Text(json.into())).await
                        })
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => {
                                tracing::warn!(
                                    "Failed to send event for game {}: {}",
                                    game_uuid,
                                    e
                                );
                                break;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Timeout sending event for game {}: {}",
                                    game_uuid,
                                    e
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Event stream error for game {}: {}", game_uuid, e);
                    break;
                }
            }
        }
    });

    // Handle incoming messages from client
    let mut message_task = tokio::spawn(async move {
        while let Some(msg_result) = receiver.next().await {
            match msg_result {
                Ok(Message::Pong(_)) => {
                    // Client responded to ping, connection is alive
                }
                Ok(Message::Ping(payload)) => {
                    // Reply to client ping with pong
                    match timeout(Duration::from_secs(2), async {
                        let mut guard = sender_for_messages.lock().await;
                        guard.send(Message::Pong(payload)).await
                    })
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            tracing::debug!("Failed to echo pong for game {}: {}", game_uuid, e);
                        }
                        Err(e) => {
                            tracing::debug!("Timeout sending pong for game {}: {}", game_uuid, e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::debug!("Client closed connection for game {}", game_uuid);
                    break;
                }
                Ok(Message::Text(text)) => {
                    if text.len() > max_message_size {
                        tracing::warn!(
                            "Received oversized message for game {}: {} bytes",
                            game_uuid,
                            text.len()
                        );
                        break;
                    }
                    tracing::debug!(
                        "Received message from client for game {}: {} bytes",
                        game_uuid,
                        text.len()
                    );
                }
                Ok(Message::Binary(_)) => {
                    tracing::warn!("Binary message rejected for game {}", game_uuid);
                    break;
                }
                Err(e) => {
                    tracing::error!("WebSocket receive error for game {}: {}", game_uuid, e);
                    break;
                }
            }
        }
    });

    let connection_timer = tokio::time::sleep(connection_timeout);
    tokio::pin!(connection_timer);

    // Wait for any task to fail or complete, with overall timeout
    tokio::select! {
        _ = &mut event_task => {
            tracing::info!("Event task ended for game {}", game_uuid);
        }
        _ = &mut message_task => {
            tracing::info!("Message task ended for game {}", game_uuid);
        }
        _ = &mut heartbeat_task => {
            tracing::info!("Heartbeat task ended for game {}", game_uuid);
        }
        _ = &mut connection_timer => {
            tracing::info!("WebSocket connection timeout for game {}", game_uuid);
        }
    }

    if !event_task.is_finished() {
        event_task.abort();
    }
    if !message_task.is_finished() {
        message_task.abort();
    }
    if !heartbeat_task.is_finished() {
        heartbeat_task.abort();
    }

    tracing::info!("WebSocket connection closed for game {}", game_uuid);

    // Record WebSocket metrics
    metrics::counter!(
        "websocket_connections_total",
        &[("game_id", game_uuid.to_string())]
    )
    .increment(1);
    metrics::gauge!(
        "websocket_active_connections",
        &[("game_id", game_uuid.to_string())]
    )
    .decrement(1.0);
}
