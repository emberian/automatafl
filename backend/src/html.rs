//! HTML frontend endpoints using Askama templates

use askama::Template;
use axum::{
    Form,
    extract::{Path, Query, State},
    http::{StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Redirect},
};
use std::collections::HashMap;
use surrealdb::RecordId;
use uuid::Uuid;

use crate::{
    common::{AppState, GameLifecycleState, ServerState, broadcast_event, timestamp},
    db::{self, PlayerRecord, PlayerStatsRecord, as_uuid},
    services::AuthServiceError,
    web::{self, AdminWebPlayer, HtmlTemplate, WebPlayer},
};
use automatafl_api_types::{
    GameLifecycle, GameListItem, GameStateResponse, JoinMatchmakingRequest,
};
use automatafl_logic::Pid;

// ============================================================================
// Session Cookie Helpers
// ============================================================================

// CSRF Protection: Implemented via middleware.rs using multiple layers:
// 1. SameSite=Strict cookies (primary defense)
// 2. Origin/Referer header validation
// 3. Content-Type validation for form submissions
// 4. Session cookie requirement for state-changing operations
// See: backend/src/middleware.rs::csrf_protection()

fn get_session_from_cookies(headers: &axum::http::HeaderMap) -> Option<Uuid> {
    web::session_id_from_headers(headers)
}

async fn get_player_from_session(
    state: &AppState,
    session_id: Uuid,
) -> Option<(Uuid, PlayerRecord)> {
    web::resolve_session_player(state, session_id).await
}

fn auth_error_message(err: &AuthServiceError) -> String {
    match err {
        AuthServiceError::InvalidCredentials => "Invalid username or password".to_string(),
        AuthServiceError::DisplaynameTaken(name) => {
            format!("Display name '{}' is already taken", name)
        }
        AuthServiceError::ValidationError(msg) => msg.clone(),
        AuthServiceError::PasswordHash(_) => "Password hashing failed".to_string(),
        AuthServiceError::Database(_) => "An authentication error occurred".to_string(),
    }
}

fn empty_stats(player_id: Uuid) -> PlayerStatsRecord {
    PlayerStatsRecord {
        player_id: RecordId::from_table_key("players", player_id),
        games_played: 0,
        games_won: 0,
        total_playtime: 0,
    }
}

fn set_session_cookie(session_id: Uuid) -> String {
    // FIXED: Secure flag conditional on environment (breaks HTTP in development)
    let secure_flag = if cfg!(debug_assertions) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict;{}; Max-Age={}",
        web::SESSION_COOKIE_NAME,
        session_id,
        secure_flag,
        60 * 60 * 24 * 7 // 7 days
    )
}

fn clear_session_cookie() -> String {
    // FIXED: Secure flag conditional on environment (breaks HTTP in development)
    let secure_flag = if cfg!(debug_assertions) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Strict;{}; Max-Age=0",
        web::SESSION_COOKIE_NAME,
        secure_flag
    )
}

// ============================================================================
// Askama Templates
// ============================================================================

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    #[allow(unused)]
    session_id: Option<String>,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    #[allow(unused)]
    session_id: Option<String>,
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "register.html")]
struct RegisterTemplate {
    #[allow(unused)]
    session_id: Option<String>,
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "games.html")]
struct GamesTemplate {
    #[allow(unused)]
    session_id: Option<String>,
    games: Vec<GameListItem>,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    displayname: String,
    player_id: Uuid,
    elo_rating: i32,
    stats: db::PlayerStatsRecord,
    recent_games: Vec<GameListItem>,
    is_admin: bool,
}

#[derive(Template)]
#[template(path = "profile.html")]
struct ProfileTemplate {
    player_id: Uuid,
    displayname: String,
    bio: Option<String>,
    avatar_url: Option<String>,
    elo_rating: i32,
    created_at: u64,
    stats: db::PlayerStatsRecord,
    is_own_profile: bool,
}

#[derive(Template)]
#[template(path = "game_detail.html")]
struct GameDetailTemplate {
    game_id: Uuid,
    game_state: GameStateResponse,
    player_names: Vec<(Pid, String)>,
    is_player: bool,
    player_pid: Option<Pid>,
    pending_move: Option<automatafl_logic::Move>,
    all_moves_ready: bool,
}

