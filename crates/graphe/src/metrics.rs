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

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&SESSIONS_TOTAL);
    LazyLock::force(&BACKUP_DURATION_SECONDS);
}

/// Record a session creation.
pub fn record_session_created(nous_id: &str, session_type: &str) {
    // kanon:ignore RUST/pub-visibility
    SESSIONS_TOTAL
        .with_label_values(&[nous_id, session_type])
        .inc();
}

/// Record a backup operation duration.
pub fn record_backup_duration(duration_secs: f64, success: bool) {
    // kanon:ignore RUST/pub-visibility
    let status = if success { "ok" } else { "error" };
    BACKUP_DURATION_SECONDS
        .with_label_values(&[status])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_session_created_does_not_panic() {
        record_session_created("test-nous", "primary");
        record_session_created("test-nous", "ephemeral");
    }

    #[test]
    fn record_backup_duration_does_not_panic() {
        record_backup_duration(5.0, true);
        record_backup_duration(1.0, false);
    }
}
