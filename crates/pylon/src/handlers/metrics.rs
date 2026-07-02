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

/// Label names redacted from `/metrics` output unless `metrics.detailed` is
/// `true` (#5322).
///
/// WHY: these labels carry high-cardinality identifiers (`nous_id`), local
/// path layout (`path`), and operational detail (tool names, model/provider
/// choices) that leak instance posture when scraped from a non-loopback or
/// public endpoint.
const REDACTED_LABELS: &[&str] = &[
    "nous_id",
    "tool_name",
    "path",
    "provider",
    "model",
    "topic",
    "cause",
    "event_type",
    "reason",
    "stage",
    "error_type",
    "task_type",
];

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
        (status = 401, description = "Unauthorized when mode is bearer", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden for non-loopback clients in local_only mode"),
        (status = 404, description = "Metrics endpoint disabled"),
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
    use taxis::config::MetricsMode;

    match state.metrics_mode {
        MetricsMode::Disabled => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                [(axum::http::header::CONTENT_TYPE, "text/plain")],
                "metrics endpoint is disabled",
            )
                .into_response();
        }
        MetricsMode::LocalOnly => {
            // SECURITY(#5322): Restrict to loopback when mode is local_only.
            // ConnectInfo is only present when the router is served through
            // `into_make_service_with_connect_info`; unit tests invoke handlers
            // directly without it, so treat a missing peer as non-loopback
            // (deny-by-default under the local-only policy).
            let is_loopback = request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .is_some_and(|ci| ci.ip().is_loopback());
            if !is_loopback {
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    [(axum::http::header::CONTENT_TYPE, "text/plain")],
                    "metrics endpoint is restricted to loopback connections",
                )
                    .into_response();
            }
        }
        // WHY: `Bearer` is enforced by the `require_bearer_auth` route layer
        // applied in `router.rs`; reaching the handler means auth passed.
        MetricsMode::Bearer | MetricsMode::Public => {}
    }

    let uptime = state.start_time.elapsed().as_secs_f64();

    let session_count = state.session_store.lock().await.session_count();
    // NOTE: session count fits in i64; saturate on theoretical overflow.
    let session_count = i64::try_from(session_count).unwrap_or(i64::MAX);

    crate::metrics::update_system_gauges(uptime, session_count);

    let mut buffer = String::new();
    state
        .metrics_registry
        .encode(&mut buffer)
        .expect("encoding into a String is infallible");

    if !state.metrics_detailed {
        buffer = redact_labels(&buffer);
    }

    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], buffer).into_response()
}

/// Redact sensitive/high-cardinality label values from OpenMetrics text output.
///
/// WHY(#5322): the shared registry contains labels that leak operational
/// detail. Post-processing the text is coarser than pluggable encoding, but
/// it keeps the change localized to the exposition surface while preserving
/// Prometheus compatibility.
#[must_use]
fn redact_labels(text: &str) -> String {
    let pattern = label_redaction_pattern();
    pattern.replace_all(text, "${name}=\"redacted\"").into_owned()
}

/// Build the redaction regex once.
///
/// INVARIANT: `REDACTED_LABELS` contains valid Prometheus label names, so the
/// generated regex never needs to escape them as pattern metacharacters.
fn label_redaction_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        let names = REDACTED_LABELS.join("|");
        // Match `name="value"` where value may contain escaped quotes.
        let expr = format!(r#"(?P<name>\b(?:{names})\b)\s*=\s*(?P<value>"([^"\\]|\\.)*")"#);
        regex::Regex::new(&expr).expect("redaction regex is statically valid")
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Instant;

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use koina::metrics::MetricsRegistry;
    use mneme::store::SessionStore;
    use taxis::config::MetricsMode;

    use super::*;
    use crate::state::MetricsState;

    fn test_metrics_state(mode: MetricsMode, detailed: bool) -> MetricsState {
        MetricsState {
            session_store: Arc::new(tokio::sync::Mutex::new(SessionStore::open_in_memory().unwrap())),
            start_time: Instant::now(),
            metrics_registry: MetricsRegistry::new(),
            metrics_mode: mode,
            metrics_detailed: detailed,
        }
    }

    fn loopback_request() -> Request<Body> {
        let mut req = Request::get("/metrics").body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));
        req
    }

    fn remote_request() -> Request<Body> {
        let mut req = Request::get("/metrics").body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 1], 1234))));
        req
    }

    fn request_without_connect_info() -> Request<Body> {
        Request::get("/metrics").body(Body::empty()).unwrap()
    }

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

    #[tokio::test]
    async fn disabled_metrics_returns_not_found() {
        let state = test_metrics_state(MetricsMode::Disabled, false);
        let resp = expose(State(state), request_without_connect_info()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn local_only_allows_loopback() {
        let state = test_metrics_state(MetricsMode::LocalOnly, false);
        let resp = expose(State(state), loopback_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn local_only_denies_remote() {
        let state = test_metrics_state(MetricsMode::LocalOnly, false);
        let resp = expose(State(state), remote_request()).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn local_only_denies_missing_connect_info() {
        let state = test_metrics_state(MetricsMode::LocalOnly, false);
        let resp = expose(State(state), request_without_connect_info()).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn public_mode_allows_remote() {
        let state = test_metrics_state(MetricsMode::Public, false);
        let resp = expose(State(state), remote_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn bearer_mode_allows_remote_without_middleware_check() {
        // The bearer auth check is performed by the route-layer middleware in
        // `router.rs`; the handler itself does not re-validate.
        let state = test_metrics_state(MetricsMode::Bearer, false);
        let resp = expose(State(state), remote_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn redaction_strips_sensitive_label_values() {
        let input = r#"aletheia_http_requests_total{method="GET",path="/api/v1/sessions/abc-123",status="200"} 1
aletheia_tool_failures_total{nous_id="nous-1",tool_name="shell_execute"} 2
aletheia_llm_tokens_total{provider="anthropic",model="claude-opus",direction="input"} 100
aletheia_uptime_seconds{} 42
"#;
        let redacted = redact_labels(input);
        assert!(redacted.contains(r#"path="redacted""#), "got: {redacted}");
        assert!(redacted.contains(r#"nous_id="redacted""#), "got: {redacted}");
        assert!(redacted.contains(r#"tool_name="redacted""#), "got: {redacted}");
        assert!(redacted.contains(r#"provider="redacted""#), "got: {redacted}");
        assert!(redacted.contains(r#"model="redacted""#), "got: {redacted}");
        assert!(redacted.contains(r#"method="GET""#), "low-sensitivity labels preserved; got: {redacted}");
        assert!(redacted.contains(r#"status="200""#), "low-sensitivity labels preserved; got: {redacted}");
        assert!(redacted.contains("aletheia_uptime_seconds{} 42"), "got: {redacted}");
    }

    #[test]
    fn redaction_handles_escaped_quotes() {
        let input = r#"aletheia_tool_failures_total{nous_id="nous-\"quoted\"",tool_name="x"} 1"#;
        let redacted = redact_labels(input);
        assert!(
            redacted.contains(r#"nous_id="redacted""#),
            "escaped-quote value was not redacted: {redacted}"
        );
        assert!(redacted.contains(r#"tool_name="redacted""#), "got: {redacted}");
    }
}
