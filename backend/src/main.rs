use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::{SystemTime, Duration}};

use automatafl_logic::{Board, Coord, Move, MoveFeedback, Pid};
use automatafl_api_types::*;

use axum::{
    Json, Router,
    extract::{Path, State, FromRequestParts, ws::{WebSocket, WebSocketUpgrade, Message}},
    http::{Response, StatusCode, request::Parts},
    response::IntoResponse,
    routing::{get, post},
};

use futures::{stream::StreamExt, SinkExt};
use tokio::sync::{RwLock, broadcast};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use tracing_subscriber::prelude::*;


const CARGO_PACKAGE_VERSION: Option<&str> = std::option_env!("CARGO_PACKAGE_VERSION");

// ============================================================================
// State Types
// ============================================================================

#[derive(Clone)]
struct GameState {
    core: automatafl_logic::Game,
    player_ids: HashMap<Uuid, automatafl_logic::Pid>,
    lifecycle: GameLifecycle,
    chat: Vec<ChatMessage>,
    created_at: u64,
    created_by: Uuid,
    event_tx: broadcast::Sender<GameEvent>,
}

#[derive(Clone)]
struct GameSnapshot {
    core: automatafl_logic::Game,
    player_ids: HashMap<Uuid, automatafl_logic::Pid>,
    lifecycle: GameLifecycle,
    timestamp: u64,
}

const SESSION_DURATION_SECS: u64 = 24 * 60 * 60;  // 24 hours

#[derive(Clone)]
struct AppState {
    games: HashMap<Uuid, GameState>,
    players: HashMap<Uuid, PlayerInfo>,
    sessions: HashMap<Uuid, Session>,
    snapshots: HashMap<Uuid, Vec<GameSnapshot>>,
}

type ServerState = State<Arc<RwLock<AppState>>>;

// ============================================================================
// Custom Extractors
// ============================================================================

/// Authenticated player extractor
struct AuthPlayer {
    player_id: Uuid,
    session_id: Uuid,
}

impl FromRequestParts<Arc<RwLock<AppState>>> for AuthPlayer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<RwLock<AppState>>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let session_id = Uuid::parse_str(auth_header).map_err(|_| AppError::Unauthorized)?;

        let state = state.read().await;
        let session = state
            .sessions
            .get(&session_id)
            .ok_or(AppError::Unauthorized)?;

        // Check if session has expired
        if session.expires_at < timestamp() {
            return Err(AppError::SessionExpired);
        }

        Ok(AuthPlayer {
            player_id: session.player_id,
            session_id,
        })
    }
}

/// Admin player extractor
struct AdminPlayer {
    #[allow(unused)]
    player_id: Uuid,
}

impl FromRequestParts<Arc<RwLock<AppState>>> for AdminPlayer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<RwLock<AppState>>,
    ) -> Result<Self, Self::Rejection> {
        let auth_player = AuthPlayer::from_request_parts(parts, state).await?;

        let state = state.read().await;
        let player = state
            .players
            .get(&auth_player.player_id)
            .ok_or_else(|| AppError::NoSuchPlayer(auth_player.player_id))?;

        if !player.is_admin {
            return Err(AppError::Forbidden);
        }

        Ok(AdminPlayer {
            player_id: auth_player.player_id,
        })
    }
}

/// Extractor for authenticated player + game they're in
struct PlayerInGame {
    player_uuid: Uuid,
    player_pid: Pid,
    game_uuid: Uuid,
}

impl PlayerInGame {
    async fn extract(
        auth: AuthPlayer,
        game_uuid: Uuid,
        state: &Arc<RwLock<AppState>>,
    ) -> Result<Self, AppError> {
        let state_guard = state.read().await;
        let game = state_guard
            .games
            .get(&game_uuid)
            .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

        let player_pid = *game
            .player_ids
            .get(&auth.player_id)
            .ok_or_else(|| AppError::NotInGame(auth.player_id, game_uuid))?;

        Ok(PlayerInGame {
            player_uuid: auth.player_id,
            player_pid,
            game_uuid,
        })
    }
}

// ============================================================================
// Errors
// ============================================================================

enum AppError {
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
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
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
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn broadcast_event(game: &GameState, kind: String, data: serde_json::Value) {
    let event = GameEvent { kind, data };
    let _ = game.event_tx.send(event);
}

// ============================================================================
// WebSocket Handler
// ============================================================================

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, game_uuid))
}

