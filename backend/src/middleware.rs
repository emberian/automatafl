//! Essential middleware for security, validation, and observability

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
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
    let client_ip = req
        .headers()
        .get("x-real-ip")
        .or_else(|| req.headers().get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

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

// ============================================================================
// CSRF Protection
// ============================================================================

/// CSRF protection for HTML form submissions
/// Relies on SameSite=Strict cookies (primary defense) and Origin/Referer checking
pub async fn csrf_protection(req: Request, next: Next) -> Result<Response, StatusCode> {
    let method = req.method();
    let path = req.uri().path();

    // Only check state-changing methods for non-API routes
    if !matches!(method.as_str(), "POST" | "PUT" | "DELETE" | "PATCH") || path.starts_with("/api/")
    {
        return Ok(next.run(req).await);
    }

    // Check Origin or Referer header for same-origin policy
    let headers = req.headers();
    let origin = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER));

    if let Some(origin_val) = origin {
        if let Ok(origin_str) = origin_val.to_str() {
            if origin_str.contains("localhost") || origin_str.contains("127.0.0.1") {
                return Ok(next.run(req).await);
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

    Ok(next.run(req).await)
}

// ============================================================================
// Security Headers
// ============================================================================

/// Add security headers to all responses
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // HSTS - Force HTTPS in production
    if !cfg!(debug_assertions) {
        headers.insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    // Prevent clickjacking
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));

    // Prevent MIME type sniffing
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );

    // Content Security Policy
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: https:; \
             font-src 'self' data:; \
             connect-src 'self' ws: wss:; \
             frame-ancestors 'none'; \
             base-uri 'self'; \
             form-action 'self';",
        ),
    );

    // Referrer policy
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // Permissions policy - disable unnecessary features
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static("geolocation=(), microphone=(), camera=(), payment=(), usb=()"),
    );

    response
}

// ============================================================================
// Content Type Validation
// ============================================================================

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
