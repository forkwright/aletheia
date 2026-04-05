//! Prometheus metric definitions for energeia dispatch orchestration.
//!
//! Call [`init`] once at startup to force-register all metrics with the global
//! prometheus registry. The pylon `/metrics` endpoint will then expose them.
//!
//! Recording functions are called at dispatch/session/QA event boundaries.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    CounterVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, register_counter_vec,
    register_histogram_vec, register_int_counter_vec,
};

// ---------------------------------------------------------------------------
// Metric statics
// ---------------------------------------------------------------------------

static DISPATCHES_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("energeia_dispatches_total", "Total dispatch runs completed"),
        &["project", "status"]
    )
    .expect("metric registration")
});

static SESSIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("energeia_sessions_total", "Total agent sessions dispatched"),
        &["project", "status"]
    )
    .expect("metric registration")
});

/// Float counter: USD is not integer-valued.
static COST_USD_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        Opts::new(
            "energeia_cost_usd_total",
            "Cumulative LLM cost in USD by project and model"
        ),
        &["project", "model"]
    )
    .expect("metric registration")
});

static SESSION_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "energeia_session_duration_seconds",
            "Agent session wall-clock duration in seconds"
        )
        // WHY: sessions range from <1 minute (infra failure) to several hours
        // (complex implementation prompts). Buckets cover this full range.
        .buckets(vec![
            60.0, 300.0, 900.0, 1_800.0, 3_600.0, 7_200.0, 14_400.0, 28_800.0,
        ]),
        &["project"]
    )
    .expect("metric registration")
});

static QA_VERDICTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "energeia_qa_verdicts_total",
            "Total QA evaluation verdicts by project and verdict"
        ),
        &["project", "verdict"]
    )
    .expect("metric registration")
});

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Force-initialize all energeia metric statics.
///
/// Must be called once at server startup so metrics appear in `/metrics` even
/// before any dispatch events occur. Safe to call multiple times.
pub fn init() {
    LazyLock::force(&DISPATCHES_TOTAL);
    LazyLock::force(&SESSIONS_TOTAL);
    LazyLock::force(&COST_USD_TOTAL);
    LazyLock::force(&SESSION_DURATION_SECONDS);
    LazyLock::force(&QA_VERDICTS_TOTAL);
}

// ---------------------------------------------------------------------------
// Recording functions
// ---------------------------------------------------------------------------

/// Record a completed dispatch run.
///
/// Call once per dispatch when it finishes (Completed or Failed).
pub fn record_dispatch(project: &str, status: &str) {
    DISPATCHES_TOTAL.with_label_values(&[project, status]).inc();
}

/// Record a completed agent session.
///
/// - `cost_usd` — session cost; silently skipped when zero.
/// - `duration_ms` — wall-clock duration in milliseconds.
pub fn record_session(project: &str, status: &str, cost_usd: f64, duration_ms: u64) {
    SESSIONS_TOTAL.with_label_values(&[project, status]).inc();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "duration_ms to f64: session durations are far below f64 precision threshold"
    )]
    let duration_secs = duration_ms as f64 / 1_000.0;
    SESSION_DURATION_SECONDS
        .with_label_values(&[project])
        .observe(duration_secs);

    if cost_usd > 0.0 {
        // WHY: DispatchSpec carries no model field yet; use "unknown" until the
        // store schema is extended to track per-session model selection.
        COST_USD_TOTAL
            .with_label_values(&[project, "unknown"])
            .inc_by(cost_usd);
    }
}

/// Record a QA evaluation verdict.
///
/// `verdict` should be one of `"pass"`, `"partial"`, or `"fail"`.
pub fn record_qa_verdict(project: &str, verdict: &str) {
    QA_VERDICTS_TOTAL
        .with_label_values(&[project, verdict])
        .inc();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        init();
        init(); // second call must not panic
    }

    #[test]
    fn record_dispatch_increments_counter() {
        init();
        record_dispatch("acme", "completed");
        record_dispatch("acme", "completed");
        let count = DISPATCHES_TOTAL
            .with_label_values(&["acme", "completed"])
            .get();
        assert!(count >= 2, "counter should be >= 2 after two increments");
    }

    #[test]
    fn record_session_increments_counter_and_histogram() {
        init();
        record_session("acme", "success", 0.50, 30_000);
        let count = SESSIONS_TOTAL.with_label_values(&["acme", "success"]).get();
        assert!(count >= 1);
    }

    #[test]
    fn record_session_zero_cost_skips_cost_counter() {
        init();
        // Capture cost before
        let before = COST_USD_TOTAL
            .with_label_values(&["nocost-project", "unknown"])
            .get();
        record_session("nocost-project", "failed", 0.0, 5_000);
        let after = COST_USD_TOTAL
            .with_label_values(&["nocost-project", "unknown"])
            .get();
        // Float comparison: should be unchanged
        assert!(
            (after - before).abs() < 1e-10,
            "zero-cost session must not increment cost counter"
        );
    }

    #[test]
    fn record_qa_verdict_increments_counter() {
        init();
        record_qa_verdict("acme", "pass");
        let count = QA_VERDICTS_TOTAL.with_label_values(&["acme", "pass"]).get();
        assert!(count >= 1);
    }
}