async fn handle_socket(socket: WebSocket, state: Arc<RwLock<AppState>>, game_uuid: Uuid) {
    let mut rx = {
        let state_guard = state.read().await;
        match state_guard.games.get(&game_uuid) {
            Some(game) => game.event_tx.subscribe(),
            None => return,
        }
    };

    let (mut sender, mut receiver) = socket.split();

    // Send initial game state
    {
        let state_guard = state.read().await;
        if let Some(game) = state_guard.games.get(&game_uuid) {
            let state_json = serde_json::json!({
                "kind": "STATE",
                "lifecycle": game.lifecycle,
                "game": game.core,
                "player_ids": game.player_ids,
            });
            if let Ok(msg) = serde_json::to_string(&state_json) {
                let _ = sender.send(Message::Text(msg.into())).await;
            }
        }
    }

    // Spawn task to receive events and forward to websocket with heartbeat
    let mut send_task = tokio::spawn(async move {
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
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

// ============================================================================
// Health Check
// ============================================================================

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        api_version: "v1".to_string(),
        cargo_package_version: CARGO_PACKAGE_VERSION.map(|s| s.to_string()),
        timestamp: timestamp(),
    })
}

// ============================================================================
// Auth Endpoints
// ============================================================================

async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    let mut state = state.write().await;

    // Check for duplicate displayname
    if state
        .players
        .values()
        .any(|p| p.displayname == payload.displayname)
    {
        return Err(AppError::DisplaynameTaken(payload.displayname));
    }

    // Hash password
    let password_hash = bcrypt::hash(&payload.password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::InvalidCredentials)?;

    let new_uuid = Uuid::new_v4();
    state.players.insert(
        new_uuid,
        PlayerInfo {
            id: new_uuid,
            displayname: payload.displayname,
            password_hash,
            is_admin: false,
        },
    );

    Ok(Json(RegisterResponse {
        player_id: new_uuid,
    }))
}


async fn login(
    State(state): ServerState,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let mut state = state.write().await;

    // Find player by displayname
    let player = state
        .players
        .values()
        .find(|p| p.displayname == req.displayname)
        .ok_or(AppError::InvalidCredentials)?;

    // Verify password
    if !bcrypt::verify(&req.password, &player.password_hash)
        .map_err(|_| AppError::InvalidCredentials)?
    {
        return Err(AppError::InvalidCredentials);
    }

    let player_id = player.id;

    // Create session with expiration
    let session_id = Uuid::new_v4();
    state.sessions.insert(
        session_id,
        Session {
            player_id,
            expires_at: timestamp() + SESSION_DURATION_SECS,
        },
    );

    Ok(Json(LoginResponse {
        session_id,
        player_id,
    }))
}

async fn logout(auth: AuthPlayer, State(state): ServerState) -> StatusCode {
    let mut state = state.write().await;
    state.sessions.remove(&auth.session_id);
    StatusCode::OK
}

// ============================================================================
// Game Management Endpoints
// ============================================================================

async fn list_games(State(state): ServerState) -> Json<Vec<GameListItem>> {
    let state = state.read().await;
    let games = state
        .games
        .iter()
        .map(|(id, game)| GameListItem {
            id: *id,
            lifecycle: game.lifecycle.clone(),
            player_count: game.player_ids.len(),
            max_players: game.core.player_count,
            created_at: game.created_at,
            created_by: game.created_by,
        })
        .collect();
    Json(games)
}

async fn create_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Json(req): Json<CreateGameRequest>,
) -> Json<Uuid> {
    let mut state = state.write().await;
    let board = Board::stock_two_player();

    let (event_tx, _) = broadcast::channel(100);

    let mut new_game = GameState {
        core: automatafl_logic::Game::new(board, req.player_count, req.use_column_rule),
        player_ids: HashMap::new(),
        lifecycle: GameLifecycle::Waiting,
        chat: Vec::new(),
        created_at: timestamp(),
        created_by: auth.player_id,
        event_tx,
    };

    // Set up goals for two-player game
    if req.player_count == 2 {
        new_game.core.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
        new_game.core.goals.push((Coord { x: 10, y: 0 }, Pid(0)));
        new_game.core.goals.push((Coord { x: 0, y: 10 }, Pid(1)));
        new_game.core.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
    }

    let new_uuid = Uuid::new_v4();
    state.games.insert(new_uuid, new_game);

    Json(new_uuid)
}

