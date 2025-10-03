//! Matchmaking and leaderboard endpoints

use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{Json, extract::State, http::StatusCode};
use uuid::Uuid;

use automatafl_api_types::*;
use automatafl_logic::{Board, Coord, Pid};

use crate::common::{AppError, AuthPlayer, ServerState, timestamp};
use crate::{db, middleware};

// ============================================================================
// ELO Rating System
// ============================================================================

/// Calculate new ELO ratings after a game
/// Uses standard ELO formula with K-factor from constants
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

/// Update player stats and ELO ratings after a game finishes
pub async fn update_game_completion_stats(
    db: &db::Db,
    game_id: Uuid,
    winner_pid: Pid,
    game_start_time: u64,
) -> Result<Vec<EloChange>, Box<dyn std::error::Error + Send + Sync>> {
    let game_players = db::get_game_players(db, game_id).await?;
    let game_end_time = timestamp();
    let playtime = game_end_time.saturating_sub(game_start_time);

    // Get all player IDs and their PIDs
    let mut players_info: Vec<(Uuid, u8, i32)> = Vec::new();
    for gp in &game_players {
        let player_uuid = Uuid::parse_str(&gp.player_id)?;
        let player = db::get_player(db, player_uuid)
            .await?
            .ok_or("Player not found")?;
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

        tracing::info!(
            "ELO updated for game {}: {} changes recorded",
            game_id,
            elo_changes.len()
        );
    }

    // Update stats for all players atomically (including ELO if applicable)
    for (player_uuid, player_pid, _player_elo) in players_info {
        let won = player_pid == winner_pid.0;

        // Find new ELO for this player if it was updated
        let new_elo = elo_changes
            .iter()
            .find(|ec| ec.player_id == player_uuid)
            .map(|ec| ec.new_elo);

        // Use atomic update to prevent race conditions
        crate::transactions::update_player_stats_atomic(db, player_uuid, won, playtime, new_elo)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    }

    Ok(elo_changes)
}

// Re-export EloChange from api_types
pub use automatafl_api_types::EloChange;

// ============================================================================
// Leaderboard Endpoints
// ============================================================================

