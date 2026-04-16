//! Prometheus metric definitions for the knowledge pipeline.
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

// ---------------------------------------------------------------------------
// Label sets
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousLabels {
    nous_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct NousStatusLabels {
    nous_id: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProviderLabels {
    provider: String,
}

// ---------------------------------------------------------------------------
// Metric families
// ---------------------------------------------------------------------------

static KNOWLEDGE_FACTS_TOTAL: LazyLock<Family<NousLabels, Counter>> =
    LazyLock::new(Family::default);

static KNOWLEDGE_EXTRACTIONS_TOTAL: LazyLock<Family<NousStatusLabels, Counter>> =
    LazyLock::new(Family::default);

fn recall_duration_histogram() -> Histogram {
    Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
}

type NousHistogramFamily = Family<NousLabels, Histogram, fn() -> Histogram>;

static RECALL_DURATION_SECONDS: LazyLock<NousHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(recall_duration_histogram));

fn embedding_duration_histogram() -> Histogram {
    Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
}

type ProviderHistogramFamily = Family<ProviderLabels, Histogram, fn() -> Histogram>;

static EMBEDDING_DURATION_SECONDS: LazyLock<ProviderHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(embedding_duration_histogram));

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_knowledge_facts",
        "Total knowledge facts inserted",
        KNOWLEDGE_FACTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_knowledge_extractions",
        "Total knowledge extraction operations",
        KNOWLEDGE_EXTRACTIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_recall_duration_seconds",
        "Knowledge recall duration in seconds",
        RECALL_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_embedding_duration_seconds",
        "Embedding computation duration in seconds",
        EMBEDDING_DURATION_SECONDS.clone(),
    );
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Record a fact insertion.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn record_fact_inserted(nous_id: &str) {
    KNOWLEDGE_FACTS_TOTAL
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .inc();
}

/// Record a knowledge extraction operation.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "metric recording called from extraction pipeline")
)]
pub(crate) fn record_extraction(nous_id: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    KNOWLEDGE_EXTRACTIONS_TOTAL
        .get_or_create(&NousStatusLabels {
            nous_id: nous_id.to_owned(),
            status: status.to_owned(),
        })
        .inc();
}

/// Record recall duration.
pub(crate) fn record_recall_duration(nous_id: &str, duration_secs: f64) {
    RECALL_DURATION_SECONDS
        .get_or_create(&NousLabels {
            nous_id: nous_id.to_owned(),
        })
        .observe(duration_secs);
}

/// Record embedding computation duration.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "metric recording called from embedding pipeline")
)]
pub(crate) fn record_embedding_duration(provider: &str, duration_secs: f64) {
    EMBEDDING_DURATION_SECONDS
        .get_or_create(&ProviderLabels {
            provider: provider.to_owned(),
        })
        .observe(duration_secs);
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
    fn register_and_record_fact_inserted() {
        let r = fresh_registry();
        record_fact_inserted("_test_nous_fact");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_knowledge_facts_total{nous_id=\"_test_nous_fact\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_extraction_both_statuses() {
        let r = fresh_registry();
        record_extraction("_test_nous_extract", true);
        record_extraction("_test_nous_extract", false);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_knowledge_extractions_total{nous_id=\"_test_nous_extract\",status=\"ok\"} 1"
            ),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_knowledge_extractions_total{nous_id=\"_test_nous_extract\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_recall_duration() {
        let r = fresh_registry();
        record_recall_duration("_test_nous_recall", 0.05);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_recall_duration_seconds_count{nous_id=\"_test_nous_recall\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_embedding_duration() {
        let r = fresh_registry();
        record_embedding_duration("_test_provider", 0.1);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_embedding_duration_seconds_count{provider=\"_test_provider\"} 1"
            ),
            "got: {out}"
        );
    }
}
