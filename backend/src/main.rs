mod accounts;
mod admin;
mod auth;
mod common;
mod config;
mod db;
mod game;
mod html;
mod matchmaking;
mod middleware;
mod transactions;
mod validation;

use std::sync::Arc;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, ws::WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use tower_http::{
    LatencyUnit,
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use uuid::Uuid;

use automatafl_api_types::HealthResponse;
use common::{AppState, ServerState, timestamp};
use dashmap::DashMap;

const CARGO_PACKAGE_VERSION: Option<&str> = std::option_env!("CARGO_PACKAGE_VERSION");

// ============================================================================
// Health Check
// ============================================================================

/// Health check endpoint
/// Returns OK if the server and database are healthy
async fn health_check(State(state): ServerState) -> Result<Json<HealthResponse>, StatusCode> {
    // Check database connectivity by running a simple query
    match state.db.health().await {
        Ok(_) => Ok(Json(HealthResponse {
            status: "healthy".to_string(),
            api_version: "v1".to_string(),
            cargo_package_version: CARGO_PACKAGE_VERSION.map(|s| s.to_string()),
            timestamp: timestamp(),
        })),
        Err(e) => {
            tracing::error!("Health check failed - database unhealthy: {}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

// ============================================================================
// WebSocket Handler
// ============================================================================

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): ServerState,
    Path(game_uuid): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| common::handle_socket(socket, state, game_uuid))
}

// ============================================================================
// Tracing & Metrics Setup
// ============================================================================

/// Initialize structured logging
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();
}

/// Setup Prometheus metrics recorder
fn setup_metrics_recorder() -> metrics_exporter_prometheus::PrometheusHandle {
    use metrics_exporter_prometheus::{Matcher, PrometheusBuilder};

    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .expect("Failed to set metric buckets")
        .install_recorder()
        .expect("Failed to install metrics recorder");

    // Spawn upkeep task for metrics cleanup
    let upkeep_handle = handle.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            upkeep_handle.run_upkeep();
        }
    });

    handle
}

/// Track HTTP request metrics
async fn track_metrics(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::extract::MatchedPath;

    let start = std::time::Instant::now();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|mp| mp.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!(
        "http_requests_total",
        &[
            ("method", method.to_string()),
            ("path", path.clone()),
            ("status", status.clone()),
        ]
    )
    .increment(1);

    metrics::histogram!(
        "http_requests_duration_seconds",
        &[
            ("method", method.to_string()),
            ("path", path),
            ("status", status),
        ]
    )
    .record(latency);

    response
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(10 * 1024 * 1024) // 10MiB
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(async { server_main().await });
}

