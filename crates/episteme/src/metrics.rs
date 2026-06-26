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

/// Labels for per-fact extraction quality decisions.
///
/// `status` is `"accepted"` or `"rejected"`. For rejections, `reason` holds
/// the rejection cause (e.g. `low_confidence`, `empty_field`). For admissions,
/// `reason` is empty.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ExtractionQualityLabels {
    nous_id: String,
    producer: String,
    provider: String,
    model: String,
    status: String,
    reason: String,
}

/// Labels for the extracted-fact confidence distribution histogram.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ExtractionConfidenceLabels {
    nous_id: String,
    producer: String,
    provider: String,
    model: String,
    status: String,
}

/// Labels for extraction-quality counters that are scoped by producer and
/// provider/model but do not distinguish individual facts.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ExtractionProducerLabels {
    nous_id: String,
    producer: String,
    provider: String,
    model: String,
}

/// Labels for low-confidence admissions: facts that passed the admission gate
/// despite a confidence score below the low-confidence threshold.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct LowConfidenceAdmissionLabels {
    nous_id: String,
    threshold: String,
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

/// Per-fact extraction quality: accepted/rejected by reason, labelled by
/// producer path, provider, and model.
static EXTRACTION_QUALITY_TOTAL: LazyLock<Family<ExtractionQualityLabels, Counter>> =
    LazyLock::new(Family::default);

/// Count of extractions where >80% of facts have confidence >= 0.95.
static EXTRACTION_CONFIDENCE_INFLATION_TOTAL: LazyLock<Family<ExtractionProducerLabels, Counter>> =
    LazyLock::new(Family::default);

/// Count of facts flagged as corrections during extraction refinement.
static EXTRACTION_CORRECTIONS_TOTAL: LazyLock<Family<ExtractionProducerLabels, Counter>> =
    LazyLock::new(Family::default);

/// Count of contradictions detected between a newly extracted fact and the
/// existing knowledge store.
static EXTRACTION_CONTRADICTIONS_TOTAL: LazyLock<Family<ExtractionProducerLabels, Counter>> =
    LazyLock::new(Family::default);

/// Count of all conflicts (contradictions and duplicates) detected during
/// extraction-time verification.
static EXTRACTION_CONFLICTS_TOTAL: LazyLock<Family<ExtractionProducerLabels, Counter>> =
    LazyLock::new(Family::default);

/// Count of facts admitted despite having a confidence below the low-confidence
/// threshold.
static KNOWLEDGE_LOW_CONFIDENCE_ADMISSION_TOTAL: LazyLock<
    Family<LowConfidenceAdmissionLabels, Counter>,
> = LazyLock::new(Family::default);

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

fn extraction_confidence_histogram() -> Histogram {
    // WHY: eleven buckets across [0.05, 1.0] so operators can see calibration
    // shape without label explosion. Values below 0.05 land in the first bucket.
    Histogram::new([0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0])
}

type ExtractionConfidenceFamily = Family<ExtractionConfidenceLabels, Histogram, fn() -> Histogram>;

static EXTRACTION_CONFIDENCE_HISTOGRAM: LazyLock<ExtractionConfidenceFamily> =
    LazyLock::new(|| Family::new_with_constructor(extraction_confidence_histogram));

