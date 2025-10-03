//! Common types, extractors, and utilities shared across all modules

use std::{collections::HashMap, fmt, sync::Arc, time::SystemTime};

use automatafl_api_types::*;
use automatafl_logic::Pid;
use dashmap::DashMap;
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
use tokio::sync::broadcast;

use crate::db;

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

/// Per-game channels for broadcasting events
pub struct GameChannels {
    pub event_tx: broadcast::Sender<GameEvent>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    /// Map of game_id -> broadcast channel for real-time events
    pub game_channels: Arc<DashMap<Uuid, Arc<GameChannels>>>,
    /// Application configuration
    pub config: Arc<crate::config::Config>,
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

        let player_id = Uuid::parse_str(&session.player_id).map_err(|_| AppError::Unauthorized)?;

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
            .find(|gp| gp.player_id == auth.player_id.to_string())
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

pub async fn broadcast_event(
    state: &AppState,
    game_id: Uuid,
    event_data: GameEventData,
) -> Result<(), AppError> {
    let ts = timestamp();

    // Extract kind and data for database storage
    let (kind, data) = match &event_data {
        GameEventData::PlayerJoined {
            player_id,
            player_pid,
            displayname,
        } => (
            "PLAYER_JOINED",
            serde_json::json!({
                "player_id": player_id,
                "player_pid": player_pid,
                "displayname": displayname,
            }),
        ),
        GameEventData::GameStarted => ("GAME_STARTED", serde_json::json!({})),
        GameEventData::MoveAcknowledged {
            player_pid,
            from,
            to,
        } => (
            "MOVE_ACK",
            serde_json::json!({
                "player_pid": player_pid,
                "from": from,
                "to": to,
            }),
        ),
        GameEventData::MoveInvalid {
            player_pid,
            feedback,
        } => (
            "MOVE_INVALID",
            serde_json::json!({
                "player_pid": player_pid,
                "feedback": feedback,
            }),
        ),
        GameEventData::Move {
            player_pid,
            from,
            to,
            result,
        } => (
            "MOVE",
            serde_json::json!({
                "player_pid": player_pid,
                "from": from,
                "to": to,
                "result": result,
            }),
        ),
        GameEventData::AutomatonStep { location } => (
            "AUTOMATON_STEP",
            serde_json::json!({
                "location": location,
            }),
        ),
        GameEventData::GameOver { winner } => (
            "GAME_OVER",
            serde_json::json!({
                "winner": winner,
            }),
        ),
        GameEventData::EloUpdate { changes } => (
            "ELO_UPDATE",
            serde_json::json!({
                "changes": changes,
            }),
        ),
        GameEventData::RoundComplete => ("ROUND_COMPLETE", serde_json::json!({})),
        GameEventData::Conflicts {
            locked_players,
            conflict_coords,
        } => (
            "CONFLICTS",
            serde_json::json!({
                "locked_players": locked_players,
                "conflict_coords": conflict_coords,
            }),
        ),
        GameEventData::Chat {
            timestamp,
            player_id,
            displayname,
            message,
        } => (
            "CHAT",
            serde_json::json!({
                "timestamp": timestamp,
                "player_id": player_id,
                "displayname": displayname,
                "message": message,
            }),
        ),
        GameEventData::GameLoaded { snapshot_index } => (
            "GAME_LOADED",
            serde_json::json!({
                "snapshot_index": snapshot_index,
            }),
        ),
        GameEventData::State {
            lifecycle,
            game,
            player_ids,
        } => (
            "STATE",
            serde_json::json!({
                "lifecycle": lifecycle,
                "game": game,
                "player_ids": player_ids,
            }),
        ),
    };

    // Store event in database
    db::add_game_event(&state.db, game_id, ts, kind.to_string(), data)
        .await
        .map_err(|_| AppError::NoSuchGame(game_id))?;

    // Broadcast via channel if exists
    if let Some(channels) = state.game_channels.get(&game_id) {
        let event = GameEvent {
            data: event_data,
            timestamp: Some(ts),
        };
        let _ = channels.event_tx.send(event);
    }

    Ok(())
}

// ============================================================================
// WebSocket Handler
// ============================================================================

pub async fn handle_socket(socket: WebSocket, state: Arc<AppState>, game_uuid: Uuid) {
    // Get or create game channel
    let channels = state.game_channels.entry(game_uuid).or_insert_with(|| {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_SIZE);
        Arc::new(GameChannels { event_tx })
    });

    let mut rx = channels.event_tx.subscribe();

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
                        Uuid::parse_str(&gp.player_id)
                            .ok()
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

    // Spawn task to receive events and forward to websocket with heartbeat
    let mut send_task = tokio::spawn(async move {
        let mut heartbeat_interval = tokio::time::interval(WEBSOCKET_PING_INTERVAL);
        loop {
            tokio::select! {
                event_result = rx.recv() => {
                    match event_result {
                        Ok(event) => {
                            if let Ok(json) = serde_json::to_string(&event) {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
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

/// Spawn a background task to clean up stale game channels
/// Removes channels for games that no longer exist or are finished
pub fn spawn_game_channel_cleanup(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // Every 5 minutes
        loop {
            interval.tick().await;

            let channel_count_before = state.game_channels.len();
            let mut removed = 0;

            // Check each game channel
            let game_ids: Vec<Uuid> = state
                .game_channels
                .iter()
                .map(|entry| *entry.key())
                .collect();

            for game_id in game_ids {
                // Check if game still exists and isn't finished
                match db::get_game(&state.db, game_id).await {
                    Ok(Some(game)) => {
                        if let Ok(lifecycle) =
                            serde_json::from_str::<GameLifecycle>(&game.lifecycle)
                        {
                            if lifecycle == GameLifecycle::Finished {
                                state.game_channels.remove(&game_id);
                                removed += 1;
                            }
                        }
                    }
                    Ok(None) => {
                        // Game doesn't exist, remove channel
                        state.game_channels.remove(&game_id);
                        removed += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to check game {} status during cleanup: {}",
                            game_id,
                            e
                        );
                    }
                }
            }

            if removed > 0 {
                tracing::info!(
                    "Game channel cleanup: removed {} stale channels ({} -> {})",
                    removed,
                    channel_count_before,
                    state.game_channels.len()
                );
            } else {
                tracing::debug!(
                    "Game channel cleanup: no stale channels found ({} active)",
                    state.game_channels.len()
                );
            }
        }
    })
}