async fn join_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Pid>, AppError> {
    let mut state = state.write().await;
    let player_uuid = auth.player_id;

    // Get player name first before accessing game
    let player_name = state
        .players
        .get(&player_uuid)
        .map(|p| p.displayname.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let gm = state
        .games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    if gm.player_ids.len() >= (gm.core.player_count as usize) {
        return Err(AppError::GameFull(game_uuid));
    }
    if gm.player_ids.contains_key(&player_uuid) {
        return Err(AppError::PlayerAlreadyExists(player_uuid));
    }

    let new_player_id = Pid(gm.player_ids.len() as u8);
    gm.player_ids.insert(player_uuid, new_player_id);

    // Broadcast join event
    broadcast_event(
        gm,
        "PLAYER_JOINED".to_string(),
        serde_json::json!({
            "player_id": player_uuid,
            "player_pid": new_player_id,
            "displayname": player_name,
        }),
    ).await;

    // Auto-start game when full
    if gm.player_ids.len() == gm.core.player_count as usize {
        gm.lifecycle = GameLifecycle::InProgress;
        broadcast_event(
            gm,
            "GAME_STARTED".to_string(),
            serde_json::json!({}),
        ).await;
    }

    Ok(Json(new_player_id))
}

async fn get_game_state(
    _auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<GameStateResponse>, AppError> {
    let state = state.read().await;

    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(GameStateResponse {
        lifecycle: gm.lifecycle.clone(),
        game: gm.core.clone(),
        player_ids: gm.player_ids.clone(),
    }))
}

async fn get_goals(
    _auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<(Coord, Pid)>>, AppError> {
    let state = state.read().await;

    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(gm.core.goals.to_vec()))
}

// ============================================================================
// Move Endpoints
// ============================================================================

async fn pending_move(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Option<automatafl_logic::Move>>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let state = state.read().await;

    let gm = state
        .games
        .get(&player_in_game.game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(player_in_game.game_uuid))?;

    Ok(Json(
        gm.core
            .pending_moves
            .iter()
            .find(|mv| mv.who == player_in_game.player_pid)
            .cloned(),
    ))
}

async fn perform_move(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Json(move_to_make): Json<PerformMove>,
) -> Result<Json<MoveResult>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let mut state = state.write().await;

    let gm = state
        .games
        .get_mut(&player_in_game.game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(player_in_game.game_uuid))?;

    let m = Move {
        who: player_in_game.player_pid,
        from: move_to_make.from,
        to: move_to_make.to,
    };
    let (feedback, ready_to_complete) = gm.core.propose_move(m);

    // Broadcast move acknowledgment
    if feedback == MoveFeedback::Committed {
        broadcast_event(
            gm,
            "MOVE_ACK".to_string(),
            serde_json::json!({
                "player_pid": player_in_game.player_pid,
                "from": move_to_make.from,
                "to": move_to_make.to,
            }),
        ).await;
    } else {
        broadcast_event(
            gm,
            "MOVE_INVALID".to_string(),
            serde_json::json!({
                "player_pid": player_in_game.player_pid,
                "feedback": feedback,
            }),
        ).await;
    }

    let mut auto_completed = false;

    // AUTO-PROGRESSION: If all moves are in, try to complete the round
    if ready_to_complete {
        match gm.core.try_complete_round() {
            Ok(results) => {
                // Broadcast all move results
                for (mv, result) in &results {
                    broadcast_event(
                        gm,
                        "MOVE".to_string(),
                        serde_json::json!({
                            "player_pid": mv.who,
                            "from": mv.from,
                            "to": mv.to,
                            "result": result,
                        }),
                    ).await;
                }

                // Broadcast automaton move
                broadcast_event(
                    gm,
                    "AUTOMATON_STEP".to_string(),
                    serde_json::json!({
                        "location": gm.core.board.automaton_location,
                    }),
                ).await;

                // Check for winner
                if let Some(winner) = gm.core.winner {
                    gm.lifecycle = GameLifecycle::Finished;
                    broadcast_event(
                        gm,
                        "GAME_OVER".to_string(),
                        serde_json::json!({
                            "winner": winner,
                        }),
                    ).await;
                } else {
                    broadcast_event(
                        gm,
                        "ROUND_COMPLETE".to_string(),
                        serde_json::json!({}),
                    ).await;
                }

                auto_completed = true;
            }
            Err(_) => {
                // Conflicts occurred
                broadcast_event(
                    gm,
                    "CONFLICTS".to_string(),
                    serde_json::json!({
                        "locked_players": gm.core.locked_players,
                        "conflict_coords": gm.core.board.conflict_list,
                    }),
                ).await;
            }
        }
    }

    Ok(Json(MoveResult {
        feedback,
        ready_to_complete,
        auto_completed,
    }))
}

