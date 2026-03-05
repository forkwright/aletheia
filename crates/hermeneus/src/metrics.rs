//! Prometheus metric definitions for LLM providers.

use std::sync::LazyLock;

use prometheus::{CounterVec, IntCounterVec, Opts, register_counter_vec, register_int_counter_vec};

static LLM_TOKENS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_llm_tokens_total", "Total LLM tokens consumed"),
        &["provider", "direction"]
    )
    .expect("metric registration")
});

static LLM_COST_TOTAL: LazyLock<CounterVec> = LazyLock::new(|| {
    register_counter_vec!(
        Opts::new("aletheia_llm_cost_total", "Total LLM cost in USD"),
        &["provider"]
    )
    .expect("metric registration")
});

static LLM_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("aletheia_llm_requests_total", "Total LLM API requests"),
        &["provider", "status"]
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    LazyLock::force(&LLM_TOKENS_TOTAL);
    LazyLock::force(&LLM_COST_TOTAL);
    LazyLock::force(&LLM_REQUESTS_TOTAL);
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
