//! Prometheus metric definitions for the distillation engine.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousStatusLabels {
    nous_id: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousLabels {
    nous_id: String,
}

// ── Metric families ──

static DISTILLATION_TOTAL: LazyLock<Family<NousStatusLabels, Counter>> =
    LazyLock::new(Family::default);

fn distillation_duration_histogram() -> Histogram {
    Histogram::new([1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0])
}

type NousHistogramFamily = Family<NousLabels, Histogram, fn() -> Histogram>;

static DISTILLATION_DURATION_SECONDS: LazyLock<NousHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(distillation_duration_histogram));

static TOKENS_SAVED_TOTAL: LazyLock<Family<NousLabels, Counter>> = LazyLock::new(Family::default);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_distillation",
        "Total distillation operations",
        DISTILLATION_TOTAL.clone(),
    );
    registry.register(
        "aletheia_distillation_duration_seconds",
        "Distillation duration in seconds",
        DISTILLATION_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_tokens_saved",
        "Total tokens saved by distillation",
        TOKENS_SAVED_TOTAL.clone(),
    );
}

// ── Recording ──

/// Record a completed distillation operation.
pub(crate) fn record_distillation(nous_id: &str, duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    DISTILLATION_TOTAL
        .get_or_create(&NousStatusLabels {
            nous_id: nous_id.to_owned(),
            status: status.to_owned(),
        })
        .inc();
    DISTILLATION_DURATION_SECONDS
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .observe(duration_secs);
}

/// Record tokens saved by a distillation pass.
pub(crate) fn record_tokens_saved(nous_id: &str, tokens: u64) {
    TOKENS_SAVED_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc_by(tokens);
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

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
    fn register_and_record_distillation_success_and_failure() {
        let r = fresh_registry();
        record_distillation("_test_nous", 5.0, true);
        record_distillation("_test_nous", 2.0, false);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_distillation_total{nous_id=\"_test_nous\",status=\"ok\"} 1"),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_distillation_total{nous_id=\"_test_nous\",status=\"error\"} 1"),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_distillation_duration_seconds_count{nous_id=\"_test_nous\"} 2"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_tokens_saved() {
        let r = fresh_registry();
        record_tokens_saved("_test_nous_tokens", 1000);
        record_tokens_saved("_test_nous_tokens", 500);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_tokens_saved_total{nous_id=\"_test_nous_tokens\"} 1500"),
            "got: {out}"
        );
    }
}