#[derive(Template)]
#[template(path = "create_game.html")]
struct CreateGameTemplate {
    displayname: String,
}

#[derive(Template)]
#[template(path = "matchmaking.html")]
struct MatchmakingTemplate {
    displayname: String,
    in_queue: bool,
}

#[derive(Template)]
#[template(path = "leaderboard.html")]
struct LeaderboardTemplate {
    leaderboard_type: String,
    entries: Vec<LeaderboardEntry>,
}

#[derive(Clone)]
struct LeaderboardEntry {
    rank: usize,
    displayname: String,
    value: i64,
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminTemplate {
    stats: AdminStats,
}

struct AdminStats {
    total_players: usize,
    total_games: usize,
    active_sessions: usize,
    queue_size: usize,
}

#[derive(Template)]
#[template(path = "admin_players.html")]
struct AdminPlayersTemplate {
    players: Vec<db::PlayerRecord>,
}

#[derive(Template)]
#[template(path = "admin_query.html")]
struct AdminQueryTemplate {
    query_text: String,
    result: Option<String>,
    error: Option<String>,
    timestamp: u64,
}

#[derive(Template)]
#[template(path = "admin_games.html")]
struct AdminGamesTemplate {
    games: Vec<GameSummary>,
    filter: String,
}

#[derive(Template)]
#[template(path = "admin_sessions.html")]
struct AdminSessionsTemplate {
    sessions: Vec<SessionInfo>,
}

#[derive(Template)]
#[template(path = "admin_queue.html")]
struct AdminQueueTemplate {
    queue: Vec<QueueEntry>,
}

#[derive(Template)]
#[template(path = "admin_events.html")]
struct AdminEventsTemplate {
    events: Vec<GameEventInfo>,
    game_id_filter: Option<String>,
}

// ============================================================================
// Form Types
// ============================================================================

#[derive(serde::Deserialize)]
pub struct LoginForm {
    displayname: String,
    password: String,
}

#[derive(serde::Deserialize)]
pub struct RegisterForm {
    displayname: String,
    password: String,
    password_confirm: String,
}

#[derive(serde::Deserialize)]
pub struct CreateGameForm {
    player_count: u8,
    use_column_rule: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct UpdateProfileForm {
    bio: Option<String>,
    avatar_url: Option<String>,
}

// ============================================================================
// Public Pages
// ============================================================================

pub async fn html_index(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Check if user is logged in
    if let Some(session_id) = get_session_from_cookies(&headers) {
        if let Some(_) = get_player_from_session(state.as_ref(), session_id).await {
            // Redirect to dashboard if logged in
            return Redirect::to("/dashboard").into_response();
        }
    }

    let template = IndexTemplate { session_id: None };
    HtmlTemplate(template).into_response()
}

pub async fn html_login_page() -> impl IntoResponse {
    let template = LoginTemplate {
        session_id: None,
        error: None,
    };
    HtmlTemplate(template).into_response()
}

pub async fn html_register_page() -> impl IntoResponse {
    let template = RegisterTemplate {
        session_id: None,
        error: None,
    };
    HtmlTemplate(template).into_response()
}

pub async fn html_login_submit(
    State(state): ServerState,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    match state
        .auth_service
        .login(form.displayname.clone(), form.password.clone())
        .await
    {
        Ok(login) => {
            let mut response = Redirect::to("/dashboard").into_response();
            if let Ok(cookie_value) = set_session_cookie(login.session_id).parse() {
                response.headers_mut().insert(SET_COOKIE, cookie_value);
            }
            response
        }
        Err(err) => {
            let template = LoginTemplate {
                session_id: None,
                error: Some(auth_error_message(&err)),
            };

            HtmlTemplate(template).into_response()
        }
    }
}

pub async fn html_register_submit(
    State(state): ServerState,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    // Validate passwords match
    if form.password != form.password_confirm {
        let template = RegisterTemplate {
            session_id: None,
            error: Some("Passwords do not match".to_string()),
        };
        return HtmlTemplate(template).into_response();
    }

    match state
        .auth_service
        .register(form.displayname.clone(), form.password.clone(), false)
        .await
    {
        Ok(_) => Redirect::to("/login").into_response(),
        Err(err) => {
            let template = RegisterTemplate {
                session_id: None,
                error: Some(auth_error_message(&err)),
            };

            HtmlTemplate(template).into_response()
        }
    }
}

pub async fn html_logout(State(state): ServerState, web_player: WebPlayer) -> impl IntoResponse {
    let session_id = web_player.session_id();
    let _ = state.auth_service.logout(session_id).await;

    let mut response = Redirect::to("/").into_response();
    if let Ok(cookie_value) = clear_session_cookie().parse() {
        response.headers_mut().insert(SET_COOKIE, cookie_value);
    }
    response
}

pub async fn html_games_list(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = get_session_from_cookies(&headers);
    let games = crate::game::list_games(State(state)).await.0;
    let template = GamesTemplate {
        session_id: session_id.map(|s| s.to_string()),
        games,
    };
    HtmlTemplate(template).into_response()
}

// ============================================================================
// Authenticated Pages
// ============================================================================

pub async fn html_dashboard(State(state): ServerState, web_player: WebPlayer) -> impl IntoResponse {
    let WebPlayer {
        player_id, player, ..
    } = web_player;

    let stats = match state.player_service.get_stats(player_id).await {
        Ok(Some(stats)) => stats,
        Ok(None) => empty_stats(player_id),
        Err(err) => {
            tracing::error!(player_id = %player_id, "Failed to load stats: {}", err);
            empty_stats(player_id)
        }
    };

    let recent_games = state
        .game_service
        .list_recent_games_for_player(player_id, 10)
        .await
        .unwrap_or_else(|err| {
            tracing::error!(player_id = %player_id, "Failed to list recent games: {}", err);
            Vec::new()
        });

    let template = DashboardTemplate {
        displayname: player.displayname,
        player_id,
        elo_rating: player.elo_rating,
        stats,
        recent_games,
        is_admin: player.is_admin,
    };

    HtmlTemplate(template).into_response()
}

pub async fn html_profile(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(player_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_player_id = if let Some(session_id) = get_session_from_cookies(&headers) {
        get_player_from_session(state.as_ref(), session_id)
            .await
            .map(|(id, _)| id)
    } else {
        None
    };

    let player = match state.player_service.get_player(player_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Player not found").into_response();
        }
        Err(err) => {
            tracing::error!(player_id = %player_id, "Failed to load player: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load player").into_response();
        }
    };

    let stats = match state.player_service.get_stats(player_id).await {
        Ok(Some(stats)) => stats,
        Ok(None) => empty_stats(player_id),
        Err(err) => {
            tracing::error!(player_id = %player_id, "Failed to load stats: {}", err);
            empty_stats(player_id)
        }
    };

    let template = ProfileTemplate {
        player_id,
        displayname: player.displayname,
        bio: player.bio,
        avatar_url: player.avatar_url,
        elo_rating: player.elo_rating,
        created_at: player.created_at,
        stats,
        is_own_profile: session_player_id == Some(player_id),
    };

    HtmlTemplate(template).into_response()
}

pub async fn html_update_profile(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(player_id): Path<Uuid>,
    Form(form): Form<UpdateProfileForm>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (session_player_id, _) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Only allow updating own profile
    if session_player_id != player_id {
        return Redirect::to(&format!("/profile/{}", player_id)).into_response();
    }

    let UpdateProfileForm { bio, avatar_url } = form;
    if let Err(err) = state
        .player_service
        .update_profile(player_id, bio, avatar_url)
        .await
    {
        tracing::error!(player_id = %player_id, "Failed to update profile: {}", err);
    }

    Redirect::to(&format!("/profile/{}", player_id)).into_response()
}

pub async fn html_game_detail(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_player_id = if let Some(session_id) = get_session_from_cookies(&headers) {
        get_player_from_session(state.as_ref(), session_id)
            .await
            .map(|(id, _)| id)
    } else {
        None
    };

    let (game_core, lifecycle, player_ids) = match state.game_service.load_game(game_id).await {
        Ok(state) => state,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Game not found").into_response();
        }
    };

