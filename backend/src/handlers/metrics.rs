use axum::response::Response;
use axum::http::{header, StatusCode};

use crate::{
    error::{AppError, Result},
    metrics::get_metrics_handle,
};

pub async fn metrics_handler() -> Result<Response> {
    let handle = get_metrics_handle()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Metrics not initialized")))?;
    
    let metrics = handle.render();
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; version=0.0.4")
        .body(axum::body::Body::from(metrics))
        .unwrap())
}
