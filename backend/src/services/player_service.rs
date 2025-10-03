use std::fmt;

use crate::{
    db::{PlayerRecord, PlayerStatsRecord},
    repositories::PlayerRepository,
    validation,
};
use uuid::Uuid;

pub struct PlayerService {
    player_repo: PlayerRepository,
}

#[derive(Debug)]
pub enum PlayerServiceError {
    Validation(String),
    Database(surrealdb::Error),
    NotFound(Uuid),
}

impl From<surrealdb::Error> for PlayerServiceError {
    fn from(value: surrealdb::Error) -> Self {
        Self::Database(value)
    }
}

impl fmt::Display for PlayerServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerServiceError::Validation(msg) => write!(f, "Validation error: {}", msg),
            PlayerServiceError::Database(err) => write!(f, "Database error: {}", err),
            PlayerServiceError::NotFound(id) => write!(f, "Player not found: {}", id),
        }
    }
}

impl std::error::Error for PlayerServiceError {}

impl PlayerService {
    pub fn new(player_repo: PlayerRepository) -> Self {
        Self { player_repo }
    }

    pub async fn get_player(
        &self,
        player_id: Uuid,
    ) -> Result<Option<PlayerRecord>, PlayerServiceError> {
        self.player_repo.get(player_id).await.map_err(Into::into)
    }

    pub async fn get_player_or_error(
        &self,
        player_id: Uuid,
    ) -> Result<PlayerRecord, PlayerServiceError> {
        self.player_repo
            .get(player_id)
            .await?
            .ok_or(PlayerServiceError::NotFound(player_id))
    }

    pub async fn update_profile(
        &self,
        player_id: Uuid,
        bio: Option<String>,
        avatar_url: Option<String>,
    ) -> Result<(), PlayerServiceError> {
        if let Some(ref bio) = bio {
            validation::validate_bio(bio)
                .map_err(|e| PlayerServiceError::Validation(e.to_string()))?;
        }
        if let Some(ref url) = avatar_url {
            validation::validate_avatar_url(url)
                .map_err(|e| PlayerServiceError::Validation(e.to_string()))?;
        }

        self.player_repo
            .update_profile(player_id, bio, avatar_url)
            .await?;
        Ok(())
    }

    pub async fn get_stats(
        &self,
        player_id: Uuid,
    ) -> Result<Option<PlayerStatsRecord>, PlayerServiceError> {
        self.player_repo
            .get_stats(player_id)
            .await
            .map_err(Into::into)
    }

    pub async fn update_admin_fields(
        &self,
        player_id: Uuid,
        displayname: Option<String>,
        is_admin: Option<bool>,
        bio: Option<String>,
        avatar_url: Option<String>,
        elo_rating: Option<i32>,
    ) -> Result<(), PlayerServiceError> {
        self.player_repo
            .update_admin_fields(
                player_id,
                displayname,
                is_admin,
                bio,
                avatar_url,
                elo_rating,
            )
            .await?;
        Ok(())
    }

    pub async fn delete_player(&self, player_id: Uuid) -> Result<(), PlayerServiceError> {
        self.player_repo.delete(player_id).await?;
        Ok(())
    }

    pub async fn delete_player_stats(&self, player_id: Uuid) -> Result<(), PlayerServiceError> {
        self.player_repo.delete_stats(player_id).await?;
        Ok(())
    }

    pub async fn update_stats_fields(
        &self,
        player_id: Uuid,
        games_played: Option<u32>,
        games_won: Option<u32>,
        total_playtime: Option<u64>,
    ) -> Result<(), PlayerServiceError> {
        self.player_repo
            .update_stats_fields(player_id, games_played, games_won, total_playtime)
            .await?;
        Ok(())
    }

    pub async fn list_players(&self) -> Result<Vec<PlayerRecord>, PlayerServiceError> {
        self.player_repo.list_all().await.map_err(Into::into)
    }

    pub async fn leaderboard_by_elo(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, usize)>, PlayerServiceError> {
        self.player_repo
            .get_leaderboard_by_elo(limit)
            .await
            .map_err(Into::into)
    }

    pub async fn leaderboard_by_wins(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, PlayerServiceError> {
        self.player_repo
            .get_leaderboard_by_wins(limit)
            .await
            .map_err(Into::into)
    }

    pub async fn leaderboard_by_games(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, PlayerServiceError> {
        self.player_repo
            .get_leaderboard_by_games(limit)
            .await
            .map_err(Into::into)
    }
}