    let game_state = GameStateResponse {
        lifecycle,
        game: game_core.clone(),
        player_ids: player_ids.clone(),
    };

    // Get player names
    let mut player_names = Vec::new();
    for (uuid, pid) in &player_ids {
        match state.player_service.get_player(*uuid).await {
            Ok(Some(player)) => player_names.push((*pid, player.displayname)),
            Ok(None) => {}
            Err(err) => {
                tracing::error!(player_id = %uuid, "Failed to resolve player for game detail: {}", err);
            }
        }
    }
    player_names.sort_by_key(|(pid, _)| pid.0);

    let is_player = session_player_id
        .map(|id| player_ids.contains_key(&id))
        .unwrap_or(false);
    let player_pid = session_player_id.and_then(|id| player_ids.get(&id).copied());

    // Get pending move if player is in game
    let pending_move = if let Some(pid) = player_pid {
        game_core
            .pending_moves
            .iter()
            .find(|m| m.who == pid)
            .cloned()
    } else {
        None
    };

    // Check if all moves are ready (all players have pending moves)
    let all_moves_ready = game_core.pending_moves.len() == player_ids.len();

    let template = GameDetailTemplate {
        game_id,
        game_state,
        player_names,
        is_player,
        player_pid,
        pending_move,
        all_moves_ready,
    };

