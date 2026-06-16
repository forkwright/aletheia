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

use crate::admission::RejectionFactor;

// ── Label sets ──────────────────────────────────────────────────────────────

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

/// Labels for admission decisions: agent, fact type, outcome (admitted/rejected),
/// and rejection reason (empty string for admitted facts).
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct AdmissionLabels {
    nous_id: String,
    fact_type: String,
    outcome: String,
    reason: String,
}

// ── Metric families ─────────────────────────────────────────────────────────

static KNOWLEDGE_FACTS_TOTAL: LazyLock<Family<NousLabels, Counter>> =
    LazyLock::new(Family::default);

static KNOWLEDGE_EXTRACTIONS_TOTAL: LazyLock<Family<NousStatusLabels, Counter>> =
    LazyLock::new(Family::default);

/// Admission decisions: labelled by agent, fact type, outcome, and reason.
///
/// `outcome` is `"admitted"` or `"rejected"`. For rejections, `reason` holds
/// the [`RejectionFactor`] string. For admissions, `reason` is empty.
static KNOWLEDGE_ADMISSION_TOTAL: LazyLock<Family<AdmissionLabels, Counter>> =
    LazyLock::new(Family::default);

static CONFLICT_UNCLASSIFIABLE_TOTAL: LazyLock<Counter> = LazyLock::new(Counter::default);

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
    registry.register(
        "aletheia_knowledge_admission",
        "Total admission decisions by agent, fact type, outcome, and rejection reason",
        KNOWLEDGE_ADMISSION_TOTAL.clone(),
    );
    registry.register(
        "aletheia_conflict_unclassifiable",
        "Total unclassifiable conflict classifier responses; use rate() for unclassifiable-rate alerts",
        CONFLICT_UNCLASSIFIABLE_TOTAL.clone(),
    );
}

// ── Recording ───────────────────────────────────────────────────────────────

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

/// Record a fact rejection with its rejection factor and fact type.
///
/// `reason` is the [`RejectionFactor`] display string and is used as a
/// Prometheus label so operators can slice rejection counts by cause.
pub(crate) fn record_admission_rejected(nous_id: &str, fact_type: &str, factor: RejectionFactor) {
    KNOWLEDGE_ADMISSION_TOTAL
        .get_or_create(&AdmissionLabels {
            nous_id: nous_id.to_owned(),
            fact_type: fact_type.to_owned(),
            outcome: "rejected".to_owned(),
            reason: factor.to_string(),
        })
        .inc();
}

/// Record a successful admission (fact was admitted).
pub(crate) fn record_admission_ok(nous_id: &str, fact_type: &str) {
    KNOWLEDGE_ADMISSION_TOTAL
        .get_or_create(&AdmissionLabels {
            nous_id: nous_id.to_owned(),
            fact_type: fact_type.to_owned(),
            outcome: "admitted".to_owned(),
            reason: String::new(),
        })
        .inc();
}

/// Record an unparseable conflict-classifier response.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn record_conflict_unclassifiable() {
    CONFLICT_UNCLASSIFIABLE_TOTAL.inc();
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

    #[test]
    fn register_and_record_admission_decisions() {
        let r = fresh_registry();
        record_admission_rejected(
            "_test_nous_admit",
            "preference",
            RejectionFactor::LowConfidence,
        );
        record_admission_ok("_test_nous_admit", "preference");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_knowledge_admission_total{nous_id=\"_test_nous_admit\",\
                 fact_type=\"preference\",outcome=\"rejected\",reason=\"low_confidence\"} 1"
            ),
            "rejected label set missing; got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_knowledge_admission_total{nous_id=\"_test_nous_admit\",\
                 fact_type=\"preference\",outcome=\"admitted\",reason=\"\"} 1"
            ),
            "admitted label set missing; got: {out}"
        );
    }

    #[test]
    fn register_and_record_conflict_unclassifiable() {
        let r = fresh_registry();
        record_conflict_unclassifiable();
        let out = encode(&r);
        assert!(
            out.contains("aletheia_conflict_unclassifiable_total"),
            "got: {out}"
        );
    }
}
