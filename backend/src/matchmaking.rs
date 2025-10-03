//! Matchmaking and leaderboard endpoints

use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{Json, extract::State, http::StatusCode};
use uuid::Uuid;

use automatafl_api_types::*;
use automatafl_logic::{Board, Coord, Pid};

use crate::common::{AppError, AuthPlayer, ServerState, timestamp};
use crate::db::as_uuid;
use crate::middleware;
use crate::services::{MatchmakingServiceError, PlayerServiceError, QueueEntry};

// ============================================================================
// ELO Rating System
// ============================================================================

// Re-export EloChange from api_types
pub use automatafl_api_types::EloChange;

// ============================================================================
// Leaderboard Endpoints
// ============================================================================

pub async fn get_leaderboard_elo(
    State(app_state): ServerState,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let ranked = app_state
        .player_service
        .leaderboard_by_elo(100)
        .await
        .map_err(|err| map_player_error("leaderboard_elo", err))?;

    // Get stats for each player to populate leaderboard
    let mut entries: Vec<LeaderboardEntry> = Vec::new();
    for (player, rank) in ranked {
        let player_id = as_uuid(&player.id);
        let stats = match app_state.player_service.get_stats(player_id).await {
            Ok(stats) => stats,
            Err(err) => {
                tracing::error!(player_id = %player_id, "Failed to load stats: {}", err);
                None
            }
        };
        entries.push(LeaderboardEntry {
            rank,
            player_id,
            displayname: player.displayname,
            value: player.elo_rating as i64,
            elo_rating: Some(player.elo_rating),
            games_played: stats.as_ref().map(|s| s.games_played),
            games_won: stats.as_ref().map(|s| s.games_won),
        });
    }

    let total_players = entries.len();

    Ok(Json(LeaderboardResponse {
        entries,
        total_players,
    }))
}

pub async fn get_leaderboard_wins(
    State(app_state): ServerState,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let ranked = app_state
        .player_service
        .leaderboard_by_wins(100)
        .await
        .map_err(|err| map_player_error("leaderboard_wins", err))?;

    let entries: Vec<LeaderboardEntry> = ranked
        .into_iter()
        .filter_map(|(player, stats, rank)| {
            Some(as_uuid(&player.id)).map(|id| LeaderboardEntry {
                rank,
                player_id: id,
                displayname: player.displayname.clone(),
                value: stats.games_won as i64,
                elo_rating: Some(player.elo_rating),
                games_played: Some(stats.games_played),
                games_won: Some(stats.games_won),
            })
        })
        .collect();

    let total_players = entries.len();

    Ok(Json(LeaderboardResponse {
        entries,
        total_players,
    }))
}

pub async fn get_leaderboard_games(
    State(app_state): ServerState,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let ranked = app_state
        .player_service
        .leaderboard_by_games(100)
        .await
        .map_err(|err| map_player_error("leaderboard_games", err))?;

    let entries: Vec<LeaderboardEntry> = ranked
        .into_iter()
        .filter_map(|(player, stats, rank)| {
            Some(as_uuid(&player.id)).map(|id| LeaderboardEntry {
                rank,
                player_id: id,
                displayname: player.displayname.clone(),
                value: stats.games_played as i64,
                elo_rating: Some(player.elo_rating),
                games_played: Some(stats.games_played),
                games_won: Some(stats.games_won),
            })
        })
        .collect();

    let total_players = entries.len();

    Ok(Json(LeaderboardResponse {
        entries,
        total_players,
    }))
}

// ============================================================================
// Matchmaking Endpoints
// ============================================================================

#[tracing::instrument(skip(auth, app_state, req), fields(player_id = %auth.player_id, player_count = %req.player_count))]
pub async fn join_matchmaking(
    auth: AuthPlayer,
    State(app_state): ServerState,
    Json(req): Json<JoinMatchmakingRequest>,
) -> Result<StatusCode, AppError> {
    // Rate limit matchmaking joins - 5 joins per minute per player
    middleware::check_rate_limit_with_error(
        &format!("matchmaking:{}", auth.player_id),
        5.0,
        5.0 / 60.0,
    )?;

    let preferences = serde_json::to_string(&req).unwrap();

    app_state
        .matchmaking_service
        .join_queue(auth.player_id, timestamp(), preferences)
        .await
        .map_err(|err| map_matchmaking_error("join", auth.player_id, err))?;

    tracing::info!("Player {} joined matchmaking queue", auth.player_id);
    Ok(StatusCode::OK)
}

