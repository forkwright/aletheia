//! Prometheus metric definitions for LLM providers.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`] at startup. Recording functions operate on global
//! `LazyLock` families that share `Arc`-internal state with the registered
//! copies — recording is cheap and lock-free after registration.

use std::sync::LazyLock;
use std::sync::atomic::AtomicU64;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProviderDirectionLabels {
    provider: String,
    direction: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProviderLabels {
    provider: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProviderStatusLabels {
    provider: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ModelStatusLabels {
    model: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CircuitTransitionLabels {
    provider: String,
    from: String,
    to: String,
}

static LLM_TOKENS_TOTAL: LazyLock<Family<ProviderDirectionLabels, Counter>> =
    LazyLock::new(Family::default);

static LLM_COST_TOTAL: LazyLock<Family<ProviderLabels, Counter<f64, AtomicU64>>> =
    LazyLock::new(Family::default);

static LLM_REQUESTS_TOTAL: LazyLock<Family<ProviderStatusLabels, Counter>> =
    LazyLock::new(Family::default);

static LLM_CACHE_TOKENS_TOTAL: LazyLock<Family<ProviderDirectionLabels, Counter>> =
    LazyLock::new(Family::default);

fn request_duration_histogram() -> Histogram {
    Histogram::new([0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0])
}

type ModelStatusHistogramFamily = Family<ModelStatusLabels, Histogram, fn() -> Histogram>;

static LLM_REQUEST_DURATION_SECONDS: LazyLock<ModelStatusHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(request_duration_histogram));

fn ttft_histogram() -> Histogram {
    Histogram::new([0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0])
}

static LLM_TTFT_SECONDS: LazyLock<ModelStatusHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(ttft_histogram));

static LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL: LazyLock<Family<CircuitTransitionLabels, Counter>> =
    LazyLock::new(Family::default);

static LLM_CONCURRENCY_LIMIT: LazyLock<Family<ProviderLabels, Gauge>> =
    LazyLock::new(Family::default);

static LLM_CONCURRENCY_LATENCY_EWMA: LazyLock<Family<ProviderLabels, Gauge<f64, AtomicU64>>> =
    LazyLock::new(Family::default);

static LLM_CONCURRENCY_IN_FLIGHT: LazyLock<Family<ProviderLabels, Gauge>> =
    LazyLock::new(Family::default);

/// Normalize a model ID to a family name for use as a Prometheus label.
///
/// WHY(#5684): `Family<ModelStatusLabels, Histogram>` never evicts entries,
/// so using raw model IDs (e.g. `claude-opus-4-20250514`) causes cardinality
/// to grow unboundedly as versioned snapshots accumulate. Stripping the
/// last dash-segment collapses all snapshots of a family to one label key.
fn model_family_label(model: &str) -> &str {
    model
        .rfind('-')
        .map_or(model, |pos| model.get(..pos).unwrap_or(model))
}

/// Register this crate's metrics with the shared registry.
///
/// Called once at startup. Counter names registered here drop the `_total`
/// suffix because `prometheus-client` appends it automatically during
/// exposition — register `aletheia_llm_tokens`, not `aletheia_llm_tokens_total`.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_llm_tokens",
        "Total LLM tokens consumed",
        LLM_TOKENS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_llm_cost_usd",
        "Total LLM cost in USD",
        LLM_COST_TOTAL.clone(),
    );
    registry.register(
        "aletheia_llm_requests",
        "Total LLM API requests",
        LLM_REQUESTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_llm_cache_tokens",
        "Total LLM cache tokens (read and written)",
        LLM_CACHE_TOKENS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_llm_request_duration_seconds",
        "LLM request duration in seconds",
        LLM_REQUEST_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_llm_ttft_seconds",
        "LLM time to first token in seconds (streaming only)",
        LLM_TTFT_SECONDS.clone(),
    );
    registry.register(
        "aletheia_llm_circuit_breaker_transitions",
        "Circuit breaker state transitions per provider",
        LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_llm_concurrency_limit",
        "Current adaptive concurrency limit per provider",
        LLM_CONCURRENCY_LIMIT.clone(),
    );
    registry.register(
        "aletheia_llm_concurrency_latency_ewma_seconds",
        "EWMA of LLM response latency used by the adaptive concurrency limiter",
        LLM_CONCURRENCY_LATENCY_EWMA.clone(),
    );
    registry.register(
        "aletheia_llm_concurrency_in_flight",
        "Number of in-flight LLM requests per provider",
        LLM_CONCURRENCY_IN_FLIGHT.clone(),
    );
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
        .get_or_create(&ProviderStatusLabels {
            provider: provider.to_owned(),
            status: status.to_owned(),
        })
        .inc();
    LLM_TOKENS_TOTAL
        .get_or_create(&ProviderDirectionLabels {
            provider: provider.to_owned(),
            direction: "input".to_owned(),
        })
        .inc_by(input_tokens);
    LLM_TOKENS_TOTAL
        .get_or_create(&ProviderDirectionLabels {
            provider: provider.to_owned(),
            direction: "output".to_owned(),
        })
        .inc_by(output_tokens);
    if cost_usd > 0.0 {
        LLM_COST_TOTAL
            .get_or_create(&ProviderLabels {
                provider: provider.to_owned(),
            })
            .inc_by(cost_usd);
    }
}

