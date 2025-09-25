use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use automatafl_logic::{AutomatonDecision, Board, Coord, Move, MoveFeedback, Pid};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use tracing_subscriber::prelude::*;

#[derive(Clone)]
struct PlayerInfo {
    id: Uuid,
    displayname: String,
    password: String,
}

#[derive(Clone)]
struct GameState {
    core: automatafl_logic::Game,
    player_ids: HashMap<Uuid, automatafl_logic::Pid>,
}

#[derive(Clone)]
struct AppState {
    games: HashMap<Uuid, GameState>,
    players: HashMap<Uuid, PlayerInfo>,
}

type ServerState = State<Arc<RwLock<AppState>>>;

enum AppError {
    NoSuchGame(Uuid),
    NoSuchPlayer(Uuid),
    PlayerAlreadyExists(Uuid),
    GameFull(Uuid),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        // i don't like this:
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

async fn list_games(State(state): ServerState) -> Json<Vec<Uuid>> {
    let state = state.read().await;
    axum::Json(state.games.keys().cloned().collect())
}

async fn create_game(State(state): ServerState) -> Json<Uuid> {
    let mut state = state.write().await;
    let new_game = GameState {
        core: automatafl_logic::Game::new(Board::stock_two_player(), 2, true),
        player_ids: HashMap::new(),
    };
    let new_uuid = Uuid::new_v4();

    state.games.insert(new_uuid.clone(), new_game);

    Json(new_uuid)
}

async fn join_game(
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(player_uuid): Query<Uuid>,
) -> Result<Json<Pid>, AppError> {
    let mut state = state.write().await;
    let &mut AppState {
        ref mut players,
        ref mut games,
        ..
    } = &mut *state;

    let game_uuid: &Uuid = &game_uuid;
    let player_uuid: &Uuid = &player_uuid;

    let gm = games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid.clone()))?;

    let _pl = players
        .get_mut(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid.clone()))?;

    if gm.player_ids.len() >= (gm.core.player_count as usize) {
        return Err(AppError::GameFull(game_uuid.clone()));
    }
    if gm.player_ids.contains_key(&player_uuid) {
        return Err(AppError::PlayerAlreadyExists(player_uuid.clone()));
    }

    let new_player_id = gm.player_ids.values().cloned().max().unwrap_or(Pid(0));
    gm.player_ids.insert(*player_uuid, new_player_id);

    Ok(Json(new_player_id))
}

#[derive(Deserialize)]
struct RegisterPlayer {
    displayname: String,
    password: String,
}
async fn register_player(
    State(state): ServerState,
    Json(payload): Json<RegisterPlayer>,
) -> Result<Json<Uuid>, AppError> {
    let new_uuid = Uuid::new_v4();
    // scan for duplicated displayname
    state.write().await.players.insert(
        new_uuid.clone(),
        PlayerInfo {
            id: new_uuid,
            displayname: payload.displayname,
            password: payload.password,
        },
    );

    Ok(Json(new_uuid))
}

async fn pending_move(
    State(state): ServerState,
    Path((game_uuid,)): Path<(Uuid,)>,
    Query(player_uuid): Query<Uuid>,
) -> Result<Json<Option<automatafl_logic::Move>>, AppError> {
    let state = state.read().await;

    let gm = state
        .games
        .get(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid.clone()))?;
    let pid = gm
        .player_ids
        .get(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid.clone()))?;

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
struct MoveResult {
    feedback: MoveFeedback,
    enqueued: bool,
}
async fn perform_move(
    State(state): ServerState,
    Path((game_uuid,)): Path<(Uuid,)>,
    Query(player_uuid): Query<Uuid>,
    Json(move_to_make): Json<PerformMove>,
) -> Result<Json<MoveResult>, AppError> {
    let mut state = state.write().await;

    let gm = state
        .games
        .get_mut(&game_uuid)
        .ok_or_else(|| AppError::NoSuchGame(game_uuid.clone()))?;

    let pid = gm
        .player_ids
        .get(&player_uuid)
        .ok_or_else(|| AppError::NoSuchPlayer(player_uuid.clone()))?;

    let m = Move {
        who: *pid,
        from: move_to_make.from,
        to: move_to_make.to,
    };
    let (feedback, enqueued) = gm.core.propose_move(m);

    Ok(Json(MoveResult { feedback, enqueued }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    // functionality: create, join, play, end games.

    let app = Router::new()
        .route("/api/v1/games", get(list_games).post(create_game))
        .route("/api/v1/games/{id}/move", get(pending_move).post(perform_move))
        .route("/api/v1/games/{id}", post(join_game))
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
