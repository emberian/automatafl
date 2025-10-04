//! Essential middleware for security, validation, and observability

use axum::{
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

// ============================================================================
// Rate Limiting
// ============================================================================

/// Simple token bucket rate limiter
#[derive(Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    fn try_consume(&mut self) -> bool {
        // Refill tokens based on time elapsed
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        // Try to consume a token
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

static RATE_LIMITERS: OnceLock<Arc<DashMap<String, TokenBucket>>> = OnceLock::new();

fn get_rate_limiters() -> &'static Arc<DashMap<String, TokenBucket>> {
    RATE_LIMITERS.get_or_init(|| Arc::new(DashMap::new()))
}

/// Spawn a background task to periodically clean up old rate limiter entries
pub fn spawn_rate_limiter_cleanup() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600)); // Every hour
        loop {
            interval.tick().await;
            let limiters = get_rate_limiters();

            // Remove entries older than 1 hour
            let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(3600);
            limiters.retain(|_key, bucket| bucket.last_refill > cutoff);

            tracing::debug!(
                "Rate limiter cleanup completed, {} entries remaining",
                limiters.len()
            );
        }
    })
}

/// Rate limiting middleware
/// Default: 100 requests per minute per IP
pub async fn rate_limit(req: Request, next: Next) -> Result<Response, StatusCode> {
    // Get client IP from various headers
    let client_ip = extract_client_ip(&req.headers());

    let limiters = get_rate_limiters();

    // Get or create rate limiter for this IP
    let mut entry = limiters
        .entry(client_ip.clone())
        .or_insert_with(|| TokenBucket::new(100.0, 100.0 / 60.0)); // 100 requests per minute

    if entry.try_consume() {
        Ok(next.run(req).await)
    } else {
        tracing::warn!("Rate limit exceeded for IP: {}", client_ip);
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

/// Extract client IP from request headers with proper fallbacks
fn extract_client_ip(headers: &HeaderMap) -> String {
    // Try X-Real-IP first (set by proxies like nginx)
    if let Some(ip) = headers
        .get("x-real-ip")
        .and_then(|h| h.to_str().ok())
        .filter(|ip| !ip.is_empty() && *ip != "unknown")
    {
        return ip.to_string();
    }

    // Try X-Forwarded-For (can contain multiple IPs, take the first)
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .filter(|xff| !xff.is_empty())
    {
        if let Some(first_ip) = xff.split(',').next().map(|ip| ip.trim()) {
            if !first_ip.is_empty() && first_ip != "unknown" {
                return first_ip.to_string();
            }
        }
    }

    // Try CF-Connecting-IP (Cloudflare specific)
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|h| h.to_str().ok())
        .filter(|ip| !ip.is_empty())
    {
        return ip.to_string();
    }

    // Fallback to unknown
    "unknown".to_string()
}

/// Stricter rate limiting for auth endpoints
/// 10 requests per minute per IP
pub async fn auth_rate_limit(req: Request, next: Next) -> Result<Response, StatusCode> {
    let client_ip = req
        .headers()
        .get("x-real-ip")
        .or_else(|| req.headers().get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let limiters = get_rate_limiters();

    let key = format!("auth:{}", client_ip);
    let mut entry = limiters
        .entry(key.clone())
        .or_insert_with(|| TokenBucket::new(10.0, 10.0 / 60.0)); // 10 requests per minute

    if entry.try_consume() {
        Ok(next.run(req).await)
    } else {
        tracing::warn!("Auth rate limit exceeded for IP: {}", client_ip);
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

/// Check rate limit for expensive operations (for use in handlers)
/// Returns Ok(()) if allowed, Err(()) if rate limited
pub fn check_rate_limit_expensive(
    key: &str,
    capacity: f64,
    refill_rate: f64,
) -> Result<(), StatusCode> {
    let limiters = get_rate_limiters();

    let rate_key = format!("expensive:{}", key);
    let mut entry = limiters
        .entry(rate_key.clone())
        .or_insert_with(|| TokenBucket::new(capacity, refill_rate));

    if entry.try_consume() {
        Ok(())
    } else {
        tracing::warn!("Rate limit exceeded for expensive operation: {}", key);
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

/// Enhanced rate limiting that returns proper error information
pub fn check_rate_limit_with_error(
    key: &str,
    capacity: f64,
    refill_rate: f64,
) -> Result<(), crate::common::AppError> {
    let limiters = get_rate_limiters();

    let rate_key = format!("expensive:{}", key);
    let mut entry = limiters
        .entry(rate_key.clone())
        .or_insert_with(|| TokenBucket::new(capacity, refill_rate));

    if entry.try_consume() {
        Ok(())
    } else {
        tracing::warn!("Rate limit exceeded for expensive operation: {}", key);
        Err(crate::common::AppError::RateLimited(
            "Too many requests, please slow down".to_string(),
        ))
    }
}

// ============================================================================
// CSRF Protection
// ============================================================================

/// Enhanced CSRF protection for HTML form submissions
/// Uses multiple layers of defense:
/// 1. SameSite=Strict cookies (primary defense)
/// 2. Origin/Referer header validation
/// 3. Content-Type validation for form submissions
/// 4. Request size limits
pub async fn csrf_protection(req: Request, next: Next) -> Result<Response, StatusCode> {
    let method = req.method();
    let path = req.uri().path();
    let headers = req.headers();

    // Skip CSRF checks for public authentication endpoints that cannot have a
    // session cookie yet (e.g. login form submissions).
    if is_csrf_exempt(method, path) {
        return Ok(next.run(req).await);
    }

    // Only check state-changing methods for non-API routes
    if !matches!(method.as_str(), "POST" | "PUT" | "DELETE" | "PATCH") || path.starts_with("/api/")
    {
        return Ok(next.run(req).await);
    }

    // For form submissions, ensure proper content type
    if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
            if let Ok(ct) = content_type.to_str() {
                // Allow form submissions and JSON
                if !ct.starts_with("application/x-www-form-urlencoded")
                    && !ct.starts_with("application/json")
                    && !ct.starts_with("multipart/form-data")
                {
                    tracing::warn!(path, content_type = %ct, "Suspicious content type for state-changing request");
                    return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
                }
            }
        }
    }

    // Check Origin header for same-origin policy
    if let Some(origin) = headers.get(header::ORIGIN) {
        if let Ok(origin_str) = origin.to_str() {
            // In production, we should validate against known origins
            // For development, allow localhost
            if !is_allowed_origin(origin_str) {
                tracing::warn!(path, origin = %origin_str, "Origin not allowed");
                return Err(StatusCode::FORBIDDEN);
            }
        }
    } else {
        // If no Origin header, check Referer as fallback
        if let Some(referer) = headers.get(header::REFERER) {
            if let Ok(referer_str) = referer.to_str() {
                if !is_allowed_referer(referer_str) {
                    tracing::warn!(path, referer = %referer_str, "Referer not allowed");
                    return Err(StatusCode::FORBIDDEN);
                }
            }
        }
    }

    // If we have a session cookie, allow (SameSite=Strict provides CSRF protection)
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            if cookie_str.contains("automatafl_session=") {
                return Ok(next.run(req).await);
            }
        }
    }

    // For requests without session cookies, be more strict
    tracing::warn!(path, method = %method, "State-changing request without session cookie");
    Err(StatusCode::UNAUTHORIZED)
}

fn is_csrf_exempt(method: &Method, path: &str) -> bool {
    const EXEMPT_ROUTES: &[(&str, &str)] = &[("POST", "/login"), ("POST", "/register")];

    EXEMPT_ROUTES.iter().any(|(allowed_method, allowed_path)| {
        allowed_path == &path && method.as_str().eq_ignore_ascii_case(allowed_method)
    })
}

/// Check if origin is allowed for CSRF protection
fn is_allowed_origin(origin: &str) -> bool {
    if cfg!(debug_assertions) {
        // Development: Allow localhost origins
        origin.starts_with("http://localhost:")
            || origin.starts_with("http://127.0.0.1:")
            || origin.starts_with("https://localhost:")
            || origin.starts_with("https://127.0.0.1:")
    } else {
        // Production: Should validate against configured allowed origins
        // For now, be strict and only allow if explicitly configured
        std::env::var("ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .any(|allowed| allowed.trim() == origin)
    }
}

/// Check if referer is allowed for CSRF protection
fn is_allowed_referer(referer: &str) -> bool {
    if cfg!(debug_assertions) {
        // Development: Allow localhost referers
        referer.starts_with("http://localhost:")
            || referer.starts_with("http://127.0.0.1:")
            || referer.starts_with("https://localhost:")
            || referer.starts_with("https://127.0.0.1:")
    } else {
        // Production: Should validate against configured allowed referers
        std::env::var("ALLOWED_REFERERS")
            .unwrap_or_default()
            .split(',')
            .any(|allowed| referer.starts_with(allowed.trim()))
    }
}

// ============================================================================
// Security Headers
// ============================================================================

/// Add comprehensive security headers to all responses
pub async fn security_headers(mut req: Request, next: Next) -> Response {
    // Extract path before consuming request
    let is_api_request = req.uri().path().starts_with("/api/");

    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // HSTS - Force HTTPS in production
    if !cfg!(debug_assertions) {
        headers.insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
        );
    }

    // Prevent clickjacking
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));

    // Prevent MIME type sniffing
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );

    // XSS Protection
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );

    // Enhanced Content Security Policy
    let csp = if cfg!(debug_assertions) {
        // Development: Allow localhost WebSocket connections
        "default-src 'self'; \
         script-src 'self' 'unsafe-inline' 'unsafe-eval'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: https: http:; \
         font-src 'self' data:; \
         connect-src 'self' ws://localhost:* wss://localhost:* ws://127.0.0.1:* wss://127.0.0.1:*; \
         frame-ancestors 'none'; \
         base-uri 'self'; \
         form-action 'self'; \
         upgrade-insecure-requests;"
    } else {
        // Production: Strict CSP
        "default-src 'self'; \
         script-src 'self'; \
         style-src 'self'; \
         img-src 'self' data: https:; \
         font-src 'self' data:; \
         connect-src 'self' wss:; \
         frame-ancestors 'none'; \
         base-uri 'self'; \
         form-action 'self'; \
         upgrade-insecure-requests; \
         block-all-mixed-content;"
    };

    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_str(csp).unwrap(),
    );

    // Referrer policy
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // Permissions policy - disable unnecessary features
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static(
            "geolocation=(), microphone=(), camera=(), payment=(), usb=(), \
             bluetooth=(), magnetometer=(), gyroscope=(), accelerometer=(), \
             ambient-light-sensor=(), autoplay=(), encrypted-media=(), \
             fullscreen=(self), picture-in-picture=()",
        ),
    );

    // Remove server information
    headers.insert("Server", HeaderValue::from_static(""));
    headers.remove("X-Powered-By");

    // Cache control for API responses
    if is_api_request {
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
        headers.insert("Pragma", HeaderValue::from_static("no-cache"));
        headers.insert("Expires", HeaderValue::from_static("0"));
    }

    response
}