async fn complete_round(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let mut state = state.write().await;

    let gm = state
        .games
        .get_mut(&player_in_game.game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(player_in_game.game_uuid))?;

    match gm.lifecycle {
        GameLifecycle::InProgress => {
            match gm.core.try_complete_round() {
                Ok(results) => {
                    // Broadcast all move results
                    for (mv, result) in &results {
                        broadcast_event(
                            gm,
                            "MOVE".to_string(),
                            serde_json::json!({
                                "player_pid": mv.who,
                                "from": mv.from,
                                "to": mv.to,
                                "result": result,
                            }),
                        ).await;
                    }

                    // Broadcast automaton move
                    broadcast_event(
                        gm,
                        "AUTOMATON_STEP".to_string(),
                        serde_json::json!({
                            "location": gm.core.board.automaton_location,
                        }),
                    ).await;

                    // Check if game is now over
                    if gm.core.winner.is_some() {
                        gm.lifecycle = GameLifecycle::Finished;
                        broadcast_event(
                            gm,
                            "GAME_OVER".to_string(),
                            serde_json::json!({
                                "winner": gm.core.winner,
                            }),
                        ).await;
                    } else {
                        broadcast_event(
                            gm,
                            "ROUND_COMPLETE".to_string(),
                            serde_json::json!({}),
                        ).await;
                    }

                    Ok(Json(CompleteRoundResponse {
                        success: true,
                        message: "Round completed successfully".to_string(),
                    }))
                }
                Err(_) => {
                    broadcast_event(
                        gm,
                        "CONFLICTS".to_string(),
                        serde_json::json!({
                            "locked_players": gm.core.locked_players,
                            "conflict_coords": gm.core.board.conflict_list,
                        }),
                    ).await;

                    Ok(Json(CompleteRoundResponse {
                        success: false,
                        message: "Conflict resolution needed".to_string(),
                    }))
                }
            }
        }
        _ => Err(AppError::GameNotInProgress),
    }
}

// ============================================================================
// Chat Endpoints
// ============================================================================


async fn post_chat(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Json(req): Json<PostChatRequest>,
) -> Result<Json<PostChatResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let mut state = state.write().await;

    // Get player name first before accessing game
    let player_name = state
        .players
        .get(&player_in_game.player_uuid)
        .map(|p| p.displayname.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let gm = state
        .games
        .get_mut(&player_in_game.game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(player_in_game.game_uuid))?;

    let ts = timestamp();
    let chat_msg = ChatMessage {
        timestamp: ts,
        player_id: player_in_game.player_uuid,
        displayname: player_name.clone(),
        message: req.message.clone(),
    };

    gm.chat.push(chat_msg.clone());

    broadcast_event(
        gm,
        "CHAT".to_string(),
        serde_json::json!({
            "timestamp": ts,
            "player_id": player_in_game.player_uuid,
            "displayname": player_name,
            "message": req.message,
        }),
    ).await;

    Ok(Json(PostChatResponse { timestamp: ts }))
}

async fn get_chat(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<ChatMessage>>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let state = state.read().await;

    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(gm.chat.clone()))
}

// ============================================================================
// Save/Load Endpoints
// ============================================================================

async fn save_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<SaveGameResponse>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let mut state = state.write().await;

    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    let snapshot = GameSnapshot {
        core: gm.core.clone(),
        player_ids: gm.player_ids.clone(),
        lifecycle: gm.lifecycle.clone(),
        timestamp: timestamp(),
    };

    let snapshots = state.snapshots.entry(game_uuid).or_insert_with(Vec::new);
    snapshots.push(snapshot);
    let index = snapshots.len() - 1;

    Ok(Json(SaveGameResponse {
        snapshot_index: index,
    }))
}

