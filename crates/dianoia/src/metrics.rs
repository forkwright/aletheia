//! Prometheus metric definitions for planning and project orchestration.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{IntCounterVec, Opts, register_int_counter_vec};

static PHASE_TRANSITIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_phase_transitions_total",
            "Total project state transitions"
        ),
        &["from", "to"]
    )
    .expect("metric registration")
});

static STUCK_DETECTIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_stuck_detections_total",
            "Total stuck pattern detections"
        ),
        &["pattern"]
    )
    .expect("metric registration")
});

/// Record a project state transition.
pub(crate) fn record_phase_transition(from: &str, to: &str) {
    PHASE_TRANSITIONS_TOTAL.with_label_values(&[from, to]).inc();
}

/// Force-initialize all lazy metric statics.
pub fn init() {
    LazyLock::force(&PHASE_TRANSITIONS_TOTAL);
    LazyLock::force(&STUCK_DETECTIONS_TOTAL);
}

/// Record a stuck pattern detection.
pub(crate) fn record_stuck_detection(pattern: &str) {
    STUCK_DETECTIONS_TOTAL.with_label_values(&[pattern]).inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_initializes_metrics() {
        // Verify init() completes without panic and metrics are accessible
        init();
        // After init, metrics should be recordable without panic
        record_phase_transition("planning", "executing");
        let metric = &*PHASE_TRANSITIONS_TOTAL;
        // Just verify we can read the metric (actual value depends on test order)
        let _count = metric.with_label_values(&["planning", "executing"]).get();
    }

    #[test]
    fn record_phase_transition_increments_counter() {
        // Get initial count
        let metric = &*PHASE_TRANSITIONS_TOTAL;
        let initial = metric.with_label_values(&["planning", "executing"]).get();

        record_phase_transition("planning", "executing");

        let after = metric.with_label_values(&["planning", "executing"]).get();
        assert_eq!(after, initial + 1, "counter should increment by 1");
    }

    #[test]
    fn record_stuck_detection_increments_counter() {
        // Get initial count
        let metric = &*STUCK_DETECTIONS_TOTAL;
        let initial = metric.with_label_values(&["repeated_error"]).get();

        record_stuck_detection("repeated_error");

        let after = metric.with_label_values(&["repeated_error"]).get();
        assert_eq!(after, initial + 1, "counter should increment by 1");
    }
}
