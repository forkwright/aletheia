//! Prometheus metric definitions for the HTTP gateway.
//!
//! Metrics are registered with a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`] at startup. Recording functions operate on global
//! [`std::sync::LazyLock`] families backed by `Arc`-internal state, so they
//! are cheap to call from middleware without locking the registry.

use std::sync::LazyLock;
use std::sync::atomic::AtomicU64;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

#[cfg(test)]
use koina::metrics::MetricsRegistry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct HttpRequestLabels {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) status: u16,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct HttpDurationLabels {
    pub(crate) method: String,
    pub(crate) path: String,
}

static HTTP_REQUESTS_TOTAL: LazyLock<Family<HttpRequestLabels, Counter>> =
    LazyLock::new(Family::default);

// WHY: `Family<L, Histogram, fn() -> Histogram>` pins the constructor type
// so we can use it in a LazyLock. The tuple form is needed because
// `Histogram::new` takes an `IntoIterator` and we need a zero-arg constructor.
fn http_duration_histogram() -> Histogram {
    Histogram::new([
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ])
}

type HttpDurationFamily = Family<HttpDurationLabels, Histogram, fn() -> Histogram>;

static HTTP_REQUEST_DURATION_SECONDS: LazyLock<HttpDurationFamily> =
    LazyLock::new(|| Family::new_with_constructor(http_duration_histogram));

static ACTIVE_SESSIONS: LazyLock<Gauge> = LazyLock::new(Gauge::default);
static UPTIME_SECONDS: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct EventBusDropLabels {
    pub(crate) topic: String,
    pub(crate) cause: String,
}

static EVENT_BUS_DROPS_TOTAL: LazyLock<Family<EventBusDropLabels, Counter>> =
    LazyLock::new(Family::default);

/// Register this crate's metrics with the shared registry.
///
/// Called once at startup from the binary crate's `register_all_metrics`.
pub fn register(registry: &mut Registry) {
    registry.register(
        // WHY: `_total` is appended automatically by the encoder for counters.
        "aletheia_http_requests",
        "Total HTTP requests",
        HTTP_REQUESTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_http_request_duration_seconds",
        "HTTP request duration in seconds",
        HTTP_REQUEST_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_active_sessions",
        "Number of active sessions",
        ACTIVE_SESSIONS.clone(),
    );
    registry.register(
        "aletheia_uptime_seconds",
        "Server uptime in seconds",
        UPTIME_SECONDS.clone(),
    );
    registry.register(
        "aletheia_event_bus_drops",
        "Total domain events dropped due to no active subscribers",
        EVENT_BUS_DROPS_TOTAL.clone(),
    );
}

/// Record a dropped event-bus publish (no active receivers).
pub(crate) fn record_event_bus_drop(topic: &str, cause: &str) {
    EVENT_BUS_DROPS_TOTAL
        .get_or_create(&EventBusDropLabels {
            topic: topic.to_owned(),
            cause: cause.to_owned(),
        })
        .inc();
}

/// Register metrics on the shared wrapper.
///
/// Helper for test harnesses that build an `AppState` without running the
/// binary's `register_all_metrics`. Production code uses the binary entry
/// point (see `aletheia::runtime::register_all_metrics`).
#[cfg(test)]
pub(crate) fn init(registry: &MetricsRegistry) {
    registry.with_registry(register);
}

/// Record an HTTP request metric.
pub(crate) fn record_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    HTTP_REQUESTS_TOTAL
        .get_or_create(&HttpRequestLabels {
            method: method.to_owned(),
            path: path.to_owned(),
            status,
        })
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .get_or_create(&HttpDurationLabels {
            method: method.to_owned(),
            path: path.to_owned(),
        })
        .observe(duration_secs);
}

/// Update system gauge metrics.
pub(crate) fn update_system_gauges(uptime_secs: f64, active_sessions: i64) {
    UPTIME_SECONDS.set(uptime_secs);
    ACTIVE_SESSIONS.set(active_sessions);
}

/// Normalize a URL path by replacing dynamic segments with `{id}`.
///
/// Prevents label explosion from unique IDs in prometheus metrics.
#[must_use]
pub(crate) fn normalize_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let normalized: Vec<&str> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i > 0 && looks_like_id(part) {
                "{id}"
            } else {
                part
            }
        })
        .collect();
    normalized.join("/")
}

fn looks_like_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // NOTE: ULIDs are 26 alphanumeric chars; UUIDs are 36 chars with hyphens.
    let len = s.len();
    (len >= 20 && s.chars().all(|c| c.is_ascii_alphanumeric()))
        || (len == 36 && s.contains('-') && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_static_paths() {
        assert_eq!(normalize_path("/api/health"), "/api/health");
        assert_eq!(normalize_path("/api/nous"), "/api/nous");
        assert_eq!(normalize_path("/metrics"), "/metrics");
    }

    #[test]
    fn normalize_dynamic_paths() {
        assert_eq!(
            normalize_path("/api/sessions/01JTEST1234567890ABCDEFGH"),
            "/api/sessions/{id}"
        );
        assert_eq!(
            normalize_path("/api/nous/01JTEST1234567890ABCDEFGH/tools"),
            "/api/nous/{id}/tools"
        );
    }

    #[test]
    fn normalize_uuid_paths() {
        assert_eq!(
            normalize_path("/api/sessions/550e8400-e29b-41d4-a716-446655440000"),
            "/api/sessions/{id}"
        );
    }

    #[test]
    fn normalize_short_names_preserved() {
        assert_eq!(normalize_path("/api/nous/syn"), "/api/nous/syn");
        assert_eq!(normalize_path("/api/nous/syn/tools"), "/api/nous/syn/tools");
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test: encoding metrics into a String buffer is infallible"
    )]
    fn register_and_encode_roundtrip() {
        let registry = MetricsRegistry::new();
        init(&registry);
        record_request("GET", "/api/health", 200, 0.001);
        update_system_gauges(10.0, 3);

        let mut buffer = String::new();
        registry.encode(&mut buffer).expect("encode");
        assert!(
            buffer.contains("aletheia_http_requests_total"),
            "expected http counter; got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_uptime_seconds"),
            "expected uptime gauge; got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_active_sessions"),
            "expected sessions gauge; got: {buffer}"
        );
    }
}
