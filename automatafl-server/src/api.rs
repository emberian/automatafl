//! Axum HTTP + SSE API router for the Automatafl fog-of-war server (`R5`).
//!
//! Enforces strict viewer-scoped fog-of-war isolation across REST and SSE:
//! - Every endpoint and every SSE connection authenticates its caller's `seat_token`
//!   independently and projections are always computed via `session.view_for(viewer)`.
//! - Broadcast notifications carry only a monotonic revision counter; each SSE
//!   subscriber renders its own `session.view_for(viewer)` snapshot so secret moves
//!   can never leak across channels.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::bot::BotStrategy;
use crate::matchmaking::{MatchmakingQueue, SeekRequest};
use crate::persistence::GameStore;
use crate::protocol::{Coord, GameConfig, Seat};
use crate::session::{FinishReason, GameSession, GameView, LobbyGameSummary, PlayedTurn};
use crate::telemetry::{StatusSnapshot, global_telemetry};
use crate::web_ui::INDEX_HTML;

pub const DEFAULT_MAX_IN_MEMORY_GAMES: usize = 256;

pub struct AppState {
    games: RwLock<HashMap<String, Arc<Mutex<GameSession>>>>,
    lobby_summaries: RwLock<HashMap<String, LobbyGameSummary>>,
    notifiers: RwLock<HashMap<String, broadcast::Sender<u64>>>,
    matchmaking: Mutex<MatchmakingQueue>,
    store: GameStore,
    next_game_seq: AtomicU64,
    max_in_memory_games: usize,
}

impl AppState {
    pub fn new(store: GameStore) -> Result<Arc<Self>, String> {
        Self::with_max_in_memory_games(store, DEFAULT_MAX_IN_MEMORY_GAMES)
    }

    pub fn with_max_in_memory_games(
        store: GameStore,
        max_in_memory_games: usize,
    ) -> Result<Arc<Self>, String> {
        let persisted = store.load_all_and_verify()?;
        let mut games = HashMap::new();
        let mut lobby_summaries = HashMap::new();
        let mut notifiers = HashMap::new();
        let mut max_seq = 0u64;

        for session in persisted {
            if let Some(num) = session
                .id
                .strip_prefix("af-")
                .and_then(|s| s.parse::<u64>().ok())
            {
                max_seq = max_seq.max(num);
            }
            if session.is_public {
                lobby_summaries.insert(session.id.clone(), session.lobby_summary());
            }
            let (tx, _) = broadcast::channel(64);
            notifiers.insert(session.id.clone(), tx);
            games.insert(session.id.clone(), Arc::new(Mutex::new(session)));
        }

        let state = Arc::new(Self {
            games: RwLock::new(games),
            lobby_summaries: RwLock::new(lobby_summaries),
            notifiers: RwLock::new(notifiers),
            matchmaking: Mutex::new(MatchmakingQueue::new()),
            store,
            next_game_seq: AtomicU64::new(max_seq),
            max_in_memory_games: max_in_memory_games.max(1),
        });
        state.evict_excess_games(None);
        Ok(state)
    }

    pub fn new_in_memory() -> Arc<Self> {
        Self::new(GameStore::memory()).expect("in-memory store never fails")
    }

    pub fn in_memory_game_count(&self) -> usize {
        self.games.read().expect("games lock").len()
    }

    pub fn is_in_memory(&self, game_id: &str) -> bool {
        self.games.read().expect("games lock").contains_key(game_id)
    }

    pub fn store(&self) -> &GameStore {
        &self.store
    }

    fn update_lobby_summary(&self, session: &GameSession) {
        if session.is_public {
            self.lobby_summaries
                .write()
                .expect("lobby_summaries lock")
                .insert(session.id.clone(), session.lobby_summary());
        }
    }

    fn allocate_game_id(&self) -> (String, u64) {
        let seq = self.next_game_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(seq);
        let rand_bits: u64 = rand::Rng::random(&mut rand::rng());
        let seed = now_ns ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ rand_bits;
        (format!("af-{seq}"), seed)
    }

    pub fn insert_session(&self, session: GameSession) -> Result<(), String> {
        self.store.save_game(&session)?;
        global_telemetry()
            .disk_saves_total
            .fetch_add(1, Ordering::Relaxed);
        self.update_lobby_summary(&session);
        let id = session.id.clone();
        let (tx, _) = broadcast::channel(64);
        self.notifiers
            .write()
            .expect("notifiers lock")
            .entry(id.clone())
            .or_insert(tx);
        self.games
            .write()
            .expect("games lock")
            .insert(id.clone(), Arc::new(Mutex::new(session)));
        self.evict_excess_games(Some(&id));
        Ok(())
    }

