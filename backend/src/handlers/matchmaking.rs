use axum::{
    extract::State,
    Json,
};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use automatafl_api::{JoinMatchmakingRequest, MatchmakingStatusResponse, GameStateResponse};

use crate::{
    auth::AuthUser,
    error::{AppError, Result},
    models::User,
    state::AppState,
};

pub async fn join_matchmaking(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(req): Json<JoinMatchmakingRequest>,
) -> Result<Json<MatchmakingStatusResponse>> {
    // Get user info
    let user = sqlx::query_as!(
        User,
        r#"SELECT id as "id!: Uuid", username, password_hash, rating as "rating: i32", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>" 
           FROM users WHERE id = ?"#,
        auth_user.id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;
    
    // Join the matchmaking queue
    state.matchmaking_queue
        .join(&user, req.rating_range)
        .await
        .map_err(|e| AppError::BadRequest(e))?;
    
    // Try to find a match immediately
    if let Some(opponent) = state.matchmaking_queue.find_match(auth_user.id).await {
        // Create a game with the matched opponent
        let _game_state = create_matched_game(&state, &user, opponent).await?;
        
        return Ok(Json(MatchmakingStatusResponse {
            in_queue: false,
            estimated_wait_time_seconds: Some(0),
            players_in_queue: 0,
        }));
    }
    
    // Otherwise, return queue status
    let (queue_size, estimated_wait) = state.matchmaking_queue.get_queue_status().await;
    
    Ok(Json(MatchmakingStatusResponse {
        in_queue: true,
        estimated_wait_time_seconds: estimated_wait,
        players_in_queue: queue_size as u32,
    }))
}

pub async fn leave_matchmaking(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<MatchmakingStatusResponse>> {
    state.matchmaking_queue.leave(auth_user.id).await
        .map_err(|e| AppError::BadRequest(e))?;
    
    Ok(Json(MatchmakingStatusResponse {
        in_queue: false,
        estimated_wait_time_seconds: None,
        players_in_queue: 0,
    }))
}

pub async fn matchmaking_status(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<MatchmakingStatusResponse>> {
    let in_queue = state.matchmaking_queue.is_in_queue(&auth_user.id);
    let (queue_size, estimated_wait) = state.matchmaking_queue.get_queue_status().await;
    
    Ok(Json(MatchmakingStatusResponse {
        in_queue,
        estimated_wait_time_seconds: if in_queue { estimated_wait } else { None },
        players_in_queue: queue_size as u32,
    }))
}

// Background task to process matchmaking queue
pub async fn matchmaking_worker(state: AppState) {
    loop {
        // Check every 5 seconds
        sleep(Duration::from_secs(5)).await;
        
        // Get all users in queue
        let queue_snapshot = {
            let queue = state.matchmaking_queue.queue.lock().await;
            queue.clone()
        };
        
        // Try to match users
        for entry in queue_snapshot {
            if let Some(opponent) = state.matchmaking_queue.find_match(entry.user_id).await {
                // Create game for matched players
                match create_matched_game_from_entries(&state, &entry, &opponent).await {
                    Ok(_game_id) => {
                        tracing::info!(
                            "Matched {} ({}) with {} ({})", 
                            entry.username, entry.rating,
                            opponent.username, opponent.rating
                        );
                        
                        // TODO: Send WebSocket notification to both players
                    }
                    Err(e) => {
                        tracing::error!("Failed to create matched game: {:?}", e);
                        // Re-add players to queue
                        // Note: In a real implementation, you'd want more sophisticated error handling
                    }
                }
            }
        }
    }
}

async fn create_matched_game(
    state: &AppState,
    user: &User,
    opponent: crate::matchmaking::MatchmakingEntry,
) -> Result<GameStateResponse> {
    // Determine who plays white (higher rating or random if equal)
    let (white_player_id, black_player_id) = if user.rating > opponent.rating {
        (user.id, opponent.user_id)
    } else if user.rating < opponent.rating {
        (opponent.user_id, user.id)
    } else {
        // Equal rating, randomize
        if rand::random::<bool>() {
            (user.id, opponent.user_id)
        } else {
            (opponent.user_id, user.id)
        }
    };
    
    // Create the game using existing handler logic
    // This is a simplified version - in production you'd refactor the create_game logic
    let game_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    
    // Create game logic
    let board = automatafl_logic::Board::stock_two_player();
    let mut game_logic = automatafl_logic::Game::new(board, 2, true);
    game_logic.goals.push((automatafl_logic::Coord { x: 5, y: 0 }, automatafl_logic::Pid(0)));
    game_logic.goals.push((automatafl_logic::Coord { x: 5, y: 10 }, automatafl_logic::Pid(1)));
    
    let stored_state = crate::models::StoredGameState {
        game: game_logic,
        white_player_id: Some(white_player_id),
        black_player_id: Some(black_player_id),
        move_count: 0,
    };
    
    let state_json = serde_json::to_string(&stored_state)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to serialize game state")))?;
    
    // Insert into database
    sqlx::query!(
        r#"INSERT INTO games (id, white_player_id, black_player_id, current_state, status, winner_id, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        game_id,
        white_player_id,
        black_player_id,
        state_json,
        crate::models::DbGameStatus(automatafl_api::GameStatus::Active),
        None::<uuid::Uuid>,
        now,
        now
    )
    .execute(&state.db)
    .await?;
    
    // Store in active games
    state.active_games.insert(game_id, std::sync::Arc::new(tokio::sync::Mutex::new(stored_state)));
    
    // Record metric
    crate::metrics::record_game_created();
    
    // Create response (simplified - in production you'd use game_to_response)
    Ok(GameStateResponse {
        id: game_id,
        white_player: None, // Would fetch these in production
        black_player: None,
        board: automatafl_api::BoardState {
            width: 11,
            height: 11,
            cells: vec![],
            automaton_position: automatafl_api::Position { x: 5, y: 5 },
        },
        round_state: "Fresh".to_string(),
        winner: None,
        current_player_turn: Some(0),
        spectator_count: 0,
    })
}

async fn create_matched_game_from_entries(
    state: &AppState,
    entry1: &crate::matchmaking::MatchmakingEntry,
    entry2: &crate::matchmaking::MatchmakingEntry,
) -> Result<uuid::Uuid> {
    // Similar to create_matched_game but with entries
    let game_id = uuid::Uuid::new_v4();
    
    // Determine who plays white
    let (white_player_id, black_player_id) = if entry1.rating > entry2.rating {
        (entry1.user_id, entry2.user_id)
    } else if entry1.rating < entry2.rating {
        (entry2.user_id, entry1.user_id)
    } else {
        if rand::random::<bool>() {
            (entry1.user_id, entry2.user_id)
        } else {
            (entry2.user_id, entry1.user_id)
        }
    };
    
    let now = chrono::Utc::now();
    
    // Create game logic
    let board = automatafl_logic::Board::stock_two_player();
    let mut game_logic = automatafl_logic::Game::new(board, 2, true);
    game_logic.goals.push((automatafl_logic::Coord { x: 5, y: 0 }, automatafl_logic::Pid(0)));
    game_logic.goals.push((automatafl_logic::Coord { x: 5, y: 10 }, automatafl_logic::Pid(1)));
    
    let stored_state = crate::models::StoredGameState {
        game: game_logic,
        white_player_id: Some(white_player_id),
        black_player_id: Some(black_player_id),
        move_count: 0,
    };
    
    let state_json = serde_json::to_string(&stored_state)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to serialize game state")))?;
    
    // Insert into database
    sqlx::query!(
        r#"INSERT INTO games (id, white_player_id, black_player_id, current_state, status, winner_id, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        game_id,
        white_player_id,
        black_player_id,
        state_json,
        crate::models::DbGameStatus(automatafl_api::GameStatus::Active),
        None::<uuid::Uuid>,
        now,
        now
    )
    .execute(&state.db)
    .await?;
    
    // Store in active games
    state.active_games.insert(game_id, std::sync::Arc::new(tokio::sync::Mutex::new(stored_state)));
    
    // Record metric
    crate::metrics::record_game_created();
    
    Ok(game_id)
}
