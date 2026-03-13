use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
struct RootResponse {
    message: &'static str,
    version: &'static str,
    description: &'static str,
    endpoints: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    success: bool,
    status: &'static str,
    service: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
}

async fn root_handler() -> Json<RootResponse> {
    Json(RootResponse {
        message: "Cat API (Rust rewrite)",
        version: "0.1.0",
        description: "Rust rewrite of the home monitor backend",
        endpoints: vec![
            "GET /",
            "GET /health",
            "GET /api/",
            "GET /api/health",
            "POST /api/auth/login",
            "POST /api/auth/verify",
            "GET /api/tempo",
            "POST /api/tempo/refresh",
            "GET /api/tempo/predictions",
            "GET /api/tempo/state",
        ],
    })
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        success: true,
        status: "healthy",
        service: "cat-monitor-rust-backend",
    })
}
