//! Admin-only endpoints for managing players and games

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::common::{AdminPlayer, AppError, ServerState};
use crate::db;
use automatafl_api_types::*;

// ============================================================================
// Player Management
// ============================================================================

pub async fn admin_list_players(
    _admin: AdminPlayer,
    State(app_state): ServerState,
) -> Json<Vec<PlayerListItem>> {
    let players = db::get_all_players(&app_state.db).await.unwrap_or_default();

    let player_list = players
        .into_iter()
        .filter_map(|p| {
            Uuid::parse_str(&p.id).ok().map(|id| PlayerListItem {
                id,
                displayname: p.displayname,
                is_admin: p.is_admin,
            })
        })
        .collect();

    Json(player_list)
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
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
    Query(query): Query<AdminGetPlayerQuery>,
) -> Result<Json<AdminGetPlayerResponse>, AppError> {
    let player = db::get_player(&app_state.db, player_id)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?
        .ok_or(AppError::NoSuchPlayer(player_id))?;

    let stats = if query.include_stats.unwrap_or(false) {
        db::get_player_stats(&app_state.db, player_id)
            .await
            .ok()
            .flatten()
            .map(|s| PlayerStats {
                games_played: s.games_played,
                games_won: s.games_won,
                total_playtime: s.total_playtime,
                win_rate: if s.games_played > 0 {
                    s.games_won as f64 / s.games_played as f64
                } else {
                    0.0
                },
            })
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
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
    Json(req): Json<AdminUpdatePlayerRequest>,
) -> Result<StatusCode, AppError> {
    // Build update JSON
    let mut updates = serde_json::json!({});

    if let Some(displayname) = req.displayname {
        updates["displayname"] = serde_json::json!(displayname);
    }
    if let Some(is_admin) = req.is_admin {
        updates["is_admin"] = serde_json::json!(is_admin);
    }
    if req.bio.is_some() {
        updates["bio"] = serde_json::json!(req.bio);
    }
    if req.avatar_url.is_some() {
        updates["avatar_url"] = serde_json::json!(req.avatar_url);
    }
    if let Some(elo_rating) = req.elo_rating {
        updates["elo_rating"] = serde_json::json!(elo_rating);
    }

    // Apply updates using SurrealDB's merge functionality
    let _: Option<db::PlayerRecord> = app_state
        .db
        .update(("players", player_id.to_string()))
        .merge(updates)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    Ok(StatusCode::OK)
}

pub async fn admin_delete_player(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    // Delete player sessions
    app_state
        .db
        .query("DELETE sessions WHERE player_id = $player_id")
        .bind(("player_id", player_id.to_string()))
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    // Delete player from game_players
    app_state
        .db
        .query("DELETE game_players WHERE player_id = $player_id")
        .bind(("player_id", player_id.to_string()))
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    // Delete player stats
    app_state
        .db
        .delete::<Option<db::PlayerStatsRecord>>(("player_stats", player_id.to_string()))
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    // Delete player from matchmaking queue
    let _ = db::leave_matchmaking_queue(&app_state.db, player_id).await;

    // Delete player record
    app_state
        .db
        .delete::<Option<db::PlayerRecord>>(("players", player_id.to_string()))
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Game Management
// ============================================================================

pub async fn admin_list_all_games(
    _admin: AdminPlayer,
    State(app_state): ServerState,
) -> Json<Vec<GameListItem>> {
    crate::game::list_games(State(app_state)).await
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
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<AdminGetGameResponse>, AppError> {
    let (game_core, lifecycle, player_ids) = db::load_game_state(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    let game_record = db::get_game(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    Ok(Json(AdminGetGameResponse {
        id: game_uuid,
        game_state: game_core,
        lifecycle,
        player_ids,
        created_at: game_record.created_at,
        created_by: game_record.created_by,
        player_count: game_record.player_count,
    }))
}

pub async fn admin_delete_game(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    db::delete_game(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn admin_force_complete_round(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<CompleteRoundResponse>, AppError> {
    // Load game state
    let (mut game_core, mut lifecycle, _) = db::load_game_state(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    match game_core.try_complete_round() {
        Ok(_results) => {
            if let Some(winner) = game_core.winner {
                lifecycle = GameLifecycle::Finished;

                // Get game creation time for playtime calculation
                if let Ok(Some(game_record)) = db::get_game(&app_state.db, game_uuid).await {
                    // Update player stats and ELO ratings
                    match crate::matchmaking::update_game_completion_stats(
                        &app_state.db,
                        game_uuid,
                        winner,
                        game_record.created_at,
                    )
                    .await
                    {
                        Ok(elo_changes) if !elo_changes.is_empty() => {
                            // Broadcast ELO changes
                            let _ = crate::common::broadcast_event(
                                &app_state,
                                game_uuid,
                                automatafl_api_types::GameEventData::EloUpdate {
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
            }

            // Save updated state
            db::update_game_state(&app_state.db, game_uuid, &game_core, &lifecycle)
                .await
                .map_err(|_| AppError::NoSuchGame(game_uuid))?;

            Ok(Json(CompleteRoundResponse {
                success: true,
                message: "Admin forced round completion".to_string(),
            }))
        }
        Err(_) => {
            // Save state even with conflicts
            db::update_game_state(&app_state.db, game_uuid, &game_core, &lifecycle)
                .await
                .map_err(|_| AppError::NoSuchGame(game_uuid))?;

            Ok(Json(CompleteRoundResponse {
                success: false,
                message: "Conflict resolution needed".to_string(),
            }))
        }
    }
}

pub async fn admin_set_game_lifecycle(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Json(lifecycle): Json<GameLifecycle>,
) -> Result<StatusCode, AppError> {
    let (game_core, _, _) = db::load_game_state(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?
        .ok_or(AppError::NoSuchGame(game_uuid))?;

    db::update_game_state(&app_state.db, game_uuid, &game_core, &lifecycle)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

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
    State(app_state): ServerState,
) -> Result<Json<Vec<AdminSessionInfo>>, AppError> {
    let sessions: Vec<db::SessionRecord> = app_state
        .db
        .select("sessions")
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let mut session_info = Vec::new();
    for session in sessions {
        if let (Ok(session_id), Ok(player_id)) = (
            Uuid::parse_str(&session.id),
            Uuid::parse_str(&session.player_id),
        ) {
            // Get player displayname
            if let Ok(Some(player)) = db::get_player(&app_state.db, player_id).await {
                session_info.push(AdminSessionInfo {
                    id: session_id,
                    player_id,
                    player_displayname: player.displayname,
                    expires_at: session.expires_at,
                });
            }
        }
    }

    Ok(Json(session_info))
}

pub async fn admin_delete_session(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(session_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    db::delete_session(&app_state.db, session_id)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Serialize)]
pub struct SessionCleanupResponse {
    pub deleted_count: u64,
}

pub async fn admin_cleanup_expired_sessions(
    _admin: AdminPlayer,
    State(app_state): ServerState,
) -> Result<Json<SessionCleanupResponse>, AppError> {
    let now = crate::common::timestamp();
    let deleted = db::cleanup_expired_sessions(&app_state.db, now)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    Ok(Json(SessionCleanupResponse { deleted_count: deleted }))
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
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
    Query(query): Query<AdminEventsQuery>,
) -> Result<Json<Vec<db::GameEventRecord>>, AppError> {
    let events = db::get_game_history(
        &app_state.db,
        game_uuid,
        query.since,
        query.until,
        query.event_kind,
    )
    .await
    .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(events))
}

pub async fn admin_get_game_chat(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<db::ChatMessageRecord>>, AppError> {
    let messages = db::get_chat_messages(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(messages))
}

pub async fn admin_delete_chat_message(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path((game_uuid, timestamp)): Path<(Uuid, u64)>,
) -> Result<StatusCode, AppError> {
    let record_id = format!("{}:{}", game_uuid, timestamp);
    app_state
        .db
        .delete::<Option<db::ChatMessageRecord>>(("chat_messages", record_id))
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Snapshots
// ============================================================================

pub async fn admin_list_snapshots(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> Result<Json<Vec<db::SnapshotRecord>>, AppError> {
    let snapshots = db::list_snapshots(&app_state.db, game_uuid)
        .await
        .map_err(|_| AppError::NoSuchGame(game_uuid))?;

    Ok(Json(snapshots))
}

pub async fn admin_delete_snapshot(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path((game_uuid, index)): Path<(Uuid, usize)>,
) -> Result<StatusCode, AppError> {
    let record_id = format!("{}:{}", game_uuid, index);
    app_state
        .db
        .delete::<Option<db::SnapshotRecord>>(("snapshots", record_id))
        .await
        .map_err(|_| AppError::NoSuchSnapshot(game_uuid))?;

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
    State(app_state): ServerState,
) -> Result<Json<Vec<AdminMatchmakingInfo>>, AppError> {
    let queue = db::get_matchmaking_queue(&app_state.db)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let now = crate::common::timestamp();
    let mut queue_info = Vec::new();

    for entry in queue {
        if let Ok(player_id) = Uuid::parse_str(&entry.player_id)
            && let Ok(Some(player)) = db::get_player(&app_state.db, player_id).await
        {
            let prefs: JoinMatchmakingRequest = serde_json::from_str(&entry.game_preferences)
                .unwrap_or(JoinMatchmakingRequest {
                    player_count: 2,
                    use_column_rule: false,
                });

            queue_info.push(AdminMatchmakingInfo {
                player_id,
                player_displayname: player.displayname,
                queued_at: entry.queued_at,
                wait_time_seconds: now.saturating_sub(entry.queued_at),
                game_preferences: prefs,
            });
        }
    }

    Ok(Json(queue_info))
}

pub async fn admin_remove_from_matchmaking(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    db::leave_matchmaking_queue(&app_state.db, player_id)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Player Stats
// ============================================================================

pub async fn admin_get_player_stats(
    _admin: AdminPlayer,
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
) -> Result<Json<db::PlayerStatsRecord>, AppError> {
    let stats = db::get_player_stats(&app_state.db, player_id)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?
        .unwrap_or(db::PlayerStatsRecord {
            player_id: player_id.to_string(),
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
    State(app_state): ServerState,
    Path(player_id): Path<Uuid>,
    Json(req): Json<AdminUpdateStatsRequest>,
) -> Result<StatusCode, AppError> {
    let mut updates = serde_json::json!({});

    if let Some(games_played) = req.games_played {
        updates["games_played"] = serde_json::json!(games_played);
    }
    if let Some(games_won) = req.games_won {
        updates["games_won"] = serde_json::json!(games_won);
    }
    if let Some(total_playtime) = req.total_playtime {
        updates["total_playtime"] = serde_json::json!(total_playtime);
    }

    let _: Option<db::PlayerStatsRecord> = app_state
        .db
        .update(("player_stats", player_id.to_string()))
        .merge(updates)
        .await
        .map_err(|_| AppError::NoSuchPlayer(player_id))?;

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
    State(app_state): ServerState,
) -> Result<Json<DatabaseStats>, AppError> {
    let players: Vec<db::PlayerRecord> = app_state.db.select("players").await.unwrap_or_default();
    let games: Vec<db::GameRecord> = app_state.db.select("games").await.unwrap_or_default();
    let sessions: Vec<db::SessionRecord> =
        app_state.db.select("sessions").await.unwrap_or_default();
    let queue: Vec<db::MatchmakingQueueRecord> = app_state
        .db
        .select("matchmaking_queue")
        .await
        .unwrap_or_default();

    let now = crate::common::timestamp();
    let active_sessions = sessions.iter().filter(|s| s.expires_at > now).count();

    // Count chat messages across all games
    let mut total_chat_messages = 0;
    for game in &games {
        if let Ok(game_id) = Uuid::parse_str(&game.id)
            && let Ok(messages) = db::get_chat_messages(&app_state.db, game_id).await
        {
            total_chat_messages += messages.len();
        }
    }

    // Count snapshots across all games
    let mut total_snapshots = 0;
    for game in &games {
        if let Ok(game_id) = Uuid::parse_str(&game.id)
            && let Ok(snapshots) = db::list_snapshots(&app_state.db, game_id).await
        {
            total_snapshots += snapshots.len();
        }
    }

    Ok(Json(DatabaseStats {
        total_players: players.len(),
        total_games: games.len(),
        total_sessions: sessions.len(),
        active_sessions,
        matchmaking_queue_size: queue.len(),
        total_chat_messages,
        total_snapshots,
    }))
}

#[derive(serde::Serialize)]
pub struct TableInfo {
    pub name: String,
    pub record_count: usize,
}

pub async fn admin_list_tables(
    _admin: AdminPlayer,
    State(app_state): ServerState,
) -> Result<Json<Vec<TableInfo>>, AppError> {
    let tables = vec![
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

    let mut table_info = Vec::new();
    for table_name in tables {
        #[derive(serde::Deserialize)]
        struct CountResult {
            count: i64,
        }

        let query = format!("SELECT count() as count FROM {} GROUP ALL", table_name);
        let mut response = app_state.db.query(&query).await.map_err(|_| AppError::Unauthorized)?;
        let results: Vec<CountResult> = response.take(0).map_err(|_| AppError::Unauthorized)?;
        let record_count = results.first().map(|r| r.count as usize).unwrap_or(0);

        table_info.push(TableInfo {
            name: table_name.to_string(),
            record_count,
        });
    }

    Ok(Json(table_info))
}