    HtmlTemplate(template).into_response()
}

pub async fn html_create_game_page(
    State(_state): ServerState,
    web_player: WebPlayer,
) -> impl IntoResponse {
    let WebPlayer { player, .. } = web_player;
    let template = CreateGameTemplate {
        displayname: player.displayname,
    };

    HtmlTemplate(template).into_response()
}

pub async fn html_create_game_submit(
    State(state): ServerState,
    web_player: WebPlayer,
    Form(form): Form<CreateGameForm>,
) -> impl IntoResponse {
    let WebPlayer { player_id, .. } = web_player;

    let use_column_rule = form.use_column_rule.is_some();

    match state
        .game_service
        .create_game(player_id, form.player_count, use_column_rule)
        .await
    {
        Ok(game_id) => Redirect::to(&format!("/game/{}", game_id)).into_response(),
        Err(err) => {
            tracing::error!(player_id = %player_id, "Failed to create game: {}", err);
            Redirect::to("/create-game").into_response()
        }
    }
}

pub async fn html_matchmaking_page(
    State(state): ServerState,
    web_player: WebPlayer,
) -> impl IntoResponse {
    let WebPlayer {
        player_id, player, ..
    } = web_player;

    let in_queue = state
        .matchmaking_service
        .get_status(player_id)
        .await
        .ok()
        .flatten()
        .is_some();

    let template = MatchmakingTemplate {
        displayname: player.displayname,
        in_queue,
    };

    HtmlTemplate(template).into_response()
}

#[derive(serde::Deserialize)]
pub struct LeaderboardQuery {
    pub sort_by: Option<String>,
}

pub async fn html_leaderboard(
    State(state): ServerState,
    Query(query): Query<LeaderboardQuery>,
) -> impl IntoResponse {
    let sort_by = query.sort_by.as_deref().unwrap_or("elo");

    let (leaderboard_type, entries) = match sort_by {
        "wins" => match state.player_service.leaderboard_by_wins(100).await {
            Ok(ranked) => {
                let entries = ranked
                    .into_iter()
                    .map(|(player, stats, rank)| LeaderboardEntry {
                        rank,
                        displayname: player.displayname,
                        value: stats.games_won as i64,
                    })
                    .collect();
                ("Wins".to_string(), entries)
            }
            Err(err) => {
                tracing::error!("Failed to load wins leaderboard: {}", err);
                ("Wins".to_string(), Vec::new())
            }
        },
        "games" => match state.player_service.leaderboard_by_games(100).await {
            Ok(ranked) => {
                let entries = ranked
                    .into_iter()
                    .map(|(player, stats, rank)| LeaderboardEntry {
                        rank,
                        displayname: player.displayname,
                        value: stats.games_played as i64,
                    })
                    .collect();
                ("Games Played".to_string(), entries)
            }
            Err(err) => {
                tracing::error!("Failed to load games leaderboard: {}", err);
                ("Games Played".to_string(), Vec::new())
            }
        },
        _ => match state.player_service.leaderboard_by_elo(100).await {
            Ok(ranked) => {
                let entries = ranked
                    .into_iter()
                    .map(|(player, rank)| LeaderboardEntry {
                        rank,
                        displayname: player.displayname,
                        value: player.elo_rating as i64,
                    })
                    .collect();
                ("ELO Rating".to_string(), entries)
            }
            Err(err) => {
                tracing::error!("Failed to load ELO leaderboard: {}", err);
                ("ELO Rating".to_string(), Vec::new())
            }
        },
    };

    let template = LeaderboardTemplate {
        leaderboard_type,
        entries,
    };

    HtmlTemplate(template).into_response()
}

// ============================================================================
// Game Action Handlers
// ============================================================================

#[derive(serde::Deserialize)]
pub struct MoveForm {
    from_x: u8,
    from_y: u8,
    to_x: u8,
    to_y: u8,
}

pub async fn html_join_game(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    match state.game_service.join_game(game_id, player_id).await {
        Ok((_pid, events)) => {
            for event in events {
                if let Err(broadcast_err) = broadcast_event(state.as_ref(), game_id, event).await {
                    tracing::error!(
                        game_id = %game_id,
                        "Failed to broadcast join event: {}",
                        broadcast_err
                    );
                }
            }
        }
        Err(err) => {
            tracing::error!(player_id = %player_id, game_id = %game_id, "Failed to join game: {}", err);
        }
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

/// HTML form handler for submitting moves - REFACTORED to use service layer
pub async fn html_submit_move(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
    Form(move_form): Form<MoveForm>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Get player's PID in this game via service lookup
    let player_pid = match state.game_service.player_pid(game_id, player_id).await {
        Ok(pid) => pid,
        Err(err) => {
            tracing::error!(player_id = %player_id, game_id = %game_id, "Failed to resolve player PID: {}", err);
            return Redirect::to(&format!("/game/{}", game_id)).into_response();
        }
    };

    use automatafl_logic::Coord;
    let from = Coord {
        x: move_form.from_x,
        y: move_form.from_y,
    };
    let to = Coord {
        x: move_form.to_x,
        y: move_form.to_y,
    };

    // Get game start time for potential completion stats
    let game_start_time = state
        .game_service
        .get_game_created_at(game_id)
        .await
        .ok()
        .flatten();

    // Call service - ALL business logic is there
    match state
        .game_service
        .submit_move_and_maybe_complete(game_id, player_pid, from, to, game_start_time)
        .await
    {
        Ok((_response, events)) => {
            // Broadcast all events from service
            for event in events {
                if let Err(err) = broadcast_event(state.as_ref(), game_id, event).await {
                    tracing::error!(game_id = %game_id, "Failed to broadcast move event: {}", err);
                }
            }
        }
        Err(err) => {
            tracing::error!(player_id = %player_id, game_id = %game_id, "Failed to submit move: {}", err);
        }
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

pub async fn html_complete_round(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Verify player is in this game
    if let Err(err) = state.game_service.player_pid(game_id, player_id).await {
        tracing::error!(player_id = %player_id, game_id = %game_id, "Player not in game or game not found: {}", err);
        return Redirect::to(&format!("/game/{}", game_id)).into_response();
    }

    // Get game start time for potential completion stats
    let game_start_time = state
        .game_service
        .get_game_created_at(game_id)
        .await
        .ok()
        .flatten();

    // Complete the round
    match state
        .game_service
        .complete_round(game_id, game_start_time)
        .await
    {
        Ok((_response, events)) => {
            // Broadcast all events from service
            for event in events {
                if let Err(err) = broadcast_event(state.as_ref(), game_id, event).await {
                    tracing::error!(game_id = %game_id, "Failed to broadcast round completion event: {}", err);
                }
            }
        }
        Err(err) => {
            tracing::error!(game_id = %game_id, "Failed to complete round: {}", err);
        }
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

// ============================================================================
// Admin Pages
// ============================================================================

pub async fn html_admin_panel(
    State(state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let stats = match state.admin_service.dashboard_stats().await {
        Ok(snapshot) => AdminStats {
            total_players: snapshot.total_players,
            total_games: snapshot.total_games,
            active_sessions: snapshot.active_sessions,
            queue_size: snapshot.queue_size,
        },
        Err(err) => {
            tracing::error!("Failed to load admin dashboard stats: {}", err);
            AdminStats {
                total_players: 0,
                total_games: 0,
                active_sessions: 0,
                queue_size: 0,
            }
        }
    };

    let template = AdminTemplate { stats };
    HtmlTemplate(template).into_response()
}

pub async fn html_admin_players(
    State(state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let players = state
        .admin_service
        .list_players()
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to list players for admin view: {}", err);
            Vec::new()
        });
    let template = AdminPlayersTemplate { players };
    HtmlTemplate(template).into_response()
}

pub async fn html_admin_query_page(
    State(_state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let template = AdminQueryTemplate {
        query_text: String::new(),
        result: None,
        error: None,
        timestamp: timestamp(),
    };
    HtmlTemplate(template).into_response()
}

#[derive(serde::Deserialize)]
pub struct QueryForm {
    query: String,
}

pub async fn html_admin_query_execute(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Form(form): Form<QueryForm>,
) -> impl IntoResponse {
    // Execute query through admin service
    let query_result = state.admin_service.run_query(&form.query).await;

    let (result, error) = match query_result {
        Ok(mut response) => {
            // Try to extract results
            let results: Result<Vec<serde_json::Value>, _> = response.take(0);
            match results {
                Ok(data) => {
                    let json = serde_json::to_string_pretty(&data)
                        .unwrap_or_else(|_| "Failed to serialize".to_string());
                    (Some(json), None)
                }
                Err(e) => (None, Some(format!("Query error: {}", e))),
            }
        }
        Err(e) => (None, Some(format!("Database error: {}", e))),
    };

    let template = AdminQueryTemplate {
        query_text: form.query,
        result,
        error,
        timestamp: timestamp(),
    };
    HtmlTemplate(template).into_response()
}

pub async fn html_admin_cleanup_sessions(
    State(state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let now = timestamp();
    if let Err(err) = state.admin_service.cleanup_expired_sessions(now).await {
        tracing::error!("Failed to cleanup expired sessions: {}", err);
    }

    Redirect::to("/admin").into_response()
}

// Helper struct for session display
struct SessionInfo {
    session_id: String,
    player_name: String,
    created_at: u64,
    expires_at: u64,
    is_expired: bool,
}

pub async fn html_admin_sessions(
    State(state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let sessions = state
        .admin_service
        .list_sessions()
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to list sessions for admin view: {}", err);
            Vec::new()
        });

    let player_ids: Vec<Uuid> = sessions.iter().map(|s| as_uuid(&s.player_id)).collect();

    let player_lookup = state
        .admin_service
        .players_by_ids(&player_ids)
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to resolve player names for sessions: {}", err);
            HashMap::new()
        });

    let now = timestamp();
    let session_infos: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| {
            let session_id = as_uuid(&s.id);
            let player_id = as_uuid(&s.player_id);
            let player_name = player_lookup
                .get(&player_id)
                .map(|p| p.displayname.clone())
                .unwrap_or_else(|| player_id.to_string());

            SessionInfo {
                session_id: session_id.to_string(),
                player_name,
                created_at: 0, // Not available in current schema
                expires_at: s.expires_at,
                is_expired: s.expires_at < now,
            }
        })
        .collect();

    let template = AdminSessionsTemplate {
        sessions: session_infos,
    };
    HtmlTemplate(template).into_response()
}

pub async fn html_admin_delete_session(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Path(session_to_delete): Path<String>,
) -> impl IntoResponse {
    if let Ok(session_uuid) = Uuid::parse_str(&session_to_delete) {
        if let Err(err) = state.admin_service.delete_session(session_uuid).await {
            tracing::error!("Failed to delete session {}: {}", session_to_delete, err);
        }
    } else {
        tracing::warn!(
            "Invalid session id provided for deletion: {}",
            session_to_delete
        );
    }

    Redirect::to("/admin/sessions").into_response()
}

// Helper struct for game summary
struct GameSummary {
    id: String,
    lifecycle: GameLifecycle,
    player_count: usize,
    max_players: u8,
    round: String,
    created_at: u64,
}

pub async fn html_admin_games(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let filter = params.get("filter").map(|s| s.as_str()).unwrap_or("all");

    let mut game_records = match state.admin_service.list_game_records().await {
        Ok(records) => records,
        Err(err) => {
            tracing::error!("Failed to list games for admin view: {}", err);
            Vec::new()
        }
    };

    game_records.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let mut games: Vec<GameSummary> = vec![];
    for gs in game_records {
        // Parse lifecycle from string using the new enum
        let lifecycle_state = match GameLifecycleState::try_from(gs.lifecycle.clone()) {
            Ok(state) => state,
            Err(_) => continue, // Skip invalid lifecycle states
        };

        // Convert to the API type
        let lifecycle = match lifecycle_state {
            GameLifecycleState::Waiting => GameLifecycle::Waiting,
            GameLifecycleState::InProgress => GameLifecycle::InProgress,
            GameLifecycleState::Finished => GameLifecycle::Finished,
        };

        // Apply filter
        let matches_filter = match filter {
            "waiting" => matches!(lifecycle, GameLifecycle::Waiting),
            "in_progress" => matches!(lifecycle, GameLifecycle::InProgress),
            "finished" => matches!(lifecycle, GameLifecycle::Finished),
            _ => true,
        };

        if !matches_filter {
            continue;
        }

        // Decode the game from JSON
        let game: automatafl_logic::Game = match serde_json::from_str(&gs.game_state) {
            Ok(g) => g,
            Err(_) => continue,
        };

        // Count actual players from board - use player_count from game struct
        let actual_player_count = game.player_count as usize;

        games.push(GameSummary {
            id: as_uuid(&gs.id).to_string(),
            lifecycle,
            player_count: actual_player_count,
            max_players: gs.player_count,
            round: match game.round {
                automatafl_logic::RoundState::Fresh => "Fresh".to_string(),
                automatafl_logic::RoundState::PartiallySubmitted => "Partial".to_string(),
                automatafl_logic::RoundState::ResolvingConflict => "Resolving".to_string(),
                automatafl_logic::RoundState::GameOver => "Game Over".to_string(),
            },
            created_at: gs.created_at,
        });
    }

    let template = AdminGamesTemplate {
        games,
        filter: filter.to_string(),
    };

    HtmlTemplate(template).into_response()
}

pub async fn html_admin_delete_game(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Path(game_id): Path<String>,
) -> impl IntoResponse {
    match Uuid::parse_str(&game_id) {
        Ok(game_uuid) => {
            if let Err(err) = state.admin_service.delete_game(game_uuid).await {
                tracing::error!(game_id = %game_uuid, "Failed to delete game: {}", err);
            }
        }
        Err(_) => {
            tracing::warn!("Invalid game id provided for deletion: {}", game_id);
        }
    }

    Redirect::to("/admin/games").into_response()
}

pub async fn html_admin_force_complete(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Path(game_id): Path<String>,
) -> impl IntoResponse {
    // Parse game UUID
    let game_uuid = match game_id.parse::<uuid::Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return Redirect::to(&format!("/game/{}", game_id)).into_response(),
    };

    if let Err(err) = state.admin_service.force_complete_round(game_uuid).await {
        tracing::error!(game_id = %game_uuid, "Failed to force complete round: {}", err);
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

// Helper struct for queue entry
struct QueueEntry {
    player_id: String,
    displayname: String,
    elo_rating: i32,
    joined_at: u64,
    waiting_time: String,
}

pub async fn html_admin_queue(
    State(state): ServerState,
    _admin: AdminWebPlayer,
) -> impl IntoResponse {
    let queue_entries = state
        .admin_service
        .queue_entries()
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to fetch matchmaking queue for admin view: {}", err);
            Vec::new()
        });

    let mut queue = vec![];
    for entry in queue_entries {
        if let (Some(displayname), Some(elo_rating)) =
            (entry.player_displayname, entry.player_elo_rating)
        {
            queue.push(QueueEntry {
                player_id: entry.player_id.to_string(),
                displayname,
                elo_rating,
                joined_at: entry.queued_at,
                waiting_time: format!("{}s", entry.wait_time_seconds),
            });
        }
    }

    let template = AdminQueueTemplate { queue };
    HtmlTemplate(template).into_response()
}

pub async fn html_admin_remove_from_queue(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Path(player_id): Path<String>,
) -> impl IntoResponse {
    match Uuid::parse_str(&player_id) {
        Ok(uuid) => {
            if let Err(err) = state.admin_service.remove_from_queue(uuid).await {
                tracing::error!(player_id = %uuid, "Failed to remove player from queue: {}", err);
            }
        }
        Err(_) => {
            tracing::warn!(
                "Invalid player id provided for queue removal: {}",
                player_id
            );
        }
    }

    Redirect::to("/admin/queue").into_response()
}

// Helper struct for event display
struct GameEventInfo {
    timestamp: u64,
    game_id: String,
    event_type: String,
    details: String,
}

pub async fn html_admin_events(
    State(state): ServerState,
    _admin: AdminWebPlayer,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let game_id_filter = params.get("game_id").map(|s| s.to_string());

    let filter_uuid = match game_id_filter.as_ref() {
        Some(id_str) => match Uuid::parse_str(id_str) {
            Ok(uuid) => Some(uuid),
            Err(_) => {
                tracing::warn!("Invalid game id provided for events filter: {}", id_str);
                None
            }
        },
        None => None,
    };

    let events_records = state
        .admin_service
        .recent_game_events(filter_uuid, 100)
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to load game events for admin view: {}", err);
            Vec::new()
        });

    let events: Vec<GameEventInfo> = events_records
        .into_iter()
        .map(|ev| {
            let event_value = ev.event;
            let event_type = match serde_json::from_value::<automatafl_api_types::GameEventData>(
                event_value.clone(),
            ) {
                Ok(automatafl_api_types::GameEventData::PlayerJoined { .. }) => "PLAYER_JOINED",
                Ok(automatafl_api_types::GameEventData::GameStarted { .. }) => "GAME_STARTED",
                Ok(automatafl_api_types::GameEventData::MoveAcknowledged { .. }) => "MOVE_ACK",
                Ok(automatafl_api_types::GameEventData::MoveInvalid { .. }) => "MOVE_INVALID",
                Ok(automatafl_api_types::GameEventData::Move { .. }) => "MOVE",
                Ok(automatafl_api_types::GameEventData::AutomatonStep { .. }) => "AUTOMATON_STEP",
                Ok(automatafl_api_types::GameEventData::GameOver { .. }) => "GAME_OVER",
                Ok(automatafl_api_types::GameEventData::EloUpdate { .. }) => "ELO_UPDATE",
                Ok(automatafl_api_types::GameEventData::RoundComplete { .. }) => "ROUND_COMPLETE",
                Ok(automatafl_api_types::GameEventData::Conflicts { .. }) => "CONFLICTS",
                Ok(automatafl_api_types::GameEventData::Chat { .. }) => "CHAT",
                Ok(automatafl_api_types::GameEventData::GameLoaded { .. }) => "GAME_LOADED",
                Ok(automatafl_api_types::GameEventData::State { .. }) => "STATE",
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to deserialize game event for admin view");
                    "UNKNOWN"
                }
            };
            GameEventInfo {
                timestamp: ev.timestamp,
                game_id: as_uuid(&ev.game_id).to_string(),
                event_type: event_type.to_string(),
                details: serde_json::to_string_pretty(&event_value)
                    .unwrap_or_else(|_| "{}".to_string()),
            }
        })
        .collect();

    let template = AdminEventsTemplate {
        events,
        game_id_filter,
    };
    HtmlTemplate(template).into_response()
}

// Matchmaking HTML handlers
pub async fn html_matchmaking_join(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _player) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    let default_request = JoinMatchmakingRequest {
        player_count: 2,
        use_column_rule: false,
    };

    let preferences = serde_json::to_string(&default_request).unwrap_or_else(|_| "{}".to_string());

    if let Err(err) = state
        .matchmaking_service
        .join_queue(player_id, timestamp(), preferences)
        .await
    {
        tracing::error!(player_id = %player_id, "Failed to join matchmaking queue: {}", err);
    }

    Redirect::to("/matchmaking").into_response()
}

pub async fn html_matchmaking_leave(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _) = match get_player_from_session(state.as_ref(), session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if let Err(err) = state.matchmaking_service.leave_queue(player_id).await {
        tracing::error!(player_id = %player_id, "Failed to leave matchmaking queue: {}", err);
    }

    Redirect::to("/matchmaking").into_response()
}
