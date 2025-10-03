use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Config {
    /// Database connection URI
    pub database_url: String,

    /// Server bind address
    pub bind_address: SocketAddr,

    /// Session duration in seconds
    pub session_duration: u64,

    /// Enable Prometheus metrics endpoint
    pub enable_metrics: bool,

    /// Metrics bind address (if metrics enabled)
    pub metrics_bind_address: String,

    /// Matchmaking interval in seconds
    pub matchmaking_interval: u64,

    /// ELO tolerance for matchmaking
    pub matchmaking_elo_tolerance: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "surrealkv://automatafl.db".to_string(),
            bind_address: "127.0.0.1:3000".parse().unwrap(),
            session_duration: 24 * 60 * 60, // 24 hours
            enable_metrics: false,
            metrics_bind_address: "127.0.0.1:9090".to_string(),
            matchmaking_interval: 5,
            matchmaking_elo_tolerance: 200,
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(url) = std::env::var("DATABASE_URL") {
            config.database_url = url;
        }

        if let Ok(addr) = std::env::var("BIND_ADDRESS") {
            if let Ok(parsed) = addr.parse() {
                config.bind_address = parsed;
            } else {
                tracing::warn!("Invalid BIND_ADDRESS: {}, using default", addr);
            }
        }

        if let Ok(duration) = std::env::var("SESSION_DURATION") {
            if let Ok(secs) = duration.parse() {
                config.session_duration = secs;
            } else {
                tracing::warn!("Invalid SESSION_DURATION: {}, using default", duration);
            }
        }

        if let Ok(enable) = std::env::var("ENABLE_METRICS") {
            config.enable_metrics = enable.to_lowercase() == "true" || enable == "1";
        }

        if let Ok(addr) = std::env::var("METRICS_BIND_ADDRESS") {
            config.metrics_bind_address = addr;
        }

        if let Ok(interval) = std::env::var("MATCHMAKING_INTERVAL") {
            if let Ok(secs) = interval.parse() {
                config.matchmaking_interval = secs;
            } else {
                tracing::warn!("Invalid MATCHMAKING_INTERVAL: {}, using default", interval);
            }
        }

        if let Ok(tolerance) = std::env::var("MATCHMAKING_ELO_TOLERANCE") {
            if let Ok(val) = tolerance.parse() {
                config.matchmaking_elo_tolerance = val;
            } else {
                tracing::warn!("Invalid MATCHMAKING_ELO_TOLERANCE: {}, using default", tolerance);
            }
        }

        config
    }

    /// Print configuration for debugging
    pub fn log(&self) {
        tracing::info!("Configuration:");
        tracing::info!("  Database URL: {}", self.database_url);
        tracing::info!("  Bind address: {}", self.bind_address);
        tracing::info!("  Session duration: {}s", self.session_duration);
        tracing::info!("  Metrics enabled: {}", self.enable_metrics);
        if self.enable_metrics {
            tracing::info!("  Metrics address: {}", self.metrics_bind_address);
        }
        tracing::info!("  Matchmaking interval: {}s", self.matchmaking_interval);
        tracing::info!("  Matchmaking ELO tolerance: {}", self.matchmaking_elo_tolerance);
    }
}
