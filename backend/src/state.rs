use dashmap::DashMap;
use sqlx::SqlitePool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{models::StoredGameState, matchmaking::MatchmakingQueue};
use tokio::sync::{broadcast, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub jwt_secret: String,
    pub active_games: Arc<DashMap<Uuid, Arc<Mutex<StoredGameState>>>>,
    pub game_broadcasts: Arc<DashMap<Uuid, broadcast::Sender<String>>>,
    pub matchmaking_queue: Arc<MatchmakingQueue>,
}

impl AppState {
    pub fn new(db: SqlitePool) -> Self {
        // JWT_SECRET must be set in production
        let jwt_secret = std::env::var("JWT_SECRET")
            .expect("JWT_SECRET environment variable must be set");
        
        Self {
            db,
            jwt_secret,
            active_games: Arc::new(DashMap::new()),
            game_broadcasts: Arc::new(DashMap::new()),
            matchmaking_queue: Arc::new(MatchmakingQueue::new()),
        }
    }
}
