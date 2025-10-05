use crate::common::GameSnapshot;
use crate::db::{GameEventRecord, SnapshotRecord, as_uuid};
use crate::repositories::game_repository::GameRepository;
use crate::repositories::player_repository::{PlayerRepository, PlayerStatsUpdate};
use crate::transactions::{self, TransactionError};
use automatafl_api_types::{
    ChatMessage, CompleteRoundResponse, EloChange, GameEvent, GameEventData, GameLifecycle,
    GameListItem, MoveResultResponse,
};
use automatafl_logic::{Board, Coord, Game, Move, MoveFeedback, Pid};
use futures::{Stream, StreamExt};
use std::{collections::HashMap, pin::Pin};
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
    GameFull(Uuid),
    PlayerAlreadyInGame(Uuid),
    PlayerNotFound(Uuid),
    ValidationFailed(String),
    SnapshotNotFound(Uuid, usize),
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
            ServiceError::GameFull(id) => write!(f, "Game is already full: {}", id),
            ServiceError::PlayerAlreadyInGame(id) => write!(f, "Player already in game: {}", id),
            ServiceError::PlayerNotFound(id) => write!(f, "Player not found: {}", id),
            ServiceError::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            ServiceError::SnapshotNotFound(game_id, index) => {
                write!(f, "Snapshot {} not found for game {}", index, game_id)
            }
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
    pub async fn list_games_with_player_counts(&self) -> Result<Vec<GameListItem>, ServiceError> {
        self.game_repo
            .list_with_player_counts()
            .await
            .map_err(ServiceError::from)
    }

    /// Update lifecycle for a game
    pub async fn set_lifecycle(
        &self,
        game_id: Uuid,
        lifecycle: GameLifecycle,
    ) -> Result<(), ServiceError> {
        self.game_repo
            .update_lifecycle(game_id, &lifecycle)
            .await
            .map_err(ServiceError::from)
    }

    /// List the most recent games that a player participated in
    pub async fn list_recent_games_for_player(
        &self,
        player_id: Uuid,
        limit: usize,
    ) -> Result<Vec<GameListItem>, ServiceError> {
        self.game_repo
            .list_recent_for_player(player_id, limit)
            .await
            .map_err(ServiceError::from)
    }

    /// Create a new game and register the creator as the first player
    pub async fn create_game(
        &self,
        creator_id: Uuid,
        player_count: u8,
        use_column_rule: bool,
    ) -> Result<Uuid, ServiceError> {
        if player_count < 2 || player_count > 4 {
            return Err(ServiceError::ValidationFailed(
                "Player count must be between 2 and 4".to_string(),
            ));
        }

        let board = Board::stock_two_player();
        let mut game = Game::new(board, player_count, use_column_rule);

        // Configure standard goals for two-player games
        if player_count == 2 {
            game.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
            game.goals.push((Coord { x: 10, y: 0 }, Pid(0)));
            game.goals.push((Coord { x: 0, y: 10 }, Pid(1)));
            game.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
        }

        let lifecycle = GameLifecycle::Waiting;
        let game_id = Uuid::new_v4();

        self.game_repo
            .create(game_id, &game, &lifecycle, creator_id, player_count)
            .await?;

        self.game_repo
            .add_player(game_id, creator_id, Pid(0))
            .await?;

        Ok(game_id)
    }

    /// Create a new game and associate all players atomically
    pub async fn create_game_with_players(
        &self,
        game_id: Uuid,
        game_state: &Game,
        lifecycle: GameLifecycle,
        creator_id: Uuid,
        player_count: u8,
        player_assignments: &[(Uuid, u8)],
    ) -> Result<(), ServiceError> {
        let assignments = player_assignments.to_vec();

        transactions::create_game_with_players(
            &self.game_repo.db,
            game_id,
            game_state,
            &lifecycle,
            creator_id,
            player_count,
            assignments,
        )
        .await
        .map_err(|err| match err {
            TransactionError::DbError(e) => ServiceError::DatabaseError(e),
            TransactionError::SerializationError(msg) => ServiceError::SerializationError(msg),
        })
    }

    /// Join a game and return PID plus events to broadcast
    pub async fn join_game(
        &self,
        game_id: Uuid,
        player_id: Uuid,
    ) -> Result<(Pid, Vec<GameEventData>), ServiceError> {
        let player = self
            .player_repo
            .get(player_id)
            .await?
            .ok_or(ServiceError::PlayerNotFound(player_id))?;

        let displayname = player.displayname.clone();

        let (game, mut lifecycle, mut player_map) = self
            .game_repo
            .load(game_id)
            .await?
            .ok_or(ServiceError::GameNotFound(game_id))?;

        if player_map.contains_key(&player_id) {
            return Err(ServiceError::PlayerAlreadyInGame(player_id));
        }

        if player_map.len() >= game.player_count as usize {
            return Err(ServiceError::GameFull(game_id));
        }

        // Determine next available PID (fill lowest unused slot)
        let mut taken: Vec<u8> = player_map.values().map(|pid| pid.0).collect();
        taken.sort_unstable();
        let mut next_pid = 0u8;
        while taken.contains(&next_pid) {
            next_pid += 1;
        }
        if next_pid >= game.player_count {
            return Err(ServiceError::GameFull(game_id));
        }

        let pid = Pid(next_pid);

        self.game_repo.add_player(game_id, player_id, pid).await?;

        player_map.insert(player_id, pid);

        let mut events = vec![GameEventData::PlayerJoined {
            player_id,
            player_pid: pid,
            displayname,
            player_ids: player_map.clone(),
        }];

        if lifecycle == GameLifecycle::Waiting && player_map.len() == game.player_count as usize {
            lifecycle = GameLifecycle::InProgress;
            self.game_repo.update_lifecycle(game_id, &lifecycle).await?;

            events.push(GameEventData::GameStarted {
                new_lifecycle: GameLifecycle::InProgress,
            });
        }

        Ok((pid, events))
    }

    /// Fetch full game state from the repository
    pub async fn load_game(
        &self,
        game_id: Uuid,
    ) -> Result<(Game, GameLifecycle, HashMap<Uuid, Pid>), ServiceError> {
        self.game_repo
            .load(game_id)
            .await?
            .ok_or(ServiceError::GameNotFound(game_id))
    }

    /// Retrieve the game creation timestamp if the game exists
    pub async fn get_game_created_at(&self, game_id: Uuid) -> Result<Option<u64>, ServiceError> {
        let record = self.game_repo.get(game_id).await?;
        Ok(record.map(|r| r.created_at))
    }

    /// Return the PID for a player in a game if present
    pub async fn player_pid(&self, game_id: Uuid, player_id: Uuid) -> Result<Pid, ServiceError> {
        let (_, _, player_map) = self.load_game(game_id).await?;
        player_map
            .get(&player_id)
            .copied()
            .ok_or(ServiceError::PlayerNotFound(player_id))
    }

    /// Store a chat message and return the event for broadcasting
    pub async fn post_chat(
        &self,
        game_id: Uuid,
        player_id: Uuid,
        message: String,
    ) -> Result<(u64, GameEventData), ServiceError> {
        if message.trim().is_empty() {
            return Err(ServiceError::ValidationFailed(
                "Chat message cannot be empty".to_string(),
            ));
        }

        let player = self
            .player_repo
            .get(player_id)
            .await?
            .ok_or(ServiceError::PlayerNotFound(player_id))?;

        let timestamp = crate::common::timestamp();

        self.game_repo
            .add_chat_message(
                game_id,
                timestamp,
                player_id,
                player.displayname.clone(),
                message.clone(),
            )
            .await?;

        Ok((
            timestamp,
            GameEventData::Chat {
                timestamp,
                player_id,
                displayname: player.displayname,
                message,
            },
        ))
    }

    /// Fetch chat history for a game
    pub async fn get_chat_messages(&self, game_id: Uuid) -> Result<Vec<ChatMessage>, ServiceError> {
        let records = self.game_repo.get_chat_messages(game_id).await?;
        let messages = records
            .into_iter()
            .filter_map(|record| {
                Some(ChatMessage {
                    timestamp: record.timestamp,
                    player_id: as_uuid(&record.player_id),
                    displayname: record.displayname,
                    message: record.message,
                })
            })
            .collect();

        Ok(messages)
    }

    /// Get historical events with optional filters
    pub async fn get_history(
        &self,
        game_id: Uuid,
        since: Option<u64>,
        until: Option<u64>,
        event_kind: Option<String>,
    ) -> Result<Vec<GameEvent>, ServiceError> {
        let records: Vec<GameEventRecord> = self
            .game_repo
            .get_history(game_id, since, until, event_kind)
            .await?;

        fn decode_event_value(
            value: serde_json::Value,
        ) -> Result<GameEventData, serde_json::Error> {
            match serde_json::from_value::<GameEventData>(value.clone()) {
                Ok(event) => Ok(event),
                Err(primary_err) => {
                    if let serde_json::Value::Object(mut map) = value {
                        if let Some(nested) = map.remove("event") {
                            return decode_event_value(nested);
                        }

                        if map.len() == 1 {
                            if let Some((kind, data)) = map.clone().into_iter().next() {
                                let normalized = serde_json::json!({
                                    "kind": kind,
                                    "data": data,
                                });
                                return serde_json::from_value(normalized);
                            }
                        }

                        let payload = map.remove("payload").or_else(|| map.remove("data"));
                        if let (Some(serde_json::Value::String(kind)), Some(data)) =
                            (map.remove("kind"), payload)
                        {
                            let normalized = serde_json::json!({
                                "kind": kind,
                                "data": data,
                            });
                            return serde_json::from_value(normalized);
                        }
                    }

                    Err(primary_err)
                }
            }
        }

        let mut events = Vec::with_capacity(records.len());
        for record in records {
            let timestamp = record.timestamp;
            match decode_event_value(record.event) {
                Ok(data) => events.push(GameEvent {
                    data,
                    timestamp: Some(timestamp),
                }),
                Err(err) => {
                    tracing::warn!(
                        game_id = %game_id,
                        error = %err,
                        "Skipping unreadable game event"
                    );
                }
            }
        }

        Ok(events)
    }

    /// Persist a snapshot of the current game state and return its index
    pub async fn save_snapshot(&self, game_id: Uuid) -> Result<usize, ServiceError> {
        let (game, lifecycle, player_ids) = self.load_game(game_id).await?;

        let snapshot = GameSnapshot {
            core: game,
            player_ids,
            lifecycle,
            timestamp: crate::common::timestamp(),
        };

        let existing = self.game_repo.list_snapshots(game_id).await?;
        let index = existing.len();

        let snapshot_json = serde_json::to_string(&snapshot)
            .map_err(|e| ServiceError::SerializationError(e.to_string()))?;

        self.game_repo
            .save_snapshot(game_id, index, snapshot.timestamp, &snapshot_json)
            .await?;

        Ok(index)
    }

    /// List raw snapshot records for a game
    pub async fn list_snapshots(&self, game_id: Uuid) -> Result<Vec<SnapshotRecord>, ServiceError> {
        self.game_repo
            .list_snapshots(game_id)
            .await
            .map_err(Into::into)
    }

    /// Load a snapshot, replace current game state, and return the snapshot contents
    pub async fn load_snapshot(
        &self,
        game_id: Uuid,
        index: usize,
    ) -> Result<GameSnapshot, ServiceError> {
        let snapshot_record = self
            .game_repo
            .get_snapshot(game_id, index)
            .await?
            .ok_or(ServiceError::SnapshotNotFound(game_id, index))?;

        let snapshot: GameSnapshot = serde_json::from_str(&snapshot_record.snapshot_data)
            .map_err(|e| ServiceError::SerializationError(e.to_string()))?;

        self.game_repo
            .save_with_events(game_id, &snapshot.core, &snapshot.lifecycle, &[])
            .await?;

        Ok(snapshot)
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

        tracing::debug!(
            game_id = %game_id,
            ?feedback,
            ready_to_complete,
            "Evaluated proposed move"
        );

        // Broadcast move acknowledgment or invalid
        if feedback == MoveFeedback::Committed {
            if lifecycle == GameLifecycle::Waiting {
                lifecycle = GameLifecycle::InProgress;
                events.push(GameEventData::GameStarted {
                    new_lifecycle: GameLifecycle::InProgress,
                });
            }
            events.push(GameEventData::MoveAcknowledged {
                player_pid,
                from,
                to,
                pending_move: m.clone(),
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
                                    tracing::error!(
                                        "Failed to update game completion stats: {}",
                                        e
                                    );
                                }
                                _ => {}
                            }
                        }

                        events.push(GameEventData::GameOver {
                            winner,
                            new_lifecycle: GameLifecycle::Finished,
                        });
                    } else {
                        events.push(GameEventData::RoundComplete {
                            new_lifecycle: lifecycle.clone(),
                        });
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

                    events.push(GameEventData::GameOver {
                        winner,
                        new_lifecycle: GameLifecycle::Finished,
                    });
                } else {
                    events.push(GameEventData::RoundComplete {
                        new_lifecycle: lifecycle.clone(),
                    });
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

    /// Force completion of the current round regardless of pending moves
    pub async fn force_complete_round(
        &self,
        game_id: Uuid,
        game_start_time: Option<u64>,
    ) -> Result<(CompleteRoundResponse, Vec<GameEventData>), ServiceError> {
        let (mut game, mut lifecycle, _players) = self
            .game_repo
            .load(game_id)
            .await?
            .ok_or(ServiceError::GameNotFound(game_id))?;

        let mut events: Vec<GameEventData> = Vec::new();

        match game.try_complete_round() {
            Ok(results) => {
                for (mv, result) in &results {
                    events.push(GameEventData::Move {
                        player_pid: mv.who,
                        from: mv.from,
                        to: mv.to,
                        result: *result,
                    });
                }

                events.push(GameEventData::AutomatonStep {
                    location: game.board.automaton_location,
                });

                if let Some(winner) = game.winner {
                    lifecycle = GameLifecycle::Finished;

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

                    events.push(GameEventData::GameOver {
                        winner,
                        new_lifecycle: GameLifecycle::Finished,
                    });
                } else {
                    events.push(GameEventData::RoundComplete {
                        new_lifecycle: lifecycle.clone(),
                    });
                }

                self.game_repo
                    .save_with_events(game_id, &game, &lifecycle, &events)
                    .await?;

                Ok((
                    CompleteRoundResponse {
                        success: true,
                        message: "Admin forced round completion".to_string(),
                    },
                    events,
                ))
            }
            Err(_) => {
                events.push(GameEventData::Conflicts {
                    locked_players: game.locked_players.to_vec(),
                    conflict_coords: game.board.conflict_list.to_vec(),
                });

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
        use crate::common::timestamp;
        use crate::db;

        // Get all players in the game
        // FIXED: Use RecordId instead of string
        let mut result = self
            .game_repo
            .db
            .query("SELECT * FROM game_players WHERE game_id = $game_id")
            .bind((
                "game_id",
                surrealdb::RecordId::from_table_key("games", game_id),
            ))
            .await?;

        let game_players: Vec<db::GamePlayerRecord> = result.take(0)?;
        let game_end_time = timestamp();
        let playtime = game_end_time.saturating_sub(game_start_time);

        // Get all player IDs and their PIDs
        let mut players_info: Vec<(Uuid, u8, i32)> = Vec::new();
        for gp in &game_players {
            let player_uuid = as_uuid(&gp.player_id);
            let player = self.player_repo.get(player_uuid).await?.ok_or(
                ServiceError::SerializationError("Player not found".to_string()),
            )?;
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

    /// Append a standalone game event without modifying persistent game state
    pub async fn append_event(
        &self,
        game_id: Uuid,
        event: GameEventData,
    ) -> Result<(), ServiceError> {
        let timestamp = crate::common::timestamp();
        self.game_repo
            .append_event(game_id, timestamp, event)
            .await?;
        Ok(())
    }

    /// Build a snapshot event representing the current game state
    pub async fn state_snapshot_event(
        &self,
        game_id: Uuid,
    ) -> Result<Option<GameEvent>, ServiceError> {
        if let Some((game, lifecycle, player_ids)) = self.game_repo.load(game_id).await? {
            let event = GameEvent {
                data: GameEventData::State {
                    lifecycle,
                    game: Box::new(game),
                    player_ids,
                },
                timestamp: Some(crate::common::timestamp()),
            };
            Ok(Some(event))
        } else {
            Ok(None)
        }
    }

    /// Stream live game events emitted from the database for a specific game
    pub async fn live_event_stream(
        &self,
        game_id: Uuid,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<GameEvent, ServiceError>> + Send>>, ServiceError>
    {
        let stream = self.game_repo.live_event_stream(game_id).await?;
        // FIXED: Handle deserialization errors gracefully instead of unwrap()
        // Capture game_id by move for the closure
        let mapped = stream.filter_map(move |result| async move {
            match result {
                Ok(record) => {
                    match serde_json::from_value(record.event) {
                        Ok(data) => Some(Ok(GameEvent {
                            data,
                            timestamp: Some(record.timestamp),
                        })),
                        Err(e) => {
                            tracing::warn!(game_id = %game_id, error = %e, "Failed to deserialize live event, skipping");
                            None
                        }
                    }
                }
                Err(err) => Some(Err(ServiceError::from(err))),
            }
        });
        Ok(Box::pin(mapped))
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
