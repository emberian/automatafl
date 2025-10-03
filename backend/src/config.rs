use std::{fmt, net::SocketAddr, time::Duration};

/// Configuration error types
#[derive(Debug)]
pub enum ConfigError {
    InvalidValue(String, String),
    ValidationError(String),
    EnvironmentError(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidValue(key, value) => {
                write!(
                    f,
                    "Invalid value '{}' for configuration key '{}'",
                    value, key
                )
            }
            ConfigError::ValidationError(msg) => {
                write!(f, "Configuration validation error: {}", msg)
            }
            ConfigError::EnvironmentError(msg) => write!(f, "Environment error: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::env::VarError> for ConfigError {
    fn from(err: std::env::VarError) -> Self {
        ConfigError::EnvironmentError(err.to_string())
    }
}

/// Enhanced configuration with validation and better defaults
#[derive(Debug, Clone)]
pub struct Config {
    /// Environment (development, production, testing)
    pub environment: Environment,

    /// Database type
    pub database_type: DatabaseType,

    /// Database connection URI
    pub database_url: String,

    /// Server bind address
    pub bind_address: SocketAddr,

    /// Session duration
    pub session_duration: Duration,

    /// Enable Prometheus metrics endpoint
    pub enable_metrics: bool,

    /// Metrics bind address (if metrics enabled)
    pub metrics_bind_address: SocketAddr,

    /// Matchmaking interval
    pub matchmaking_interval: Duration,

    /// ELO tolerance for matchmaking
    pub matchmaking_elo_tolerance: i32,

    /// Maximum number of concurrent games
    pub max_concurrent_games: usize,

    /// Maximum number of players per game
    pub max_players_per_game: u8,

    /// Rate limiting configuration
    pub rate_limiting: RateLimitingConfig,

    /// WebSocket configuration
    pub websocket: WebSocketConfig,

    /// Security configuration
    pub security: SecurityConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitingConfig {
    /// General API rate limit (requests per minute per IP)
    pub general_rpm: f64,

    /// Authentication endpoints rate limit
    pub auth_rpm: f64,

    /// Expensive operations rate limit
    pub expensive_rpm: f64,

    /// Matchmaking join rate limit
    pub matchmaking_rpm: f64,
}

/// WebSocket configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// WebSocket ping interval
    pub ping_interval: Duration,

    /// WebSocket timeout
    pub timeout: Duration,

    /// Maximum message size
    pub max_message_size: usize,

    /// Maximum concurrent connections per game
    pub max_connections_per_game: usize,
}

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable HTTPS redirect in production
    pub force_https: bool,

    /// Allowed origins for CORS (production)
    pub allowed_origins: Vec<String>,

    /// Session cookie security settings
    pub cookie_security: CookieSecurityConfig,
}

/// Cookie security configuration
#[derive(Debug, Clone)]
pub struct CookieSecurityConfig {
    /// Cookie domain
    pub domain: Option<String>,

    /// Secure flag (HTTPS only)
    pub secure: bool,

    /// HttpOnly flag
    pub http_only: bool,

    /// SameSite policy
    pub same_site: SameSitePolicy,
}

/// SameSite cookie policy
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SameSitePolicy {
    Strict,
    Lax,
    None,
}

impl Default for SameSitePolicy {
    fn default() -> Self {
        SameSitePolicy::Strict
    }
}

impl From<SameSitePolicy> for String {
    fn from(policy: SameSitePolicy) -> String {
        match policy {
            SameSitePolicy::Strict => "Strict".to_string(),
            SameSitePolicy::Lax => "Lax".to_string(),
            SameSitePolicy::None => "None".to_string(),
        }
    }
}

/// Environment type for configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
    Testing,
}

impl Default for Environment {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Environment::Development
        } else {
            Environment::Production
        }
    }
}

impl From<&str> for Environment {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "development" | "dev" => Environment::Development,
            "production" | "prod" => Environment::Production,
            "testing" | "test" => Environment::Testing,
            _ => Environment::Development, // Default fallback
        }
    }
}

