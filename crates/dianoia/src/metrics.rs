//! Prometheus metric definitions for planning and project orchestration.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{IntCounterVec, IntGauge, Opts, register_int_counter_vec, register_int_gauge};

static PROJECTS_ACTIVE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "aletheia_projects_active",
        "Number of currently active projects"
    )
    .expect("metric registration")
});

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

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&PROJECTS_ACTIVE);
    LazyLock::force(&PHASE_TRANSITIONS_TOTAL);
    LazyLock::force(&STUCK_DETECTIONS_TOTAL);
}

/// Set the number of currently active projects.
pub fn set_projects_active(count: i64) {
    // kanon:ignore RUST/pub-visibility
    PROJECTS_ACTIVE.set(count);
}

/// Record a project state transition.
pub fn record_phase_transition(from: &str, to: &str) {
    // kanon:ignore RUST/pub-visibility
    PHASE_TRANSITIONS_TOTAL.with_label_values(&[from, to]).inc();
}

/// Record a stuck pattern detection.
pub fn record_stuck_detection(pattern: &str) {
    // kanon:ignore RUST/pub-visibility
    STUCK_DETECTIONS_TOTAL.with_label_values(&[pattern]).inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn set_projects_active_does_not_panic() {
        set_projects_active(3);
        set_projects_active(0);
    }

    #[test]
    fn record_phase_transition_does_not_panic() {
        record_phase_transition("planning", "executing");
        record_phase_transition("executing", "verifying");
    }

    #[test]
    fn record_stuck_detection_does_not_panic() {
        record_stuck_detection("repeated_error");
        record_stuck_detection("same_tool_same_args");
    }
}