#[tracing::instrument(skip(auth, app_state), fields(player_id = %auth.player_id))]
pub async fn leave_matchmaking(
    auth: AuthPlayer,
    State(app_state): ServerState,
) -> Result<StatusCode, AppError> {
    app_state
        .matchmaking_service
        .leave_queue(auth.player_id)
        .await
        .map_err(|err| map_matchmaking_error("leave", auth.player_id, err))?;

    Ok(StatusCode::OK)
}

#[tracing::instrument(skip(auth, app_state), fields(player_id = %auth.player_id))]
pub async fn get_matchmaking_status(
    auth: AuthPlayer,
    State(app_state): ServerState,
) -> Result<Json<MatchmakingStatus>, AppError> {
    let status = app_state
        .matchmaking_service
        .get_status(auth.player_id)
        .await
        .map_err(|err| map_matchmaking_error("status", auth.player_id, err))?;

    match status {
        Some(record) => {
            let now = timestamp();
            let wait_time = now.saturating_sub(record.queued_at);

            Ok(Json(MatchmakingStatus {
                in_queue: true,
                queued_at: Some(record.queued_at),
                estimated_wait_time: Some(wait_time),
            }))
        }
        None => Ok(Json(MatchmakingStatus {
            in_queue: false,
            queued_at: None,
            estimated_wait_time: None,
        })),
    }
}

fn map_player_error(action: &str, err: PlayerServiceError) -> AppError {
    match err {
        PlayerServiceError::Validation(msg) => AppError::ValidationError(msg),
        PlayerServiceError::NotFound(_) => AppError::Unauthorized,
        PlayerServiceError::Database(e) => {
            tracing::error!(action = action, error = %e, "Player service error while generating leaderboard");
            AppError::Unauthorized
        }
    }
}

fn map_matchmaking_error(action: &str, player_id: Uuid, err: MatchmakingServiceError) -> AppError {
    match err {
        MatchmakingServiceError::PlayerNotFound(_) => AppError::NoSuchPlayer(player_id),
        MatchmakingServiceError::Database(e) => {
            tracing::error!(player_id = %player_id, action = action, error = %e, "Matchmaking service error");
            AppError::Unauthorized
        }
    }
}

// ============================================================================
// Background Matchmaking Task
// ============================================================================

#[derive(serde::Deserialize)]
struct MatchPrefs {
    player_count: u8,
    use_column_rule: bool,
}

/// Enhanced matchmaking task with better error handling and performance
pub async fn matchmaking_task(state: Arc<crate::common::AppState>) {
    let mut interval = tokio::time::interval(state.config.matchmaking_interval);

    tracing::info!(
        "Matchmaking task started with {:?} interval",
        state.config.matchmaking_interval
    );

    loop {
        interval.tick().await;

        if let Err(e) = run_matchmaking_cycle(&state).await {
            tracing::error!("Matchmaking cycle failed: {}", e);
        }
    }
}

/// Single matchmaking cycle - extracted for better error handling
async fn run_matchmaking_cycle(
    state: &Arc<crate::common::AppState>,
) -> Result<(), crate::common::AppError> {
    // Get all players in queue
    let queue = state.matchmaking_service.list_queue().await.map_err(|e| {
        tracing::error!("Failed to load matchmaking queue: {}", e);
        crate::common::AppError::DatabaseError("Failed to load matchmaking queue".to_string())
    })?;

    if queue.len() < 2 {
        tracing::debug!("Not enough players in queue ({}), waiting...", queue.len());
        return Ok(());
    }

    tracing::debug!("Processing matchmaking for {} players", queue.len());

    // Group by preferences
    let mut by_prefs: HashMap<String, Vec<QueueEntry>> = HashMap::new();
    for entry in queue {
        let preferences = entry.game_preferences.clone();
        by_prefs.entry(preferences).or_default().push(entry);
    }

    // Match players with same preferences
    for (_prefs, players) in by_prefs {
        if players.len() < 2 {
            continue;
        }

        if let Err(e) = process_preference_group(state, &players).await {
            tracing::error!("Failed to process preference group: {}", e);
        }
    }

    Ok(())
}

