//! HTML frontend endpoints using Askama templates

use askama::Template;
use axum::{
    Form,
    extract::{Path, Query, State},
    http::{
        StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
    response::{IntoResponse, Redirect},
};
use surrealdb::RecordId;
use uuid::Uuid;

use crate::db;
use crate::{
    common::{ServerState, broadcast_event, timestamp},
    db::as_uuid,
};
use automatafl_api_types::{GameEventData, GameLifecycle, GameListItem, GameStateResponse};
use automatafl_logic::Pid;

// ============================================================================
// Session Cookie Helpers
// ============================================================================

const SESSION_COOKIE_NAME: &str = "automatafl_session";

fn get_session_from_cookies(headers: &axum::http::HeaderMap) -> Option<Uuid> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|cookie| {
            let cookie = cookie.trim();
            if let Some(value) = cookie.strip_prefix(&format!("{}=", SESSION_COOKIE_NAME)) {
                Uuid::parse_str(value).ok()
            } else {
                None
            }
        })
}

async fn get_player_from_session(
    db: &db::Db,
    session_id: Uuid,
) -> Option<(Uuid, db::PlayerRecord)> {
    let session = db::get_session(db, session_id).await.ok()??;

    // Check expiration
    if session.expires_at < timestamp() {
        return None;
    }

    let player_id = as_uuid(&session.player_id);
    let player = db::get_player(db, player_id).await.ok()??;
    Some((player_id, player))
}

fn set_session_cookie(session_id: Uuid) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age={}",
        SESSION_COOKIE_NAME,
        session_id,
        60 * 60 * 24 * 7 // 7 days
    )
}

fn clear_session_cookie() -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Strict; Secure; Max-Age=0",
        SESSION_COOKIE_NAME
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
        if let Some(_) = get_player_from_session(&state.db, session_id).await {
            // Redirect to dashboard if logged in
            return Redirect::to("/dashboard").into_response();
        }
    }

    let template = IndexTemplate { session_id: None };
    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_login_page() -> impl IntoResponse {
    let template = LoginTemplate {
        session_id: None,
        error: None,
    };
    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_register_page() -> impl IntoResponse {
    let template = RegisterTemplate {
        session_id: None,
        error: None,
    };
    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_login_submit(
    State(state): ServerState,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    // Find player
    match db::find_player_by_displayname(&state.db, form.displayname.clone()).await {
        Ok(Some(player)) => {
            // Verify password
            use argon2::{
                Argon2,
                password_hash::{PasswordHash, PasswordVerifier},
            };

            if let Ok(parsed_hash) = PasswordHash::new(&player.password_hash) {
                if Argon2::default()
                    .verify_password(form.password.as_bytes(), &parsed_hash)
                    .is_ok()
                {
                    // Create session
                    let player_id = db::as_uuid(&player.id);
                    let session_id = Uuid::new_v4();
                    let expires_at = timestamp() + state.config.session_duration;

                    if db::create_session(&state.db, session_id, player_id, expires_at)
                        .await
                        .is_ok()
                    {
                        let mut response = Redirect::to("/dashboard").into_response();
                        response
                            .headers_mut()
                            .insert(SET_COOKIE, set_session_cookie(session_id).parse().unwrap());
                        return response;
                    }
                }
            }
        }
        _ => {}
    }

    // Failed login
    let template = LoginTemplate {
        session_id: None,
        error: Some("Invalid username or password".to_string()),
    };
    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
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
        return match template.render() {
            Ok(html) => axum::response::Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", err),
            )
                .into_response(),
        };
    }

    // Validate username length
    if form.displayname.len() < 3 {
        let template = RegisterTemplate {
            session_id: None,
            error: Some("Username must be at least 3 characters".to_string()),
        };
        return match template.render() {
            Ok(html) => axum::response::Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", err),
            )
                .into_response(),
        };
    }

    // Check if username is taken
    match db::find_player_by_displayname(&state.db, form.displayname.clone()).await {
        Ok(Some(_)) => {
            let template = RegisterTemplate {
                session_id: None,
                error: Some("Username already taken".to_string()),
            };
            return match template.render() {
                Ok(html) => axum::response::Html(html).into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Template error: {}", err),
                )
                    .into_response(),
            };
        }
        _ => {}
    }

    // Hash password with argon2
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = match argon2.hash_password(form.password.as_bytes(), &salt) {
        Ok(hash) => hash.to_string(),
        Err(_) => {
            let template = RegisterTemplate {
                session_id: None,
                error: Some("Failed to hash password".to_string()),
            };
            return match template.render() {
                Ok(html) => axum::response::Html(html).into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Template error: {}", err),
                )
                    .into_response(),
            };
        }
    };

    // Create player
    let new_uuid = Uuid::new_v4();
    match db::create_player(&state.db, new_uuid, form.displayname, password_hash, false).await {
        Ok(_) => Redirect::to("/login").into_response(),
        Err(e) => {
            let template = RegisterTemplate {
                session_id: None,
                error: Some(format!("Failed to create account: {}", e)),
            };
            match template.render() {
                Ok(html) => axum::response::Html(html).into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Template error: {}", err),
                )
                    .into_response(),
            }
        }
    }
}