/// Threshold below which an admitted fact is considered "low-confidence".
pub(crate) const LOW_CONFIDENCE_ADMISSION_THRESHOLD: f64 = 0.5;

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
    registry.register(
        "aletheia_extraction_quality",
        "Total extraction refinement decisions by producer, provider, model, status, and reason",
        EXTRACTION_QUALITY_TOTAL.clone(),
    );
    registry.register(
        "aletheia_extraction_confidence",
        "Distribution of extracted fact confidence scores by status",
        EXTRACTION_CONFIDENCE_HISTOGRAM.clone(),
    );
    registry.register(
        "aletheia_extraction_confidence_inflation",
        "Extractions where >80% of facts have confidence >= 0.95",
        EXTRACTION_CONFIDENCE_INFLATION_TOTAL.clone(),
    );
    registry.register(
        "aletheia_extraction_corrections",
        "Facts flagged as corrections during extraction refinement",
        EXTRACTION_CORRECTIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_extraction_contradictions",
        "Contradictions detected during extraction-time verification",
        EXTRACTION_CONTRADICTIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_extraction_conflicts",
        "All conflicts (contradictions and duplicates) detected during extraction-time verification",
        EXTRACTION_CONFLICTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_knowledge_low_confidence_admissions",
        "Facts admitted with confidence below the low-confidence threshold",
        KNOWLEDGE_LOW_CONFIDENCE_ADMISSION_TOTAL.clone(),
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
pub fn record_extraction(nous_id: &str, success: bool) {
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
pub fn record_embedding_duration(provider: &str, duration_secs: f64) {
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

/// Record an extraction refinement decision for a single fact.
///
/// `status` is `"accepted"` or `"rejected"`. `reason` is empty for accepted
/// facts and holds the rejection cause for rejected facts.
pub fn record_extraction_quality(
    nous_id: &str,
    producer: &str,
    provider: &str,
    model: &str,
    status: &str,
    reason: &str,
) {
    EXTRACTION_QUALITY_TOTAL
        .get_or_create(&ExtractionQualityLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
            status: status.to_owned(),
            reason: reason.to_owned(),
        })
        .inc();
}

/// Observe a fact confidence value into the extraction confidence histogram.
///
/// `status` is one of `"extracted"`, `"accepted"`, or `"rejected"` so the
/// distribution can be compared before and after quality filtering.
pub fn record_extraction_confidence(
    nous_id: &str,
    producer: &str,
    provider: &str,
    model: &str,
    status: &str,
    confidence: f64,
) {
    EXTRACTION_CONFIDENCE_HISTOGRAM
        .get_or_create(&ExtractionConfidenceLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
            status: status.to_owned(),
        })
        .observe(confidence.clamp(0.0, 1.0));
}

/// Record that an extraction batch had confidence inflation.
pub fn record_confidence_inflation(nous_id: &str, producer: &str, provider: &str, model: &str) {
    EXTRACTION_CONFIDENCE_INFLATION_TOTAL
        .get_or_create(&ExtractionProducerLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
        .inc();
}

/// Record that a fact was flagged as a correction during extraction refinement.
pub fn record_extraction_correction(nous_id: &str, producer: &str, provider: &str, model: &str) {
    EXTRACTION_CORRECTIONS_TOTAL
        .get_or_create(&ExtractionProducerLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
        .inc();
}

/// Record that a contradiction was detected during extraction-time verification.
pub fn record_extraction_contradiction(nous_id: &str, producer: &str, provider: &str, model: &str) {
    EXTRACTION_CONTRADICTIONS_TOTAL
        .get_or_create(&ExtractionProducerLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
        .inc();
}

/// Record that any conflict (contradiction or duplicate) was detected during
/// extraction-time verification.
pub fn record_extraction_conflict(nous_id: &str, producer: &str, provider: &str, model: &str) {
    EXTRACTION_CONFLICTS_TOTAL
        .get_or_create(&ExtractionProducerLabels {
            nous_id: nous_id.to_owned(),
            producer: producer.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
        .inc();
}

/// Record that a fact was admitted despite having low source confidence.
///
/// The threshold is fixed at 0.5; the `threshold` label documents this so
/// rate queries can be interpreted correctly even if the constant changes.
pub(crate) fn record_low_confidence_admission(nous_id: &str) {
    KNOWLEDGE_LOW_CONFIDENCE_ADMISSION_TOTAL
        .get_or_create(&LowConfidenceAdmissionLabels {
            nous_id: nous_id.to_owned(),
            threshold: format!("{LOW_CONFIDENCE_ADMISSION_THRESHOLD}"),
        })
        .inc();
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

    #[test]
    fn register_and_record_extraction_quality() {
        let r = fresh_registry();
        record_extraction_quality(
            "_test_nous_quality",
            "test_producer",
            "test_provider",
            "test_model",
            "rejected",
            "low_confidence",
        );
        record_extraction_quality(
            "_test_nous_quality",
            "test_producer",
            "test_provider",
            "test_model",
            "accepted",
            "",
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_extraction_quality_total{nous_id=\"_test_nous_quality\",\
                 producer=\"test_producer\",provider=\"test_provider\",model=\"test_model\",\
                 status=\"rejected\",reason=\"low_confidence\"} 1"
            ),
            "rejected quality label set missing; got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_extraction_quality_total{nous_id=\"_test_nous_quality\",\
                 producer=\"test_producer\",provider=\"test_provider\",model=\"test_model\",\
                 status=\"accepted\",reason=\"\"} 1"
            ),
            "accepted quality label set missing; got: {out}"
        );
    }

    #[test]
    fn register_and_record_extraction_confidence() {
        let r = fresh_registry();
        record_extraction_confidence(
            "_test_nous_conf",
            "test_producer",
            "test_provider",
            "test_model",
            "extracted",
            0.85,
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_extraction_confidence_count{nous_id=\"_test_nous_conf\",\
                 producer=\"test_producer\",provider=\"test_provider\",model=\"test_model\",\
                 status=\"extracted\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_confidence_inflation() {
        let r = fresh_registry();
        record_confidence_inflation(
            "_test_nous_inflation",
            "test_producer",
            "test_provider",
            "test_model",
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_extraction_confidence_inflation_total{nous_id=\"_test_nous_inflation\",\
                 producer=\"test_producer\",provider=\"test_provider\",model=\"test_model\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_extraction_correction() {
        let r = fresh_registry();
        record_extraction_correction(
            "_test_nous_correction",
            "test_producer",
            "test_provider",
            "test_model",
        );
        let out = encode(&r);
        assert!(
            out.contains("aletheia_extraction_corrections_total"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_extraction_contradiction() {
        let r = fresh_registry();
        record_extraction_contradiction(
            "_test_nous_contradiction",
            "test_producer",
            "test_provider",
            "test_model",
        );
        let out = encode(&r);
        assert!(
            out.contains("aletheia_extraction_contradictions_total"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_extraction_conflict() {
        let r = fresh_registry();
        record_extraction_conflict(
            "_test_nous_conflict",
            "test_producer",
            "test_provider",
            "test_model",
        );
        let out = encode(&r);
        assert!(
            out.contains("aletheia_extraction_conflicts_total"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_low_confidence_admission() {
        let r = fresh_registry();
        record_low_confidence_admission("_test_nous_low_conf");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_knowledge_low_confidence_admissions_total{nous_id=\"_test_nous_low_conf\",\
                 threshold=\"0.5\"} 1"
            ),
            "got: {out}"
        );
    }
}
