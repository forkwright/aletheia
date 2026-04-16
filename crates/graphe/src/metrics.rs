//! Prometheus metric definitions for the session persistence layer.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ---------------------------------------------------------------------------
// Label sets
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SessionLabels {
    nous_id: String,
    session_type: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BackupStatusLabels {
    status: String,
}

// ---------------------------------------------------------------------------
// Metric families
// ---------------------------------------------------------------------------

static SESSIONS_TOTAL: LazyLock<Family<SessionLabels, Counter>> = LazyLock::new(Family::default);

fn backup_duration_histogram() -> Histogram {
    Histogram::new([0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0])
}

type BackupDurationFamily = Family<BackupStatusLabels, Histogram, fn() -> Histogram>;

static BACKUP_DURATION_SECONDS: LazyLock<BackupDurationFamily> =
    LazyLock::new(|| Family::new_with_constructor(backup_duration_histogram));

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_sessions",
        "Total sessions created",
        SESSIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_backup_duration_seconds",
        "Database backup duration in seconds",
        BACKUP_DURATION_SECONDS.clone(),
    );
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Record a session creation.
///
/// Compiled when either the `sqlite` or `fjall` feature is enabled — both
/// store backends call this on successful session creation.

pub(crate) fn record_session_created(nous_id: &str, session_type: &str) {
    SESSIONS_TOTAL
        .get_or_create(&SessionLabels {
            nous_id: nous_id.to_owned(),
            session_type: session_type.to_owned(),
        })
        .inc();
}

/// Record a backup operation duration.
///
/// Currently unused: the only call site (`backup::create_backup`) was removed
/// along with rusqlite in #3446. Retained — together with `BACKUP_DURATION_SECONDS`
/// and its registration — so fjall-based backup work can re-attach to the same
/// metric name without a schema migration.
#[expect(
    dead_code,
    reason = "reserved for fjall backup work; call site removed in #3446"
)]
pub(crate) fn record_backup_duration(duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    BACKUP_DURATION_SECONDS
        .get_or_create(&BackupStatusLabels {
            status: status.to_owned(),
        })
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    #[test]
    fn register_and_record_session_created() {
        let r = fresh_registry();
        record_session_created("_test_nous", "primary");
        record_session_created("_test_nous", "primary");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_sessions_total{nous_id=\"_test_nous\",session_type=\"primary\"} 2"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_backup_duration() {
        let r = fresh_registry();
        record_backup_duration(5.0, true);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_backup_duration_seconds_count{status=\"ok\"} 1"),
            "got: {out}"
        );
    }
}
