//! HTTP router construction with middleware layers.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info_span;

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
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let request_id = ulid::Ulid::new().to_string();
                    info_span!("http_request",
                        http.method = %request.method(),
                        http.path = %request.uri().path(),
                        http.request_id = %request_id,
                        http.status_code = tracing::field::Empty,
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: Duration,
                     span: &tracing::Span| {
                        span.record("http.status_code", response.status().as_u16());
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "HTTP latency fits in u64"
                        )]
                        let duration_ms = latency.as_millis() as u64;
                        tracing::debug!(
                            duration_ms,
                            status = response.status().as_u16(),
                            "request complete"
                        );
                    },
                ),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}