// ============================================================================
// Input Validation & Security
// ============================================================================

/// Enhanced input validation for all requests
pub async fn validate_request_security(req: Request, next: Next) -> Result<Response, StatusCode> {
    let method = req.method();
    let path = req.uri().path();
    let headers = req.headers();

    // Validate request size (prevent DoS attacks)
    if let Some(content_length) = headers.get("content-length") {
        if let Ok(length) = content_length.to_str().unwrap_or("0").parse::<usize>() {
            // Different limits for different request types
            let max_size = match method.as_str() {
                "POST" | "PUT" | "PATCH" => {
                    if path.starts_with("/api/") {
                        2 * 1024 * 1024 // 2MB for API requests
                    } else {
                        10 * 1024 * 1024 // 10MB for file uploads
                    }
                }
                _ => 1 * 1024 * 1024, // 1MB for other requests
            };

            if length > max_size {
                tracing::warn!(
                    path,
                    content_length = length,
                    max_size,
                    "Request size exceeds limit"
                );
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
        }
    }

    // Validate User-Agent header (helps with bot detection)
    if let Some(user_agent) = headers.get("user-agent") {
        if let Ok(ua) = user_agent.to_str() {
            // Check for suspicious user agents
            if ua.is_empty() || ua.len() > 500 || ua.contains('\0') {
                tracing::warn!(path, user_agent = %ua, "Suspicious user agent");
                return Err(StatusCode::BAD_REQUEST);
            }
        }
    }

    // Validate Accept header for API requests
    if path.starts_with("/api/") {
        if let Some(accept) = headers.get("accept") {
            if let Ok(accept_str) = accept.to_str() {
                // Only allow expected content types for API responses
                if !accept_str.contains("application/json")
                    && !accept_str.contains("text/html")
                    && !accept_str.contains("*/*")
                {
                    tracing::warn!(path, accept = %accept_str, "Unexpected accept header for API request");
                }
            }
        }
    }

    // Log security-relevant events for audit purposes
    if matches!(method.as_str(), "POST" | "PUT" | "DELETE" | "PATCH") {
        if path.starts_with("/api/v1/auth") || path.starts_with("/api/v1/admin") {
            let client_ip = extract_client_ip(headers);
            tracing::info!(
                method = %method,
                path = %path,
                client_ip = %client_ip,
                user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok()).unwrap_or("unknown"),
                "Security audit: sensitive endpoint access"
            );
        }
    }

    Ok(next.run(req).await)
}