pub async fn get_leaderboard_elo(
    State(app_state): ServerState,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let ranked = db::get_leaderboard_by_elo(&app_state.db, 100)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    // Get stats for each player to populate leaderboard
    let mut entries: Vec<LeaderboardEntry> = Vec::new();
    for (player, rank) in ranked {
        if let Ok(player_id) = Uuid::parse_str(&player.id) {
            let stats = db::get_player_stats(&app_state.db, player_id)
                .await
                .ok()
                .flatten();
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
    let ranked = db::get_leaderboard_by_wins(&app_state.db, 100)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let entries: Vec<LeaderboardEntry> = ranked
        .into_iter()
        .filter_map(|(player, stats, rank)| {
            Uuid::parse_str(&player.id).ok().map(|id| LeaderboardEntry {
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
    let ranked = db::get_leaderboard_by_games(&app_state.db, 100)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let entries: Vec<LeaderboardEntry> = ranked
        .into_iter()
        .filter_map(|(player, stats, rank)| {
            Uuid::parse_str(&player.id).ok().map(|id| LeaderboardEntry {
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
    middleware::check_rate_limit_expensive(
        &format!("matchmaking:{}", auth.player_id),
        5.0,
        5.0 / 60.0,
    )
    .map_err(|_| {
        AppError::ValidationError("Too many matchmaking requests, please wait".to_string())
    })?;

    let preferences = serde_json::to_string(&req).unwrap();

    db::join_matchmaking_queue(&app_state.db, auth.player_id, timestamp(), preferences)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to join matchmaking queue for player {}: {}",
                auth.player_id,
                e
            );
            AppError::Unauthorized
        })?;

    tracing::info!("Player {} joined matchmaking queue", auth.player_id);
    Ok(StatusCode::OK)
}

#[tracing::instrument(skip(auth, app_state), fields(player_id = %auth.player_id))]
pub async fn leave_matchmaking(
    auth: AuthPlayer,
    State(app_state): ServerState,
) -> Result<StatusCode, AppError> {
    db::leave_matchmaking_queue(&app_state.db, auth.player_id)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    Ok(StatusCode::OK)
}

#[tracing::instrument(skip(auth, app_state), fields(player_id = %auth.player_id))]
pub async fn get_matchmaking_status(
    auth: AuthPlayer,
    State(app_state): ServerState,
) -> Result<Json<MatchmakingStatus>, AppError> {
    let status = db::get_matchmaking_status(&app_state.db, auth.player_id)
        .await
        .map_err(|_| AppError::Unauthorized)?;

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

// ============================================================================
// Background Matchmaking Task
// ============================================================================

pub async fn matchmaking_task(state: Arc<crate::common::AppState>) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(state.config.matchmaking_interval));

    loop {
        interval.tick().await;

        // Get all players in queue
        let Ok(queue) = db::get_matchmaking_queue(&state.db).await else {
            continue;
        };

        if queue.len() < 2 {
            continue;
        }

        // Group by preferences
        let mut by_prefs: HashMap<String, Vec<db::MatchmakingQueueRecord>> = HashMap::new();
        for entry in queue {
            by_prefs
                .entry(entry.game_preferences.clone())
                .or_default()
                .push(entry);
        }

        // Match players with same preferences
        for (_prefs, players) in by_prefs {
            if players.len() < 2 {
                continue;
            }

            #[derive(serde::Deserialize)]
            struct MatchPrefs {
                player_count: u8,
                use_column_rule: bool,
            }

            let prefs: MatchPrefs = match serde_json::from_str(&players[0].game_preferences) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let num_players = prefs.player_count as usize;
            if players.len() < num_players {
                continue;
            }

            // Get player ELO ratings
            let mut players_with_elo: Vec<(db::MatchmakingQueueRecord, i32)> = Vec::new();
            for player in players {
                if let Ok(player_uuid) = Uuid::parse_str(&player.player_id) {
                    if let Ok(Some(player_record)) = db::get_player(&state.db, player_uuid).await {
                        players_with_elo.push((player, player_record.elo_rating));
                    }
                }
            }

            if players_with_elo.len() < num_players {
                continue;
            }

            // Sort by wait time first (for fairness)
            players_with_elo.sort_by_key(|(p, _)| p.queued_at);

            // Try to match players with similar ELO (oldest player first, then find best match)
            let mut matched_indices = Vec::new();
            let elo_tolerance = state.config.matchmaking_elo_tolerance;

            // Take the player who waited longest
            matched_indices.push(0);
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

            // If we couldn't find enough players within tolerance, expand search
            if matched_indices.len() < num_players {
                matched_indices.clear();
                // Just take the first N players sorted by wait time (fairness over perfect ELO matching)
                for i in 0..num_players.min(players_with_elo.len()) {
                    matched_indices.push(i);
                }
            }

            if matched_indices.len() < num_players {
                continue;
            }

            let matched_players: Vec<_> = matched_indices
                .iter()
                .map(|&i| &players_with_elo[i].0)
                .collect();

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
            let first_player_id = match Uuid::parse_str(&matched_players[0].player_id) {
                Ok(id) => id,
                Err(_) => continue,
            };

            // Parse player UUIDs first
            let mut player_uuids = Vec::new();
            for player in &matched_players {
                if let Ok(player_uuid) = Uuid::parse_str(&player.player_id) {
                    player_uuids.push(player_uuid);
                } else {
                    tracing::error!("Failed to parse player UUID: {}", player.player_id);
                }
            }

            if player_uuids.len() != matched_players.len() {
                tracing::error!("Failed to parse all player UUIDs, skipping match");
                continue;
            }

            // Create game and add players atomically using transaction
            let player_pids: Vec<(Uuid, u8)> = player_uuids
                .iter()
                .enumerate()
                .map(|(i, &uuid)| (uuid, i as u8))
                .collect();

            match crate::transactions::create_game_with_players(
                &state.db,
                game_id,
                &game_core,
                &lifecycle,
                first_player_id,
                prefs.player_count,
                player_pids,
            )
            .await
            {
                Ok(_) => {
                    tracing::info!(
                        "Created game {} with {} players atomically",
                        game_id,
                        player_uuids.len()
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to create game atomically: {}", e);
                    continue;
                }
            }

            // Remove matched players from queue (separate operation, logs errors but continues)
            if let Err(e) = db::remove_from_queue(&state.db, player_uuids.clone()).await {
                tracing::error!("Failed to remove players from queue: {}", e);
            }

            // Broadcast match found event (would need WebSocket notification system)
            tracing::info!(
                "Match found! Game {} created with {} players",
                game_id,
                player_uuids.len()
            );
        }
    }
}
