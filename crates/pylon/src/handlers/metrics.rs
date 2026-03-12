//! Prometheus metrics exposition endpoint.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use prometheus::{Encoder, TextEncoder};

use crate::state::AppState;

/// Prometheus content type for the metrics endpoint.
pub(crate) const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

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

    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::{Encoder, TextEncoder};

    #[test]
    fn content_type_is_prometheus_text_format() {
        assert!(METRICS_CONTENT_TYPE.starts_with("text/plain"));
        assert!(METRICS_CONTENT_TYPE.contains("version=0.0.4"));
        assert!(METRICS_CONTENT_TYPE.contains("charset=utf-8"));
    }

    #[test]
    fn text_encoder_produces_valid_utf8() {
        let encoder = TextEncoder::new();
        let families = prometheus::gather();
        let mut buf = Vec::new();
        encoder.encode(&families, &mut buf).unwrap();
        // Must be valid UTF-8 (Prometheus text format is UTF-8)
        assert!(std::str::from_utf8(&buf).is_ok());
    }

    #[test]
    fn text_encoder_content_type_is_text_plain() {
        let encoder = TextEncoder::new();
        // prometheus TextEncoder declares "text/plain; version=0.0.4";
        // we append charset=utf-8 to our served header
        assert!(encoder.format_type().starts_with("text/plain"));
        assert!(encoder.format_type().contains("version=0.0.4"));
    }
}