/// Validate Content-Type for API requests
pub async fn validate_content_type(req: Request, next: Next) -> Result<Response, StatusCode> {
    let method = req.method();
    let path = req.uri().path();

    // Only validate POST/PUT/PATCH for API endpoints
    if !matches!(method.as_str(), "POST" | "PUT" | "PATCH") || !path.starts_with("/api/") {
        return Ok(next.run(req).await);
    }

    // Skip validation for WebSocket upgrades
    if path.contains("/ws") {
        return Ok(next.run(req).await);
    }

    // Require application/json for API endpoints
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());

    match content_type {
        Some(ct) if ct.starts_with("application/json") => Ok(next.run(req).await),
        _ => {
            tracing::warn!(path, content_type, "Invalid Content-Type for API request");
            Err(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        }
    }
}

// ============================================================================
// Performance Monitoring
// ============================================================================

/// Track and log slow requests
pub async fn track_slow_requests(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let start = std::time::Instant::now();

    let response = next.run(req).await;
    let duration = start.elapsed();

    // Log slow requests (> 1 second)
    if duration.as_secs() >= 1 {
        tracing::warn!(
            method = %method,
            path = %path,
            duration_ms = duration.as_millis(),
            "Slow request"
        );
    }

    // Record metrics for moderately slow requests (> 500ms)
    if duration.as_millis() > 500 {
        metrics::histogram!(
            "http_slow_requests",
            &[("method", method.to_string()), ("path", path),]
        )
        .record(duration.as_millis() as f64);
    }

    response
}

// ============================================================================
// Error Response Enhancement
// ============================================================================

/// Enhance error responses with helpful headers
pub async fn enhance_error_response(req: Request, next: Next) -> Response {
    let response = next.run(req).await;

    // Add retry-after hint for 503 Service Unavailable
    if response.status() == StatusCode::SERVICE_UNAVAILABLE {
        let mut enhanced = response;
        enhanced
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return enhanced;
    }

    response
}