/// Database type for configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseType {
    Embedded,
    External,
}

impl From<&str> for DatabaseType {
    fn from(s: &str) -> Self {
        if s.starts_with("surrealkv://") {
            DatabaseType::Embedded
        } else {
            DatabaseType::External
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let environment = Environment::default();
        let database_type = DatabaseType::Embedded;

        Self {
            environment,
            database_type,
            database_url: "surrealkv://automatafl.db".to_string(),
            bind_address: "127.0.0.1:3000".parse().unwrap(),
            session_duration: Duration::from_secs(24 * 60 * 60), // 24 hours
            enable_metrics: false,
            metrics_bind_address: "127.0.0.1:9090".parse().unwrap(),
            matchmaking_interval: Duration::from_secs(5),
            matchmaking_elo_tolerance: 200,
            max_concurrent_games: 1000,
            max_players_per_game: 4,
            rate_limiting: RateLimitingConfig::default(),
            websocket: WebSocketConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            general_rpm: 100.0,
            auth_rpm: 10.0,
            expensive_rpm: 5.0,
            matchmaking_rpm: 5.0,
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(30),
            timeout: Duration::from_secs(60),
            max_message_size: 64 * 1024, // 64KB
            max_connections_per_game: 10,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            force_https: false,
            allowed_origins: vec![],
            cookie_security: CookieSecurityConfig::default(),
        }
    }
}

impl Default for CookieSecurityConfig {
    fn default() -> Self {
        Self {
            domain: None,
            secure: false,
            http_only: true,
            same_site: SameSitePolicy::Strict,
        }
    }
}

impl Config {
    /// Load configuration from environment variables with validation
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Self::default();

        // Environment configuration
        if let Ok(env) = std::env::var("ENVIRONMENT") {
            config.environment = Environment::from(env.as_str());
        }

        // Database configuration
        if let Ok(url) = std::env::var("DATABASE_URL") {
            config.database_url = url;
            config.database_type = DatabaseType::from(config.database_url.as_str());
        }

        // Server configuration
        if let Ok(addr) = std::env::var("BIND_ADDRESS") {
            config.bind_address = addr
                .parse()
                .map_err(|_| ConfigError::InvalidValue("BIND_ADDRESS".to_string(), addr.clone()))?;
        }

        // Session configuration
        if let Ok(duration) = std::env::var("SESSION_DURATION") {
            let secs = duration.parse::<u64>().map_err(|_| {
                ConfigError::InvalidValue("SESSION_DURATION".to_string(), duration.clone())
            })?;
            config.session_duration = Duration::from_secs(secs);
        }

        // Metrics configuration
        if let Ok(enable) = std::env::var("ENABLE_METRICS") {
            config.enable_metrics = enable.to_lowercase() == "true" || enable == "1";
        }

        if let Ok(addr) = std::env::var("METRICS_BIND_ADDRESS") {
            config.metrics_bind_address = addr.parse().map_err(|_| {
                ConfigError::InvalidValue("METRICS_BIND_ADDRESS".to_string(), addr.clone())
            })?;
        }

        // Matchmaking configuration
        if let Ok(interval) = std::env::var("MATCHMAKING_INTERVAL") {
            let secs = interval.parse::<u64>().map_err(|_| {
                ConfigError::InvalidValue("MATCHMAKING_INTERVAL".to_string(), interval.clone())
            })?;
            config.matchmaking_interval = Duration::from_secs(secs);
        }

        if let Ok(tolerance) = std::env::var("MATCHMAKING_ELO_TOLERANCE") {
            config.matchmaking_elo_tolerance = tolerance.parse().map_err(|_| {
                ConfigError::InvalidValue(
                    "MATCHMAKING_ELO_TOLERANCE".to_string(),
                    tolerance.clone(),
                )
            })?;
        }

        // Rate limiting configuration
        if let Ok(rpm) = std::env::var("RATE_LIMIT_GENERAL_RPM") {
            config.rate_limiting.general_rpm = rpm.parse().map_err(|_| {
                ConfigError::InvalidValue("RATE_LIMIT_GENERAL_RPM".to_string(), rpm.clone())
            })?;
        }

