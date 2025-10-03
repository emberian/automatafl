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

use futures::{SinkExt, stream::StreamExt};
use std::time::Duration;

use crate::{db::{self, as_uuid}, services};

// ============================================================================
// Constants
// ============================================================================

/// ELO K-factor for rating calculations
pub const ELO_K_FACTOR: f64 = 32.0;

/// Default starting ELO rating
pub const DEFAULT_ELO: i32 = 1200;

/// Broadcast channel size for game events
pub const EVENT_CHANNEL_SIZE: usize = 100;

/// WebSocket heartbeat interval
pub const WEBSOCKET_PING_INTERVAL: Duration = Duration::from_secs(30);

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

        let session = db::get_session(&state.db, session_id)
            .await
            .map_err(|_| AppError::Unauthorized)?
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

        let player = db::get_player(&state.db, auth_player.player_id)
            .await
            .map_err(|_| AppError::NoSuchPlayer(auth_player.player_id))?
            .ok_or(AppError::NoSuchPlayer(auth_player.player_id))?;

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
        // Verify game exists
        db::get_game(&state.db, game_uuid)
            .await
            .map_err(|_| AppError::NoSuchGame(game_uuid))?
            .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

        // Get game players to find this player's PID
        let game_players = db::get_game_players(&state.db, game_uuid)
            .await
            .map_err(|_| AppError::NoSuchGame(game_uuid))?;

        let player_record = game_players
            .iter()
            .find(|gp| as_uuid(&gp.player_id) == auth.player_id)
            .ok_or_else(|| AppError::NotInGame(auth.player_id, game_uuid))?;

        Ok(PlayerInGame {
            player_uuid: auth.player_id,
            player_pid: Pid(player_record.player_pid),
            game_uuid,
        })
    }
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Clone)]
pub enum AppError {
    NoSuchGame(Uuid),
    NoSuchPlayer(Uuid),
    PlayerAlreadyExists(Uuid),
    GameFull(Uuid),
    Unauthorized,
    SessionExpired,
    Forbidden,
    InvalidCredentials,
    DisplaynameTaken(String),
    GameNotInProgress,
    NotInGame(Uuid, Uuid),
    NoSuchSnapshot(Uuid),
    ValidationError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        // Log errors for observability
        match &self {
            AppError::NoSuchGame(uuid) => {
                tracing::debug!("Error: No such game: {}", uuid);
            }
            AppError::NoSuchPlayer(uuid) => {
                tracing::debug!("Error: No such player: {}", uuid);
            }
            AppError::Unauthorized | AppError::SessionExpired | AppError::InvalidCredentials => {
                tracing::info!("Authentication error: {}", self);
            }
            AppError::Forbidden => {
                tracing::warn!("Authorization error: {}", self);
            }
            AppError::ValidationError(msg) => {
                tracing::info!("Validation error: {}", msg);
            }
            _ => {
                tracing::debug!("Application error: {}", self);
            }
        }

        match self {
            AppError::NoSuchGame(uuid) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(format!("No such game: {}", uuid).into())
                .unwrap(),
            AppError::NoSuchPlayer(uuid) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(format!("No such player: {}", uuid).into())
                .unwrap(),
            AppError::PlayerAlreadyExists(uuid) => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(format!("Player already exists: {}", uuid).into())
                .unwrap(),
            AppError::GameFull(uuid) => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(format!("Game already full: {}", uuid).into())
                .unwrap(),
            AppError::Unauthorized => Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body("Unauthorized".into())
                .unwrap(),
            AppError::SessionExpired => Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body("Session expired - please log in again".into())
                .unwrap(),
            AppError::Forbidden => Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body("Forbidden - admin access required".into())
                .unwrap(),
            AppError::InvalidCredentials => Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body("Invalid credentials".into())
                .unwrap(),
            AppError::DisplaynameTaken(name) => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(format!("Displayname already taken: {}", name).into())
                .unwrap(),
            AppError::GameNotInProgress => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body("Game is not in progress".into())
                .unwrap(),
            AppError::NotInGame(player, game) => Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(format!("Player {} not in game {}", player, game).into())
                .unwrap(),
            AppError::NoSuchSnapshot(uuid) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(format!("No such snapshot for game: {}", uuid).into())
                .unwrap(),
            AppError::ValidationError(msg) => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(format!("Validation error: {}", msg).into())
                .unwrap(),
        }
    }
}

