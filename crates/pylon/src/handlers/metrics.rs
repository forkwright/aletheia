//! Prometheus metrics exposition endpoint.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use prometheus::{Encoder, TextEncoder};

use crate::state::AppState;

/// GET /metrics -- Prometheus text format exposition.
pub async fn expose(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs_f64();

    let session_count = state
        .session_store
        .lock()
        .await
        .list_sessions(None)
        .ok()
        .map_or(0, |sessions| {
            #[expect(clippy::cast_possible_wrap, reason = "session count fits in i64")]
            let count = sessions.len() as i64;
            count
        });

    crate::metrics::update_system_gauges(uptime, session_count);

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("prometheus encoding");

    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        buffer,
    )
}
