//! HTTP router construction with middleware layers.

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::handlers::{health, nous, sessions};
use crate::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health::check))
        .route("/api/sessions", post(sessions::create))
        .route(
            "/api/sessions/{id}",
            get(sessions::get_session).delete(sessions::close),
        )
        .route("/api/sessions/{id}/messages", post(sessions::send_message))
        .route("/api/sessions/{id}/history", get(sessions::history))
        .route("/api/nous", get(nous::list))
        .route("/api/nous/{id}", get(nous::get_status))
        .route("/api/nous/{id}/tools", get(nous::tools))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