/// Process a group of players with the same preferences
async fn process_preference_group(
    state: &Arc<crate::common::AppState>,
    players: &[QueueEntry],
) -> Result<(), crate::common::AppError> {
    let prefs: MatchPrefs = serde_json::from_str(&players[0].game_preferences).map_err(|e| {
        tracing::warn!("Invalid game preferences format: {}", e);
        crate::common::AppError::ValidationError("Invalid game preferences".to_string())
    })?;

    let num_players = prefs.player_count as usize;
    if players.len() < num_players {
        tracing::debug!(
            "Not enough players for {} player game (have {})",
            num_players,
            players.len()
        );
        return Ok(());
    }

    // Get player ELO ratings in batch
    let players_with_elo = get_players_with_elo(state, players).await?;

    if players_with_elo.len() < num_players {
        tracing::debug!(
            "Not enough valid players for {} player game (have {})",
            num_players,
            players_with_elo.len()
        );
        return Ok(());
    }

    // Sort by wait time first (for fairness)
    let mut players_with_elo = players_with_elo;
    players_with_elo.sort_by_key(|(p, _)| p.queued_at);

    // Try to match players with similar ELO
    let matched_indices = find_best_matches(
        &players_with_elo,
        num_players,
        state.config.matchmaking_elo_tolerance,
    );

    if matched_indices.len() < num_players {
        tracing::debug!(
            "Could not find enough similar players, need {} but found {}",
            num_players,
            matched_indices.len()
        );
        return Ok(());
    }

    let matched_players: Vec<QueueEntry> = matched_indices
        .iter()
        .map(|&i| players_with_elo[i].0.clone())
        .collect();

    // Create game for matched players
    create_game_for_players(state, &matched_players, &prefs).await?;

    Ok(())
}

/// Get ELO ratings for all players in batch
async fn get_players_with_elo(
    state: &Arc<crate::common::AppState>,
    players: &[QueueEntry],
) -> Result<Vec<(QueueEntry, i32)>, crate::common::AppError> {
    let mut players_with_elo = Vec::new();

    for player in players {
        let player_uuid = player.player_id;
        match state.player_service.get_player(player_uuid).await {
            Ok(Some(player_record)) => {
                players_with_elo.push((player.clone(), player_record.elo_rating));
            }
            Ok(None) => {
                tracing::warn!(player_id = %player_uuid, "Player not found during matchmaking");
            }
            Err(e) => {
                tracing::error!(player_id = %player_uuid, "Failed to load player during matchmaking: {}", e);
            }
        }
    }

    Ok(players_with_elo)
}

/// Find the best matches for a game based on ELO similarity and wait time
fn find_best_matches(
    players_with_elo: &[(QueueEntry, i32)],
    num_players: usize,
    elo_tolerance: i32,
) -> Vec<usize> {
    if players_with_elo.len() < num_players {
        return Vec::new();
    }

    // Start with the player who waited longest (index 0 after sorting)
    let mut matched_indices = vec![0];
    let base_elo = players_with_elo[0].1;

    // Find other players within ELO tolerance
    for i in 1..players_with_elo.len() {
        if matched_indices.len() >= num_players {
            break;
        }
        let elo_diff = (players_with_elo[i].1 - base_elo).abs();
        if elo_diff <= elo_tolerance {
            matched_indices.push(i);
        }
    }

    // If we couldn't find enough players within tolerance, fall back to wait time
    if matched_indices.len() < num_players {
        matched_indices.clear();
        // Just take the first N players sorted by wait time
        for i in 0..num_players.min(players_with_elo.len()) {
            matched_indices.push(i);
        }
    }

    matched_indices
}

/// Create a game for the matched players
async fn create_game_for_players(
    state: &Arc<crate::common::AppState>,
    matched_players: &[QueueEntry],
    prefs: &MatchPrefs,
) -> Result<(), crate::common::AppError> {
    // Create game for matched players
    let board = Board::stock_two_player();
    let mut game_core =
        automatafl_logic::Game::new(board, prefs.player_count, prefs.use_column_rule);

    // Set up goals for two-player game
    if prefs.player_count == 2 {
        game_core.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 10, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 0, y: 10 }, Pid(1)));
        game_core.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
    }

    let game_id = Uuid::new_v4();
    let lifecycle = GameLifecycle::Waiting;
    let first_player_id = matched_players[0].player_id;

    // Parse player UUIDs
    let player_uuids: Vec<Uuid> = matched_players.iter().map(|p| p.player_id).collect();

    // Create game and add players atomically using transaction
    let player_pids: Vec<(Uuid, u8)> = player_uuids
        .iter()
        .enumerate()
        .map(|(i, &uuid)| (uuid, i as u8))
        .collect();

    state
        .game_service
        .create_game_with_players(
            game_id,
            &game_core,
            lifecycle,
            first_player_id,
            prefs.player_count,
            &player_pids,
        )
        .await?;

    tracing::info!(
        "Created game {} with {} players atomically",
        game_id,
        player_uuids.len()
    );

    // Remove matched players from queue
    state
        .matchmaking_service
        .remove_players(&player_uuids)
        .await
        .map_err(|e| {
            tracing::error!("Failed to remove players from queue: {}", e);
            crate::common::AppError::DatabaseError(
                "Failed to remove players from queue".to_string(),
            )
        })?;

    tracing::info!(
        "Match found! Game {} created with {} players",
        game_id,
        player_uuids.len()
    );

    Ok(())
}
