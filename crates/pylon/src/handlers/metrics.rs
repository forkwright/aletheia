//! Prometheus metrics exposition endpoint.

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;

use crate::state::MetricsState;

/// `OpenMetrics` text content type for the metrics endpoint.
///
/// WHY: `prometheus-client` emits `OpenMetrics` text format, which Prometheus
/// scrapers accept natively. The content type advertises the `OpenMetrics`
/// version so compatible scrapers parse it as `OpenMetrics` (with unit and
/// UNIT lines) rather than plain Prometheus text 0.0.4.
pub(crate) const METRICS_CONTENT_TYPE: &str =
    "application/openmetrics-text; version=1.0.0; charset=utf-8";

/// GET /metrics: Prometheus/OpenMetrics text-format metrics exposition.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/metrics",
    responses(
        (status = 200, description = "OpenMetrics text-format metrics", content_type = "application/openmetrics-text"),
    ),
)]
#[expect(
    clippy::expect_used,
    reason = "writing into a String never fails; the fmt::Error branch is unreachable"
)]
pub async fn expose(
    State(state): State<MetricsState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    // SECURITY(#4995): Restrict to loopback when loopback_only_metrics is set.
    // This is the default when gateway.bind = "localhost". ConnectInfo is only
    // present when the router is served through `into_make_service_with_connect_info`;
    // unit tests invoke handlers directly without it, so treat a missing peer as
    // non-loopback (deny-by-default under the loopback-only policy).
    let is_loopback = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|ci| ci.ip().is_loopback());
    if state.loopback_only_metrics && !is_loopback {
        return (
            axum::http::StatusCode::FORBIDDEN,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            "metrics endpoint is restricted to loopback connections",
        )
            .into_response();
    }
    let uptime = state.start_time.elapsed().as_secs_f64();

    let session_count = state
        .session_store
        .lock()
        .await
        .session_count();
    // NOTE: session count fits in i64; saturate on theoretical overflow.
    let session_count = i64::try_from(session_count).unwrap_or(i64::MAX);

    crate::metrics::update_system_gauges(uptime, session_count);

    let mut buffer = String::new();
    state
        .metrics_registry
        .encode(&mut buffer)
        .expect("encoding into a String is infallible");

    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], buffer).into_response()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    #[test]
    fn content_type_is_openmetrics_text_format() {
        assert!(METRICS_CONTENT_TYPE.starts_with("application/openmetrics-text"));
        assert!(METRICS_CONTENT_TYPE.contains("version=1.0.0"));
        assert!(METRICS_CONTENT_TYPE.contains("charset=utf-8"));
    }

    #[test]
    fn empty_registry_encodes_without_error() {
        let registry = MetricsRegistry::new();
        let mut buffer = String::new();
        registry.encode(&mut buffer).unwrap();
        // NOTE: OpenMetrics text format mandates UTF-8.
        assert!(buffer.is_char_boundary(0));
    }
}