async fn list_snapshots(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<ListSnapshotsResponse>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let state = state.read().await;

    let snapshots = state
        .snapshots
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchSnapshot(game_uuid))?;

    let info: Vec<_> = snapshots
        .iter()
        .enumerate()
        .map(|(index, snap)| SnapshotInfo {
            index,
            timestamp: snap.timestamp,
        })
        .collect();

    Ok(Json(ListSnapshotsResponse { snapshots: info }))
}

async fn load_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Path((game_uuid, snapshot_index)): Path<(Uuid, usize)>,
) -> Result<StatusCode, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;
    let mut state = state.write().await;

    let snapshot = state
        .snapshots
        .get(&game_uuid)
        .and_then(|snaps| snaps.get(snapshot_index))
        .ok_or_else(|| AppError::NoSuchSnapshot(game_uuid))?
        .clone();

    let gm = state
        .games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    gm.core = snapshot.core;
    gm.player_ids = snapshot.player_ids;
    gm.lifecycle = snapshot.lifecycle;

    broadcast_event(
        gm,
        "GAME_LOADED".to_string(),
        serde_json::json!({
            "snapshot_index": snapshot_index,
        }),
    ).await;

    Ok(StatusCode::OK)
}

// ============================================================================
// Admin Endpoints
// ============================================================================

async fn admin_list_players(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Json<Vec<PlayerListItem>> {
    let state = state.read().await;
    let players = state
        .players
        .values()
        .map(|p| PlayerListItem {
            id: p.id,
            displayname: p.displayname.clone(),
            is_admin: p.is_admin,
        })
        .collect();
    Json(players)
}

async fn admin_list_all_games(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Json<Vec<GameListItem>> {
    list_games(State(state)).await
}

async fn admin_delete_game(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let mut state = state.write().await;
    state
        .games
        .remove(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_force_complete_round(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    let mut state = state.write().await;

    let gm = state
        .games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    match gm.core.try_complete_round() {
        Ok(_results) => {
            if gm.core.winner.is_some() {
                gm.lifecycle = GameLifecycle::Finished;
            }
            Ok(Json(CompleteRoundResponse {
                success: true,
                message: "Admin forced round completion".to_string(),
            }))
        }
        Err(_) => Ok(Json(CompleteRoundResponse {
            success: false,
            message: "Conflict resolution needed".to_string(),
        })),
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Configure permissive CORS for open web/API access
    let cors = CorsLayer::permissive();

    let app = Router::new()
        // Health check (no auth required)
        .route("/api/health", get(health_check))
        // Public endpoints
        .route("/api/v1/register", post(register_player))
        .route("/api/v1/login", post(login))
        // Authenticated endpoints
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/games", get(list_games).post(create_game))
        .route("/api/v1/games/{:id}", get(get_game_state).post(join_game))
        .route("/api/v1/games/{:id}/goals", get(get_goals))
        .route("/api/v1/games/{:id}/move", get(pending_move).post(perform_move))
        .route("/api/v1/games/{:id}/complete", post(complete_round))
        .route("/api/v1/games/{:id}/chat", get(get_chat).post(post_chat))
        .route("/api/v1/games/{:id}/save", post(save_game))
        .route("/api/v1/games/{:id}/snapshots", get(list_snapshots))
        .route("/api/v1/games/{:id}/load/{:snapshot_index}", post(load_game))
        .route("/api/v1/games/{:id}/ws", get(ws_handler))
        // Admin endpoints
        .route("/api/v1/admin/players", get(admin_list_players))
        .route("/api/v1/admin/games", get(admin_list_all_games))
        .route("/api/v1/admin/games/{:id}", axum::routing::delete(admin_delete_game))
        .route("/api/v1/admin/games/{:id}/force-complete", post(admin_force_complete_round))
        .layer(cors)  // Apply CORS to all routes
        .with_state(Arc::new(RwLock::new(AppState {
            games: HashMap::new(),
            players: HashMap::new(),
            sessions: HashMap::new(),
            snapshots: HashMap::new(),
        })));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