    /// Look up a game session in RAM, or transparently lazy-load it from disk if it was evicted.
    pub fn get_or_load_session(&self, game_id: &str) -> Option<Arc<Mutex<GameSession>>> {
        if let Some(arc) = self.games.read().expect("games lock").get(game_id).cloned() {
            return Some(arc);
        }
        // Not in RAM: check durable disk store outside the games lock.
        let mut loaded = self.store.load_game(game_id).ok().flatten()?;
        global_telemetry()
            .disk_loads_total
            .fetch_add(1, Ordering::Relaxed);
        loaded.last_active_ms = now_ms();
        self.update_lobby_summary(&loaded);
        let arc = Arc::new(Mutex::new(loaded));
        {
            let mut notifiers = self.notifiers.write().expect("notifiers lock");
            notifiers
                .entry(game_id.to_string())
                .or_insert_with(|| broadcast::channel(64).0);
        }
        let result = {
            let mut games = self.games.write().expect("games lock");
            games
                .entry(game_id.to_string())
                .or_insert_with(|| Arc::clone(&arc))
                .clone()
        };
        self.evict_excess_games(Some(game_id));
        Some(result)
    }

    /// Bounded RAM eviction: when `games.len() > max_in_memory_games`, evicts finished games first
    /// and least-recently-active (`last_active_ms`) games next.
    /// Uses `try_lock()` and `Arc::strong_count == 1` so eviction never blocks on an active game's
    /// mutex and never evicts a session with an in-flight request.
    pub fn evict_excess_games(&self, protect_id: Option<&str>) {
        let mut games = self.games.write().expect("games lock");
        if games.len() <= self.max_in_memory_games {
            return;
        }
        let excess = games.len() - self.max_in_memory_games;
        let mut candidates: Vec<(String, bool, u64)> = games
            .iter()
            .filter(|(id, arc)| {
                protect_id != Some(id.as_str()) && Arc::strong_count(arc) == 1
            })
            .filter_map(|(id, arc)| {
                let s = arc.try_lock().ok()?;
                Some((id.clone(), s.ended, s.last_active_ms))
            })
            .collect();

        // Sort so finished games (`ended == true`) come first, then oldest `last_active_ms`, then id.
        candidates.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut evicted_arcs = Vec::with_capacity(excess);
        {
            let mut notifiers = self.notifiers.write().expect("notifiers lock");
            for (id, _, _) in candidates.into_iter().take(excess) {
                if let Some(arc) = games.remove(&id) {
                    evicted_arcs.push(arc);
                }
                if let Some(tx) = notifiers.get(&id) {
                    if tx.receiver_count() == 0 {
                        notifiers.remove(&id);
                    }
                }
            }
        }
        drop(games);

        if !evicted_arcs.is_empty() {
            global_telemetry()
                .cache_evictions_total
                .fetch_add(evicted_arcs.len() as u64, Ordering::Relaxed);
        }

        // Persist any evicted sessions outside `games` and `notifiers` write locks so readers never wait on disk I/O
        for arc in evicted_arcs {
            if let Ok(s) = arc.try_lock() {
                if self.store.save_game(&s).is_ok() {
                    global_telemetry()
                        .disk_saves_total
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn status_snapshot(&self) -> StatusSnapshot {
        let (in_memory_games, active_games, finished_games_in_ram) = {
            let games = self.games.read().expect("games lock");
            let total = games.len();
            let mut active = 0usize;
            let mut finished = 0usize;
            for arc in games.values() {
                if let Ok(s) = arc.try_lock() {
                    if s.ended {
                        finished += 1;
                    } else {
                        active += 1;
                    }
                } else {
                    active += 1;
                }
            }
            (total, active, finished)
        };
        let lobby_public_games = self
            .lobby_summaries
            .read()
            .expect("lobby_summaries lock")
            .len();
        let matchmaking_queued = self
            .matchmaking
            .lock()
            .map(|mm| mm.waiting_count())
            .unwrap_or(0);
        global_telemetry().snapshot(
            in_memory_games,
            self.max_in_memory_games,
            active_games,
            finished_games_in_ram,
            lobby_public_games,
            matchmaking_queued,
            self.store.root_dir(),
        )
    }

    fn notify_game(&self, game_id: &str) {
        if let Some(tx) = self
            .notifiers
            .read()
            .expect("notifiers lock")
            .get(game_id)
        {
            let _ = tx.send(1);
        }
    }
}

async fn http_telemetry_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let skip_health_count = path == "/api/health" || path == "/api/status" || path == "/metrics";
    let t0 = Instant::now();
    let resp = next.run(req).await;
    if !skip_health_count {
        let dur_us = t0.elapsed().as_micros() as u64;
        global_telemetry().record_http_request(resp.status().as_u16(), dur_us);
    }
    resp
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/status", get(serve_index))
        .route("/lag", get(serve_index))
        .route("/game/{id}", get(serve_index))
        .route("/watch/{id}", get(serve_index))
        .route("/api/health", get(health_check))
        .route("/api/status", get(server_status))
        .route("/metrics", get(prometheus_metrics))
        .route("/api/games", get(list_games).post(create_game))
        .route("/api/games/{id}", get(get_game_view))
        .route("/api/games/{id}/join", post(join_game))
        .route("/api/games/{id}/select", post(select_piece))
        .route("/api/games/{id}/move", post(submit_move))
        .route("/api/games/{id}/commit", post(commit_move))
        .route("/api/games/{id}/reveal", post(reveal_round))
        .route("/api/games/{id}/resolve", post(resolve_round))
        .route("/api/games/{id}/resign", post(resign_game))
        .route("/api/games/{id}/replay", get(export_replay))
        .route("/api/games/{id}/events", get(game_events_sse))
        .route("/api/matchmaking/seek", post(matchmaking_seek))
        .route("/api/matchmaking/status/{ticket_id}", get(matchmaking_status))
        .route(
            "/api/matchmaking/seek/{ticket_id}",
            delete(matchmaking_cancel),
        )
        .layer(middleware::from_fn(http_telemetry_middleware))
        .with_state(state)
}

#[derive(Debug, Default, Deserialize)]
pub struct TokenQuery {
    #[serde(default)]
    pub token: Option<String>,
}

fn extract_token(headers: &HeaderMap, query: &TokenQuery) -> Option<String> {
    if let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(tok) = auth.strip_prefix("Bearer ") {
            let trimmed = tok.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    if let Some(tok) = headers
        .get("x-seat-token")
        .and_then(|v| v.to_str().ok())
    {
        let trimmed = tok.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(ref tok) = query.token {
        let trimmed = tok.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

fn bad_request(msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError { error: msg.into() }),
    )
        .into_response()
}

fn unauthorized(msg: impl Into<String>) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError { error: msg.into() }),
    )
        .into_response()
}

fn not_found(msg: impl Into<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError { error: msg.into() }),
    )
        .into_response()
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn server_status(State(state): State<Arc<AppState>>) -> Json<StatusSnapshot> {
    Json(state.status_snapshot())
}

async fn prometheus_metrics(State(state): State<Arc<AppState>>) -> Response {
    let snap = state.status_snapshot();
    let body = global_telemetry().render_prometheus_metrics(&snap);
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

fn record_session_transition_telemetry(
    session: &GameSession,
    prev_turns: usize,
    prev_round: usize,
    prev_ended: bool,
    dur_us: u64,
    action_label: &str,
) {
    let tel = global_telemetry();
    let new_turns = session.turns_played.len();
    if new_turns > prev_turns {
        let delta = (new_turns - prev_turns) as u64;
        tel.turns_resolved_total.fetch_add(delta, Ordering::Relaxed);
        if let Some(last_t) = session.turns_played.last() {
            let clash_cnt = last_t.conflict_subs.len();
            if clash_cnt > 0 {
                tel.conflicts_triggered_total
                    .fetch_add(clash_cnt as u64, Ordering::Relaxed);
            }
            tel.trace_event(
                "INFO",
                "TURN_RESOLVE",
                Some(&session.id),
                Some(dur_us),
                format!(
                    "turn={} auto=({},{})->({},{}) clashes={} ({})",
                    last_t.turn_no,
                    last_t.auto_step.from.0,
                    last_t.auto_step.from.1,
                    last_t.auto_step.to.0,
                    last_t.auto_step.to.1,
                    clash_cnt,
                    last_t.auto_step.why
                ),
            );
        }
    } else if session.conflict_subs.len() > prev_round {
        tel.conflicts_triggered_total
            .fetch_add(1, Ordering::Relaxed);
        tel.trace_event(
            "INFO",
            "CONFLICT_CLASH",
            Some(&session.id),
            Some(dur_us),
            format!(
                "turn={} clash_round={} marks={}",
                session.turn_no,
                session.conflict_subs.len(),
                session.marks.len()
            ),
        );
    } else {
        tel.trace_event(
            "INFO",
            "ROUND_SEAL",
            Some(&session.id),
            Some(dur_us),
            format!(
                "{} turn={} phase={:?} (fog-of-war sealed)",
                action_label,
                session.turn_no,
                session.phase()
            ),
        );
    }

    if session.ended && !prev_ended {
        tel.games_finished_total.fetch_add(1, Ordering::Relaxed);
        match session.winner {
            Some(Seat::A) => {
                tel.wins_seat_a.fetch_add(1, Ordering::Relaxed);
            }
            Some(Seat::B) => {
                tel.wins_seat_b.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                tel.draws_total.fetch_add(1, Ordering::Relaxed);
            }
        }
        tel.trace_event(
            "INFO",
            "GAME_FINISH",
            Some(&session.id),
            Some(dur_us),
            format!(
                "winner={:?} reason={:?} turns={}",
                session.winner,
                session.finish_reason,
                session.turns_played.len()
            ),
        );
    }
}

async fn list_games(State(state): State<Arc<AppState>>) -> Json<Vec<LobbyGameSummary>> {
    global_telemetry()
        .ep_list_games
        .fetch_add(1, Ordering::Relaxed);
    let entries: Vec<(String, Arc<Mutex<GameSession>>)> = {
        let games = state.games.read().expect("games lock");
        games.iter().map(|(k, v)| (k.clone(), Arc::clone(v))).collect()
    };
    let mut list: Vec<LobbyGameSummary> = Vec::with_capacity(entries.len());
    for (id, arc) in entries {
        if let Ok(s) = arc.try_lock() {
            if s.is_public {
                list.push(s.lobby_summary());
            }
        } else if let Some(cached) = state
            .lobby_summaries
            .read()
            .expect("lobby_summaries lock")
            .get(&id)
            .cloned()
        {
            list.push(cached);
        }
    }
    list.sort_by(|a, b| b.id.cmp(&a.id));
    Json(list)
}

#[derive(Debug, Default, Deserialize)]
pub struct CreateGameRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_true")]
    pub is_public: bool,
    #[serde(default)]
    pub config: GameConfig,
    /// Optional AI bot opponent strategy (`novice`, `tactical`, `minimax`, `decoupled_uct`, `regret_matching`).
    #[serde(default)]
    pub vs_bot: Option<BotStrategy>,
    /// Seat claimed by the creator (`A` or `B`, defaults to `A`).
    #[serde(default)]
    pub creator_seat: Option<Seat>,
    #[serde(default)]
    pub player_name: Option<String>,
    /// If true, creates an unclaimed table where both seats must join explicitly.
    #[serde(default)]
    pub unclaimed: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateGameResponse {
    pub game_id: String,
    pub creator_seat: Option<Seat>,
    /// Private token for the creator's seat (`None` if `unclaimed` was true).
    pub creator_token: Option<String>,
    /// Direct invite token for the opponent seat ONLY when the creator explicitly creates an open
    /// challenge table (`vs_bot == None && !unclaimed`).
    pub open_invite_token: Option<String>,
    pub view: GameView,
}

async fn create_game(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGameRequest>,
) -> Response {
    global_telemetry()
        .ep_create_game
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    tokio::task::spawn_blocking(move || {
        let (game_id, seed) = state.allocate_game_id();
        let table_name = req
            .name
            .unwrap_or_else(|| format!("Table {game_id}"));
        let vs_bot = req.vs_bot;
        let is_public = req.is_public;
        let preset = req.config.preset;
        let confluence = req.config.confluence;
        let mut session = match GameSession::new(
            game_id.clone(),
            table_name.clone(),
            req.is_public,
            req.config,
            seed,
            now_ms(),
        ) {
            Ok(s) => s,
            Err(e) => return bad_request(e),
        };

        let mut creator_seat = None;
        let mut creator_token = None;
        let mut open_invite_token = None;

        if !req.unclaimed {
            let seat = req.creator_seat.unwrap_or(Seat::A);
            let p_name = req
                .player_name
                .unwrap_or_else(|| format!("Seat {}", seat.label()));
            let (_, tok) = match session.join_seat(Some(seat), p_name) {
                Ok(res) => res,
                Err(e) => return bad_request(e),
            };
            creator_seat = Some(seat);
            creator_token = Some(tok);

            let other = seat.other();
            if let Some(strategy) = req.vs_bot {
                session.set_bot(other, strategy);
            } else {
                open_invite_token = Some(session.seat_tokens[other.idx()].clone());
            }
        } else if let Some(strategy) = req.vs_bot {
            session.set_bot(Seat::B, strategy);
        }

        let view = session.view_for(creator_seat);
        if let Err(e) = state.insert_session(session) {
            return bad_request(e);
        }

        let dur_us = t0.elapsed().as_micros() as u64;
        let tel = global_telemetry();
        tel.games_created_total.fetch_add(1, Ordering::Relaxed);
        tel.trace_event(
            "INFO",
            "GAME_CREATE",
            Some(&game_id),
            Some(dur_us),
            format!(
                "name={:?} public={} preset={:?} confluence={:?} bot={:?}",
                table_name, is_public, preset, confluence, vs_bot
            ),
        );

        Json(CreateGameResponse {
            game_id,
            creator_seat,
            creator_token,
            open_invite_token,
            view,
        })
        .into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("create_game task error: {e}")))
}

async fn get_game_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    global_telemetry()
        .ep_get_view
        .fetch_add(1, Ordering::Relaxed);
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let viewer = session.authenticate(tok.as_deref());
            session.view_for(viewer)
        };
        drop(session_arc);
        state.evict_excess_games(None);
        Json(view).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("get_game_view task error: {e}")))
}

