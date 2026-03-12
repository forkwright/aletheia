//! Prometheus metric definitions for the tool system.

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec,
    register_int_counter_vec,
};

static TOOL_INVOCATIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error"
    )]
    register_int_counter_vec!(
        Opts::new("aletheia_tool_invocations_total", "Total tool invocations"),
        &["tool_name", "status"]
    )
    .expect("metric registration")
});

static TOOL_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error"
    )]
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_tool_duration_seconds",
            "Tool execution duration in seconds"
        )
        .buckets(vec![
            0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0
        ]),
        &["tool_name"]
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    LazyLock::force(&TOOL_INVOCATIONS_TOTAL);
    LazyLock::force(&TOOL_DURATION_SECONDS);
}

/// Record a tool invocation.
pub fn record_invocation(tool_name: &str, duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    TOOL_INVOCATIONS_TOTAL
        .with_label_values(&[tool_name, status])
        .inc();
    TOOL_DURATION_SECONDS
        .with_label_values(&[tool_name])
        .observe(duration_secs);
}
