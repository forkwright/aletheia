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

use mneme::store::SessionStatusCounts;
use mneme::types::SessionStatus;

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

/// Label set for the lifecycle-partitioned session gauge.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct SessionStatusLabels {
    pub(crate) status: String,
}

static SESSIONS_BY_STATUS: LazyLock<Family<SessionStatusLabels, Gauge>> =
    LazyLock::new(Family::default);
static SESSIONS_TOTAL: LazyLock<Gauge> = LazyLock::new(Gauge::default);
static UPTIME_SECONDS: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);

// WHY(#4694): server-side counterparts of the memory-health components
// theatron/proskenion computes client-side (`state/meta/mod.rs`'s
// `MemoryHealthStore`). Computed in
// `handlers::knowledge::health_metrics::compute_memory_health_metrics` and
// recorded here via `update_memory_health_gauges` so the trend is visible
// to a Prometheus scraper without opening the TUI. See
// `docs/OBSERVABILITY.md`'s Memory Health SLO section for thresholds.
static MEMORY_HEALTH_SCORE: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);
static MEMORY_AVG_CONFIDENCE: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);
static MEMORY_ORPHAN_RATIO: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);
static MEMORY_STALENESS_RATIO: LazyLock<Gauge<f64, AtomicU64>> = LazyLock::new(Gauge::default);

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
        "Number of sessions with lifecycle status active",
        ACTIVE_SESSIONS.clone(),
    );
    registry.register(
        "aletheia_sessions",
        "Number of retained sessions by lifecycle status",
        SESSIONS_BY_STATUS.clone(),
    );
    registry.register(
        "aletheia_sessions_total",
        "Number of retained sessions across all lifecycle statuses",
        SESSIONS_TOTAL.clone(),
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
    registry.register(
        "aletheia_memory_health_score",
        "Composite memory health score (0.0-1.0), server-computed",
        MEMORY_HEALTH_SCORE.clone(),
    );
    registry.register(
        "aletheia_memory_avg_confidence",
        "Average confidence across active (non-forgotten, non-superseded) facts",
        MEMORY_AVG_CONFIDENCE.clone(),
    );
    registry.register(
        "aletheia_memory_orphan_ratio",
        "Fraction of entities with no relationship and no fact link",
        MEMORY_ORPHAN_RATIO.clone(),
    );
    registry.register(
        "aletheia_memory_staleness_ratio",
        "Fraction of active facts unreviewed past the staleness threshold",
        MEMORY_STALENESS_RATIO.clone(),
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
///
/// WHY: `aletheia_active_sessions` means what its name says — sessions with
/// lifecycle status `Active`. Retained history is reported separately by
/// status so operators can see it rather than have it silently inflate live
/// workload (issue #5039).
pub(crate) fn update_system_gauges(uptime_secs: f64, sessions: &SessionStatusCounts) {
    UPTIME_SECONDS.set(uptime_secs);
    ACTIVE_SESSIONS.set(saturating_gauge(sessions.active));
    SESSIONS_TOTAL.set(saturating_gauge(sessions.total()));
    for status in [
        SessionStatus::Active,
        SessionStatus::Archived,
        SessionStatus::Distilled,
    ] {
        SESSIONS_BY_STATUS
            .get_or_create(&SessionStatusLabels {
                status: status.as_str().to_owned(),
            })
            .set(saturating_gauge(sessions.get(status)));
    }
}

/// NOTE: session counts fit in i64; saturate on theoretical overflow.
fn saturating_gauge(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Update the memory-health gauges from a server-computed snapshot.
///
/// WHY not computed here: the knowledge-store query lives in
/// `handlers::knowledge::health_metrics` (feature-gated on
/// `knowledge-store`) so this crate's metrics module stays free of a
/// direct `mneme`/query-engine dependency; this function only records
/// the already-computed result.
#[cfg(feature = "knowledge-store")]
pub(crate) fn update_memory_health_gauges(
    metrics: crate::handlers::knowledge::MemoryHealthMetrics,
) {
    MEMORY_HEALTH_SCORE.set(metrics.health_score);
    MEMORY_AVG_CONFIDENCE.set(metrics.avg_confidence);
    MEMORY_ORPHAN_RATIO.set(metrics.orphan_ratio);
    MEMORY_STALENESS_RATIO.set(metrics.staleness_ratio);
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

/// Serializes tests that write or scrape the process-global gauge handles.
///
/// WHY this is crate-visible rather than local to this module's tests: the
/// metric families above are `LazyLock` statics backed by `Arc`-internal
/// state, so a per-test `MetricsRegistry::new()` does not give a test its own
/// gauges — every registry encodes the same shared values. Both the direct
/// `update_system_gauges` tests here and the `GET /metrics` handler tests in
/// `crate::tests::metrics` write them, so every writer must take the same
/// lock or the suite fails nondeterministically under cargo's parallel test
/// threads.
#[cfg(test)]
pub(crate) static GAUGE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the gauge lock, ignoring poisoning.
///
/// WHY poisoning is ignored: the guarded data is `()`. A panicking test leaves
/// no invariant broken here, and propagating the poison would turn one real
/// failure into a cascade of unrelated ones.
#[cfg(test)]
pub(crate) fn gauge_lock() -> std::sync::MutexGuard<'static, ()> {
    GAUGE_TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        let _guard = gauge_lock();
        let registry = MetricsRegistry::new();
        init(&registry);
        record_request("GET", "/api/health", 200, 0.001);
        update_system_gauges(
            10.0,
            &SessionStatusCounts {
                active: 3,
                archived: 0,
                distilled: 0,
            },
        );

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

    /// The gauge named `active` must report only `Active` sessions, and the
    /// retained history must be visible rather than folded into it (#5039).
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test: encoding metrics into a String buffer is infallible"
    )]
    fn session_gauges_separate_active_from_retained_history() {
        let _guard = gauge_lock();
        let registry = MetricsRegistry::new();
        init(&registry);
        update_system_gauges(
            1.0,
            &SessionStatusCounts {
                active: 2,
                archived: 5,
                distilled: 7,
            },
        );

        let mut buffer = String::new();
        registry.encode(&mut buffer).expect("encode");

        // The pre-fix code set this to the total (14) rather than the active count.
        assert!(
            buffer.contains("aletheia_active_sessions 2\n"),
            "active gauge must exclude archived and distilled; got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_sessions_total 14\n"),
            "total gauge must count every status; got: {buffer}"
        );
        for (status, count) in [("active", 2), ("archived", 5), ("distilled", 7)] {
            let expected = format!("aletheia_sessions{{status=\"{status}\"}} {count}\n");
            assert!(
                buffer.contains(&expected),
                "expected `{expected}` in: {buffer}"
            );
        }
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test: encoding metrics into a String buffer is infallible"
    )]
    fn memory_health_gauges_record_the_computed_snapshot() {
        let _guard = gauge_lock();
        let registry = MetricsRegistry::new();
        init(&registry);
        update_memory_health_gauges(crate::handlers::knowledge::MemoryHealthMetrics {
            avg_confidence: 0.8,
            orphan_ratio: 0.1,
            staleness_ratio: 0.2,
            health_score: 0.71,
        });

        let mut buffer = String::new();
        registry.encode(&mut buffer).expect("encode");
        assert!(
            buffer.contains("aletheia_memory_health_score 0.71"),
            "got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_memory_avg_confidence 0.8"),
            "got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_memory_orphan_ratio 0.1"),
            "got: {buffer}"
        );
        assert!(
            buffer.contains("aletheia_memory_staleness_ratio 0.2"),
            "got: {buffer}"
        );
    }
}
