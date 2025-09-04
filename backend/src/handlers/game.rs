use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use automatafl_logic::{Board, Coord, Game as GameLogic, Move as GameMove, Pid};

use crate::{
    auth::AuthUser,
    error::{AppError, Result},
    models::{
        BoardState, CellState, CreateGameRequest, Game, GameStateResponse,
        GameStatus, DbGameStatus, Position, StoredGameState, SubmitMoveRequest, SubmitMoveResponse,
        UserInfo,
    },
    state::AppState,
};

pub async fn list_games(State(state): State<AppState>) -> Result<Json<Vec<GameStateResponse>>> {
    let games = sqlx::query_as!(
        Game,
        r#"SELECT id as "id!: Uuid", 
           white_player_id as "white_player_id?: Uuid", 
           black_player_id as "black_player_id?: Uuid", 
           current_state, 
           status as "status: DbGameStatus", 
           winner_id as "winner_id?: Uuid", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
           updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
           time_control_seconds as "time_control_seconds?: i32",
           white_time_remaining_ms as "white_time_remaining_ms?: i32",
           black_time_remaining_ms as "black_time_remaining_ms?: i32",
           last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
           FROM games 
           WHERE status IN ('waiting', 'active') 
           ORDER BY created_at DESC 
           LIMIT 50"#
    )
    .fetch_all(&state.db)
    .await?;

    let mut game_responses = Vec::new();

    for game in games {
        let response = game_to_response(&state, game).await?;
        game_responses.push(response);
    }

    Ok(Json(game_responses))
}

pub async fn create_game(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(_req): Json<CreateGameRequest>,
) -> Result<Json<GameStateResponse>> {
    let game_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // Create a new game with stock board
    let board = Board::stock_two_player();
    let mut game_logic = GameLogic::new(board, 2, true);
    
    // Set up goals for two players
    game_logic.goals.push((Coord { x: 5, y: 0 }, Pid(0))); // White player goal
    game_logic.goals.push((Coord { x: 5, y: 10 }, Pid(1))); // Black player goal

    let stored_state = StoredGameState {
        game: game_logic,
        white_player_id: Some(auth_user.id),
        black_player_id: None,
        move_count: 0,
    };

    let state_json = serde_json::to_string(&stored_state)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to serialize game state")))?;

    // Insert into database
    sqlx::query!(
        r#"INSERT INTO games (id, white_player_id, black_player_id, current_state, status, winner_id, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
        game_id,
        auth_user.id,
        None::<Uuid>,
        state_json,
        DbGameStatus(GameStatus::Waiting),
        None::<Uuid>,
        now,
        now
    )
    .execute(&state.db)
    .await?;

    // Store in active games
    state.active_games.insert(game_id, Arc::new(Mutex::new(stored_state)));

    let game = Game {
        id: game_id,
        white_player_id: Some(auth_user.id),
        black_player_id: None,
        current_state: state_json,
        status: DbGameStatus(GameStatus::Waiting),
        winner_id: None,
        created_at: now,
        updated_at: now,
        time_control_seconds: None,
        white_time_remaining_ms: None,
        black_time_remaining_ms: None,
        last_move_at: None,
    };

    let response = game_to_response(&state, game).await?;
    
    // Record metric
    crate::metrics::record_game_created();
    
    Ok(Json(response))
}

