//! Prometheus metric definitions for the agent pipeline.

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts,
    register_histogram_vec, register_int_counter_vec,
};

static PIPELINE_TURNS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_pipeline_turns_total", "Total pipeline turns processed"),
        &["nous_id"]
    )
    .expect("metric registration")
});

static PIPELINE_STAGE_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_pipeline_stage_duration_seconds",
            "Pipeline stage duration in seconds"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0]),
        &["nous_id", "stage"]
    )
    .expect("metric registration")
});

static PIPELINE_ERRORS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_pipeline_errors_total", "Total pipeline errors"),
        &["nous_id", "stage", "error_type"]
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    LazyLock::force(&PIPELINE_TURNS_TOTAL);
    LazyLock::force(&PIPELINE_STAGE_DURATION_SECONDS);
    LazyLock::force(&PIPELINE_ERRORS_TOTAL);
}

/// Record a completed pipeline stage.
pub fn record_stage(nous_id: &str, stage: &str, duration_secs: f64) {
    PIPELINE_STAGE_DURATION_SECONDS
        .with_label_values(&[nous_id, stage])
        .observe(duration_secs);
}

/// Record a completed turn.
pub fn record_turn(nous_id: &str) {
    PIPELINE_TURNS_TOTAL.with_label_values(&[nous_id]).inc();
}

/// Record a pipeline error.
pub fn record_error(nous_id: &str, stage: &str, error_type: &str) {
    PIPELINE_ERRORS_TOTAL
        .with_label_values(&[nous_id, stage, error_type])
        .inc();
}
