//! Prometheus metric definitions for LLM providers.

use std::sync::LazyLock;

use prometheus::{
    CounterVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts,
    register_counter_vec, register_histogram_vec, register_int_counter_vec, register_int_gauge_vec,
};

static LLM_TOKENS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_counter_vec!(
        Opts::new("aletheia_llm_tokens_total", "Total LLM tokens consumed"),
        &["provider", "direction"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_COST_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_counter_vec!(
        Opts::new("aletheia_llm_cost_total", "Total LLM cost in USD"),
        &["provider"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_counter_vec!(
        Opts::new("aletheia_llm_requests_total", "Total LLM API requests"),
        &["provider", "status"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_CACHE_TOKENS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_counter_vec!(
        Opts::new(
            "aletheia_llm_cache_tokens_total",
            "Total LLM cache tokens (read and written)"
        ),
        &["provider", "direction"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_llm_request_duration_seconds",
            "LLM request duration in seconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0]),
        &["model", "status"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_TTFT_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_llm_ttft_seconds",
            "LLM time to first token in seconds (streaming only)"
        )
        .buckets(vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0]),
        &["model", "status"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_counter_vec!(
        Opts::new(
            "aletheia_llm_circuit_breaker_transitions_total",
            "Circuit breaker state transitions per provider"
        ),
        &["provider", "from", "to"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_CONCURRENCY_LIMIT: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_gauge_vec!(
        Opts::new(
            "aletheia_llm_concurrency_limit",
            "Current adaptive concurrency limit per provider"
        ),
        &["provider"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_CONCURRENCY_LATENCY_EWMA: LazyLock<prometheus::GaugeVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    prometheus::register_gauge_vec!(
        Opts::new(
            "aletheia_llm_concurrency_latency_ewma_seconds",
            "EWMA of LLM response latency used by the adaptive concurrency limiter"
        ),
        &["provider"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

static LLM_CONCURRENCY_IN_FLIGHT: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored"
    )]
    register_int_gauge_vec!(
        Opts::new(
            "aletheia_llm_concurrency_in_flight",
            "Number of in-flight LLM requests per provider"
        ),
        &["provider"]
    )
    .expect("metric registration fails only on name/label collision, a startup-time programming error that must not be silently ignored") // kanon:ignore RUST/expect
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&LLM_TOKENS_TOTAL);
    LazyLock::force(&LLM_COST_TOTAL);
    LazyLock::force(&LLM_REQUESTS_TOTAL);
    LazyLock::force(&LLM_CACHE_TOKENS_TOTAL);
    LazyLock::force(&LLM_REQUEST_DURATION_SECONDS);
    LazyLock::force(&LLM_TTFT_SECONDS);
    LazyLock::force(&LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL);
    LazyLock::force(&LLM_CONCURRENCY_LIMIT);
    LazyLock::force(&LLM_CONCURRENCY_LATENCY_EWMA);
    LazyLock::force(&LLM_CONCURRENCY_IN_FLIGHT);
}

/// Record a completed LLM API call.
pub fn record_completion(
    // kanon:ignore RUST/pub-visibility
    provider: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    success: bool,
) {
    let status = if success { "ok" } else { "error" };
    LLM_REQUESTS_TOTAL
        .with_label_values(&[provider, status])
        .inc();
    LLM_TOKENS_TOTAL
        .with_label_values(&[provider, "input"])
        .inc_by(input_tokens);
    LLM_TOKENS_TOTAL
        .with_label_values(&[provider, "output"])
        .inc_by(output_tokens);
    if cost_usd > 0.0 {
        LLM_COST_TOTAL
            .with_label_values(&[provider]) // kanon:ignore RUST/indexing-slicing
            .inc_by(cost_usd);
    }
}

/// Record LLM request latency.
pub fn record_latency(model: &str, status: &str, duration_secs: f64) {
    // kanon:ignore RUST/pub-visibility
    LLM_REQUEST_DURATION_SECONDS
        .with_label_values(&[model, status])
        .observe(duration_secs);
}

/// Record time to first token (streaming only).
pub fn record_ttft(model: &str, status: &str, duration_secs: f64) {
    // kanon:ignore RUST/pub-visibility
    LLM_TTFT_SECONDS
        .with_label_values(&[model, status])
        .observe(duration_secs);
}

/// Record cache token usage from a completed LLM API call.
pub fn record_cache_tokens(provider: &str, cache_read_tokens: u64, cache_write_tokens: u64) {
    // kanon:ignore RUST/pub-visibility
    if cache_read_tokens > 0 {
        LLM_CACHE_TOKENS_TOTAL
            .with_label_values(&[provider, "read"])
            .inc_by(cache_read_tokens);
    }
    if cache_write_tokens > 0 {
        LLM_CACHE_TOKENS_TOTAL
            .with_label_values(&[provider, "write"])
            .inc_by(cache_write_tokens);
    }
}

/// Record a circuit breaker state transition.
///
/// `from` and `to` are lowercase state names: `closed`, `open`, `half_open`.
pub(crate) fn record_circuit_transition(provider: &str, from: &str, to: &str) {
    LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL
        .with_label_values(&[provider, from, to])
        .inc();
}

/// Set the current adaptive concurrency limit for a provider.
pub(crate) fn set_concurrency_limit(provider: &str, limit: u32) {
    LLM_CONCURRENCY_LIMIT
        .with_label_values(&[provider]) // kanon:ignore RUST/indexing-slicing
        .set(i64::from(limit));
}

/// Set the current EWMA latency estimate for a provider.
pub(crate) fn set_concurrency_latency_ewma(provider: &str, ewma_secs: f64) {
    LLM_CONCURRENCY_LATENCY_EWMA
        .with_label_values(&[provider]) // kanon:ignore RUST/indexing-slicing
        .set(ewma_secs);
}

/// Set the current in-flight request count for a provider.
pub(crate) fn set_concurrency_in_flight(provider: &str, in_flight: u32) {
    LLM_CONCURRENCY_IN_FLIGHT
        .with_label_values(&[provider]) // kanon:ignore RUST/indexing-slicing
        .set(i64::from(in_flight));
}

#[cfg(test)]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "prometheus counter values are known non-negative small integers in tests"
)]
mod tests {
    use super::*;

    /// Read a Prometheus counter value by metric name and label values.
    fn read_counter(name: &str, label_names: &[&str], label_values: &[&str]) -> u64 {
        let families = prometheus::default_registry().gather();
        for family in &families {
            if family.name() == name {
                for metric in family.get_metric() {
                    let labels = metric.get_label();
                    // Match labels by name->value
                    let label_map: std::collections::HashMap<_, _> = labels
                        .iter()
                        .map(|l| (l.name(), l.value()))
                        .collect();
                    let matches =
                        label_names
                            .iter()
                            .zip(label_values.iter())
                            .all(|(name, expected)| {
                                label_map.get(name).is_some_and(|v| *v == *expected)
                            });
                    if matches && labels.len() == label_values.len() {
                        return metric.get_counter().value() as u64;
                    }
                }
            }
        }
        0
    }

    /// Read a Prometheus float counter value by metric name and label values.
    fn read_float_counter(name: &str, label_names: &[&str], label_values: &[&str]) -> f64 {
        let families = prometheus::default_registry().gather();
        for family in &families {
            if family.name() == name {
                for metric in family.get_metric() {
                    let labels = metric.get_label();
                    let label_map: std::collections::HashMap<_, _> = labels
                        .iter()
                        .map(|l| (l.name(), l.value()))
                        .collect();
                    let matches =
                        label_names
                            .iter()
                            .zip(label_values.iter())
                            .all(|(name, expected)| {
                                label_map.get(name).is_some_and(|v| *v == *expected)
                            });
                    if matches && labels.len() == label_values.len() {
                        return metric.get_counter().value();
                    }
                }
            }
        }
        0.0
    }

    /// Read a Prometheus histogram count by metric name and label values.
    fn read_histogram_count(name: &str, label_names: &[&str], label_values: &[&str]) -> u64 {
        let families = prometheus::default_registry().gather();
        for family in &families {
            if family.name() == name {
                for metric in family.get_metric() {
                    let labels = metric.get_label();
                    let label_map: std::collections::HashMap<_, _> = labels
                        .iter()
                        .map(|l| (l.name(), l.value()))
                        .collect();
                    let matches =
                        label_names
                            .iter()
                            .zip(label_values.iter())
                            .all(|(name, expected)| {
                                label_map.get(name).is_some_and(|v| *v == *expected)
                            });
                    if matches && labels.len() == label_values.len() {
                        return metric.get_histogram().sample_count();
                    }
                }
            }
        }
        0
    }

    #[test]
    fn init_registers_all_metrics() {
        // Initialize metrics - this forces all LazyLock statics to register
        init();

        // Record values on metrics to ensure they appear in gather()
        record_completion("test-init", 1, 1, 0.001, true);
        record_cache_tokens("test-init", 1, 1);
        record_latency("test-init", "ok", 0.1);
        record_ttft("test-init", "ok", 0.1);
        // Set concurrency metrics so they appear in the registry
        set_concurrency_limit("test-init", 10);
        set_concurrency_latency_ewma("test-init", 0.5);
        set_concurrency_in_flight("test-init", 5);

        let families = prometheus::default_registry().gather();
        let metric_names: std::collections::HashSet<_> = families
            .iter()
            .map(prometheus::proto::MetricFamily::name)
            .collect();

        // Core metrics that are always registered after recording
        assert!(metric_names.contains("aletheia_llm_tokens_total"), "tokens metric should be registered");
        assert!(metric_names.contains("aletheia_llm_cost_total"), "cost metric should be registered");
        assert!(metric_names.contains("aletheia_llm_requests_total"), "requests metric should be registered");
        assert!(metric_names.contains("aletheia_llm_cache_tokens_total"), "cache tokens metric should be registered");
        assert!(metric_names.contains("aletheia_llm_request_duration_seconds"), "request duration metric should be registered");
        assert!(metric_names.contains("aletheia_llm_ttft_seconds"), "ttft metric should be registered");
        // Concurrency metrics registered after setting values
        assert!(metric_names.contains("aletheia_llm_concurrency_limit"), "concurrency limit metric should be registered");
        assert!(metric_names.contains("aletheia_llm_concurrency_latency_ewma_seconds"), "concurrency latency metric should be registered");
        assert!(metric_names.contains("aletheia_llm_concurrency_in_flight"), "concurrency in-flight metric should be registered");
    }

    #[test]
    fn record_completion_success_updates_all_metrics() {
        // Use a unique provider name to avoid test interference
        let provider = "test-completion-success";
        let before_requests = read_counter("aletheia_llm_requests_total", &["provider", "status"], &[provider, "ok"]);
        let before_tokens_in =
            read_counter("aletheia_llm_tokens_total", &["provider", "direction"], &[provider, "input"]);
        let before_tokens_out =
            read_counter("aletheia_llm_tokens_total", &["provider", "direction"], &[provider, "output"]);
        let before_cost = read_float_counter("aletheia_llm_cost_total", &["provider"], &[provider]);

        record_completion(provider, 100, 50, 0.001, true);

        assert_eq!(
            read_counter("aletheia_llm_requests_total", &["provider", "status"], &[provider, "ok"]),
            before_requests + 1,
            "request counter should increment"
        );
        assert_eq!(
            read_counter("aletheia_llm_tokens_total", &["provider", "direction"], &[provider, "input"]),
            before_tokens_in + 100,
            "input token counter should increment by 100"
        );
        assert_eq!(
            read_counter("aletheia_llm_tokens_total", &["provider", "direction"], &[provider, "output"]),
            before_tokens_out + 50,
            "output token counter should increment by 50"
        );
        let after_cost = read_float_counter("aletheia_llm_cost_total", &["provider"], &[provider]);
        assert!(
            after_cost > before_cost,
            "cost counter should increase (before: {before_cost}, after: {after_cost})"
        );
    }

    #[test]
    fn record_completion_failure_updates_error_metrics() {
        let provider = "test-completion-failure";
        let before = read_counter("aletheia_llm_requests_total", &["provider", "status"], &[provider, "error"]);

        record_completion(provider, 0, 0, 0.0, false);

        assert_eq!(
            read_counter("aletheia_llm_requests_total", &["provider", "status"], &[provider, "error"]),
            before + 1,
            "error request counter should increment"
        );
    }

    #[test]
    fn record_latency_records_observation() {
        let model = "test-model-latency";
        let status = "ok";
        let before = read_histogram_count(
            "aletheia_llm_request_duration_seconds",
            &["model", "status"],
            &[model, status],
        );

        record_latency(model, status, 1.5);

        assert_eq!(
            read_histogram_count(
                "aletheia_llm_request_duration_seconds",
                &["model", "status"],
                &[model, status]
            ),
            before + 1,
            "latency histogram should have one more observation"
        );
    }

    #[test]
    fn record_ttft_records_observation() {
        let model = "test-model-ttft";
        let status = "ok";
        let before = read_histogram_count(
            "aletheia_llm_ttft_seconds",
            &["model", "status"],
            &[model, status],
        );

        record_ttft(model, status, 0.25);

        assert_eq!(
            read_histogram_count("aletheia_llm_ttft_seconds", &["model", "status"], &[model, status]),
            before + 1,
            "ttft histogram should have one more observation"
        );
    }

    #[test]
    fn record_cache_tokens_increments_counters() {
        let provider = "test-cache-tokens";
        let before_read =
            read_counter("aletheia_llm_cache_tokens_total", &["provider", "direction"], &[provider, "read"]);
        let before_write =
            read_counter("aletheia_llm_cache_tokens_total", &["provider", "direction"], &[provider, "write"]);

        record_cache_tokens(provider, 1000, 500);

        assert_eq!(
            read_counter("aletheia_llm_cache_tokens_total", &["provider", "direction"], &[provider, "read"]),
            before_read + 1000,
            "read cache token counter should increment by 1000"
        );
        assert_eq!(
            read_counter("aletheia_llm_cache_tokens_total", &["provider", "direction"], &[provider, "write"]),
            before_write + 500,
            "write cache token counter should increment by 500"
        );
    }
}