/// Record LLM request latency.
pub fn record_latency(model: &str, status: &str, duration_secs: f64) {
    LLM_REQUEST_DURATION_SECONDS
        .get_or_create(&ModelStatusLabels {
            model: model_family_label(model).to_owned(),
            status: status.to_owned(),
        })
        .observe(duration_secs);
}

/// Record time to first token (streaming only).
pub fn record_ttft(model: &str, status: &str, duration_secs: f64) {
    LLM_TTFT_SECONDS
        .get_or_create(&ModelStatusLabels {
            model: model_family_label(model).to_owned(),
            status: status.to_owned(),
        })
        .observe(duration_secs);
}

/// Record cache token usage from a completed LLM API call.
pub fn record_cache_tokens(provider: &str, cache_read_tokens: u64, cache_write_tokens: u64) {
    if cache_read_tokens > 0 {
        LLM_CACHE_TOKENS_TOTAL
            .get_or_create(&ProviderDirectionLabels {
                provider: provider.to_owned(),
                direction: "read".to_owned(),
            })
            .inc_by(cache_read_tokens);
    }
    if cache_write_tokens > 0 {
        LLM_CACHE_TOKENS_TOTAL
            .get_or_create(&ProviderDirectionLabels {
                provider: provider.to_owned(),
                direction: "write".to_owned(),
            })
            .inc_by(cache_write_tokens);
    }
}

/// Record a circuit breaker state transition.
///
/// `from` and `to` are lowercase state names: `closed`, `open`, `half_open`.
pub(crate) fn record_circuit_transition(provider: &str, from: &str, to: &str) {
    LLM_CIRCUIT_BREAKER_TRANSITIONS_TOTAL
        .get_or_create(&CircuitTransitionLabels {
            provider: provider.to_owned(),
            from: from.to_owned(),
            to: to.to_owned(),
        })
        .inc();
}

/// Set the current adaptive concurrency limit for a provider.
pub(crate) fn set_concurrency_limit(provider: &str, limit: u32) {
    LLM_CONCURRENCY_LIMIT
        .get_or_create(&ProviderLabels {
            provider: provider.to_owned(),
        })
        .set(i64::from(limit));
}

/// Set the current EWMA latency estimate for a provider.
pub(crate) fn set_concurrency_latency_ewma(provider: &str, ewma_secs: f64) {
    LLM_CONCURRENCY_LATENCY_EWMA
        .get_or_create(&ProviderLabels {
            provider: provider.to_owned(),
        })
        .set(ewma_secs);
}