        if let Ok(rpm) = std::env::var("RATE_LIMIT_AUTH_RPM") {
            config.rate_limiting.auth_rpm = rpm.parse().map_err(|_| {
                ConfigError::InvalidValue("RATE_LIMIT_AUTH_RPM".to_string(), rpm.clone())
            })?;
        }

        if let Ok(rpm) = std::env::var("RATE_LIMIT_EXPENSIVE_RPM") {
            config.rate_limiting.expensive_rpm = rpm.parse().map_err(|_| {
                ConfigError::InvalidValue("RATE_LIMIT_EXPENSIVE_RPM".to_string(), rpm.clone())
            })?;
        }

        if let Ok(rpm) = std::env::var("RATE_LIMIT_MATCHMAKING_RPM") {
            config.rate_limiting.matchmaking_rpm = rpm.parse().map_err(|_| {
                ConfigError::InvalidValue("RATE_LIMIT_MATCHMAKING_RPM".to_string(), rpm.clone())
            })?;
        }

        // WebSocket configuration
        if let Ok(interval) = std::env::var("WEBSOCKET_PING_INTERVAL") {
            let secs = interval.parse::<u64>().map_err(|_| {
                ConfigError::InvalidValue("WEBSOCKET_PING_INTERVAL".to_string(), interval.clone())
            })?;
            config.websocket.ping_interval = Duration::from_secs(secs);
        }

        if let Ok(timeout) = std::env::var("WEBSOCKET_TIMEOUT") {
            let secs = timeout.parse::<u64>().map_err(|_| {
                ConfigError::InvalidValue("WEBSOCKET_TIMEOUT".to_string(), timeout.clone())
            })?;
            config.websocket.timeout = Duration::from_secs(secs);
        }

        if let Ok(max_size) = std::env::var("WEBSOCKET_MAX_MESSAGE_SIZE") {
            config.websocket.max_message_size = max_size.parse().map_err(|_| {
                ConfigError::InvalidValue(
                    "WEBSOCKET_MAX_MESSAGE_SIZE".to_string(),
                    max_size.clone(),
                )
            })?;
        }

        if let Ok(max_conn) = std::env::var("WEBSOCKET_MAX_CONNECTIONS_PER_GAME") {
            config.websocket.max_connections_per_game = max_conn.parse().map_err(|_| {
                ConfigError::InvalidValue(
                    "WEBSOCKET_MAX_CONNECTIONS_PER_GAME".to_string(),
                    max_conn.clone(),
                )
            })?;
        }

        // Security configuration
        if let Ok(origins) = std::env::var("ALLOWED_ORIGINS") {
            config.security.allowed_origins =
                origins.split(',').map(|s| s.trim().to_string()).collect();
        }

        if let Ok(secure) = std::env::var("COOKIE_SECURE") {
            config.security.cookie_security.secure =
                secure.to_lowercase() == "true" || secure == "1";
        }

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Validate the configuration for consistency and security
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate database URL
        if self.database_url.is_empty() {
            return Err(ConfigError::ValidationError(
                "Database URL cannot be empty".to_string(),
            ));
        }

        // Validate session duration
        if self.session_duration < Duration::from_secs(300) {
            return Err(ConfigError::ValidationError(
                "Session duration must be at least 5 minutes".to_string(),
            ));
        }
        if self.session_duration > Duration::from_secs(30 * 24 * 60 * 60) {
            return Err(ConfigError::ValidationError(
                "Session duration cannot exceed 30 days".to_string(),
            ));
        }

        // Validate matchmaking interval
        if self.matchmaking_interval < Duration::from_secs(1) {
            return Err(ConfigError::ValidationError(
                "Matchmaking interval must be at least 1 second".to_string(),
            ));
        }
        if self.matchmaking_interval > Duration::from_secs(300) {
            return Err(ConfigError::ValidationError(
                "Matchmaking interval cannot exceed 5 minutes".to_string(),
            ));
        }