#[derive(Debug, Default, Deserialize)]
pub struct JoinGameRequest {
    #[serde(default)]
    pub preferred_seat: Option<Seat>,
    #[serde(default)]
    pub player_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinGameResponse {
    pub game_id: String,
    pub seat: Seat,
    pub seat_token: String,
    pub view: GameView,
}

async fn join_game(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<JoinGameRequest>,
) -> Response {
    global_telemetry()
        .ep_join_game
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let p_name = req
            .player_name
            .unwrap_or_else(|| "Challenger".to_string());
        let (seat, seat_token, view) = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let (seat, seat_token) = match session.join_seat(req.preferred_seat, p_name.clone()) {
                Ok(res) => res,
                Err(e) => return bad_request(e),
            };
            if let Err(e) = state.store.save_game(&session) {
                return bad_request(e);
            }
            state.update_lobby_summary(&session);
            let view = session.view_for(Some(seat));
            (seat, seat_token, view)
        };
        drop(session_arc);
        state.evict_excess_games(None);
        state.notify_game(&id);

        let dur_us = t0.elapsed().as_micros() as u64;
        global_telemetry().trace_event(
            "INFO",
            "SEAT_JOIN",
            Some(&id),
            Some(dur_us),
            format!("seat={} player={:?}", seat.label(), p_name),
        );

