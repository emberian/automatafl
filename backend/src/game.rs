//! Game management, moves, chat, and save/load endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use uuid::Uuid;

use automatafl_api_types::*;
use automatafl_logic::{Coord, Move, Pid};

use crate::common::{AppError, AuthPlayer, PlayerInGame, ServerState, broadcast_event};
use crate::services::game_service::ServiceError;
use crate::validation;

// ============================================================================
// Game Management
// ============================================================================

#[tracing::instrument(skip(state))]
pub async fn list_games(State(state): ServerState) -> Json<Vec<GameListItem>> {
    // Use service layer - single query, no N+1 problem
    let games = state
        .game_service
        .list_games_with_player_counts()
        .await
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "Failed to list games");
            Vec::new()
        });

    Json(games)
}

#[tracing::instrument(skip(auth, state, req), fields(player_id = %auth.player_id, player_count = %req.player_count))]
pub async fn create_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Json(req): Json<CreateGameRequest>,
) -> Result<Json<Uuid>, AppError> {
    let game_id = state
        .game_service
        .create_game(auth.player_id, req.player_count, req.use_column_rule)
        .await
        .map_err(|err| map_game_error(None, err))?;

    Ok(Json(game_id))
}

#[tracing::instrument(skip(auth, state), fields(player_id = %auth.player_id, game_id = %game_uuid))]
pub async fn join_game(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Pid>, AppError> {
    let player_uuid = auth.player_id;

    let (pid, events) = state
        .game_service
        .join_game(game_uuid, player_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    for event in events {
        broadcast_event(&state, game_uuid, event).await?;
    }

    Ok(Json(pid))
}

#[tracing::instrument(skip(_auth, state), fields(game_id = %game_uuid))]
pub async fn get_game_state(
    _auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<GameStateResponse>, AppError> {
    let (game_core, lifecycle, player_ids) = state
        .game_service
        .load_game(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

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
    let (game_core, _, _) = state
        .game_service
        .load_game(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    Ok(Json(game_core.goals.to_vec()))
}

// ============================================================================
// Move Endpoints
// ============================================================================

pub async fn pending_move(
    auth: AuthPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Option<Move>>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &state).await?;

    let (game_core, _, _) = state
        .game_service
        .load_game(player_in_game.game_uuid)
        .await
        .map_err(|err| map_game_error(Some(player_in_game.game_uuid), err))?;

    Ok(Json(
        game_core
            .pending_moves
            .iter()
            .find(|mv| mv.who == player_in_game.player_pid)
            .cloned(),
    ))
}

/// Perform a move - REFACTORED to use service layer
#[tracing::instrument(skip(auth, app_state, move_to_make), fields(game_id = %game_uuid, player_id = %auth.player_id))]
pub async fn perform_move(
    State(app_state): ServerState,
    auth: AuthPlayer,
    Path(game_uuid): Path<Uuid>,
    Json(move_to_make): Json<PerformMove>,
) -> Result<Json<MoveResultResponse>, AppError> {
    let player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Validate move coordinates
    validation::validate_move_coords(
        move_to_make.from.x,
        move_to_make.from.y,
        move_to_make.to.x,
        move_to_make.to.y,
    )?;

    // Get game start time for potential completion stats
    let game_start_time = app_state
        .game_service
        .get_game_created_at(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    // Call service - ALL business logic is there
    let (response, events) = app_state
        .game_service
        .submit_move_and_maybe_complete(
            player_in_game.game_uuid,
            player_in_game.player_pid,
            move_to_make.from,
            move_to_make.to,
            game_start_time,
        )
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    // Broadcast all events from service
    for event in events {
        broadcast_event(&app_state, player_in_game.game_uuid, event).await?;
    }

    Ok(Json(response))
}

/// Complete round - REFACTORED to use service layer
#[tracing::instrument(skip(auth, app_state), fields(game_id = %game_uuid, player_id = %auth.player_id))]
pub async fn complete_round(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    // Get game start time
    let game_start_time = app_state
        .game_service
        .get_game_created_at(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    // Call service
    let (response, events) = app_state
        .game_service
        .complete_round(game_uuid, game_start_time)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    // Broadcast all events
    for event in events {
        broadcast_event(&app_state, game_uuid, event).await?;
    }

    Ok(Json(response))
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

    crate::validation::validate_chat_message(&req.message)?;

    let (timestamp, event) = app_state
        .game_service
        .post_chat(game_uuid, player_in_game.player_uuid, req.message.clone())
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    broadcast_event(&app_state, player_in_game.game_uuid, event).await?;

    Ok(Json(PostChatResponse { timestamp }))
}

pub async fn get_chat(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<ChatMessage>>, AppError> {
    let _player_in_game = PlayerInGame::extract(auth, game_uuid, &app_state).await?;

    let messages = app_state
        .game_service
        .get_chat_messages(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    Ok(Json(messages))
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

    let events = app_state
        .game_service
        .get_history(game_uuid, query.since, query.until, query.event_kind)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    Ok(Json(events))
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

    let index = app_state
        .game_service
        .save_snapshot(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

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

    let snapshots = app_state
        .game_service
        .list_snapshots(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

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

    let _snapshot = app_state
        .game_service
        .load_snapshot(game_uuid, snapshot_index)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    broadcast_event(
        &app_state,
        game_uuid,
        GameEventData::GameLoaded { snapshot_index },
    )
    .await?;

    Ok(StatusCode::OK)
}

fn map_game_error(game_id: Option<Uuid>, err: ServiceError) -> AppError {
    match err {
        ServiceError::GameNotFound(id) => AppError::NoSuchGame(id),
        ServiceError::GameNotInProgress => AppError::GameNotInProgress,
        ServiceError::GameFull(id) => AppError::GameFull(id),
        ServiceError::PlayerAlreadyInGame(id) => AppError::PlayerAlreadyExists(id),
        ServiceError::PlayerNotFound(id) => AppError::NoSuchPlayer(id),
        ServiceError::ValidationFailed(msg) => AppError::ValidationError(msg),
        ServiceError::SnapshotNotFound(id, _) => AppError::NoSuchSnapshot(id),
        ServiceError::DatabaseError(e) => {
            tracing::error!(error = %e, ?game_id, "Game service operation failed");
            if let Some(gid) = game_id {
                AppError::NoSuchGame(gid)
            } else {
                AppError::ValidationError("Operation failed".to_string())
            }
        }
        ServiceError::SerializationError(e) => {
            tracing::error!(error = %e, ?game_id, "Game service serialization error");
            if let Some(gid) = game_id {
                AppError::NoSuchGame(gid)
            } else {
                AppError::ValidationError("Operation failed".to_string())
            }
        }
    }
}
