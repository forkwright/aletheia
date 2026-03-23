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

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&DISTILLATION_TOTAL);
    LazyLock::force(&DISTILLATION_DURATION_SECONDS);
    LazyLock::force(&TOKENS_SAVED_TOTAL);
}

/// Record a completed distillation operation.
pub fn record_distillation(nous_id: &str, duration_secs: f64, success: bool) {
    // kanon:ignore RUST/pub-visibility
    let status = if success { "ok" } else { "error" };
    DISTILLATION_TOTAL
        .with_label_values(&[nous_id, status])
        .inc();
    DISTILLATION_DURATION_SECONDS
        .with_label_values(&[nous_id])
        .observe(duration_secs);
}

/// Record tokens saved by a distillation pass.
pub fn record_tokens_saved(nous_id: &str, tokens: u64) {
    // kanon:ignore RUST/pub-visibility
    TOKENS_SAVED_TOTAL
        .with_label_values(&[nous_id])
        .inc_by(tokens);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_distillation_does_not_panic() {
        record_distillation("test-nous", 5.0, true);
        record_distillation("test-nous", 2.0, false);
    }

    #[test]
    fn record_tokens_saved_does_not_panic() {
        record_tokens_saved("test-nous", 1000);
    }
}