/// Set the current in-flight request count for a provider.
pub(crate) fn set_concurrency_in_flight(provider: &str, in_flight: u32) {
    LLM_CONCURRENCY_IN_FLIGHT
        .get_or_create(&ProviderLabels {
            provider: provider.to_owned(),
        })
        .set(i64::from(in_flight));
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    /// Set up a fresh registry with this crate's metrics registered.
    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    #[test]
    fn register_exposes_all_metric_families() {
        let r = fresh_registry();
        // Emit at least one sample per family so the encoder includes it.
        record_completion("test-init", 1, 1, 0.001, true);
        record_cache_tokens("test-init", 1, 1);
        record_latency("test-init", "ok", 0.1);
        record_ttft("test-init", "ok", 0.1);
        set_concurrency_limit("test-init", 10);
        set_concurrency_latency_ewma("test-init", 0.5);
        set_concurrency_in_flight("test-init", 5);
        record_circuit_transition("test-init", "closed", "open");

        let out = encode(&r);
        for fragment in [
            "aletheia_llm_tokens_total",
            "aletheia_llm_cost_usd_total",
            "aletheia_llm_requests_total",
            "aletheia_llm_cache_tokens_total",
            "aletheia_llm_request_duration_seconds",
            "aletheia_llm_ttft_seconds",
            "aletheia_llm_circuit_breaker_transitions_total",
            "aletheia_llm_concurrency_limit",
            "aletheia_llm_concurrency_latency_ewma_seconds",
            "aletheia_llm_concurrency_in_flight",
        ] {
            assert!(out.contains(fragment), "missing `{fragment}` in: {out}");
        }
    }

    #[test]
    fn record_completion_success_increments_counters() {
        let r = fresh_registry();
        record_completion("success-test", 100, 50, 0.01, true);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_llm_requests_total{provider=\"success-test\",status=\"ok\"} 1"),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_llm_tokens_total{provider=\"success-test\",direction=\"input\"} 100"
            ),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_llm_tokens_total{provider=\"success-test\",direction=\"output\"} 50"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn record_completion_failure_increments_error() {
        let r = fresh_registry();
        record_completion("fail-test", 0, 0, 0.0, false);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_llm_requests_total{provider=\"fail-test\",status=\"error\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn record_latency_emits_histogram() {
        let r = fresh_registry();
        record_latency("latency-test", "ok", 1.5);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_llm_request_duration_seconds_count"),
            "got: {out}"
        );
    }

    #[test]
    fn record_ttft_emits_histogram() {
        let r = fresh_registry();
        record_ttft("ttft-test", "ok", 0.25);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_llm_ttft_seconds_count"),
            "got: {out}"
        );
    }

    #[test]
    fn record_cache_tokens_skips_zero() {
        let r = fresh_registry();
        record_cache_tokens("cache-test", 0, 0);
        let out = encode(&r);
        // No sample emitted; family registered but empty.
        assert!(!out.contains("cache-test"), "got: {out}");

        record_cache_tokens("cache-test", 1000, 500);
        let out2 = encode(&r);
        assert!(
            out2.contains(
                "aletheia_llm_cache_tokens_total{provider=\"cache-test\",direction=\"read\"} 1000"
            ),
            "got: {out2}"
        );
        assert!(
            out2.contains(
                "aletheia_llm_cache_tokens_total{provider=\"cache-test\",direction=\"write\"} 500"
            ),
            "got: {out2}"
        );
    }

    #[test]
    fn model_family_label_collapses_versioned_snapshots() {
        // WHY(#5684): versioned model IDs must collapse to one label key so the
        // Family<ModelStatusLabels, Histogram> does not grow unboundedly.
        assert_eq!(
            model_family_label("claude-opus-4-20250514"),
            "claude-opus-4"
        );
        assert_eq!(model_family_label("claude-sonnet-4-6"), "claude-sonnet-4");
        assert_eq!(
            model_family_label("claude-haiku-4-5-20251001"),
            "claude-haiku-4-5"
        );
        // Models without a dash version suffix are returned as-is.
        assert_eq!(model_family_label("gpt-4o"), "gpt-4");
        assert_eq!(model_family_label("llama3"), "llama3");
    }

    #[test]
    fn record_latency_deduplicates_versioned_model_labels() {
        // WHY(#5684): two versioned snapshots of the same family must produce
        // a single histogram entry, not one per snapshot version string.
        let r = fresh_registry();
        record_latency("claude-opus-4-20250514", "ok", 1.0);
        record_latency("claude-opus-4-20260101", "ok", 2.0);
        let out = encode(&r);
        // Both calls must map to the same label key "claude-opus-4".
        assert!(
            out.contains("model=\"claude-opus-4\""),
            "expected normalized family label, got: {out}"
        );
        // The versioned snapshots must NOT appear as separate label sets.
        assert!(
            !out.contains("claude-opus-4-20250514"),
            "raw versioned model ID must not appear as a label: {out}"
        );
        assert!(
            !out.contains("claude-opus-4-20260101"),
            "raw versioned model ID must not appear as a label: {out}"
        );
    }
}