        Json(JoinGameResponse {
            game_id: id,
            seat,
            seat_token,
            view,
        })
        .into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("join_game task error: {e}")))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionResponse {
    pub view: GameView,
}

#[derive(Debug, Deserialize)]
pub struct SelectPieceRequest {
    pub coord: Option<Coord>,
    #[serde(default)]
    pub seat: Option<Seat>,
}

async fn select_piece(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    Json(req): Json<SelectPieceRequest>,
) -> Response {
    global_telemetry()
        .ep_select_piece
        .fetch_add(1, Ordering::Relaxed);
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let Some(seat) = session.authenticate(tok.as_deref()) else {
                return unauthorized("valid seat token required to select a piece");
            };
            if req.seat.is_some_and(|expected| expected != seat) {
                return unauthorized("seat token does not match requested seat");
            }
            match req.coord {
                Some(c) => {
                    if let Err(e) = session.select_source(seat, c) {
                        return bad_request(e);
                    }
                }
                None => session.clear_selection(seat),
            }
            // Note: Piece selection is ephemeral UI state — never write session.json to disk here!
            session.view_for(Some(seat))
        };
        drop(session_arc);
        state.evict_excess_games(None);
        Json(ActionResponse { view }).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("select_piece task error: {e}")))
}

#[derive(Debug, Deserialize)]
pub struct SubmitMoveRequest {
    pub from: Coord,
    pub to: Coord,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub seat: Option<Seat>,
}

