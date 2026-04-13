//! Prometheus metric definitions for the session persistence layer.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec,
    register_int_counter_vec,
};

static SESSIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_sessions_total", "Total sessions created"),
        &["nous_id", "session_type"]
    )
    .expect("metric registration")
});

static BACKUP_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_backup_duration_seconds",
            "Database backup duration in seconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0]),
        &["status"]
    )
    .expect("metric registration")
});

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "metric init called from server startup")
)]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&SESSIONS_TOTAL);
    LazyLock::force(&BACKUP_DURATION_SECONDS);
}

/// Record a session creation.
///
/// Compiled when either the `sqlite` or `fjall` feature is enabled — both
/// store backends call this on successful session creation.
#[cfg(any(feature = "sqlite", feature = "fjall", test))]
pub(crate) fn record_session_created(nous_id: &str, session_type: &str) {
    SESSIONS_TOTAL
        .with_label_values(&[nous_id, session_type])
        .inc();
}

/// Record a backup operation duration.
///
/// Only compiled when the `sqlite` feature is enabled — the only call site
/// (`backup::create_backup`) lives behind that feature gate.
#[cfg(any(feature = "sqlite", test))]
pub(crate) fn record_backup_duration(duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    BACKUP_DURATION_SECONDS
        .with_label_values(&[status])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_initializes_metrics() {
        // Verify init() completes without panic and metrics are accessible
        init();
        // After init, metrics should be recordable without panic
        record_session_created("test-nous", "primary");
        let metric = &*SESSIONS_TOTAL;
        // Just verify we can read the metric (actual value depends on test order)
        let _count = metric.with_label_values(&["test-nous", "primary"]).get();
    }

    #[test]
    fn record_session_created_increments_counter() {
        // WHY: use a unique label pair so concurrent tests (`init_initializes_metrics`)
        // that also call `record_session_created("test-nous", "primary")` don't
        // race on the same counter. Prometheus counters are global singletons.
        let metric = &*SESSIONS_TOTAL;
        let initial = metric
            .with_label_values(&["test-incr-nous", "primary"])
            .get();

        record_session_created("test-incr-nous", "primary");

        let after = metric
            .with_label_values(&["test-incr-nous", "primary"])
            .get();
        assert_eq!(after, initial + 1, "counter should increment by 1");
    }

    #[test]
    fn record_backup_duration_records_observation() {
        // Record an observation and verify histogram has samples
        let metric = &*BACKUP_DURATION_SECONDS;
        let initial = metric.with_label_values(&["ok"]).get_sample_count();

        record_backup_duration(5.0, true);

        let after = metric.with_label_values(&["ok"]).get_sample_count();
        assert_eq!(after, initial + 1, "histogram should have one more sample");
    }
}
