//! Game management, moves, chat, and save/load endpoints

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use uuid::Uuid;

use automatafl_api_types::*;
use automatafl_logic::{Board, Coord, Move, MoveFeedback, Pid};
use tokio::sync::broadcast;

use crate::common::{
    AppError, AuthPlayer, GameChannels, GameSnapshot, PlayerInGame, ServerState, broadcast_event,
    timestamp,
};
use crate::{db, validation};

// ============================================================================
// Game Management
// ============================================================================

#[tracing::instrument(skip(state))]
pub async fn list_games(State(state): ServerState) -> Json<Vec<GameListItem>> {
    let game_records = db::list_games(&state.db).await.unwrap_or_default();

    let mut games = Vec::new();
    for record in game_records {
        if let (Ok(game_id), Ok(lifecycle), Ok(created_by)) = (
            Uuid::parse_str(&record.id),
            serde_json::from_str::<GameLifecycle>(&record.lifecycle),
            Uuid::parse_str(&record.created_by),
        ) {
            // Get player count
            let player_count = db::get_game_players(&state.db, game_id)
                .await
                .map(|p| p.len())
                .unwrap_or(0);

            games.push(GameListItem {
                id: game_id,
                lifecycle,
                player_count,
                max_players: record.player_count,
                created_at: record.created_at,
                created_by,
            });
        }
    }

    Json(games)
}

#[tracing::instrument(skip(auth, state, req), fields(player_id = %auth.player_id, player_count = %req.player_count))]
pub async fn create_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Json(req): Json<CreateGameRequest>,
) -> Result<Json<Uuid>, AppError> {
    // Validate player count
    validation::validate_player_count(req.player_count)?;

    let board = Board::stock_two_player();

    let mut game_core = automatafl_logic::Game::new(board, req.player_count, req.use_column_rule);

    // Set up goals for two-player game
    if req.player_count == 2 {
        game_core.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 10, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 0, y: 10 }, Pid(1)));
        game_core.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
    }

    let new_uuid = Uuid::new_v4();
    let lifecycle = GameLifecycle::Waiting;

    // Store game in database
    db::create_game(
        &state.db,
        new_uuid,
        &game_core,
        &lifecycle,
        auth.player_id,
        req.player_count,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create game: {}", e);
        AppError::ValidationError("Failed to create game".to_string())
    })?;

    // Create broadcast channel for this game
    let (event_tx, _) = broadcast::channel(crate::common::EVENT_CHANNEL_SIZE);
    state
        .game_channels
        .insert(new_uuid, Arc::new(GameChannels { event_tx }));

    Ok(Json(new_uuid))
}

