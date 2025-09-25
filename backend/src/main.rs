use std::{collections::HashMap, net::SocketAddr};

use automatafl_logic::Board;
use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};

use uuid::Uuid;

use tracing_subscriber::prelude::*;

#[derive(Clone)]
struct PlayerInfo {
    id: Uuid,
}

#[derive(Clone)]
struct AppState {
    games: HashMap<Uuid, automatafl_logic::Game>,
    players: HashMap<Uuid, PlayerInfo>,
}

async fn list_games(State(state): State<AppState>) -> Json<Vec<Uuid>> {
    axum::Json(state.games.keys().cloned().collect())
}

async fn create_game(State(mut state): State<AppState>) -> Json<Uuid> {
    let new_game = automatafl_logic::Game::new(Board::stock_two_player(), 2, true);
    let new_uuid = Uuid::new_v4();

    state.games.insert(new_uuid.clone(), new_game);

    Json(new_uuid)
}

async fn join_game(State(state): State<AppState>, game_uuid: axum::extract::Path<Uuid>) -> StatusCode {
   StatusCode::OK 
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    // functionality: create, join, play, end games.

    let app = Router::new()
        .route("/api/v1", get(|| async { "Hello, World!" }))
        .route("/api/v1/games", get(list_games))
        .route("/api/v1/games/{id}", post(join_game))
        .with_state(AppState {
            games: HashMap::new(),
            players: HashMap::new(),
        });

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
