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
    .expect(
        "metric registration fails only on name/label collision, a startup-time programming error",
    ) // kanon:ignore RUST/expect
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
    .expect(
        "metric registration fails only on name/label collision, a startup-time programming error",
    ) // kanon:ignore RUST/expect
});

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "metric init called from server startup")
)]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&TOOL_INVOCATIONS_TOTAL);
    LazyLock::force(&TOOL_DURATION_SECONDS);
}

/// Record a tool invocation.
pub(crate) fn record_invocation(tool_name: &str, duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    TOOL_INVOCATIONS_TOTAL
        .with_label_values(&[tool_name, status])
        .inc();
    TOOL_DURATION_SECONDS
        .with_label_values(&[tool_name]) // kanon:ignore RUST/indexing-slicing
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_registers_all_metrics() {
        init();
        // Verify metrics are registered by accessing them
        let _ = TOOL_INVOCATIONS_TOTAL.with_label_values(&["test", "ok"]).get();
        let _ = TOOL_DURATION_SECONDS
            .with_label_values(&["test"])
            .get_sample_count();
    }

    #[test]
    fn record_invocation_records_success() {
        let tool_name = "test-tool-success";
        let ok_before = TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "ok"]).get();
        let error_before = TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "error"]).get();
        let hist_before = TOOL_DURATION_SECONDS
            .with_label_values(&[tool_name])
            .get_sample_count();

        record_invocation(tool_name, 0.05, true);
        assert_eq!(
            TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "ok"]).get(),
            ok_before + 1,
            "ok counter should increment for success=true"
        );
        assert_eq!(
            TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "error"]).get(),
            error_before,
            "error counter should not change for success=true"
        );
        assert_eq!(
            TOOL_DURATION_SECONDS
                .with_label_values(&[tool_name])
                .get_sample_count(),
            hist_before + 1,
            "histogram should have 1 sample"
        );
    }

    #[test]
    fn record_invocation_records_failure() {
        let tool_name = "test-tool-failure";
        let ok_before = TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "ok"]).get();
        let error_before = TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "error"]).get();
        let hist_before = TOOL_DURATION_SECONDS
            .with_label_values(&[tool_name])
            .get_sample_count();

        record_invocation(tool_name, 0.01, false);
        assert_eq!(
            TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "ok"]).get(),
            ok_before,
            "ok counter should not change for success=false"
        );
        assert_eq!(
            TOOL_INVOCATIONS_TOTAL.with_label_values(&[tool_name, "error"]).get(),
            error_before + 1,
            "error counter should increment for success=false"
        );
        assert_eq!(
            TOOL_DURATION_SECONDS
                .with_label_values(&[tool_name])
                .get_sample_count(),
            hist_before + 1,
            "histogram should have 1 sample"
        );
    }
}