async fn submit_move(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    Json(req): Json<SubmitMoveRequest>,
) -> Response {
    global_telemetry()
        .ep_submit_move
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let Some(seat) = session.authenticate(tok.as_deref()) else {
                return unauthorized("valid seat token required to submit a move");
            };
            if req.seat.is_some_and(|expected| expected != seat) {
                return unauthorized("seat token does not match requested seat");
            }
            let prev_turns = session.turns_played.len();
            let prev_round = session.conflict_subs.len();
            let prev_ended = session.ended;
            if let Err(e) =
                session.submit_move_with_revision(seat, req.from, req.to, req.expected_revision)
            {
                return bad_request(e);
            }
            if let Err(e) = state.store.save_game(&session) {
                return bad_request(e);
            }
            state.update_lobby_summary(&session);
            let dur_us = t0.elapsed().as_micros() as u64;
            record_session_transition_telemetry(
                &session,
                prev_turns,
                prev_round,
                prev_ended,
                dur_us,
                &format!("submit seat={}", seat.label()),
            );
            session.view_for(Some(seat))
        };
        drop(session_arc);
        state.evict_excess_games(None);
        state.notify_game(&id);
        Json(ActionResponse { view }).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("submit_move task error: {e}")))
}

#[derive(Debug, Deserialize)]
pub struct CommitRequest {
    #[serde(default)]
    pub from: Option<Coord>,
    #[serde(default)]
    pub to: Option<Coord>,
    #[serde(default)]
    pub commitment_hash: Option<String>,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub seat: Option<Seat>,
}

