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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
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
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
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
            .with_label_values(&[provider])
            .inc_by(cost_usd);
    }
}

/// Record LLM request latency.
pub fn record_latency(model: &str, status: &str, duration_secs: f64) {
    LLM_REQUEST_DURATION_SECONDS
        .with_label_values(&[model, status])
        .observe(duration_secs);
}

/// Record time to first token (streaming only).
pub fn record_ttft(model: &str, status: &str, duration_secs: f64) {
    LLM_TTFT_SECONDS
        .with_label_values(&[model, status])
        .observe(duration_secs);
}

/// Record cache token usage from a completed LLM API call.
pub fn record_cache_tokens(provider: &str, cache_read_tokens: u64, cache_write_tokens: u64) {
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
        .with_label_values(&[provider])
        .set(i64::from(limit));
}

/// Set the current EWMA latency estimate for a provider.
pub(crate) fn set_concurrency_latency_ewma(provider: &str, ewma_secs: f64) {
    LLM_CONCURRENCY_LATENCY_EWMA
        .with_label_values(&[provider])
        .set(ewma_secs);
}

/// Set the current in-flight request count for a provider.
pub(crate) fn set_concurrency_in_flight(provider: &str, in_flight: u32) {
    LLM_CONCURRENCY_IN_FLIGHT
        .with_label_values(&[provider])
        .set(i64::from(in_flight));
}