#[tracing::instrument(skip(auth, state), fields(player_id = %auth.player_id, game_id = %game_uuid))]
pub async fn join_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Pid>, AppError> {
    let player_uuid = auth.player_id;

    // Get player name
    let player = db::get_player(&state.db, player_uuid)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_uuid))?
        .ok_or(AppError::NoSuchPlayer(player_uuid))?;
    let player_name = player.displayname;

    // Load game state
    let (game_core, _, player_ids) = db::load_game_state(&state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    if player_ids.len() >= (game_core.player_count as usize) {
        return Err(AppError::GameFull(game_uuid));
    }
    if player_ids.contains_key(&player_uuid) {
        return Err(AppError::PlayerAlreadyExists(player_uuid));
    }

    let new_player_pid = Pid(player_ids.len() as u8);

    // Add player to game
    db::add_player_to_game(&state.db, game_uuid, player_uuid, new_player_pid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    // Broadcast join event
    broadcast_event(
        &state,
        game_uuid,
        GameEventData::PlayerJoined {
            player_id: player_uuid,
            player_pid: new_player_pid,
            displayname: player_name,
        },
    )
    .await?;

    // Auto-start game when full
    if player_ids.len() + 1 == game_core.player_count as usize {
        db::update_game_state(&state.db, game_uuid, &game_core, &GameLifecycle::InProgress)
            .await
            .map_err(|_| AppError::NoSuchGame(game_uuid))?;

        broadcast_event(&state, game_uuid, GameEventData::GameStarted).await?;
    }

    Ok(Json(new_player_pid))
}

#[tracing::instrument(skip(_auth, state), fields(game_id = %game_uuid))]
pub async fn get_game_state(
    _auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<GameStateResponse>, AppError> {
    let (game_core, lifecycle, player_ids) = db::load_game_state(&state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    Ok(Json(GameStateResponse {
        lifecycle,
        game: game_core,
        player_ids,
    }))
}

pub async fn get_goals(
    _auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<(Coord, Pid)>>, AppError> {
    let (game_core, _, _) = db::load_game_state(&state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    Ok(Json(game_core.goals.to_vec()))
}

// ============================================================================
// Move Endpoints
// ============================================================================

pub async fn pending_move(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Option<automatafl_logic::Move>>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;

    let (game_core, _, _) = db::load_game_state(&state.db, player_in_game.game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?
        .ok_or(AppError::NoSuchGame(player_in_game.game_uuid))?;

    Ok(Json(
        game_core
            .pending_moves
            .iter()
            .find(|mv| mv.who == player_in_game.player_pid)
            .cloned(),
    ))
}

#[tracing::instrument(skip(auth, app_state, move_to_make), fields(game_id = %game_uuid, player_id = %auth.player_id))]
pub async fn perform_move(
    State(app_state): ServerState,
    auth: AuthPlayer,
    Path(game_uuid): Path<Uuid>,
    Json(move_to_make): Json<PerformMove>,
) -> Result<Json<MoveResultResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Validate move coordinates
    crate::validation::validate_move_coords(
        move_to_make.from.x,
        move_to_make.from.y,
        move_to_make.to.x,
        move_to_make.to.y,
    )?;

    // Load game state
    let (mut game_core, mut lifecycle, _) =
        db::load_game_state(&app_state.db, player_in_game.game_uuid)
            .await
            .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?
            .ok_or(AppError::NoSuchGame(player_in_game.game_uuid))?;

    let m = Move {
        who: player_in_game.player_pid,
        from: move_to_make.from,
        to: move_to_make.to,
    };
    let (feedback, ready_to_complete) = game_core.propose_move(m);

    // Save updated game state
    db::update_game_state(
        &app_state.db,
        player_in_game.game_uuid,
        &game_core,
        &lifecycle,
    )
    .await
    .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?;

    // Broadcast move acknowledgment
    if feedback == MoveFeedback::Committed {
        broadcast_event(
            &app_state,
            player_in_game.game_uuid,
            GameEventData::MoveAcknowledged {
                player_pid: player_in_game.player_pid,
                from: move_to_make.from,
                to: move_to_make.to,
            },
        )
        .await?;
    } else {
        broadcast_event(
            &app_state,
            player_in_game.game_uuid,
            GameEventData::MoveInvalid {
                player_pid: player_in_game.player_pid,
                feedback: feedback.clone(),
            },
        )
        .await?;
    }

    let mut auto_completed = false;

    // AUTO-PROGRESSION: If all moves are in, try to complete the round
    if ready_to_complete {
        match game_core.try_complete_round() {
            Ok(results) => {
                // Broadcast all move results
                for (mv, result) in &results {
                    broadcast_event(
                        &app_state,
                        player_in_game.game_uuid,
                        GameEventData::Move {
                            player_pid: mv.who,
                            from: mv.from,
                            to: mv.to,
                            result: *result,
                        },
                    )
                    .await?;
                }

                // Broadcast automaton move
                broadcast_event(
                    &app_state,
                    player_in_game.game_uuid,
                    GameEventData::AutomatonStep {
                        location: game_core.board.automaton_location,
                    },
                )
                .await?;

                // Check for winner
                if let Some(winner) = game_core.winner {
                    lifecycle = GameLifecycle::Finished;

                    // Get game creation time for playtime calculation
                    if let Ok(Some(game_record)) =
                        db::get_game(&app_state.db, player_in_game.game_uuid).await
                    {
                        // Update player stats and ELO ratings
                        match crate::matchmaking::update_game_completion_stats(
                            &app_state.db,
                            player_in_game.game_uuid,
                            winner,
                            game_record.created_at,
                        )
                        .await
                        {
                            Ok(elo_changes) if !elo_changes.is_empty() => {
                                // Broadcast ELO changes
                                let _ = broadcast_event(
                                    &app_state,
                                    player_in_game.game_uuid,
                                    GameEventData::EloUpdate {
                                        changes: elo_changes,
                                    },
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::error!("Failed to update game completion stats: {}", e);
                            }
                            _ => {}
                        }
                    }

                    broadcast_event(
                        &app_state,
                        player_in_game.game_uuid,
                        GameEventData::GameOver { winner },
                    )
                    .await?;

                    // Clean up game channel after game finishes (prevent memory leak)
                    app_state.game_channels.remove(&player_in_game.game_uuid);
                    tracing::info!(
                        "Cleaned up game channel for finished game: {}",
                        player_in_game.game_uuid
                    );
                } else {
                    broadcast_event(
                        &app_state,
                        player_in_game.game_uuid,
                        GameEventData::RoundComplete,
                    )
                    .await?;
                }

                // Save final state after round completion
                db::update_game_state(
                    &app_state.db,
                    player_in_game.game_uuid,
                    &game_core,
                    &lifecycle,
                )
                .await
                .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?;

                auto_completed = true;
            }
            Err(_) => {
                // Conflicts occurred - save updated state
                db::update_game_state(
                    &app_state.db,
                    player_in_game.game_uuid,
                    &game_core,
                    &lifecycle,
                )
                .await
                .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?;

                broadcast_event(
                    &app_state,
                    player_in_game.game_uuid,
                    GameEventData::Conflicts {
                        locked_players: game_core.locked_players.to_vec(),
                        conflict_coords: game_core.board.conflict_list.to_vec(),
                    },
                )
                .await?;
            }
        }
    }

    Ok(Json(MoveResultResponse {
        feedback,
        ready_to_complete,
        auto_completed,
    }))
}

#[tracing::instrument(skip(auth, app_state), fields(game_id = %game_uuid, player_id = %auth.player_id))]
pub async fn complete_round(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Load game state
    let (mut game_core, mut lifecycle, _) =
        db::load_game_state(&app_state.db, player_in_game.game_uuid)
            .await
            .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?
            .ok_or(AppError::NoSuchGame(player_in_game.game_uuid))?;

    match lifecycle {
        GameLifecycle::InProgress => {
            match game_core.try_complete_round() {
                Ok(results) => {
                    // Broadcast all move results
                    for (mv, result) in &results {
                        broadcast_event(
                            &app_state,
                            player_in_game.game_uuid,
                            GameEventData::Move {
                                player_pid: mv.who,
                                from: mv.from,
                                to: mv.to,
                                result: *result,
                            },
                        )
                        .await?;
                    }

                    // Broadcast automaton move
                    broadcast_event(
                        &app_state,
                        player_in_game.game_uuid,
                        GameEventData::AutomatonStep {
                            location: game_core.board.automaton_location,
                        },
                    )
                    .await?;

                    // Check if game is now over
                    if let Some(winner) = game_core.winner {
                        lifecycle = GameLifecycle::Finished;

                        // Get game creation time for playtime calculation
                        if let Ok(Some(game_record)) =
                            db::get_game(&app_state.db, player_in_game.game_uuid).await
                        {
                            // Update player stats and ELO ratings
                            match crate::matchmaking::update_game_completion_stats(
                                &app_state.db,
                                player_in_game.game_uuid,
                                winner,
                                game_record.created_at,
                            )
                            .await
                            {
                                Ok(elo_changes) if !elo_changes.is_empty() => {
                                    // Broadcast ELO changes
                                    let _ = broadcast_event(
                                        &app_state,
                                        player_in_game.game_uuid,
                                        GameEventData::EloUpdate {
                                            changes: elo_changes,
                                        },
                                    )
                                    .await;
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

                        broadcast_event(
                            &app_state,
                            player_in_game.game_uuid,
                            GameEventData::GameOver { winner },
                        )
                        .await?;

                        // Clean up game channel after game finishes (prevent memory leak)
                        app_state.game_channels.remove(&player_in_game.game_uuid);
                        tracing::info!(
                            "Cleaned up game channel for finished game: {}",
                            player_in_game.game_uuid
                        );
                    } else {
                        broadcast_event(
                            &app_state,
                            player_in_game.game_uuid,
                            GameEventData::RoundComplete,
                        )
                        .await?;
                    }

                    // Save updated state
                    db::update_game_state(
                        &app_state.db,
                        player_in_game.game_uuid,
                        &game_core,
                        &lifecycle,
                    )
                    .await
                    .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?;

                    Ok(Json(CompleteRoundResponse {
                        success: true,
                        message: "Round completed successfully".to_string(),
                    }))
                }
                Err(_) => {
                    // Save state with conflicts
                    db::update_game_state(
                        &app_state.db,
                        player_in_game.game_uuid,
                        &game_core,
                        &lifecycle,
                    )
                    .await
                    .map_err(|_| AppError::NoSuchGame(player_in_game.game_uuid))?;

                    broadcast_event(
                        &app_state,
                        player_in_game.game_uuid,
                        GameEventData::Conflicts {
                            locked_players: game_core.locked_players.to_vec(),
                            conflict_coords: game_core.board.conflict_list.to_vec(),
                        },
                    )
                    .await?;

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

#[tracing::instrument(skip(auth, app_state, req), fields(game_id = %game_uuid, player_id = %auth.player_id))]
pub async fn post_chat(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Json(req): Json<PostChatRequest>,
) -> Result<Json<PostChatResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Validate chat message
    crate::validation::validate_chat_message(&req.message)?;

    // Get player name
    let player = db::get_player(&app_state.db, player_in_game.player_uuid)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_in_game.player_uuid))?
        .ok_or(AppError::NoSuchPlayer(player_in_game.player_uuid))?;
    let player_name = player.displayname;

    let ts = timestamp();

    // Save to database
    db::add_chat_message(
        &app_state.db,
        game_uuid,
        ts,
        player_in_game.player_uuid,
        player_name.clone(),
        req.message.clone(),
    )
    .await
    .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    // Broadcast event
    broadcast_event(
        &app_state,
        player_in_game.game_uuid,
        GameEventData::Chat {
            timestamp: ts,
            player_id: player_in_game.player_uuid,
            displayname: player_name,
            message: req.message,
        },
    )
    .await?;

    Ok(Json(PostChatResponse { timestamp: ts }))
}

pub async fn get_chat(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<ChatMessage>>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    let messages = db::get_chat_messages(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    let chat_msgs: Vec<ChatMessage> = messages
        .into_iter()
        .filter_map(|msg| {
            Uuid::parse_str(&msg.player_id).ok().map(|id| ChatMessage {
                timestamp: msg.timestamp,
                player_id: id,
                displayname: msg.displayname,
                message: msg.message,
            })
        })
        .collect();

    Ok(Json(chat_msgs))
}

// ============================================================================
// Game History
// ============================================================================

#[derive(serde::Deserialize)]
pub struct GameHistoryQuery {
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub event_kind: Option<String>,
}

pub async fn get_game_history(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(query): Query<GameHistoryQuery>,
) -> Result<Json<Vec<GameEvent>>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    let events = db::get_game_history(
        &app_state.db,
        game_uuid,
        query.since,
        query.until,
        query.event_kind,
    )
    .await
    .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    let game_events: Vec<GameEvent> = events
        .into_iter()
        .filter_map(|evt| {
            // Combine kind and data into a single JSON object for deserialization
            let combined = serde_json::json!({
                "kind": evt.event_kind,
                "data": serde_json::from_str::<serde_json::Value>(&evt.event_data).ok()?,
                "timestamp": evt.timestamp,
            });
            serde_json::from_value(combined).ok()
        })
        .collect();

    Ok(Json(game_events))
}

// ============================================================================
// Save/Load Endpoints
// ============================================================================

pub async fn save_game(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<SaveGameResponse>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Load current game state
    let (game_core, lifecycle, player_ids) = db::load_game_state(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    let snapshot = GameSnapshot {
        core: game_core,
        player_ids,
        lifecycle,
        timestamp: timestamp(),
    };

    // Get current snapshot count to determine index
    let snapshots = db::list_snapshots(&app_state.db, game_uuid)
        .await
        .unwrap_or_default();
    let index = snapshots.len();

    // Save snapshot
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    db::save_snapshot(
        &app_state.db,
        game_uuid,
        index,
        snapshot.timestamp,
        &snapshot_json,
    )
    .await
    .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(SaveGameResponse {
        snapshot_index: index,
    }))
}

pub async fn list_snapshots(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<ListSnapshotsResponse>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    let snapshots = db::list_snapshots(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchSnapshot(game_uuid))?;

    let info: Vec<_> = snapshots
        .iter()
        .map(|snap| SnapshotInfo {
            index: snap.index,
            timestamp: snap.timestamp,
        })
        .collect();

    Ok(Json(ListSnapshotsResponse { snapshots: info }))
}

pub async fn load_game(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path((game_uuid, snapshot_index)): Path<(Uuid, usize)>,
) -> Result<StatusCode, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Load snapshot
    let snapshot_record = db::get_snapshot(&app_state.db, game_uuid, snapshot_index)
        .await
        .map_err(|_| AppError::NoSuchSnapshot(game_uuid))?
        .ok_or(AppError::NoSuchSnapshot(game_uuid))?;

    let snapshot: GameSnapshot = serde_json::from_str(&snapshot_record.snapshot_data)
        .map_err(|_| AppError::NoSuchSnapshot(game_uuid))?;

    // Update game state in database
    db::update_game_state(
        &app_state.db,
        game_uuid,
        &snapshot.core,
        &snapshot.lifecycle,
    )
    .await
    .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    // Broadcast event
    broadcast_event(
        &app_state,
        game_uuid,
        GameEventData::GameLoaded { snapshot_index },
    )
    .await?;

    Ok(StatusCode::OK)
}
