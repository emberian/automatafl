mod abandonment;
mod auth;
mod db;
mod db_models;
mod error;
mod handlers;
mod matchmaking;
mod metrics;
mod models;
mod state;

use axum::{
    http,
    routing::{get, post},
    Router,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tower_http::{
    cors::{CorsLayer, Any},
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
    request_id::{MakeRequestId, RequestId, SetRequestIdLayer},
};
use tracing::{info, Level};
use uuid::Uuid;

use crate::state::AppState;

// Custom request ID generator
#[derive(Clone)]
struct UuidRequestIdMaker;

impl MakeRequestId for UuidRequestIdMaker {
    fn make_request_id<B>(&mut self, _request: &http::Request<B>) -> Option<RequestId> {
        let request_id = Uuid::new_v4().to_string();
        Some(RequestId::new(request_id.parse().ok()?))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Initialize health check
    handlers::health::init_health_check();
    
    // Initialize metrics
    metrics::init_metrics().expect("Failed to initialize metrics");

    // Create database pool
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:automatafl.db".to_string());
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Enable WAL mode for SQLite (immediate fix for concurrency)
    sqlx::query!("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await?;
    sqlx::query!("PRAGMA busy_timeout=5000")
        .execute(&pool)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Create app state
    let state = AppState::new(pool);

    // Build our application with routes
    let app = Router::new()
        // Auth routes
        .route("/api/auth/register", post(handlers::auth::register))
        .route("/api/auth/login", post(handlers::auth::login))
        .route("/api/auth/me", get(handlers::auth::me))
        
        // Game routes
        .route("/api/games", get(handlers::game::list_games))
        .route("/api/games", post(handlers::game::create_game))
        .route("/api/games/:game_id", get(handlers::game::get_game))
        .route("/api/games/:game_id/join", post(handlers::game::join_game))
        .route("/api/games/:game_id/move", post(handlers::game::submit_move))
        .route("/api/games/:game_id/spectate", get(handlers::game::spectate_game))
        
        // Chat routes
        .route("/api/games/:game_id/chat", get(handlers::chat::get_messages))
        .route("/api/games/:game_id/chat", post(handlers::chat::send_message))
        
        // History routes
        .route("/api/games/:game_id/history", get(handlers::history::get_game_history))
        .route("/api/users/:user_id/games", get(handlers::history::get_user_games))
        
        // Leaderboard route
        .route("/api/leaderboard", get(handlers::leaderboard::get_leaderboard))
        
        // User profile route
        .route("/api/users/:user_id/profile", get(handlers::profile::get_user_profile))
        
        // Matchmaking routes
        .route("/api/matchmaking/join", post(handlers::matchmaking::join_matchmaking))
        .route("/api/matchmaking/leave", post(handlers::matchmaking::leave_matchmaking))
        .route("/api/matchmaking/status", get(handlers::matchmaking::matchmaking_status))
        
        // WebSocket route
        .route("/api/ws", get(handlers::websocket::websocket_handler))
        
        // Health check routes
        .route("/api/health", get(handlers::health::health_check))
        .route("/health", get(handlers::health::health_dashboard))
        
        // Metrics endpoint
        .route("/metrics", get(handlers::metrics::metrics_handler));
        
    // Configure CORS based on environment
    let cors = if std::env::var("PRODUCTION").is_ok() {
        // Production: restrict to specific origin
        let allowed_origin = std::env::var("ALLOWED_ORIGIN")
            .expect("ALLOWED_ORIGIN must be set in production");
        CorsLayer::new()
            .allow_origin(
                allowed_origin.parse::<http::HeaderValue>()
                    .expect("Invalid ALLOWED_ORIGIN")
            )
            .allow_methods([http::Method::GET, http::Method::POST, http::Method::OPTIONS])
            .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION])
            .allow_credentials(true)
    } else {
        // Development: allow any origin
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };
    
    // Add request ID generation and tracing
    let app = app
        .layer(axum::middleware::from_fn(metrics::metrics_middleware))
        .layer(SetRequestIdLayer::x_request_id(UuidRequestIdMaker))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let request_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("unknown");
                    
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = %request_id,
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO))
        )
        .layer(cors)
        .with_state(state.clone());

    // Start matchmaking worker
    let matchmaking_state = state.clone();
    tokio::spawn(async move {
        handlers::matchmaking::matchmaking_worker(matchmaking_state).await;
    });
    
    // Start abandonment detection worker
    let abandonment_state = state.clone();
    tokio::spawn(async move {
        abandonment::abandonment_worker(abandonment_state).await;
    });
    
    // Run the server
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid u16");
    let addr = SocketAddr::from((host.parse::<std::net::IpAddr>().expect("Invalid HOST"), port));
    
    info!("Server listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}