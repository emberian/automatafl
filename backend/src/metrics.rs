use metrics::{counter, gauge, histogram, Unit};
use std::time::Instant;

// Initialize metrics
pub fn init_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    let handle = builder.install_recorder()?;
    
    // Store the handle for later use in the metrics endpoint
    let handle_clone = handle.clone();
    std::thread::spawn(move || {
        let _ = METRICS_HANDLE.set(handle_clone);
    });
    
    // Register metrics with descriptions
    metrics::describe_counter!("http_requests_total", Unit::Count, "Total number of HTTP requests");
    metrics::describe_counter!("http_errors_total", Unit::Count, "Total number of HTTP errors");
    metrics::describe_counter!("websocket_connections_total", Unit::Count, "Total number of WebSocket connections");
    metrics::describe_counter!("games_created_total", Unit::Count, "Total number of games created");
    metrics::describe_counter!("moves_submitted_total", Unit::Count, "Total number of moves submitted");
    metrics::describe_counter!("chat_messages_sent_total", Unit::Count, "Total number of chat messages sent");
    
    metrics::describe_histogram!("http_request_duration_seconds", Unit::Seconds, "HTTP request duration");
    metrics::describe_histogram!("database_query_duration_seconds", Unit::Seconds, "Database query duration");
    
    metrics::describe_gauge!("active_games", Unit::Count, "Number of currently active games");
    metrics::describe_gauge!("websocket_connections_active", Unit::Count, "Number of active WebSocket connections");
    metrics::describe_gauge!("database_pool_size", Unit::Count, "Database connection pool size");
    
    Ok(())
}

// Global handle for Prometheus metrics
static METRICS_HANDLE: std::sync::OnceLock<metrics_exporter_prometheus::PrometheusHandle> = std::sync::OnceLock::new();

pub fn get_metrics_handle() -> Option<&'static metrics_exporter_prometheus::PrometheusHandle> {
    METRICS_HANDLE.get()
}

// Metrics collection helpers
pub fn record_http_request(method: &str, path: &str, status: u16) {
    counter!("http_requests_total", 
        "method" => method.to_string(),
        "path" => path.to_string(),
        "status" => status.to_string()
    ).increment(1);
    
    if status >= 400 {
        counter!("http_errors_total",
            "method" => method.to_string(),
            "path" => path.to_string(),
            "status" => status.to_string()
        ).increment(1);
    }
}

pub fn record_websocket_connection(action: &str) {
    match action {
        "connect" => {
            counter!("websocket_connections_total").increment(1);
            gauge!("websocket_connections_active").increment(1.0);
        }
        "disconnect" => {
            gauge!("websocket_connections_active").decrement(1.0);
        }
        _ => {}
    }
}

pub fn record_game_created() {
    counter!("games_created_total").increment(1);
    gauge!("active_games").increment(1.0);
}

pub fn record_game_completed() {
    gauge!("active_games").decrement(1.0);
}

pub fn record_move_submitted() {
    counter!("moves_submitted_total").increment(1);
}

pub fn record_chat_message_sent() {
    counter!("chat_messages_sent_total").increment(1);
}

// Simple timing helper
pub fn record_duration(metric_name: &'static str, duration: f64) {
    histogram!(metric_name).record(duration);
}

// Middleware for automatic HTTP metrics
pub async fn metrics_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().to_string();
    
    // Use matched path template to avoid cardinality explosion
    let path = if let Some(matched_path) = req.extensions().get::<axum::extract::MatchedPath>() {
        matched_path.as_str().to_string()
    } else {
        // Fallback: normalize paths with UUIDs to avoid cardinality issues
        normalize_path(req.uri().path())
    };
    
    let start = Instant::now();
    
    let response = next.run(req).await;
    let status = response.status().as_u16();
    let duration = start.elapsed().as_secs_f64();
    
    record_http_request(&method, &path, status);
    
    // Record duration without labels to avoid lifetime issues
    histogram!("http_request_duration_seconds").record(duration);
    
    response
}

// Helper to normalize paths containing UUIDs
fn normalize_path(path: &str) -> String {
    // Simple UUID pattern matching - replace UUIDs with placeholder
    let uuid_regex = regex::Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap();
    uuid_regex.replace_all(path, ":id").to_string()
}