pub async fn html_logout(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(session_id) = get_session_from_cookies(&headers) {
        let _ = db::delete_session(&state.db, session_id).await;
    }

    let mut response = Redirect::to("/").into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, clear_session_cookie().parse().unwrap());
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
    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

// ============================================================================
// Authenticated Pages
// ============================================================================

pub async fn html_dashboard(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    let stats = db::get_player_stats(&state.db, player_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(db::PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        });

    // Get recent games (games the player is in)
    let all_games = db::list_games(&state.db).await.unwrap_or_default();
    let mut recent_games = Vec::new();
    for game_record in all_games.into_iter().take(10) {
        let game_id = as_uuid(&game_record.id);
        if let Ok(game_players) = db::get_game_players(&state.db, game_id).await {
            if game_players
                .iter()
                .any(|gp| as_uuid(&gp.player_id) == player_id)
            {
                if let (Ok(lifecycle), created_by) = (
                    serde_json::from_str(&game_record.lifecycle),
                    as_uuid(&game_record.created_by),
                ) {
                    recent_games.push(GameListItem {
                        id: game_id,
                        lifecycle,
                        player_count: game_players.len(),
                        max_players: game_record.player_count,
                        created_at: game_record.created_at,
                        created_by,
                    });
                }
            }
        }
    }

    let template = DashboardTemplate {
        displayname: player.displayname,
        player_id,
        elo_rating: player.elo_rating,
        stats,
        recent_games,
        is_admin: player.is_admin,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_profile(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(player_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_player_id = if let Some(session_id) = get_session_from_cookies(&headers) {
        get_player_from_session(&state.db, session_id)
            .await
            .map(|(id, _)| id)
    } else {
        None
    };

    let player = match db::get_player(&state.db, player_id).await {
        Ok(Some(p)) => p,
        _ => {
            return (StatusCode::NOT_FOUND, "Player not found").into_response();
        }
    };

    let stats = db::get_player_stats(&state.db, player_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(db::PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        });

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

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
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

    let (session_player_id, _) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Only allow updating own profile
    if session_player_id != player_id {
        return Redirect::to(&format!("/profile/{}", player_id)).into_response();
    }

    let _ = db::update_player_profile(&state.db, player_id, form.bio, form.avatar_url).await;

    Redirect::to(&format!("/profile/{}", player_id)).into_response()
}

pub async fn html_game_detail(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
) -> impl IntoResponse {
    let session_player_id = if let Some(session_id) = get_session_from_cookies(&headers) {
        get_player_from_session(&state.db, session_id)
            .await
            .map(|(id, _)| id)
    } else {
        None
    };

    let (game_core, lifecycle, player_ids) = match db::load_game_state(&state.db, game_id).await {
        Ok(Some(state)) => state,
        _ => {
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
        if let Ok(Some(player)) = db::get_player(&state.db, *uuid).await {
            player_names.push((*pid, player.displayname));
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

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_create_game_page(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    let template = CreateGameTemplate {
        displayname: player.displayname,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_create_game_submit(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Form(form): Form<CreateGameForm>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, _) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    use automatafl_logic::Board;
    let board = Board::stock_two_player();
    let use_column_rule = form.use_column_rule.is_some();
    let mut game_core = automatafl_logic::Game::new(board, form.player_count, use_column_rule);

    // Set up goals for two-player game
    if form.player_count == 2 {
        use automatafl_logic::Coord;
        game_core.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 10, y: 0 }, Pid(0)));
        game_core.goals.push((Coord { x: 0, y: 10 }, Pid(1)));
        game_core.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
    }

    let game_id = Uuid::new_v4();
    let lifecycle = GameLifecycle::Waiting;

    if db::create_game(
        &state.db,
        game_id,
        &game_core,
        &lifecycle,
        player_id,
        form.player_count,
    )
    .await
    .is_ok()
    {
        // Auto-join the creator
        let _ = db::add_player_to_game(&state.db, game_id, player_id, Pid(0)).await;
        Redirect::to(&format!("/game/{}", game_id)).into_response()
    } else {
        Redirect::to("/create-game").into_response()
    }
}

pub async fn html_matchmaking_page(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (player_id, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    let in_queue = db::get_matchmaking_status(&state.db, player_id)
        .await
        .ok()
        .flatten()
        .is_some();

    let template = MatchmakingTemplate {
        displayname: player.displayname,
        in_queue,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
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
        "wins" => {
            let ranked = db::get_leaderboard_by_wins(&state.db, 100)
                .await
                .unwrap_or_default();
            let entries: Vec<LeaderboardEntry> = ranked
                .into_iter()
                .map(|(player, stats, rank)| LeaderboardEntry {
                    rank,
                    displayname: player.displayname,
                    value: stats.games_won as i64,
                })
                .collect();
            ("Wins".to_string(), entries)
        }
        "games" => {
            let ranked = db::get_leaderboard_by_games(&state.db, 100)
                .await
                .unwrap_or_default();
            let entries: Vec<LeaderboardEntry> = ranked
                .into_iter()
                .map(|(player, stats, rank)| LeaderboardEntry {
                    rank,
                    displayname: player.displayname,
                    value: stats.games_played as i64,
                })
                .collect();
            ("Games Played".to_string(), entries)
        }
        _ => {
            let ranked = db::get_leaderboard_by_elo(&state.db, 100)
                .await
                .unwrap_or_default();
            let entries: Vec<LeaderboardEntry> = ranked
                .into_iter()
                .map(|(player, rank)| LeaderboardEntry {
                    rank,
                    displayname: player.displayname,
                    value: player.elo_rating as i64,
                })
                .collect();
            ("ELO Rating".to_string(), entries)
        }
    };

    let template = LeaderboardTemplate {
        leaderboard_type,
        entries,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
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

    let (player_id, _) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Get game and find next available PID
    let game_players = db::get_game_players(&state.db, game_id)
        .await
        .unwrap_or_default();
    let next_pid = Pid(game_players.len() as u8);

    // Add player to game
    if db::add_player_to_game(&state.db, game_id, player_id, next_pid)
        .await
        .is_ok()
    {
        // Broadcast player joined event
        if let Ok(Some(player)) = db::get_player(&state.db, player_id).await {
            let _ = crate::common::broadcast_event(
                &state,
                game_id,
                GameEventData::PlayerJoined {
                    player_id,
                    player_pid: next_pid,
                    displayname: player.displayname,
                },
            )
            .await;
        }

        // Check if game should start
        if let Ok(Some(game_record)) = db::get_game(&state.db, game_id).await {
            let player_count = game_players.len() + 1;
            if player_count == game_record.player_count as usize {
                // Start game - update lifecycle in game record
                if let Ok(Some((game_core, _, _player_ids))) =
                    db::load_game_state(&state.db, game_id).await
                {
                    let _ = db::update_game_state(
                        &state.db,
                        game_id,
                        &game_core,
                        &GameLifecycle::InProgress,
                    )
                    .await;
                    let _ = broadcast_event(&state, game_id, GameEventData::GameStarted).await;
                }
            }
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

    let (player_id, _) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Get player's PID in this game
    let game_players = db::get_game_players(&state.db, game_id)
        .await
        .unwrap_or_default();
    let player_pid = match game_players
        .iter()
        .find(|gp| as_uuid(&gp.player_id) == player_id)
    {
        Some(gp) => Pid(gp.player_pid),
        None => return Redirect::to(&format!("/game/{}", game_id)).into_response(),
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
    let game_start_time = db::get_game(&state.db, game_id)
        .await
        .ok()
        .flatten()
        .map(|g| g.created_at);

    // Call service - ALL business logic is there
    if let Ok((_response, events)) = state
        .game_service
        .submit_move_and_maybe_complete(game_id, player_pid, from, to, game_start_time)
        .await
    {
        // Broadcast all events from service
        for event in events {
            let _ = broadcast_event(&state, game_id, event).await;
        }
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

pub async fn html_complete_round(
    State(_state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<Uuid>,
) -> impl IntoResponse {
    // This is now handled automatically in html_submit_move
    // Just redirect back to the game page
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    // Verify they have a session
    if session_id.is_nil() {
        return Redirect::to("/login").into_response();
    }

    Redirect::to(&format!("/game/{}", game_id)).into_response()
}

// ============================================================================
// Admin Pages
// ============================================================================

pub async fn html_admin_panel(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Gather stats
    let players: Vec<db::PlayerRecord> = state.db.select("players").await.unwrap_or_default();
    let games: Vec<db::GameRecord> = state.db.select("games").await.unwrap_or_default();
    let sessions: Vec<db::SessionRecord> = state.db.select("sessions").await.unwrap_or_default();
    let queue: Vec<db::MatchmakingQueueRecord> = state
        .db
        .select("matchmaking_queue")
        .await
        .unwrap_or_default();

    let now = timestamp();
    let active_sessions = sessions.iter().filter(|s| s.expires_at > now).count();

    let stats = AdminStats {
        total_players: players.len(),
        total_games: games.len(),
        active_sessions,
        queue_size: queue.len(),
    };

    let template = AdminTemplate { stats };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_players(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let players: Vec<db::PlayerRecord> = state.db.select("players").await.unwrap_or_default();
    let template = AdminPlayersTemplate { players };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_query_page(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let template = AdminQueryTemplate {
        query_text: String::new(),
        result: None,
        error: None,
        timestamp: timestamp(),
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct QueryForm {
    query: String,
}

pub async fn html_admin_query_execute(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Form(form): Form<QueryForm>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Execute query
    let query_result: Result<surrealdb::Response, surrealdb::Error> =
        state.db.query(&form.query).await;

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

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_cleanup_sessions(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Delete expired sessions
    let now = timestamp();
    let _: Result<surrealdb::Response, _> = state
        .db
        .query("DELETE FROM sessions WHERE expires_at < $expires_at")
        .bind(("expires_at", now))
        .await;

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
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Get all sessions from database
    let sessions_result: Result<Vec<db::SessionRecord>, _> = state
        .db
        .query("SELECT * FROM sessions ORDER BY created_at DESC")
        .await
        .and_then(|mut resp| resp.take(0));

    let sessions = match sessions_result {
        Ok(sessions) => sessions,
        Err(_) => vec![],
    };

    let now = timestamp();
    let session_infos: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| {
            SessionInfo {
                session_id: as_uuid(&s.id).to_string(),
                player_name: as_uuid(&s.player_id).to_string(),
                created_at: 0, // Not available in current schema
                expires_at: s.expires_at,
                is_expired: s.expires_at < now,
            }
        })
        .collect();

    let template = AdminSessionsTemplate {
        sessions: session_infos,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_delete_session(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(session_to_delete): Path<String>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let _: Result<Option<db::SessionRecord>, _> =
        state.db.delete(("sessions", session_to_delete)).await;

    Redirect::to("/admin/sessions").into_response()
}

// Helper struct for game summary
struct GameSummary {
    id: String,
    lifecycle: GameLifecycle,
    player_count: usize,
    max_players: u8,
    round: u32,
    created_at: u64,
}

pub async fn html_admin_games(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let filter = params.get("filter").map(|s| s.as_str()).unwrap_or("all");

    // Get all game states
    let game_states_result: Result<Vec<db::GameRecord>, _> = state
        .db
        .query("SELECT * FROM games ORDER BY created_at DESC")
        .await
        .and_then(|mut resp| resp.take(0));

    let game_states = match game_states_result {
        Ok(states) => states,
        Err(_) => vec![],
    };

    let mut games: Vec<GameSummary> = vec![];
    for gs in game_states {
        // Parse lifecycle from string
        let lifecycle = match gs.lifecycle.as_str() {
            "Waiting" => GameLifecycle::Waiting,
            "InProgress" => GameLifecycle::InProgress,
            "Finished" => GameLifecycle::Finished,
            _ => continue,
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
            id: gs.id.key().to_string(),
            lifecycle,
            player_count: actual_player_count,
            max_players: gs.player_count,
            round: match game.round {
                automatafl_logic::RoundState::Fresh => 0,
                automatafl_logic::RoundState::PartiallySubmitted => 1,
                automatafl_logic::RoundState::ResolvingConflict => 1,
                automatafl_logic::RoundState::GameOver => 999,
            },
            created_at: gs.created_at,
        });
    }

    let template = AdminGamesTemplate {
        games,
        filter: filter.to_string(),
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_delete_game(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<String>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let _: Result<Option<db::GameRecord>, _> = state.db.delete(("games", game_id)).await;

    Redirect::to("/admin/games").into_response()
}

pub async fn html_admin_force_complete(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(game_id): Path<String>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Parse game UUID
    let game_uuid = match game_id.parse::<uuid::Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => return Redirect::to(&format!("/game/{}", game_id)).into_response(),
    };

    // Load and force complete the round
    let result = db::load_game_state(&state.db, game_uuid).await;
    if let Ok(Some((mut game_core, lifecycle, _player_ids))) = result {
        if matches!(lifecycle, GameLifecycle::InProgress) {
            // Force complete by trying to complete the round
            if let Ok(_) = game_core.try_complete_round() {
                // Save updated game state
                let game_json = serde_json::to_string(&game_core).unwrap_or_default();
                let _: Result<Option<db::GameRecord>, _> = state
                    .db
                    .query("UPDATE games SET game_state = $state WHERE id = $id")
                    .bind(("id", game_uuid.to_string()))
                    .bind(("state", game_json))
                    .await
                    .and_then(|mut r| r.take(0));
            }
        }
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
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    // Get matchmaking queue entries
    let queue_result: Result<Vec<db::MatchmakingQueueRecord>, _> = state
        .db
        .query("SELECT * FROM matchmaking_queue ORDER BY queued_at ASC")
        .await
        .and_then(|mut resp| resp.take(0));

    let queue_entries = match queue_result {
        Ok(entries) => entries,
        Err(_) => vec![],
    };

    let now = timestamp();
    let mut queue = vec![];
    for entry in queue_entries {
        // Get player info
        let player: Option<db::PlayerRecord> = state
            .db
            .select(("players", as_uuid(&entry.player_id)))
            .await
            .ok()
            .flatten();

        if let Some(p) = player {
            let waiting_time = format!("{}s", now.saturating_sub(entry.queued_at));
            queue.push(QueueEntry {
                player_id: as_uuid(&entry.player_id).to_string(),
                displayname: p.displayname,
                elo_rating: p.elo_rating,
                joined_at: entry.queued_at,
                waiting_time,
            });
        }
    }

    let template = AdminQueueTemplate { queue };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
}

pub async fn html_admin_remove_from_queue(
    State(state): ServerState,
    headers: axum::http::HeaderMap,
    Path(player_id): Path<String>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let _: Result<surrealdb::Response, _> = state
        .db
        .query("DELETE FROM matchmaking_queue WHERE player_id = $player_id")
        .bind(("player_id", player_id))
        .await;

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
    headers: axum::http::HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let session_id = match get_session_from_cookies(&headers) {
        Some(id) => id,
        None => return Redirect::to("/login").into_response(),
    };

    let (_, player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    if !player.is_admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let game_id_filter = params.get("game_id").map(|s| s.to_string());

    let query = if let Some(ref game_id) = game_id_filter {
        format!(
            "SELECT * FROM game_events WHERE game_id = '{}' ORDER BY timestamp DESC LIMIT 100",
            game_id
        )
    } else {
        "SELECT * FROM game_events ORDER BY timestamp DESC LIMIT 100".to_string()
    };

    let events_result: Result<Vec<crate::db::GameEventRecord>, _> = state
        .db
        .query(&query)
        .await
        .and_then(|mut resp| resp.take(0));

    let events_records = match events_result {
        Ok(events) => events,
        Err(_) => vec![],
    };

    let events: Vec<GameEventInfo> = events_records
        .into_iter()
        .map(|ev| {
            let event_type = match &ev.event {
                automatafl_api_types::GameEventData::PlayerJoined { .. } => "PLAYER_JOINED",
                automatafl_api_types::GameEventData::GameStarted => "GAME_STARTED",
                automatafl_api_types::GameEventData::MoveAcknowledged { .. } => "MOVE_ACK",
                automatafl_api_types::GameEventData::MoveInvalid { .. } => "MOVE_INVALID",
                automatafl_api_types::GameEventData::Move { .. } => "MOVE",
                automatafl_api_types::GameEventData::AutomatonStep { .. } => "AUTOMATON_STEP",
                automatafl_api_types::GameEventData::GameOver { .. } => "GAME_OVER",
                automatafl_api_types::GameEventData::EloUpdate { .. } => "ELO_UPDATE",
                automatafl_api_types::GameEventData::RoundComplete => "ROUND_COMPLETE",
                automatafl_api_types::GameEventData::Conflicts { .. } => "CONFLICTS",
                automatafl_api_types::GameEventData::Chat { .. } => "CHAT",
                automatafl_api_types::GameEventData::GameLoaded { .. } => "GAME_LOADED",
                automatafl_api_types::GameEventData::State { .. } => "STATE",
            };
            GameEventInfo {
                timestamp: ev.timestamp,
                game_id: as_uuid(&ev.game_id).to_string(),
                event_type: event_type.to_string(),
                details: serde_json::to_string_pretty(&ev.event)
                    .unwrap_or_else(|_| "{}".to_string()),
            }
        })
        .collect();

    let template = AdminEventsTemplate {
        events,
        game_id_filter,
    };

    match template.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", err),
        )
            .into_response(),
    }
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

    let (player_id, _player) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Add to matchmaking queue directly
    let entry = db::MatchmakingQueueRecord {
        player_id: RecordId::from_table_key("players", player_id),
        queued_at: timestamp(),
        game_preferences: r#"{"player_count":2,"use_column_rule":false}"#.to_string(),
    };

    let _: Result<Option<db::MatchmakingQueueRecord>, _> =
        state.db.create("matchmaking_queue").content(entry).await;

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

    let (player_id, _) = match get_player_from_session(&state.db, session_id).await {
        Some(p) => p,
        None => return Redirect::to("/login").into_response(),
    };

    // Remove from queue
    let _: Result<surrealdb::Response, _> = state
        .db
        .query("DELETE FROM matchmaking_queue WHERE player_id = $player_id")
        .bind(("player_id", player_id.to_string()))
        .await;

    Redirect::to("/matchmaking").into_response()
}