        // Validate ELO tolerance
        if self.matchmaking_elo_tolerance < 0 {
            return Err(ConfigError::ValidationError(
                "ELO tolerance cannot be negative".to_string(),
            ));
        }
        if self.matchmaking_elo_tolerance > 1000 {
            return Err(ConfigError::ValidationError(
                "ELO tolerance cannot exceed 1000".to_string(),
            ));
        }

        // Validate rate limits
        if self.rate_limiting.general_rpm <= 0.0 {
            return Err(ConfigError::ValidationError(
                "General rate limit must be positive".to_string(),
            ));
        }
        if self.rate_limiting.auth_rpm <= 0.0 {
            return Err(ConfigError::ValidationError(
                "Auth rate limit must be positive".to_string(),
            ));
        }

        // Validate WebSocket settings
        if self.websocket.ping_interval < Duration::from_secs(10) {
            return Err(ConfigError::ValidationError(
                "WebSocket ping interval must be at least 10 seconds".to_string(),
            ));
        }
        if self.websocket.timeout < self.websocket.ping_interval * 2 {
            return Err(ConfigError::ValidationError(
                "WebSocket timeout must be at least twice the ping interval".to_string(),
            ));
        }

        // Validate security settings based on environment
        match self.environment {
            Environment::Production => {
                if self.security.cookie_security.secure && self.security.allowed_origins.is_empty()
                {
                    return Err(ConfigError::ValidationError(
                        "Production environment with secure cookies requires allowed origins to be configured".to_string(),
                    ));
                }
            }
            Environment::Development => {
                // More lenient validation for development
                if self.websocket.ping_interval < Duration::from_secs(5) {
                    return Err(ConfigError::ValidationError(
                        "Development WebSocket ping interval must be at least 5 seconds"
                            .to_string(),
                    ));
                }
            }
            Environment::Testing => {
                // Strict validation for testing
                if self.max_concurrent_games > 10 {
                    return Err(ConfigError::ValidationError(
                        "Testing environment should have limited concurrent games".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Print configuration for debugging (only non-sensitive information)
    pub fn log(&self) {
        tracing::info!("Configuration:");
        tracing::info!("  Environment: {:?}", self.environment);
        tracing::info!("  Database type: {:?}", self.database_type);
        tracing::info!("  Database URL: {}", self.database_url);
        tracing::info!("  Bind address: {}", self.bind_address);
        tracing::info!("  Session duration: {:?}", self.session_duration);
        tracing::info!("  Metrics enabled: {}", self.enable_metrics);
        if self.enable_metrics {
            tracing::info!("  Metrics address: {}", self.metrics_bind_address);
        }
        tracing::info!("  Matchmaking interval: {:?}", self.matchmaking_interval);
        tracing::info!(
            "  Matchmaking ELO tolerance: {}",
            self.matchmaking_elo_tolerance
        );
        tracing::info!("  Max concurrent games: {}", self.max_concurrent_games);
        tracing::info!("  Max players per game: {}", self.max_players_per_game);
        tracing::info!(
            "  Rate limiting - General: {} RPM",
            self.rate_limiting.general_rpm
        );
        tracing::info!(
            "  Rate limiting - Auth: {} RPM",
            self.rate_limiting.auth_rpm
        );
        tracing::info!(
            "  Rate limiting - Expensive: {} RPM",
            self.rate_limiting.expensive_rpm
        );
        tracing::info!(
            "  Rate limiting - Matchmaking: {} RPM",
            self.rate_limiting.matchmaking_rpm
        );
        tracing::info!(
            "  WebSocket ping interval: {:?}",
            self.websocket.ping_interval
        );
        tracing::info!("  WebSocket timeout: {:?}", self.websocket.timeout);
        tracing::info!(
            "  WebSocket max message size: {}",
            self.websocket.max_message_size
        );
        tracing::info!("  Security - Force HTTPS: {}", self.security.force_https);
        tracing::info!(
            "  Security - Allowed origins: {:?}",
            self.security.allowed_origins
        );
    }
}
