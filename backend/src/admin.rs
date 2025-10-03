//! Admin-only endpoints for managing players and games

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use surrealdb::RecordId;
use uuid::Uuid;

use automatafl_api_types::{
    CompleteRoundResponse, GameLifecycle, GameListItem, JoinMatchmakingRequest, PlayerListItem,
    PlayerStats,
};

use crate::{
    common::{AdminPlayer, AppError, ServerState, broadcast_event, timestamp},
    db::{ChatMessageRecord, GameEventRecord, PlayerStatsRecord, SnapshotRecord, as_uuid},
    services::admin_service::{AdminQueueEntry, AdminServiceError},
    services::game_service::ServiceError,
    services::player_service::PlayerServiceError,
};

// ============================================================================
// Player Management
// ============================================================================

pub async fn admin_list_players(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Json<Vec<PlayerListItem>> {
    let players = state.admin_service.list_players().await.unwrap_or_default();

    let items = players
        .into_iter()
        .map(|record| PlayerListItem {
            id: as_uuid(&record.id),
            displayname: record.displayname,
            is_admin: record.is_admin,
        })
        .collect();

    Json(items)
}

#[derive(serde::Deserialize)]
pub struct AdminGetPlayerQuery {
    pub include_stats: Option<bool>,
}

#[derive(serde::Serialize)]
pub struct AdminGetPlayerResponse {
    pub id: Uuid,
    pub displayname: String,
    pub is_admin: bool,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: u64,
    pub elo_rating: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<PlayerStats>,
}

pub async fn admin_get_player(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
    Query(query): Query<AdminGetPlayerQuery>,
) -> Result<Json<AdminGetPlayerResponse>, AppError> {
    let player = state
        .admin_service
        .get_player(player_id)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?
        .ok_or(AppError::NoSuchPlayer(player_id))?;

    let stats = if query.include_stats.unwrap_or(false) {
        match state.admin_service.player_stats(player_id).await {
            Ok(Some(record)) => Some(PlayerStats {
                games_played: record.games_played,
                games_won: record.games_won,
                total_playtime: record.total_playtime,
                win_rate: if record.games_played > 0 {
                    record.games_won as f64 / record.games_played as f64
                } else {
                    0.0
                },
            }),
            Ok(None) => None,
            Err(err) => {
                tracing::error!(player_id = %player_id, "Failed to load player stats: {}", err);
                None
            }
        }
    } else {
        None
    };

    Ok(Json(AdminGetPlayerResponse {
        id: player_id,
        displayname: player.displayname,
        is_admin: player.is_admin,
        bio: player.bio,
        avatar_url: player.avatar_url,
        created_at: player.created_at,
        elo_rating: player.elo_rating,
        stats,
    }))
}

#[derive(serde::Deserialize)]
pub struct AdminUpdatePlayerRequest {
    pub displayname: Option<String>,
    pub is_admin: Option<bool>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub elo_rating: Option<i32>,
}

pub async fn admin_update_player(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
    Json(req): Json<AdminUpdatePlayerRequest>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .update_player_fields(
            player_id,
            req.displayname,
            req.is_admin,
            req.bio,
            req.avatar_url,
            req.elo_rating,
        )
        .await
        .map_err(|err| {
            tracing::error!(player_id = %player_id, "Failed to update player: {}", err);
            AppError::NoSuchPlayer(player_id)
        })?;

    Ok(StatusCode::OK)
}

pub async fn admin_delete_player(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .delete_player(player_id)
        .await
        .map_err(|err| map_admin_error_for_player(player_id, err))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Game Management
// ============================================================================

pub async fn admin_list_all_games(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Json<Vec<GameListItem>> {
    let games = state
        .game_service
        .list_games_with_player_counts()
        .await
        .unwrap_or_default();

    Json(games)
}

#[derive(serde::Serialize)]
pub struct AdminGetGameResponse {
    pub id: Uuid,
    pub game_state: automatafl_logic::Game,
    pub lifecycle: GameLifecycle,
    pub player_ids: std::collections::HashMap<Uuid, automatafl_logic::Pid>,
    pub created_at: u64,
    pub created_by: String,
    pub player_count: u8,
}

pub async fn admin_get_game(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<AdminGetGameResponse>, AppError> {
    let (game_core, lifecycle, player_ids) = state
        .game_service
        .load_game(game_uuid)
        .await
        .map_err(|err| map_game_error(Some(game_uuid), err))?;

    let game_record = state
        .admin_service
        .get_game_record(game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    Ok(Json(AdminGetGameResponse {
        id: game_uuid,
        game_state: game_core,
        lifecycle,
        player_ids,
        created_at: game_record.created_at,
        created_by: as_uuid(&game_record.created_by).to_string(),
        player_count: game_record.player_count,
    }))
}

pub async fn admin_delete_game(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .delete_game(game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn admin_force_complete_round(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    let (response, events) = state
        .admin_service
        .force_complete_round(game_uuid)
        .await
        .map_err(|err| map_admin_error_for_game(game_uuid, err))?;

    for event in events {
        broadcast_event(&state, game_uuid, event).await?;
    }

    Ok(Json(response))
}

pub async fn admin_set_game_lifecycle(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Json(lifecycle): Json<GameLifecycle>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .set_game_lifecycle(game_uuid, lifecycle)
        .await
        .map_err(|err| map_admin_error_for_game(game_uuid, err))?;

    Ok(StatusCode::OK)
}

// ============================================================================
// Session Management
// ============================================================================

#[derive(serde::Serialize)]
pub struct AdminSessionInfo {
    pub id: Uuid,
    pub player_id: Uuid,
    pub player_displayname: String,
    pub expires_at: u64,
}

pub async fn admin_list_sessions(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Result<Json<Vec<AdminSessionInfo>>, AppError> {
    let sessions = state.admin_service.list_sessions().await.map_err(|err| {
        tracing::error!(error = %err, "Failed to list admin sessions");
        AppError::Unauthorized
    })?;

    let player_ids: Vec<Uuid> = sessions
        .iter()
        .map(|session| as_uuid(&session.player_id))
        .collect();

    let players_by_id = state
        .admin_service
        .players_by_ids(&player_ids)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "Failed to load players for sessions");
            AppError::Unauthorized
        })?;

    let mut session_info = Vec::new();
    for session in sessions {
        let session_id = as_uuid(&session.id);
        let player_id = as_uuid(&session.player_id);

        if let Some(player) = players_by_id.get(&player_id) {
            session_info.push(AdminSessionInfo {
                id: session_id,
                player_id,
                player_displayname: player.displayname.clone(),
                expires_at: session.expires_at,
            });
        }
    }

    Ok(Json(session_info))
}

pub async fn admin_delete_session(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(session_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .delete_session(session_id)
        .await
        .map_err(|err| {
            tracing::error!(session_id = %session_id, error = %err, "Failed to delete session");
            AppError::Unauthorized
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Serialize)]
pub struct SessionCleanupResponse {
    pub deleted_count: u64,
}

pub async fn admin_cleanup_expired_sessions(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Result<Json<SessionCleanupResponse>, AppError> {
    let now = timestamp();
    let deleted = state
        .admin_service
        .cleanup_expired_sessions(now)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "Failed to cleanup expired sessions");
            AppError::Unauthorized
        })?;

    Ok(Json(SessionCleanupResponse {
        deleted_count: deleted,
    }))
}

// ============================================================================
// Game Events & Chat
// ============================================================================

#[derive(serde::Deserialize)]
pub struct AdminEventsQuery {
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub event_kind: Option<String>,
}

pub async fn admin_get_game_events(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(query): Query<AdminEventsQuery>,
) -> Result<Json<Vec<GameEventRecord>>, AppError> {
    let events = state
        .admin_service
        .game_events(game_uuid, query.since, query.until, query.event_kind)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_uuid, error = %err, "Failed to load game events");
            AppError::NoSuchGame(game_uuid)
        })?;

    Ok(Json(events))
}

pub async fn admin_get_game_chat(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<ChatMessageRecord>>, AppError> {
    let messages = state
        .admin_service
        .game_chat(game_uuid)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_uuid, error = %err, "Failed to load chat messages");
            AppError::NoSuchGame(game_uuid)
        })?;

    Ok(Json(messages))
}

