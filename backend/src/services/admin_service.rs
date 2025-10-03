use std::sync::Arc;

use crate::repositories::{
    GameRepository,
    MatchmakingRepository,
    PlayerRepository,
    SessionRepository,
};

use super::{GameService, PlayerService};

/// Aggregates administrative operations across repositories and services
pub struct AdminService {
    pub(crate) game_repo: GameRepository,
    pub(crate) player_repo: PlayerRepository,
    pub(crate) session_repo: SessionRepository,
    pub(crate) matchmaking_repo: MatchmakingRepository,
    pub(crate) game_service: Arc<GameService>,
    pub(crate) player_service: Arc<PlayerService>,
}

impl AdminService {
    pub fn new(
        game_repo: GameRepository,
        player_repo: PlayerRepository,
        session_repo: SessionRepository,
        matchmaking_repo: MatchmakingRepository,
        game_service: Arc<GameService>,
        player_service: Arc<PlayerService>,
    ) -> Self {
        Self {
            game_repo,
            player_repo,
            session_repo,
            matchmaking_repo,
            game_service,
            player_service,
        }
    }
}
