use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use automatafl_logic::{
    Board, Coord, Game, Move, MoveFeedback, MoveResult as LogicMoveResult, Pid, RoundState,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

#[derive(Clone)]
struct PlayerInfo {
    _id: Uuid,
    displayname: String,
    _password: String,
}

#[derive(Clone)]
struct GameState {
    core: Game,
    player_ids: HashMap<Uuid, Pid>,
    next_pid: u8,
    event_log: Vec<EventRecord>,
    next_event_seq: u64,
}

impl GameState {
    fn new(player_count: u8, use_column_rule: bool) -> Self {
        let core = Game::new(Board::stock_two_player(), player_count, use_column_rule);
        Self {
            core,
            player_ids: HashMap::new(),
            next_pid: 0,
            event_log: Vec::new(),
            next_event_seq: 0,
        }
    }

    fn record_event(&mut self, audience: Audience, event: GameEvent) {
        let seq = self.next_event_seq;
        self.next_event_seq += 1;
        self.event_log.push(EventRecord {
            seq,
            audience,
            event,
        });
    }

    fn events_for(&self, pid: Option<Pid>, since: u64) -> Vec<EventEnvelope> {
        self.event_log
            .iter()
            .filter(|evt| evt.seq >= since && evt.audience.matches(pid))
            .map(|evt| EventEnvelope {
                seq: evt.seq,
                event: evt.event.clone(),
            })
            .collect()
    }

    fn player_summaries(&self, players: &HashMap<Uuid, PlayerInfo>) -> Vec<PlayerSummary> {
        let mut out = Vec::new();
        for (uuid, pid) in &self.player_ids {
            if let Some(info) = players.get(uuid) {
                out.push(PlayerSummary {
                    uuid: *uuid,
                    pid: *pid,
                    displayname: info.displayname.clone(),
                });
            }
        }
        out.sort_by_key(|p| p.pid.0);
        out
    }
}

#[derive(Clone)]
struct AppState {
    games: HashMap<Uuid, GameState>,
    players: HashMap<Uuid, PlayerInfo>,
}

type ServerState = State<Arc<RwLock<AppState>>>;

#[derive(Debug)]
enum AppError {
    NoSuchGame(Uuid),
    NoSuchPlayer(Uuid),
    PlayerAlreadyExists(Uuid),
    GameFull(Uuid),
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GameEvent {
    PlayerJoined {
        player: PlayerSummary,
    },
    MoveQueued {
        pid: Pid,
    },
    MoveRejected {
        pid: Pid,
        feedback: MoveFeedback,
    },
    ConflictsDetected {
        moves: Vec<Move>,
    },
    MovesResolved {
        results: Vec<AppliedMove>,
        automaton: Option<AutomatonMotion>,
        winner: Option<Pid>,
    },
    RoundState {
        round: RoundState,
        locked: Vec<Pid>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct AppliedMove {
    mv: Move,
    outcome: LogicMoveResult,
}

impl Clone for AppliedMove {
    fn clone(&self) -> Self {
        let outcome = match &self.outcome {
            LogicMoveResult::NoSource => LogicMoveResult::NoSource,
            LogicMoveResult::OccupiedAt(coord) => LogicMoveResult::OccupiedAt(*coord),
            LogicMoveResult::Applied => LogicMoveResult::Applied,
        };
        Self {
            mv: self.mv,
            outcome,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomatonMotion {
    from: Coord,
    to: Coord,
}

#[derive(Clone, Copy)]
enum Audience {
    Broadcast,
    Player(Pid),
}

impl Audience {
    fn matches(&self, pid: Option<Pid>) -> bool {
        match (self, pid) {
            (Audience::Broadcast, _) => true,
            (Audience::Player(expected), Some(actual)) => expected == &actual,
            (Audience::Player(_), None) => false,
        }
    }
}

#[derive(Clone)]
struct EventRecord {
    seq: u64,
    audience: Audience,
    event: GameEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventEnvelope {
    seq: u64,
    event: GameEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventsResponse {
    events: Vec<EventEnvelope>,
    next_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlayerSummary {
    uuid: Uuid,
    pid: Pid,
    displayname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameSnapshot {
    id: Uuid,
    game: Game,
    players: Vec<PlayerSummary>,
}

fn default_goals(board: &Board, pid: Pid, total_players: u8) -> Vec<Coord> {
    let max_x = board.size.x - 1;
    let max_y = board.size.y - 1;
    match total_players {
        2 => match pid.0 {
            0 => vec![Coord { x: 0, y: 0 }, Coord { x: max_x, y: 0 }],
            1 => vec![Coord { x: max_x, y: max_y }, Coord { x: 0, y: max_y }],
            _ => Vec::new(),
        },
        4 => match pid.0 {
            0 => vec![Coord { x: 0, y: 0 }],
            1 => vec![Coord { x: max_x, y: 0 }],
            2 => vec![Coord { x: max_x, y: max_y }],
            3 => vec![Coord { x: 0, y: max_y }],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

async fn list_games(State(state): ServerState) -> Json<Vec<Uuid>> {
    let state = state.read().await;
    Json(state.games.keys().cloned().collect())
}

async fn game_snapshot(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<GameSnapshot>, AppError> {
    let state = state.read().await;
    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;
    Ok(Json(GameSnapshot {
        id: game_uuid,
        game: gm.core.clone(),
        players: gm.player_summaries(&state.players),
    }))
}

async fn create_game(State(state): ServerState) -> Json<Uuid> {
    let mut state = state.write().await;
    let mut new_game = GameState::new(2, true);
    new_game.record_event(
        Audience::Broadcast,
        GameEvent::RoundState {
            round: new_game.core.round,
            locked: Vec::new(),
        },
    );
    let new_uuid = Uuid::new_v4();
    state.games.insert(new_uuid, new_game);
    Json(new_uuid)
}

async fn join_game(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(player_uuid): Query<Uuid>,
) -> Result<Json<Pid>, AppError> {
    let mut state = state.write().await;
    let (players, games) = {
        let AppState { players, games } = &mut *state;
        (players, games)
    };

    let gm = games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;
    let info = players
        .get(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid))?;

    if gm.player_ids.contains_key(&player_uuid) {
        return Err(AppError::PlayerAlreadyExists(player_uuid));
    }

    if gm.player_ids.len() >= gm.core.player_count as usize {
        return Err(AppError::GameFull(game_uuid));
    }

    let pid = Pid(gm.next_pid);
    gm.next_pid += 1;

    gm.player_ids.insert(player_uuid, pid);

    // Assign default goals for the player.
    gm.core.goals.retain(|(_, owner)| owner != &pid);
    for goal in default_goals(&gm.core.board, pid, gm.core.player_count) {
        gm.core.goals.push((goal, pid));
    }

    gm.record_event(
        Audience::Broadcast,
        GameEvent::PlayerJoined {
            player: PlayerSummary {
                uuid: player_uuid,
                pid,
                displayname: info.displayname.clone(),
            },
        },
    );

    Ok(Json(pid))
}

#[derive(Deserialize)]
struct RegisterPlayer {
    displayname: String,
    password: String,
}

async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterPlayer>,
) -> Json<Uuid> {
    let new_uuid = Uuid::new_v4();
    state.write().await.players.insert(
        new_uuid,
        PlayerInfo {
            _id: new_uuid,
            displayname: payload.displayname,
            _password: payload.password,
        },
    );
    Json(new_uuid)
}

async fn pending_move(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(player_uuid): Query<Uuid>,
) -> Result<Json<Option<Move>>, AppError> {
    let state = state.read().await;
    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;
    let pid = gm
        .player_ids
        .get(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid))?;
    Ok(Json(
        gm.core
            .pending_moves
            .iter()
            .find(|mv| mv.who == *pid)
            .cloned(),
    ))
}

#[derive(Serialize, Deserialize)]
struct PerformMove {
    from: Coord,
    to: Coord,
}

#[derive(Serialize, Deserialize)]
struct MoveAttemptResult {
    feedback: MoveFeedback,
    enqueued: bool,
}

async fn perform_move(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(player_uuid): Query<Uuid>,
    Json(move_to_make): Json<PerformMove>,
) -> Result<Json<MoveAttemptResult>, AppError> {
    let mut state = state.write().await;

    let gm = state
        .games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    let pid = gm
        .player_ids
        .get(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid))?;

    let mv = Move {
        who: *pid,
        from: move_to_make.from,
        to: move_to_make.to,
    };

    let (feedback, enqueued) = gm.core.propose_move(mv.clone());

    match feedback.clone() {
        MoveFeedback::Committed => {
            gm.record_event(Audience::Broadcast, GameEvent::MoveQueued { pid: *pid })
        }
        _ => gm.record_event(
            Audience::Player(*pid),
            GameEvent::MoveRejected {
                pid: *pid,
                feedback: feedback.clone(),
            },
        ),
    }

    if enqueued {
        advance_round(gm);
    }

    Ok(Json(MoveAttemptResult { feedback, enqueued }))
}

fn advance_round(game: &mut GameState) {
    let automaton_start = game.core.board.automaton_location;
    let pending_before: Vec<Move> = game.core.pending_moves.iter().cloned().collect();

    match game.core.try_complete_round() {
        Ok(results) => {
            let applied: Vec<AppliedMove> = results
                .into_iter()
                .map(|(mv, outcome)| AppliedMove { mv, outcome })
                .collect();
            let automaton_end = game.core.board.automaton_location;
            let automaton = if automaton_end != automaton_start {
                Some(AutomatonMotion {
                    from: automaton_start,
                    to: automaton_end,
                })
            } else {
                None
            };

            game.core.locked_players.clear();

            game.record_event(
                Audience::Broadcast,
                GameEvent::MovesResolved {
                    results: applied,
                    automaton,
                    winner: game.core.winner,
                },
            );

            game.record_event(
                Audience::Broadcast,
                GameEvent::RoundState {
                    round: game.core.round,
                    locked: game.core.locked_players.iter().copied().collect(),
                },
            );
        }
        Err(()) => {
            let pending_after: Vec<Move> = game.core.pending_moves.iter().cloned().collect();
            let mut locked: Vec<Pid> = pending_after.iter().map(|mv| mv.who).collect();
            locked.sort_by_key(|pid| pid.0);
            locked.dedup();
            game.core.locked_players.clear();
            game.core.locked_players.extend(locked.iter().copied());

            let mut conflicts = Vec::new();
            for mv in pending_before {
                if !pending_after.contains(&mv) {
                    conflicts.push(mv);
                }
            }

            game.record_event(
                Audience::Broadcast,
                GameEvent::ConflictsDetected { moves: conflicts },
            );
            game.record_event(
                Audience::Broadcast,
                GameEvent::RoundState {
                    round: game.core.round,
                    locked,
                },
            );
        }
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    since: Option<u64>,
    player_uuid: Option<Uuid>,
}

async fn poll_events(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<EventsResponse>, AppError> {
    let state = state.read().await;
    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid))?;

    let pid = match query.player_uuid {
        Some(uuid) => Some(
            *gm.player_ids
                .get(&uuid)
                .ok_or_else(|| AppError::NoSuchPlayer(uuid))?,
        ),
        None => None,
    };

    let since = query.since.unwrap_or(0);
    let events = gm.events_for(pid, since);
    let next_seq = events.last().map(|ev| ev.seq + 1).unwrap_or_else(|| since);
    Ok(Json(EventsResponse { events, next_seq }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/api/v1/games", get(list_games).post(create_game))
        .route("/api/v1/games/{id}", post(join_game).get(game_snapshot))
        .route(
            "/api/v1/games/{id}/move",
            get(pending_move).post(perform_move),
        )
        .route("/api/v1/games/{id}/events", get(poll_events))
        .route("/api/v1/register", post(register_player))
        .with_state(Arc::new(RwLock::new(AppState {
            games: HashMap::new(),
            players: HashMap::new(),
        })));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