pub async fn admin_delete_chat_message(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path((game_uuid, timestamp)): Path<(Uuid, u64)>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .delete_chat_message(game_uuid, timestamp)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_uuid, error = %err, "Failed to delete chat message");
            AppError::NoSuchGame(game_uuid)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Snapshots
// ============================================================================

pub async fn admin_list_snapshots(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<SnapshotRecord>>, AppError> {
    let snapshots = state
        .admin_service
        .list_snapshots(game_uuid)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_uuid, error = %err, "Failed to list snapshots");
            AppError::NoSuchGame(game_uuid)
        })?;

    Ok(Json(snapshots))
}

pub async fn admin_delete_snapshot(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path((game_uuid, index)): Path<(Uuid, usize)>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .delete_snapshot(game_uuid, index)
        .await
        .map_err(|err| {
            tracing::error!(game_id = %game_uuid, snapshot_index = index, error = %err,
                "Failed to delete snapshot");
            AppError::NoSuchSnapshot(game_uuid)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Matchmaking Queue
// ============================================================================

#[derive(serde::Serialize)]
pub struct AdminMatchmakingInfo {
    pub player_id: Uuid,
    pub player_displayname: String,
    pub queued_at: u64,
    pub wait_time_seconds: u64,
    pub game_preferences: JoinMatchmakingRequest,
}

pub async fn admin_list_matchmaking_queue(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Result<Json<Vec<AdminMatchmakingInfo>>, AppError> {
    let queue = state.admin_service.queue_entries().await.map_err(|err| {
        tracing::error!(error = %err, "Failed to load matchmaking queue");
        AppError::Unauthorized
    })?;
    let queue_info = queue
        .into_iter()
        .filter_map(|entry| {
            let AdminQueueEntry {
                player_id,
                player_displayname,
                player_elo_rating,
                queued_at,
                wait_time_seconds,
                game_preferences,
            } = entry;

            match (player_displayname, player_elo_rating) {
                (Some(player_displayname), Some(_)) => Some(AdminMatchmakingInfo {
                    player_id,
                    player_displayname,
                    queued_at,
                    wait_time_seconds,
                    game_preferences,
                }),
                _ => None,
            }
        })
        .collect();

    Ok(Json(queue_info))
}

pub async fn admin_remove_from_matchmaking(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .remove_from_queue(player_id)
        .await
        .map_err(|err| {
            tracing::error!(player_id = %player_id, error = %err, "Failed to remove from queue");
            AppError::NoSuchPlayer(player_id)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Player Stats
// ============================================================================

pub async fn admin_get_player_stats(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<PlayerStatsRecord>, AppError> {
    let stats = state
        .admin_service
        .player_stats(player_id)
        .await
        .map_err(|err| {
            tracing::error!(player_id = %player_id, error = %err, "Failed to load player stats");
            AppError::NoSuchPlayer(player_id)
        })?
        .unwrap_or(PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        });

    Ok(Json(stats))
}

#[derive(serde::Deserialize)]
pub struct AdminUpdateStatsRequest {
    pub games_played: Option<u32>,
    pub games_won: Option<u32>,
    pub total_playtime: Option<u64>,
}

pub async fn admin_update_player_stats(
    _admin: AdminPlayer,
    State(state): ServerState,
    Path(player_id): Path<Uuid>,
    Json(req): Json<AdminUpdateStatsRequest>,
) -> Result<StatusCode, AppError> {
    state
        .admin_service
        .update_player_stats_fields(
            player_id,
            req.games_played,
            req.games_won,
            req.total_playtime,
        )
        .await
        .map_err(|err| {
            tracing::error!(player_id = %player_id, error = %err, "Failed to update player stats");
            AppError::NoSuchPlayer(player_id)
        })?;

    Ok(StatusCode::OK)
}

// ============================================================================
// Database Statistics & Introspection
// ============================================================================

#[derive(serde::Serialize)]
pub struct DatabaseStats {
    pub total_players: usize,
    pub total_games: usize,
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub matchmaking_queue_size: usize,
    pub total_chat_messages: usize,
    pub total_snapshots: usize,
}

pub async fn admin_get_database_stats(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Result<Json<DatabaseStats>, AppError> {
    let stats = state.admin_service.database_stats().await.map_err(|err| {
        tracing::error!(error = %err, "Failed to load database stats");
        AppError::Unauthorized
    })?;

    Ok(Json(DatabaseStats {
        total_players: stats.total_players,
        total_games: stats.total_games,
        total_sessions: stats.total_sessions,
        active_sessions: stats.active_sessions,
        matchmaking_queue_size: stats.queue_size,
        total_chat_messages: stats.total_chat_messages,
        total_snapshots: stats.total_snapshots,
    }))
}

#[derive(serde::Serialize)]
pub struct TableInfo {
    pub name: String,
    pub record_count: usize,
}

pub async fn admin_list_tables(
    _admin: AdminPlayer,
    State(state): ServerState,
) -> Result<Json<Vec<TableInfo>>, AppError> {
    let tables = [
        "players",
        "sessions",
        "games",
        "game_players",
        "game_events",
        "chat_messages",
        "snapshots",
        "matchmaking_queue",
        "player_stats",
    ];

    let counts = state
        .admin_service
        .table_counts(&tables)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "Failed to list table counts");
            AppError::Unauthorized
        })?;

    let info = counts
        .into_iter()
        .map(|(name, record_count)| TableInfo { name, record_count })
        .collect();

    Ok(Json(info))
}

// ============================================================================
// Error Mapping Helpers
// ============================================================================

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

fn map_admin_error_for_player(player_id: Uuid, err: AdminServiceError) -> AppError {
    match err {
        AdminServiceError::Player(PlayerServiceError::NotFound(id)) => AppError::NoSuchPlayer(id),
        AdminServiceError::Player(PlayerServiceError::Validation(msg)) => {
            AppError::ValidationError(msg)
        }
        AdminServiceError::Player(PlayerServiceError::Database(e))
        | AdminServiceError::Database(e) => {
            tracing::error!(player_id = %player_id, error = %e, "Admin player operation failed");
            AppError::NoSuchPlayer(player_id)
        }
        AdminServiceError::Game(game_err) => map_game_error(None, game_err),
    }
}

fn map_admin_error_for_game(game_id: Uuid, err: AdminServiceError) -> AppError {
    match err {
        AdminServiceError::Game(game_err) => map_game_error(Some(game_id), game_err),
        AdminServiceError::Player(PlayerServiceError::NotFound(id)) => AppError::NoSuchPlayer(id),
        AdminServiceError::Player(PlayerServiceError::Validation(msg)) => {
            AppError::ValidationError(msg)
        }
        AdminServiceError::Player(PlayerServiceError::Database(e))
        | AdminServiceError::Database(e) => {
            tracing::error!(game_id = %game_id, error = %e, "Admin game operation failed");
            AppError::NoSuchGame(game_id)
        }
    }
}
