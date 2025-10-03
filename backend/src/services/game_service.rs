use crate::repositories::game_repository::GameRepository;
use crate::repositories::player_repository::{PlayerRepository, PlayerStatsUpdate};
use automatafl_api_types::{
    CompleteRoundResponse, EloChange, GameEventData, GameLifecycle, MoveResultResponse,
};
use automatafl_logic::{Coord, Move, MoveFeedback, Pid};
use uuid::Uuid;

/// Service for game business logic
/// Centralizes all game operations that were duplicated across handlers
pub struct GameService {
    game_repo: GameRepository,
    player_repo: PlayerRepository,
}

/// Service-level errors
#[derive(Debug)]
pub enum ServiceError {
    GameNotFound(Uuid),
    GameNotInProgress,
    DatabaseError(surrealdb::Error),
    SerializationError(String),
}

impl From<surrealdb::Error> for ServiceError {
    fn from(e: surrealdb::Error) -> Self {
        ServiceError::DatabaseError(e)
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::GameNotFound(id) => write!(f, "Game not found: {}", id),
            ServiceError::GameNotInProgress => write!(f, "Game is not in progress"),
            ServiceError::DatabaseError(e) => write!(f, "Database error: {}", e),
            ServiceError::SerializationError(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for ServiceError {}

impl GameService {
    pub fn new(game_repo: GameRepository, player_repo: PlayerRepository) -> Self {
        Self {
            game_repo,
            player_repo,
        }
    }

    /// List all games with player counts (fixes N+1 query)
    pub async fn list_games_with_player_counts(
        &self,
    ) -> Result<Vec<automatafl_api_types::GameListItem>, ServiceError> {
        self.game_repo
            .list_with_player_counts()
            .await
            .map_err(ServiceError::from)
    }

    /// Submit a move and potentially auto-complete the round if all moves are in
    /// This is the CENTRAL business logic that was duplicated in game.rs and html.rs
    /// Returns (response, events_to_broadcast)
    #[tracing::instrument(skip(self), fields(game_id = %game_id, player_pid = %player_pid.0))]
    pub async fn submit_move_and_maybe_complete(
        &self,
        game_id: Uuid,
        player_pid: Pid,
        from: Coord,
        to: Coord,
        game_start_time: Option<u64>,
    ) -> Result<(MoveResultResponse, Vec<GameEventData>), ServiceError> {
        // Load game state
        let (mut game, mut lifecycle, _players) = self
            .game_repo
            .load(game_id)
            .await?
            .ok_or(ServiceError::GameNotFound(game_id))?;

        let mut events: Vec<GameEventData> = Vec::new();

        // Propose the move
        let m = Move {
            who: player_pid,
            from,
            to,
        };
        let (feedback, ready_to_complete) = game.propose_move(m);

        // Broadcast move acknowledgment or invalid
        if feedback == MoveFeedback::Committed {
            events.push(GameEventData::MoveAcknowledged {
                player_pid,
                from,
                to,
            });
        } else {
            events.push(GameEventData::MoveInvalid {
                player_pid,
                feedback: feedback.clone(),
            });
        }

        let mut auto_completed = false;

        // AUTO-PROGRESSION: If all moves are in, try to complete the round
        if ready_to_complete {
            match game.try_complete_round() {
                Ok(results) => {
                    // Broadcast all move results
                    for (mv, result) in &results {
                        events.push(GameEventData::Move {
                            player_pid: mv.who,
                            from: mv.from,
                            to: mv.to,
                            result: *result,
                        });
                    }

                    // Broadcast automaton move
                    events.push(GameEventData::AutomatonStep {
                        location: game.board.automaton_location,
                    });

                    // Check for winner
                    if let Some(winner) = game.winner {
                        lifecycle = GameLifecycle::Finished;

                        // Update player stats and ELO if we have game start time
                        if let Some(start_time) = game_start_time {
                            match self
                                .update_game_completion_stats(game_id, winner, start_time)
                                .await
                            {
                                Ok(elo_changes) if !elo_changes.is_empty() => {
                                    events.push(GameEventData::EloUpdate {
                                        changes: elo_changes,
                                    });
                                }
                                Err(e) => {
                                    tracing::error!("Failed to update game completion stats: {}", e);
                                }
                                _ => {}
                            }
                        }

                        events.push(GameEventData::GameOver { winner });
                    } else {
                        events.push(GameEventData::RoundComplete);
                    }

                    auto_completed = true;
                }
                Err(_) => {
                    // Conflicts occurred
                    events.push(GameEventData::Conflicts {
                        locked_players: game.locked_players.to_vec(),
                        conflict_coords: game.board.conflict_list.to_vec(),
                    });
                }
            }
        }

        // Save game state and events atomically
        let _persisted = self
            .game_repo
            .save_with_events(game_id, &game, &lifecycle, &events)
            .await?;

        let response = MoveResultResponse {
            feedback,
            ready_to_complete,
            auto_completed,
        };

        tracing::info!(
            game_id = %game_id,
            lifecycle = ?lifecycle,
            events_count = events.len(),
            "Move processed and state saved"
        );

        Ok((response, events))
    }

    /// Complete a round (manual trigger)
    /// Returns (response, events_to_broadcast)
    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    pub async fn complete_round(
        &self,
        game_id: Uuid,
        game_start_time: Option<u64>,
    ) -> Result<(CompleteRoundResponse, Vec<GameEventData>), ServiceError> {
        // Load game state
        let (mut game, mut lifecycle, _players) = self
            .game_repo
            .load(game_id)
            .await?
            .ok_or(ServiceError::GameNotFound(game_id))?;

        if lifecycle != GameLifecycle::InProgress {
            return Err(ServiceError::GameNotInProgress);
        }

        let mut events: Vec<GameEventData> = Vec::new();

        match game.try_complete_round() {
            Ok(results) => {
                // Broadcast all move results
                for (mv, result) in &results {
                    events.push(GameEventData::Move {
                        player_pid: mv.who,
                        from: mv.from,
                        to: mv.to,
                        result: *result,
                    });
                }

                // Broadcast automaton move
                events.push(GameEventData::AutomatonStep {
                    location: game.board.automaton_location,
                });

                // Check if game is now over
                if let Some(winner) = game.winner {
                    lifecycle = GameLifecycle::Finished;

                    // Update player stats and ELO if we have game start time
                    if let Some(start_time) = game_start_time {
                        match self
                            .update_game_completion_stats(game_id, winner, start_time)
                            .await
                        {
                            Ok(elo_changes) if !elo_changes.is_empty() => {
                                events.push(GameEventData::EloUpdate {
                                    changes: elo_changes,
                                });
                            }
                            Err(e) => {
                                tracing::error!("Failed to update game completion stats: {}", e);
                            }
                            _ => {}
                        }
                    }

                    events.push(GameEventData::GameOver { winner });
                } else {
                    events.push(GameEventData::RoundComplete);
                }

                // Save updated state
                self.game_repo
                    .save_with_events(game_id, &game, &lifecycle, &events)
                    .await?;

                tracing::info!(
                    game_id = %game_id,
                    lifecycle = ?lifecycle,
                    events_count = events.len(),
                    "Round completed and state saved"
                );

                Ok((
                    CompleteRoundResponse {
                        success: true,
                        message: "Round completed successfully".to_string(),
                    },
                    events,
                ))
            }
            Err(_) => {
                // Conflicts occurred
                events.push(GameEventData::Conflicts {
                    locked_players: game.locked_players.to_vec(),
                    conflict_coords: game.board.conflict_list.to_vec(),
                });

                // Save state with conflicts
                self.game_repo
                    .save_with_events(game_id, &game, &lifecycle, &events)
                    .await?;

                Ok((
                    CompleteRoundResponse {
                        success: false,
                        message: "Conflict resolution needed".to_string(),
                    },
                    events,
                ))
            }
        }
    }

    /// Update player stats and ELO ratings after a game finishes
    /// This is now done in a single batch transaction instead of per-player
    async fn update_game_completion_stats(
        &self,
        game_id: Uuid,
        winner_pid: Pid,
        game_start_time: u64,
    ) -> Result<Vec<EloChange>, ServiceError> {
        use crate::db;
        use crate::common::timestamp;

        // Get all players in the game
        let mut result = self
            .game_repo
            .db
            .query("SELECT * FROM game_players WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        let game_players: Vec<db::GamePlayerRecord> = result.take(0)?;
        let game_end_time = timestamp();
        let playtime = game_end_time.saturating_sub(game_start_time);

        // Get all player IDs and their PIDs
        let mut players_info: Vec<(Uuid, u8, i32)> = Vec::new();
        for gp in &game_players {
            let player_uuid = Uuid::parse_str(&gp.player_id)
                .map_err(|e| ServiceError::SerializationError(e.to_string()))?;
            let player = self
                .player_repo
                .get(player_uuid)
                .await?
                .ok_or(ServiceError::SerializationError("Player not found".to_string()))?;
            players_info.push((player_uuid, gp.player_pid, player.elo_rating));
        }

        let mut elo_changes = Vec::new();

        // For 2-player games, update ELO
        if players_info.len() == 2 {
            let (p1_uuid, p1_pid, p1_elo) = players_info[0];
            let (p2_uuid, _p2_pid, p2_elo) = players_info[1];

            let (new_winner_elo, new_loser_elo) = if winner_pid.0 == p1_pid {
                calculate_elo_change(p1_elo, p2_elo)
            } else {
                let (new_p2, new_p1) = calculate_elo_change(p2_elo, p1_elo);
                (new_p1, new_p2)
            };

            // Track ELO changes
            if winner_pid.0 == p1_pid {
                elo_changes.push(EloChange {
                    player_id: p1_uuid,
                    old_elo: p1_elo,
                    new_elo: new_winner_elo,
                    change: new_winner_elo - p1_elo,
                });
                elo_changes.push(EloChange {
                    player_id: p2_uuid,
                    old_elo: p2_elo,
                    new_elo: new_loser_elo,
                    change: new_loser_elo - p2_elo,
                });
            } else {
                elo_changes.push(EloChange {
                    player_id: p1_uuid,
                    old_elo: p1_elo,
                    new_elo: new_loser_elo,
                    change: new_loser_elo - p1_elo,
                });
                elo_changes.push(EloChange {
                    player_id: p2_uuid,
                    old_elo: p2_elo,
                    new_elo: new_winner_elo,
                    change: new_winner_elo - p2_elo,
                });
            }
        }

        // Build batch update for all players
        let mut updates = Vec::new();
        for (player_uuid, player_pid, _player_elo) in players_info {
            let won = player_pid == winner_pid.0;

            // Find new ELO for this player if it was updated
            let new_elo = elo_changes
                .iter()
                .find(|ec| ec.player_id == player_uuid)
                .map(|ec| ec.new_elo);

            updates.push(PlayerStatsUpdate {
                player_id: player_uuid,
                won,
                playtime,
                new_elo,
            });
        }

        // Update all player stats in a single batch transaction
        self.player_repo.update_stats_batch(&updates).await?;

        tracing::info!(
            game_id = %game_id,
            elo_changes_count = elo_changes.len(),
            "Game completion stats updated"
        );

        Ok(elo_changes)
    }
}

/// Calculate new ELO ratings after a game
/// Uses standard ELO formula with K-factor
fn calculate_elo_change(winner_elo: i32, loser_elo: i32) -> (i32, i32) {
    use crate::common::ELO_K_FACTOR;

    // Expected scores
    let expected_winner = 1.0 / (1.0 + 10_f64.powf((loser_elo - winner_elo) as f64 / 400.0));
    let expected_loser = 1.0 / (1.0 + 10_f64.powf((winner_elo - loser_elo) as f64 / 400.0));

    // Actual scores (1 for win, 0 for loss)
    let winner_change = (ELO_K_FACTOR * (1.0 - expected_winner)).round() as i32;
    let loser_change = (ELO_K_FACTOR * (0.0 - expected_loser)).round() as i32;

    let new_winner_elo = winner_elo + winner_change;
    let new_loser_elo = loser_elo + loser_change;

    (new_winner_elo, new_loser_elo)
}