async fn commit_move(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    Json(req): Json<CommitRequest>,
) -> Response {
    global_telemetry()
        .ep_commit_move
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let Some(seat) = session.authenticate(tok.as_deref()) else {
                return unauthorized("valid seat token required to commit a move");
            };
            if req.seat.is_some_and(|expected| expected != seat) {
                return unauthorized("seat token does not match requested seat");
            }

            let prev_turns = session.turns_played.len();
            let prev_round = session.conflict_subs.len();
            let prev_ended = session.ended;

            if let Some(hash) = req.commitment_hash {
                if let Err(e) =
                    session.commit_blind_hash_with_revision(seat, hash, req.expected_revision)
                {
                    return bad_request(e);
                }
            } else if let (Some(from), Some(to)) = (req.from, req.to) {
                if let Err(e) =
                    session.submit_move_with_revision(seat, from, to, req.expected_revision)
                {
                    return bad_request(e);
                }
            } else {
                return bad_request("commit requires either (from, to) or commitment_hash");
            }

            if let Err(e) = state.store.save_game(&session) {
                return bad_request(e);
            }
            state.update_lobby_summary(&session);
            let dur_us = t0.elapsed().as_micros() as u64;
            record_session_transition_telemetry(
                &session,
                prev_turns,
                prev_round,
                prev_ended,
                dur_us,
                &format!("commit seat={}", seat.label()),
            );
            session.view_for(Some(seat))
        };
        drop(session_arc);
        state.evict_excess_games(None);
        state.notify_game(&id);
        Json(ActionResponse { view }).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("commit_move task error: {e}")))
}

#[derive(Debug, Default, Deserialize)]
pub struct RevealRequest {
    #[serde(default)]
    pub from: Option<Coord>,
    #[serde(default)]
    pub to: Option<Coord>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub seat: Option<Seat>,
}

async fn reveal_round(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    body: Option<Json<RevealRequest>>,
) -> Response {
    global_telemetry()
        .ep_reveal_round
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let Some(seat) = session.authenticate(tok.as_deref()) else {
                return unauthorized("valid seat token required to reveal move");
            };

            let req = body.map(|b| b.0).unwrap_or_default();
            if req.seat.is_some_and(|expected| expected != seat) {
                return unauthorized("seat token does not match requested seat");
            }
            let prev_turns = session.turns_played.len();
            let prev_round = session.conflict_subs.len();
            let prev_ended = session.ended;

            let details = match (req.from, req.to, req.nonce) {
                (Some(f), Some(t), Some(n)) => Some((f, t, n)),
                _ => None,
            };
            if let Err(e) = session.reveal_move(seat, details) {
                return bad_request(e);
            }

            if let Err(e) = state.store.save_game(&session) {
                return bad_request(e);
            }
            state.update_lobby_summary(&session);
            let dur_us = t0.elapsed().as_micros() as u64;
            record_session_transition_telemetry(
                &session,
                prev_turns,
                prev_round,
                prev_ended,
                dur_us,
                &format!("reveal seat={}", seat.label()),
            );
            session.view_for(Some(seat))
        };
        drop(session_arc);
        state.evict_excess_games(None);
        state.notify_game(&id);
        Json(ActionResponse { view }).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("reveal_round task error: {e}")))
}

