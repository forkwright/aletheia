//! Prometheus metric definitions for the agent pipeline.

// WHY: registration panics only on duplicate name (programmer error), so .expect() is appropriate in LazyLock
#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec,
    register_int_counter_vec,
};

static PIPELINE_TURNS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_pipeline_turns_total",
            "Total pipeline turns processed"
        ),
        &["nous_id"]
    )
    .expect("metric registration") // kanon:ignore RUST/expect
});

static PIPELINE_STAGE_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_pipeline_stage_duration_seconds",
            "Pipeline stage duration in seconds"
        )
        .buckets(vec![
            0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0
        ]),
        &["nous_id", "stage"]
    )
    .expect("metric registration") // kanon:ignore RUST/expect
});

static PIPELINE_ERRORS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_pipeline_errors_total", "Total pipeline errors"),
        &["nous_id", "stage", "error_type"]
    )
    .expect("metric registration") // kanon:ignore RUST/expect
});

#[cfg_attr(not(test), expect(dead_code, reason = "startup pre-registration, not yet wired into server boot sequence"))]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&PIPELINE_TURNS_TOTAL);
    LazyLock::force(&PIPELINE_STAGE_DURATION_SECONDS);
    LazyLock::force(&PIPELINE_ERRORS_TOTAL);
}

/// Record a completed pipeline stage.
pub(crate) fn record_stage(nous_id: &str, stage: &str, duration_secs: f64) {
    PIPELINE_STAGE_DURATION_SECONDS
        .with_label_values(&[nous_id, stage])
        .observe(duration_secs);
}

/// Record a completed turn.
pub(crate) fn record_turn(nous_id: &str) {
    PIPELINE_TURNS_TOTAL.with_label_values(&[nous_id]).inc(); // kanon:ignore RUST/indexing-slicing
}

/// Record a pipeline error.
pub(crate) fn record_error(nous_id: &str, stage: &str, error_type: &str) {
    PIPELINE_ERRORS_TOTAL
        .with_label_values(&[nous_id, stage, error_type])
        .inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_stage_does_not_panic() {
        record_stage("test-nous", "context", 0.001);
        record_stage("test-nous", "execute", 1.5);
    }

    #[test]
    fn record_turn_does_not_panic() {
        record_turn("test-nous");
    }

    #[test]
    fn record_error_does_not_panic() {
        record_error("test-nous", "execute", "provider_unavailable");
    }

    #[test]
    fn record_multiple_stages_different_agents() {
        for agent in ["syn", "demiurge", "chiron"] {
            record_stage(agent, "context", 0.01);
            record_stage(agent, "execute", 0.5);
            record_turn(agent);
        }
    }
}
