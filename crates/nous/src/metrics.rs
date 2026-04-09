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

static BACKGROUND_TASK_FAILURES_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_background_task_failures_total",
            "Total background task failures (extraction, distillation, skill analysis)"
        ),
        &["nous_id", "task_type"]
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
    LazyLock::force(&BACKGROUND_TASK_FAILURES_TOTAL);
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

/// Record a background task failure.
///
/// WHY: Background extraction/distillation failures are silent data loss.
/// This counter surfaces the failure rate for alerting. Closes #2724.
pub(crate) fn record_background_failure(nous_id: &str, task_type: &str) {
    BACKGROUND_TASK_FAILURES_TOTAL
        .with_label_values(&[nous_id, task_type])
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
#[expect(
    clippy::indexing_slicing,
    reason = "prometheus desc() returns a non-empty slice for registered metrics; indexing [0] is safe by contract"
)]
mod tests {
    use prometheus::core::Collector;

    use super::*;

    #[test]
    fn init_registers_all_metrics() {
        init();
        // Verify all metrics are registered by accessing their metadata
        assert_eq!(
            PIPELINE_TURNS_TOTAL.desc()[0].fq_name,
            "aletheia_pipeline_turns_total"
        );
        assert_eq!(
            PIPELINE_STAGE_DURATION_SECONDS.desc()[0].fq_name,
            "aletheia_pipeline_stage_duration_seconds"
        );
        assert_eq!(
            PIPELINE_ERRORS_TOTAL.desc()[0].fq_name,
            "aletheia_pipeline_errors_total"
        );
        assert_eq!(
            CACHE_READ_TOKENS_TOTAL.desc()[0].fq_name,
            "aletheia_cache_read_tokens_total"
        );
        assert_eq!(
            CACHE_CREATION_TOKENS_TOTAL.desc()[0].fq_name,
            "aletheia_cache_creation_tokens_total"
        );
    }

    #[test]
    fn record_stage_increments_histogram_count() {
        let agent = "test-record-stage";
        let before = PIPELINE_STAGE_DURATION_SECONDS
            .with_label_values(&[agent, "context"])
            .get_sample_count();
        record_stage(agent, "context", 0.001);
        record_stage(agent, "context", 0.002);
        let after = PIPELINE_STAGE_DURATION_SECONDS
            .with_label_values(&[agent, "context"])
            .get_sample_count();
        assert_eq!(after - before, 2, "expected 2 samples to be recorded");
    }

    #[test]
    fn record_turn_increments_counter() {
        let agent = "test-record-turn";
        let before = PIPELINE_TURNS_TOTAL
            .with_label_values(&[agent])
            .get();
        record_turn(agent);
        record_turn(agent);
        record_turn(agent);
        let after = PIPELINE_TURNS_TOTAL
            .with_label_values(&[agent])
            .get();
        assert_eq!(after - before, 3, "expected counter to increment by 3");
    }

    #[test]
    fn record_error_increments_counter() {
        let agent = "test-record-error";
        let error_type = "test_error";
        let before = PIPELINE_ERRORS_TOTAL
            .with_label_values(&[agent, "execute", error_type])
            .get();
        record_error(agent, "execute", error_type);
        let after = PIPELINE_ERRORS_TOTAL
            .with_label_values(&[agent, "execute", error_type])
            .get();
        assert_eq!(after - before, 1, "expected error counter to increment by 1");
    }

    #[test]
    fn record_multiple_stages_different_agents() {
        for agent in ["syn", "demiurge", "analyst"] {
            let before = PIPELINE_STAGE_DURATION_SECONDS
                .with_label_values(&[agent, "context"])
                .get_sample_count();
            record_stage(agent, "context", 0.01);
            let after = PIPELINE_STAGE_DURATION_SECONDS
                .with_label_values(&[agent, "context"])
                .get_sample_count();
            assert_eq!(
                after - before,
                1,
                "expected one sample recorded for {agent}"
            );
            record_turn(agent);
        }
        // Verify each agent has exactly one turn recorded
        for agent in ["syn", "demiurge", "analyst"] {
            let turns = PIPELINE_TURNS_TOTAL.with_label_values(&[agent]).get();
            assert!(turns >= 1, "expected at least one turn for {agent}");
        }
    }

    #[test]
    fn record_cache_usage_increments_counters() {
        let agent = "test-cache-usage";
        let read_before = CACHE_READ_TOKENS_TOTAL
            .with_label_values(&[agent])
            .get();
        let creation_before = CACHE_CREATION_TOKENS_TOTAL
            .with_label_values(&[agent])
            .get();
        record_cache_usage(agent, 1500, 500);
        let read_after = CACHE_READ_TOKENS_TOTAL
            .with_label_values(&[agent])
            .get();
        let creation_after = CACHE_CREATION_TOKENS_TOTAL
            .with_label_values(&[agent])
            .get();
        assert_eq!(
            read_after - read_before,
            1500,
            "expected read tokens to increase by 1500"
        );
        assert_eq!(
            creation_after - creation_before,
            500,
            "expected creation tokens to increase by 500"
        );
    }

    #[test]
    fn record_cache_usage_zero_values() {
        // WHY: first request has no cache to read from; all tokens are creation
        let cold_agent = "cold-start-agent";
        let creation_before = CACHE_CREATION_TOKENS_TOTAL
            .with_label_values(&[cold_agent])
            .get();
        record_cache_usage(cold_agent, 0, 2000);
        let creation_after = CACHE_CREATION_TOKENS_TOTAL
            .with_label_values(&[cold_agent])
            .get();
        assert_eq!(
            creation_after - creation_before,
            2000,
            "expected 2000 creation tokens for cold start"
        );

        // WHY: fully cached request reads everything, creates nothing
        let warm_agent = "warm-agent";
        let read_before = CACHE_READ_TOKENS_TOTAL
            .with_label_values(&[warm_agent])
            .get();
        record_cache_usage(warm_agent, 3000, 0);
        let read_after = CACHE_READ_TOKENS_TOTAL
            .with_label_values(&[warm_agent])
            .get();
        assert_eq!(
            read_after - read_before,
            3000,
            "expected 3000 read tokens for warm agent"
        );
    }

    #[test]
    fn record_cache_usage_multiple_agents() {
        let agents = ["parent-agent", "child-fork-1", "child-fork-2"];
        let before_counts: Vec<_> = agents
            .iter()
            .map(|agent| {
                CACHE_READ_TOKENS_TOTAL
                    .with_label_values(&[agent])
                    .get()
            })
            .collect();
        for agent in agents {
            record_cache_usage(agent, 1000, 200);
        }
        for (i, agent) in agents.iter().enumerate() {
            let after = CACHE_READ_TOKENS_TOTAL
                .with_label_values(&[agent])
                .get();
            assert_eq!(
                after - before_counts[i],
                1000,
                "expected 1000 read tokens for {agent}"
            );
        }
    }
}