async fn resolve_round(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    global_telemetry()
        .ep_resolve_round
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let tok = extract_token(&headers, &query);
    tokio::task::spawn_blocking(move || {
        let Some(session_arc) = state.get_or_load_session(&id) else {
            return not_found(format!("game {id} not found"));
        };
        let view = {
            let mut session = session_arc.lock().expect("session lock");
            session.last_active_ms = now_ms();
            let viewer = session.authenticate(tok.as_deref());
            let both_bots = session.bot_seats.iter().all(Option::is_some);
            if viewer.is_none() && !both_bots {
                return unauthorized("valid seat token required to resolve round");
            }
            let prev_turns = session.turns_played.len();
            let prev_round = session.conflict_subs.len();
            let prev_ended = session.ended;

            if let Err(e) = session.resolve_round() {
                return bad_request(e);
            }
            if let Err(e) = state.store.save_game(&session) {
                return bad_request(e);
            }
            state.update_lobby_summary(&session);
            let dur_us = t0.elapsed().as_micros() as u64;
            record_session_transition_telemetry(
                &session,
                prev_turns,
                prev_round,
                prev_ended,
                dur_us,
                "resolve_round",
            );
            session.view_for(viewer)
        };
        drop(session_arc);
        state.evict_excess_games(None);
        state.notify_game(&id);
        Json(ActionResponse { view }).into_response()
    })
    .await
    .unwrap_or_else(|e| bad_request(format!("resolve_round task error: {e}")))
}

#[derive(Debug, Default, Deserialize)]
pub struct ResignRequest {
    #[serde(default)]
    pub seat: Option<Seat>,
}

