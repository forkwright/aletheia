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

static CACHE_READ_TOKENS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_cache_read_tokens_total",
            "Total tokens read from prompt cache (cache hits)"
        ),
        &["nous_id"]
    )
    .expect("metric registration") // kanon:ignore RUST/expect
});

static CACHE_CREATION_TOKENS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_cache_creation_tokens_total",
            "Total tokens written to prompt cache (cache misses)"
        ),
        &["nous_id"]
    )
    .expect("metric registration") // kanon:ignore RUST/expect
});

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "startup pre-registration, not yet wired into server boot sequence"
    )
)]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&PIPELINE_TURNS_TOTAL);
    LazyLock::force(&PIPELINE_STAGE_DURATION_SECONDS);
    LazyLock::force(&PIPELINE_ERRORS_TOTAL);
    LazyLock::force(&CACHE_READ_TOKENS_TOTAL);
    LazyLock::force(&CACHE_CREATION_TOKENS_TOTAL);
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

/// Record prompt cache usage from an API response.
///
/// Tracks cache effectiveness for cost monitoring. `cache_read_tokens` maps to
/// the Anthropic response `cache_read_input_tokens` field (cache hits).
/// `cache_creation_tokens` maps to `cache_creation_input_tokens` (cache misses
/// that populate the cache for subsequent requests).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "forked agent cache coherence — wired from pipeline turn completion"
    )
)]
pub(crate) fn record_cache_usage(
    nous_id: &str,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) {
    CACHE_READ_TOKENS_TOTAL
        .with_label_values(&[nous_id])
        .inc_by(cache_read_tokens);
    CACHE_CREATION_TOKENS_TOTAL
        .with_label_values(&[nous_id])
        .inc_by(cache_creation_tokens);
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

    #[test]
    fn record_cache_usage_does_not_panic() {
        record_cache_usage("test-nous", 1500, 500);
    }

    #[test]
    fn record_cache_usage_zero_values() {
        // WHY: first request has no cache to read from; all tokens are creation
        record_cache_usage("cold-start-agent", 0, 2000);
        // WHY: fully cached request reads everything, creates nothing
        record_cache_usage("warm-agent", 3000, 0);
    }

    #[test]
    fn record_cache_usage_multiple_agents() {
        for agent in ["parent-agent", "child-fork-1", "child-fork-2"] {
            record_cache_usage(agent, 1000, 200);
        }
    }
}