pub async fn get_game(
    State(state): State<AppState>,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameStateResponse>> {
    let game = sqlx::query_as!(
        Game,
        r#"SELECT id as "id!: Uuid", 
           white_player_id as "white_player_id?: Uuid", 
           black_player_id as "black_player_id?: Uuid", 
           current_state, 
           status as "status: DbGameStatus", 
           winner_id as "winner_id?: Uuid", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
           updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
           time_control_seconds as "time_control_seconds?: i32",
           white_time_remaining_ms as "white_time_remaining_ms?: i32",
           black_time_remaining_ms as "black_time_remaining_ms?: i32",
           last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
           FROM games WHERE id = ?"#,
        game_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    let response = game_to_response(&state, game).await?;
    Ok(Json(response))
}

pub async fn join_game(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameStateResponse>> {
    // Check if game exists and is waiting for players
    let mut game = sqlx::query_as!(
        Game,
        r#"SELECT id as "id!: Uuid", 
           white_player_id as "white_player_id?: Uuid", 
           black_player_id as "black_player_id?: Uuid", 
           current_state, 
           status as "status: DbGameStatus", 
           winner_id as "winner_id?: Uuid", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
           updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
           time_control_seconds as "time_control_seconds?: i32",
           white_time_remaining_ms as "white_time_remaining_ms?: i32",
           black_time_remaining_ms as "black_time_remaining_ms?: i32",
           last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
           FROM games WHERE id = ?"#,
        game_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    // Validate game state
    match game.status {
        DbGameStatus(GameStatus::Waiting) => {}
        _ => return Err(AppError::Game("Game is not accepting new players".to_string())),
    }

    // Check if user is already in the game
    if game.white_player_id == Some(auth_user.id) || game.black_player_id == Some(auth_user.id) {
        return Err(AppError::Game("You are already in this game".to_string()));
    }

    // Join as black player
    if game.black_player_id.is_none() {
        game.black_player_id = Some(auth_user.id);
        game.status = DbGameStatus(GameStatus::Active);
        
        // Update stored state
        let mut stored_state: StoredGameState = serde_json::from_str(&game.current_state)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to parse game state")))?;
        stored_state.black_player_id = Some(auth_user.id);
        
        let state_json = serde_json::to_string(&stored_state)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to serialize game state")))?;
        
        // Update database
        let now = chrono::Utc::now();
        sqlx::query!(
            "UPDATE games SET black_player_id = ?, status = ?, current_state = ?, updated_at = ? WHERE id = ?",
            auth_user.id,
            DbGameStatus(GameStatus::Active),
            state_json,
            now,
            game_id
        )
        .execute(&state.db)
        .await?;
        
        game.current_state = state_json;
        
        // Update active games
        state.active_games.insert(game_id, Arc::new(Mutex::new(stored_state)));
    } else {
        return Err(AppError::Game("Game is full".to_string()));
    }

    let response = game_to_response(&state, game).await?;
    Ok(Json(response))
}

pub async fn submit_move(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(game_id): Path<Uuid>,
    Json(req): Json<SubmitMoveRequest>,
) -> Result<Json<SubmitMoveResponse>> {
    // Get game from active games or load from database
    let game_state_mutex = if let Some(entry) = state.active_games.get(&game_id) {
        entry.clone()
    } else {
        let game = sqlx::query_as!(
            Game,
            r#"SELECT id as "id!: Uuid", 
               white_player_id as "white_player_id?: Uuid", 
               black_player_id as "black_player_id?: Uuid", 
               current_state, 
               status as "status: DbGameStatus", 
               winner_id as "winner_id?: Uuid", 
               created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
               updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
               time_control_seconds as "time_control_seconds?: i32",
               white_time_remaining_ms as "white_time_remaining_ms?: i32",
               black_time_remaining_ms as "black_time_remaining_ms?: i32",
               last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
               FROM games WHERE id = ?"#,
            game_id
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

        if game.status != DbGameStatus(GameStatus::Active) {
            return Err(AppError::Game("Game is not active".to_string()));
        }

        let stored_state: StoredGameState = serde_json::from_str(&game.current_state)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to parse game state")))?;
        
        let mutex = Arc::new(Mutex::new(stored_state));
        state.active_games.insert(game_id, mutex.clone());
        mutex
    };

    // Lock the game state for modification
    let mut stored_state = game_state_mutex.lock().await;

    // Determine player ID (0 for white, 1 for black)
    let player_pid = if stored_state.white_player_id == Some(auth_user.id) {
        Pid(0)
    } else if stored_state.black_player_id == Some(auth_user.id) {
        Pid(1)
    } else {
        return Err(AppError::Forbidden("You are not a player in this game".to_string()));
    };

    // Create the move
    let game_move = GameMove {
        who: player_pid,
        from: Coord { x: req.from_x, y: req.from_y },
        to: Coord { x: req.to_x, y: req.to_y },
    };

    // Submit the move
    let (feedback, ready_to_complete) = stored_state.game.propose_move(game_move);
    
    // Convert feedback to string for response
    let feedback_str = format!("{}", feedback);

    // If all players have submitted moves, resolve the round
    if ready_to_complete {
        match stored_state.game.try_complete_round() {
            Ok(results) => {
                // Log the move results
                for (mv, result) in results {
                    tracing::info!("Move {:?} -> {:?}", mv, result);
                }
            }
            Err(_) => {
                // Conflict resolution needed
                tracing::info!("Entering conflict resolution");
            }
        }
    }

    // Increment move count
    stored_state.move_count += 1;

    // Save move to database
    let move_id = Uuid::new_v4();
    let move_number = stored_state.move_count as i32;
    let from_x = req.from_x as i32;
    let from_y = req.from_y as i32;
    let to_x = req.to_x as i32;
    let to_y = req.to_y as i32;
    let now = chrono::Utc::now();
    
    sqlx::query!(
        r#"INSERT INTO game_moves (id, game_id, player_id, move_number, from_x, from_y, to_x, to_y, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        move_id,
        game_id,
        auth_user.id,
        move_number,
        from_x,
        from_y,
        to_x,
        to_y,
        now
    )
    .execute(&state.db)
    .await?;

    // Update game state
    let state_json = serde_json::to_string(&*stored_state)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to serialize game state")))?;

    let game_status = if stored_state.game.winner.is_some() {
        DbGameStatus(GameStatus::Completed)
    } else {
        DbGameStatus(GameStatus::Active)
    };

    let winner_id = stored_state.game.winner.map(|pid| {
        if pid.0 == 0 {
            stored_state.white_player_id.unwrap()
        } else {
            stored_state.black_player_id.unwrap()
        }
    });

    let now = chrono::Utc::now();
    sqlx::query!(
        "UPDATE games SET current_state = ?, status = ?, winner_id = ?, updated_at = ? WHERE id = ?",
        state_json,
        game_status,
        winner_id,
        now,
        game_id
    )
    .execute(&state.db)
    .await?;

    // The game state is already updated through the mutex
    
    // Create response
    let game = Game {
        id: game_id,
        white_player_id: stored_state.white_player_id,
        black_player_id: stored_state.black_player_id,
        current_state: state_json,
        status: game_status,
        winner_id,
        created_at: chrono::Utc::now(), // These don't matter for the response
        updated_at: chrono::Utc::now(),
        time_control_seconds: None,
        white_time_remaining_ms: None,
        black_time_remaining_ms: None,
        last_move_at: Some(chrono::Utc::now()),
    };

    let game_state_response = game_to_response(&state, game).await?;
    
    // Update last move time (for time controls and abandonment detection)
    if let Err(e) = crate::abandonment::update_last_move_time(&state, game_id, auth_user.id).await {
        tracing::error!("Failed to update last move time: {:?}", e);
    }
    
    // Record metrics
    crate::metrics::record_move_submitted();
    if game_status == DbGameStatus(GameStatus::Completed) {
        crate::metrics::record_game_completed();
    }
    
    // Broadcast game update to all connected clients
    crate::handlers::websocket::broadcast_game_update(&state, game_id, game_state_response.clone()).await;

    Ok(Json(SubmitMoveResponse {
        feedback: feedback_str,
        game_state: game_state_response,
    }))
}

pub async fn spectate_game(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(game_id): Path<Uuid>,
) -> Result<Json<GameStateResponse>> {
    // Check if game exists
    let game = sqlx::query_as!(
        Game,
        r#"SELECT id as "id!: Uuid", 
           white_player_id as "white_player_id?: Uuid", 
           black_player_id as "black_player_id?: Uuid", 
           current_state, 
           status as "status: DbGameStatus", 
           winner_id as "winner_id?: Uuid", 
           created_at as "created_at!: chrono::DateTime<chrono::Utc>", 
           updated_at as "updated_at!: chrono::DateTime<chrono::Utc>",
           time_control_seconds as "time_control_seconds?: i32",
           white_time_remaining_ms as "white_time_remaining_ms?: i32",
           black_time_remaining_ms as "black_time_remaining_ms?: i32",
           last_move_at as "last_move_at?: chrono::DateTime<chrono::Utc>"
           FROM games WHERE id = ?"#,
        game_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    // Add spectator if not already spectating or playing
    if game.white_player_id != Some(auth_user.id) && game.black_player_id != Some(auth_user.id) {
        let existing = sqlx::query!(
            "SELECT game_id FROM spectators WHERE game_id = ? AND user_id = ?",
            game_id,
            auth_user.id
        )
        .fetch_optional(&state.db)
        .await?;

        if existing.is_none() {
            let now = chrono::Utc::now();
            sqlx::query!(
                "INSERT INTO spectators (game_id, user_id, joined_at) VALUES (?, ?, ?)",
                game_id,
                auth_user.id,
                now
            )
            .execute(&state.db)
            .await?;
        }
    }

    let response = game_to_response(&state, game).await?;
    Ok(Json(response))
}

// Helper function to convert database game to API response
async fn game_to_response(state: &AppState, game: Game) -> Result<GameStateResponse> {
    let stored_state: StoredGameState = serde_json::from_str(&game.current_state)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Failed to parse game state")))?;

    // Get player information
    let white_player = if let Some(id) = game.white_player_id {
        sqlx::query!(r#"SELECT id as "id!: Uuid", username, rating FROM users WHERE id = ?"#, id)
            .fetch_optional(&state.db)
            .await?
            .map(|u| UserInfo {
                id: u.id,
                username: u.username,
                rating: u.rating as i32,
                created_at: None,
            })
    } else {
        None
    };

    let black_player = if let Some(id) = game.black_player_id {
        sqlx::query!(r#"SELECT id as "id!: Uuid", username, rating FROM users WHERE id = ?"#, id)
            .fetch_optional(&state.db)
            .await?
            .map(|u| UserInfo {
                id: u.id,
                username: u.username,
                rating: u.rating as i32,
                created_at: None,
            })
    } else {
        None
    };

    // Get spectator count
    let spectator_count = sqlx::query!(r#"SELECT COUNT(*) as "count!: i64" FROM spectators WHERE game_id = ?"#, game.id)
        .fetch_one(&state.db)
        .await?
        .count as usize;

    // Convert board state
    let board = &stored_state.game.board;
    let mut cells = vec![vec![CellState {
        particle: None,
        is_goal: false,
        goal_player: None,
    }; board.size.x as usize]; board.size.y as usize];

    // Fill in the cells
    for y in 0..board.size.y {
        for x in 0..board.size.x {
            let coord = Coord { x, y };
            let cell = board.particles[coord.ix()];
            
            // Check if this is a goal
            let (is_goal, goal_player) = stored_state.game.goals.iter()
                .find(|(c, _)| c == &coord)
                .map(|(_, pid)| (true, Some(pid.0)))
                .unwrap_or((false, None));

            cells[y as usize][x as usize] = CellState {
                particle: cell.what.into(),
                is_goal,
                goal_player,
            };
        }
    }

    // Determine whose turn it is
    let current_player_turn = match stored_state.game.round {
        automatafl_logic::RoundState::Fresh | automatafl_logic::RoundState::PartiallySubmitted => {
            // Check who hasn't moved yet
            if stored_state.game.pending_moves.is_empty() {
                Some(0) // White moves first
            } else {
                // Find who hasn't moved
                let moved_players: Vec<_> = stored_state.game.pending_moves.iter().map(|m| m.who.0).collect();
                if !moved_players.contains(&0) {
                    Some(0)
                } else if !moved_players.contains(&1) {
                    Some(1)
                } else {
                    None
                }
            }
        }
        _ => None,
    };

    Ok(GameStateResponse {
        id: game.id,
        white_player,
        black_player,
        board: BoardState {
            width: board.size.x,
            height: board.size.y,
            cells,
            automaton_position: Position {
                x: board.automaton_location.x,
                y: board.automaton_location.y,
            },
        },
        round_state: format!("{:?}", stored_state.game.round),
        winner: game.winner_id,
        current_player_turn,
        spectator_count,
    })
}