async fn resign_game(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    body: Option<Json<ResignRequest>>,
) -> Response {
    global_telemetry()
        .ep_resign_game
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let Some(session_arc) = state.get_or_load_session(&id) else {
        return not_found(format!("game {id} not found"));
    };
    let tok = extract_token(&headers, &query);
    let view = {
        let mut session = session_arc.lock().expect("session lock");
        session.last_active_ms = now_ms();
        let Some(seat) = session.authenticate(tok.as_deref()) else {
            return unauthorized("valid seat token required to resign");
        };
        let req = body.map(|b| b.0).unwrap_or_default();
        if req.seat.is_some_and(|expected| expected != seat) {
            return unauthorized("seat token does not match requested seat");
        }
        let prev_turns = session.turns_played.len();
        let prev_round = session.conflict_subs.len();
        let prev_ended = session.ended;

        if let Err(e) = session.resign(seat) {
            return bad_request(e);
        }
        if let Err(e) = state.store.save_game(&session) {
            return bad_request(e);
        }
        state.update_lobby_summary(&session);
        let dur_us = t0.elapsed().as_micros() as u64;
        record_session_transition_telemetry(
            &session,
            prev_turns,
            prev_round,
            prev_ended,
            dur_us,
            &format!("resign seat={}", seat.label()),
        );
        session.view_for(Some(seat))
    };
    drop(session_arc);
    state.evict_excess_games(None);
    state.notify_game(&id);
    Json(ActionResponse { view }).into_response()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayExport {
    pub game_id: String,
    pub config: GameConfig,
    pub initial_board_wire: String,
    pub current_board_wire: String,
    pub turns_played: Vec<PlayedTurn>,
    pub winner: Option<Seat>,
    pub finish_reason: Option<FinishReason>,
    pub replay_verified: bool,
}

async fn export_replay(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    global_telemetry()
        .ep_replay
        .fetch_add(1, Ordering::Relaxed);
    let Some(session_arc) = state.get_or_load_session(&id) else {
        return not_found(format!("game {id} not found"));
    };
    let export = {
        let session = session_arc.lock().expect("session lock");
        let replay_verified = session.verify_replay().is_ok();
        ReplayExport {
            game_id: session.id.clone(),
            config: session.config.clone(),
            initial_board_wire: session.start_board.to_wire(),
            current_board_wire: session.board.to_wire(),
            turns_played: session.turns_played.clone(),
            winner: session.winner,
            finish_reason: session.finish_reason.clone(),
            replay_verified,
        }
    };
    drop(session_arc);
    state.evict_excess_games(None);
    Json(export).into_response()
}

struct SseStreamGuard;

impl SseStreamGuard {
    fn new() -> Self {
        let tel = global_telemetry();
        tel.active_sse_streams.fetch_add(1, Ordering::Relaxed);
        tel.sse_connections_total.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for SseStreamGuard {
    fn drop(&mut self) {
        global_telemetry()
            .active_sse_streams
            .fetch_sub(1, Ordering::Relaxed);
    }
}

async fn game_events_sse(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    let tok = extract_token(&headers, &query);
    let Some(session_arc) = state.get_or_load_session(&id) else {
        return not_found(format!("game {id} not found"));
    };
    let (initial_json, rx) = {
        let session = session_arc.lock().expect("session lock");
        let viewer = session.authenticate(tok.as_deref());
        let initial_view = session.view_for(viewer);
        let initial_json = serde_json::to_string(&initial_view).unwrap_or_default();

        let notifiers = state.notifiers.read().expect("notifiers lock");
        let Some(tx) = notifiers.get(&id) else {
            return not_found(format!("game {id} notifier not found"));
        };
        (initial_json, tx.subscribe())
    };

    let state_clone = Arc::clone(&state);
    let game_id = id.clone();
    let tok_clone = tok.clone();

    let initial_stream = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default().data(initial_json),
    ));

    let updates_stream = BroadcastStream::new(rx).filter_map(move |msg| {
        if msg.is_err() {
            return None;
        }
        let session_arc = state_clone.get_or_load_session(&game_id)?;
        let session = session_arc.lock().ok()?;
        let viewer = session.authenticate(tok_clone.as_deref());
        let view = session.view_for(viewer);
        let json = serde_json::to_string(&view).ok()?;
        Some(Ok::<Event, Infallible>(Event::default().data(json)))
    });

    let sse_guard = SseStreamGuard::new();
    let stream = initial_stream.chain(updates_stream).map(move |item| {
        let _keep = &sse_guard;
        global_telemetry()
            .sse_events_sent_total
            .fetch_add(1, Ordering::Relaxed);
        item
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

async fn matchmaking_seek(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SeekRequest>,
) -> Response {
    global_telemetry()
        .ep_matchmaking
        .fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let (ticket_id, assignment_opt) = {
        let mut mm = state.matchmaking.lock().expect("matchmaking lock");
        match mm.enqueue(req) {
            Ok(res) => res,
            Err(e) => return bad_request(e),
        }
    };

    if let Some(assignment) = assignment_opt {
        let (game_id, seed) = state.allocate_game_id();
        let mut session = match GameSession::new(
            game_id.clone(),
            format!("Matchmaking {game_id}"),
            true,
            assignment.config.clone(),
            seed,
            now_ms(),
        ) {
            Ok(s) => s,
            Err(e) => return bad_request(e),
        };
        let name_a = assignment.name_a.clone().unwrap_or_else(|| "Player A".to_string());
        let name_b = assignment.name_b.clone().unwrap_or_else(|| "Player B".to_string());
        let (_, token_a) = session
            .join_seat(Some(Seat::A), name_a.clone())
            .expect("seat A open");
        let (_, token_b) = session
            .join_seat(Some(Seat::B), name_b.clone())
            .expect("seat B open");
        if let Err(e) = state.insert_session(session) {
            return bad_request(e);
        }
        let dur_us = t0.elapsed().as_micros() as u64;
        let tel = global_telemetry();
        tel.games_created_total.fetch_add(1, Ordering::Relaxed);
        tel.matchmaking_paired_total.fetch_add(1, Ordering::Relaxed);
        tel.trace_event(
            "INFO",
            "MATCH_PAIRED",
            Some(&game_id),
            Some(dur_us),
            format!("seat_a={:?} vs seat_b={:?}", name_a, name_b),
        );
        let mut mm = state.matchmaking.lock().expect("matchmaking lock");
        mm.record_match(&assignment, &game_id, &token_a, &token_b);
        let status = mm
            .status(&ticket_id)
            .expect("ticket was just matched");
        return Json(status).into_response();
    }

    let mm = state.matchmaking.lock().expect("matchmaking lock");
    let status = mm
        .status(&ticket_id)
        .expect("ticket was just inserted");
    Json(status).into_response()
}

async fn matchmaking_status(
    State(state): State<Arc<AppState>>,
    Path(ticket_id): Path<String>,
) -> Response {
    let mm = state.matchmaking.lock().expect("matchmaking lock");
    match mm.status(&ticket_id) {
        Some(status) => Json(status).into_response(),
        None => not_found(format!("matchmaking ticket {ticket_id} not found")),
    }
}

async fn matchmaking_cancel(
    State(state): State<Arc<AppState>>,
    Path(ticket_id): Path<String>,
) -> Response {
    let mut mm = state.matchmaking.lock().expect("matchmaking lock");
    match mm.cancel(&ticket_id) {
        Some(status) => Json(status).into_response(),
        None => not_found(format!("matchmaking ticket {ticket_id} not found")),
    }
}