async fn server_main() {
    // Initialize structured logging
    init_tracing();

    // Load configuration
    let config = config::Config::from_env();
    config.log();

    // Setup metrics if enabled
    let metrics_handle = if config.enable_metrics {
        tracing::info!("Metrics enabled");
        Some(setup_metrics_recorder())
    } else {
        None
    };

    // Initialize database
    let db = db::init_db(Some(config.database_url.clone()))
        .await
        .expect("Failed to initialize database");

    tracing::info!("Database initialized");

    // Create application state
    let app_state = Arc::new(AppState {
        db,
        game_channels: Arc::new(DashMap::new()),
        config: Arc::new(config.clone()),
    });

    // Create the main router
    let mut app = Router::new()
        // HTML Frontend routes
        .route("/", get(html::html_index))
        .route(
            "/login",
            get(html::html_login_page).post(html::html_login_submit),
        )
        .route(
            "/register",
            get(html::html_register_page).post(html::html_register_submit),
        )
        .route("/logout", get(html::html_logout))
        .route("/dashboard", get(html::html_dashboard))
        .route("/games", get(html::html_games_list))
        .route("/game/:id", get(html::html_game_detail))
        .route("/game/:id/join", post(html::html_join_game))
        .route("/game/:id/move", post(html::html_submit_move))
        .route("/game/:id/complete", post(html::html_complete_round))
        .route(
            "/profile/:id",
            get(html::html_profile).post(html::html_update_profile),
        )
        .route(
            "/create-game",
            get(html::html_create_game_page).post(html::html_create_game_submit),
        )
        .route("/matchmaking", get(html::html_matchmaking_page))
        .route("/matchmaking/join", post(html::html_matchmaking_join))
        .route("/matchmaking/leave", post(html::html_matchmaking_leave))
        .route("/leaderboard", get(html::html_leaderboard))
        .route("/admin", get(html::html_admin_panel))
        .route("/admin/players", get(html::html_admin_players))
        .route("/admin/games", get(html::html_admin_games))
        .route(
            "/admin/games/:id/delete",
            post(html::html_admin_delete_game),
        )
        .route(
            "/admin/games/:id/force-complete",
            post(html::html_admin_force_complete),
        )
        .route("/admin/sessions", get(html::html_admin_sessions))
        .route(
            "/admin/sessions/:id/delete",
            post(html::html_admin_delete_session),
        )
        .route(
            "/admin/sessions/cleanup",
            post(html::html_admin_cleanup_sessions),
        )
        .route("/admin/queue", get(html::html_admin_queue))
        .route(
            "/admin/queue/:id/remove",
            post(html::html_admin_remove_from_queue),
        )
        .route("/admin/events", get(html::html_admin_events))
        .route(
            "/admin/query",
            get(html::html_admin_query_page).post(html::html_admin_query_execute),
        )
        // Health check (no auth required)
        .route("/api/health", get(health_check))
        // Auth endpoints
        .route("/api/v1/register", post(auth::register_player))
        .route("/api/v1/login", post(auth::login))
        .route("/api/v1/logout", post(auth::logout))
        // Game management endpoints
        .route(
            "/api/v1/games",
            get(game::list_games).post(game::create_game),
        )
        .route(
            "/api/v1/games/{:id}",
            get(game::get_game_state).post(game::join_game),
        )
        .route("/api/v1/games/{:id}/goals", get(game::get_goals))
        // Move endpoints
        .route(
            "/api/v1/games/{:id}/move",
            get(game::pending_move).post(game::perform_move),
        )
        .route("/api/v1/games/{:id}/complete", post(game::complete_round))
        // Chat endpoints
        .route(
            "/api/v1/games/{:id}/chat",
            get(game::get_chat).post(game::post_chat),
        )
        // Game history
        .route("/api/v1/games/{:id}/history", get(game::get_game_history))
        // Save/Load endpoints
        .route("/api/v1/games/{:id}/save", post(game::save_game))
        .route("/api/v1/games/{:id}/snapshots", get(game::list_snapshots))
        .route(
            "/api/v1/games/{:id}/load/{:snapshot_index}",
            post(game::load_game),
        )
        // WebSocket endpoint
        .route("/api/v1/games/{:id}/ws", get(ws_handler))
        // Profile endpoints
        .route(
            "/api/v1/players/{:id}",
            get(accounts::get_player_profile).put(accounts::update_player_profile),
        )
        .route(
            "/api/v1/players/{:id}/stats",
            get(accounts::get_player_stats),
        )
        // Leaderboard endpoints
        .route(
            "/api/v1/leaderboard/elo",
            get(matchmaking::get_leaderboard_elo),
        )
        .route(
            "/api/v1/leaderboard/wins",
            get(matchmaking::get_leaderboard_wins),
        )
        .route(
            "/api/v1/leaderboard/games",
            get(matchmaking::get_leaderboard_games),
        )
        // Matchmaking endpoints
        .route(
            "/api/v1/matchmaking/join",
            post(matchmaking::join_matchmaking),
        )
        .route(
            "/api/v1/matchmaking/leave",
            post(matchmaking::leave_matchmaking),
        )
        .route(
            "/api/v1/matchmaking/status",
            get(matchmaking::get_matchmaking_status),
        )
        // Admin endpoints - Player Management
        .route("/api/v1/admin/players", get(admin::admin_list_players))
        .route(
            "/api/v1/admin/players/:id",
            get(admin::admin_get_player)
                .put(admin::admin_update_player)
                .delete(admin::admin_delete_player),
        )
        // Admin endpoints - Game Management
        .route("/api/v1/admin/games", get(admin::admin_list_all_games))
        .route(
            "/api/v1/admin/games/:id",
            get(admin::admin_get_game).delete(admin::admin_delete_game),
        )
        .route(
            "/api/v1/admin/games/:id/force-complete",
            post(admin::admin_force_complete_round),
        )
        .route(
            "/api/v1/admin/games/:id/lifecycle",
            axum::routing::put(admin::admin_set_game_lifecycle),
        )
        // Admin endpoints - Session Management
        .route("/api/v1/admin/sessions", get(admin::admin_list_sessions))
        .route(
            "/api/v1/admin/sessions/:id",
            axum::routing::delete(admin::admin_delete_session),
        )
        .route(
            "/api/v1/admin/sessions/cleanup",
            post(admin::admin_cleanup_expired_sessions),
        )
        // Admin endpoints - Game Events & Chat
        .route(
            "/api/v1/admin/games/:id/events",
            get(admin::admin_get_game_events),
        )
        .route(
            "/api/v1/admin/games/:id/chat",
            get(admin::admin_get_game_chat),
        )
        .route(
            "/api/v1/admin/games/:id/chat/:timestamp",
            axum::routing::delete(admin::admin_delete_chat_message),
        )
        // Admin endpoints - Snapshots
        .route(
            "/api/v1/admin/games/:id/snapshots",
            get(admin::admin_list_snapshots),
        )
        .route(
            "/api/v1/admin/games/:id/snapshots/:index",
            axum::routing::delete(admin::admin_delete_snapshot),
        )
        // Admin endpoints - Matchmaking
        .route(
            "/api/v1/admin/matchmaking/queue",
            get(admin::admin_list_matchmaking_queue),
        )
        .route(
            "/api/v1/admin/matchmaking/queue/:id",
            axum::routing::delete(admin::admin_remove_from_matchmaking),
        )
        // Admin endpoints - Player Stats
        .route(
            "/api/v1/admin/players/:id/stats",
            get(admin::admin_get_player_stats).put(admin::admin_update_player_stats),
        )
        // Admin endpoints - Database Introspection
        .route("/api/v1/admin/stats", get(admin::admin_get_database_stats))
        .route("/api/v1/admin/tables", get(admin::admin_list_tables));

    // Add metrics endpoint if enabled
    if let Some(handle) = metrics_handle {
        let handle_clone = handle.clone();
        app = app.route(
            "/metrics",
            get(move || {
                let metrics = handle_clone.render();
                async move { metrics }
            }),
        );
        app = app.layer(axum::middleware::from_fn(track_metrics));
        tracing::info!("Metrics available at /metrics");
    }

    // Apply middleware layers (outer layers execute first)
    let app = app
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024)) // 2MB max request body
        .layer(axum::middleware::from_fn(middleware::csrf_protection))
        .layer(axum::middleware::from_fn(middleware::validate_content_type))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(axum::middleware::from_fn(middleware::track_slow_requests))
        .layer(axum::middleware::from_fn(
            middleware::enhance_error_response,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(tracing::Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                ),
        )
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer({
            use axum::http::{HeaderValue, Method};
            use tower_http::cors::{AllowOrigin, Any};

            // Configure CORS based on environment
            let cors = if cfg!(debug_assertions) {
                // Development: Allow localhost origins
                CorsLayer::new()
                    .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
                        origin.as_bytes().starts_with(b"http://localhost:")
                            || origin.as_bytes().starts_with(b"http://127.0.0.1:")
                    }))
                    .allow_methods([
                        Method::GET,
                        Method::POST,
                        Method::PUT,
                        Method::DELETE,
                        Method::PATCH,
                    ])
                    .allow_headers(Any)
                    .allow_credentials(true)
            } else {
                // Production: Restrict to specific origins from config
                // For now, allow same-origin only in production
                CorsLayer::new()
                    .allow_origin(AllowOrigin::predicate(|_origin, _| false))
                    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                    .allow_credentials(true)
            };
            cors
        })
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        .with_state(app_state.clone());

    // Spawn background matchmaking task
    let matchmaking_handle = tokio::spawn(matchmaking::matchmaking_task(app_state.clone()));

    // Spawn rate limiter cleanup task
    let rate_limiter_cleanup = middleware::spawn_rate_limiter_cleanup();

    // Spawn game channel cleanup task
    let game_channel_cleanup = common::spawn_game_channel_cleanup(app_state.clone());

    let addr = config.bind_address;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!("Server listening on {}", addr);
    tracing::info!("Press Ctrl+C to shutdown gracefully");

    // Serve with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    // Cleanup
    matchmaking_handle.abort();
    rate_limiter_cleanup.abort();
    game_channel_cleanup.abort();
    tracing::info!("Server shutdown complete");
}

/// Handle graceful shutdown signals
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received Ctrl+C, shutting down"),
        _ = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}
