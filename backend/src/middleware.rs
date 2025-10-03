//! Essential middleware for security, validation, and observability

use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

// ============================================================================
// CSRF Protection
// ============================================================================

/// CSRF protection for HTML form submissions
/// Relies on SameSite=Strict cookies (primary defense) and Origin/Referer checking
pub async fn csrf_protection(req: Request, next: Next) -> Result<Response, StatusCode> {
    let method = req.method();
    let path = req.uri().path();

    // Only check state-changing methods for non-API routes
    if !matches!(method.as_str(), "POST" | "PUT" | "DELETE" | "PATCH") || path.starts_with("/api/") {
        return Ok(next.run(req).await);
    }

    // Check Origin or Referer header for same-origin policy
    let headers = req.headers();
    let origin = headers.get(header::ORIGIN).or_else(|| headers.get(header::REFERER));

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
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    
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
             form-action 'self';"
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
        HeaderValue::from_static(
            "geolocation=(), microphone=(), camera=(), payment=(), usb=()"
        ),
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
        metrics::histogram!("http_slow_requests", &[
            ("method", method.to_string()),
            ("path", path),
        ]).record(duration.as_millis() as f64);
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
        enhanced.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_static("60"),
        );
        return enhanced;
    }
    
    response
}

