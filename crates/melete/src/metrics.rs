//! Prometheus metric definitions for the distillation engine.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec,
    register_int_counter_vec,
};

static DISTILLATION_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_distillation_total",
            "Total distillation operations"
        ),
        &["nous_id", "status"]
    )
    .expect("metric registration")
});

static DISTILLATION_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_distillation_duration_seconds",
            "Distillation duration in seconds"
        )
        .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0]),
        &["nous_id"]
    )
    .expect("metric registration")
});

static TOKENS_SAVED_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_tokens_saved_total",
            "Total tokens saved by distillation"
        ),
        &["nous_id"]
    )
    .expect("metric registration")
});

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "called from server startup, not from within the crate"
    )
)]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&DISTILLATION_TOTAL);
    LazyLock::force(&DISTILLATION_DURATION_SECONDS);
    LazyLock::force(&TOKENS_SAVED_TOTAL);
}

/// Record a completed distillation operation.
pub(crate) fn record_distillation(nous_id: &str, duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    DISTILLATION_TOTAL
        .with_label_values(&[nous_id, status])
        .inc();
    DISTILLATION_DURATION_SECONDS
        .with_label_values(&[nous_id])
        .observe(duration_secs);
}

/// Record tokens saved by a distillation pass.
pub(crate) fn record_tokens_saved(nous_id: &str, tokens: u64) {
    TOKENS_SAVED_TOTAL
        .with_label_values(&[nous_id])
        .inc_by(tokens);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_registers_all_metrics() {
        init();
        // Verify metrics are registered by accessing them
        let _ = DISTILLATION_TOTAL.with_label_values(&["test", "ok"]).get();
        let _ = DISTILLATION_DURATION_SECONDS
            .with_label_values(&["test"])
            .get_sample_count();
        let _ = TOKENS_SAVED_TOTAL.with_label_values(&["test"]).get();
    }

    #[test]
    fn record_distillation_records_success_and_failure() {
        let nous_id = "test-nous-distillation";
        let ok_before = DISTILLATION_TOTAL.with_label_values(&[nous_id, "ok"]).get();
        let error_before = DISTILLATION_TOTAL.with_label_values(&[nous_id, "error"]).get();
        let hist_before = DISTILLATION_DURATION_SECONDS
            .with_label_values(&[nous_id])
            .get_sample_count();

        // Record successful distillation
        record_distillation(nous_id, 5.0, true);
        assert_eq!(
            DISTILLATION_TOTAL.with_label_values(&[nous_id, "ok"]).get(),
            ok_before + 1,
            "ok counter should increment for success=true"
        );
        assert_eq!(
            DISTILLATION_TOTAL.with_label_values(&[nous_id, "error"]).get(),
            error_before,
            "error counter should not change for success=true"
        );

        // Record failed distillation
        record_distillation(nous_id, 2.0, false);
        assert_eq!(
            DISTILLATION_TOTAL.with_label_values(&[nous_id, "ok"]).get(),
            ok_before + 1,
            "ok counter should be unchanged after error"
        );
        assert_eq!(
            DISTILLATION_TOTAL.with_label_values(&[nous_id, "error"]).get(),
            error_before + 1,
            "error counter should increment for success=false"
        );

        // Verify histogram has 2 samples
        assert_eq!(
            DISTILLATION_DURATION_SECONDS
                .with_label_values(&[nous_id])
                .get_sample_count(),
            hist_before + 2,
            "histogram should have 2 samples"
        );
    }

    #[test]
    fn record_tokens_saved_increments_counter() {
        let nous_id = "test-nous-tokens";
        let before = TOKENS_SAVED_TOTAL.with_label_values(&[nous_id]).get();

        record_tokens_saved(nous_id, 1000);
        assert_eq!(
            TOKENS_SAVED_TOTAL.with_label_values(&[nous_id]).get(),
            before + 1000,
            "tokens saved counter should increase by 1000"
        );

        record_tokens_saved(nous_id, 500);
        assert_eq!(
            TOKENS_SAVED_TOTAL.with_label_values(&[nous_id]).get(),
            before + 1500,
            "tokens saved counter should accumulate (1000 + 500 = 1500)"
        );
    }
}