// Implement std::error::Error for AppError
impl std::error::Error for AppError {}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NoSuchGame(uuid) => write!(f, "No such game: {}", uuid),
            AppError::NoSuchPlayer(uuid) => write!(f, "No such player: {}", uuid),
            AppError::PlayerAlreadyExists(uuid) => write!(f, "Player already exists: {}", uuid),
            AppError::GameFull(uuid) => write!(f, "Game already full: {}", uuid),
            AppError::Unauthorized => write!(f, "Unauthorized"),
            AppError::SessionExpired => write!(f, "Session expired"),
            AppError::Forbidden => write!(f, "Forbidden"),
            AppError::InvalidCredentials => write!(f, "Invalid credentials"),
            AppError::DisplaynameTaken(name) => write!(f, "Displayname already taken: {}", name),
            AppError::GameNotInProgress => write!(f, "Game is not in progress"),
            AppError::NotInGame(player, game) => {
                write!(f, "Player {} not in game {}", player, game)
            }
            AppError::NoSuchSnapshot(uuid) => write!(f, "No such snapshot for game: {}", uuid),
            AppError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
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
    let ts = timestamp();

    // Store event in database - live query subscribers will receive it automatically
    db::add_game_event(&state.db, game_id, ts, event_data)
        .await
        .map_err(|_| AppError::NoSuchGame(game_id))?;

    Ok(())
}

// ============================================================================
// WebSocket Handler
// ============================================================================

pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>, game_uuid: Uuid) {
    use futures::StreamExt;

    let (mut sender, mut receiver) = socket.split();

    // Send initial game state from database
    if let Ok(Some(game_record)) = db::get_game(&state.db, game_uuid).await {
        if let (Ok(game_state), Ok(lifecycle)) = (
            serde_json::from_str::<automatafl_logic::Game>(&game_record.game_state),
            serde_json::from_str::<GameLifecycle>(&game_record.lifecycle),
        ) {
            if let Ok(game_players) = db::get_game_players(&state.db, game_uuid).await {
                let player_ids: HashMap<Uuid, Pid> = game_players
                    .iter()
                    .filter_map(|gp| {
                        Some(as_uuid(&gp.player_id))
                            .map(|id| (id, Pid(gp.player_pid)))
                    })
                    .collect();

                let event = GameEvent {
                    data: GameEventData::State {
                        lifecycle,
                        game: Box::new(game_state),
                        player_ids,
                    },
                    timestamp: Some(timestamp()),
                };
                if let Ok(msg) = serde_json::to_string(&event) {
                    let _ = sender.send(Message::Text(msg.into())).await;
                }
            }
        }
    }

    // Create live query stream from SurrealDB
    let db_stream = match db::create_live_query(state.db.clone(), game_uuid).await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("Failed to create live query for game {}: {}", game_uuid, e);
            return;
        }
    };

    // Spawn task to receive database events and forward to websocket with heartbeat
    let mut send_task = tokio::spawn(async move {
        let mut heartbeat_interval = tokio::time::interval(WEBSOCKET_PING_INTERVAL);
        let mut db_stream = Box::pin(db_stream);

        loop {
            tokio::select! {
                notification_result = db_stream.next() => {
                    match notification_result {
                        Some(Ok(notification)) => {
                            // Extract the event record from the notification
                            // The notification contains the GameEventRecord data
                            let event = GameEvent {
                                data: notification.data.event,
                                timestamp: Some(notification.data.timestamp),
                            };
                            if let Ok(json) = serde_json::to_string(&event) {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::error!("Live query error for game {}: {}", game_uuid, e);
                            break;
                        }
                        None => {
                            tracing::info!("Live query stream ended for game {}", game_uuid);
                            break;
                        }
                    }
                }
                _ = heartbeat_interval.tick() => {
                    if sender.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Receive messages from websocket (handle pong responses)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg_result) = receiver.next().await {
            match msg_result {
                Ok(Message::Pong(_)) => {
                    // Client is alive, continue
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Err(_) => {
                    break;
                }
                _ => {
                    // Could handle other client->server messages here if needed
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }
}