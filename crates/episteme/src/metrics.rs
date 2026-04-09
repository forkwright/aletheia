//! Prometheus metric definitions for the knowledge pipeline.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec,
    register_int_counter_vec,
};

static KNOWLEDGE_FACTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_knowledge_facts_total",
            "Total knowledge facts inserted"
        ),
        &["nous_id"]
    )
    .expect("metric registration")
});

static KNOWLEDGE_EXTRACTIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_knowledge_extractions_total",
            "Total knowledge extraction operations"
        ),
        &["nous_id", "status"]
    )
    .expect("metric registration")
});

static RECALL_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_recall_duration_seconds",
            "Knowledge recall duration in seconds"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        &["nous_id"]
    )
    .expect("metric registration")
});

static EMBEDDING_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_embedding_duration_seconds",
            "Embedding computation duration in seconds"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        &["provider"]
    )
    .expect("metric registration")
});

#[cfg_attr(not(test), expect(dead_code, reason = "metric init called from server startup"))]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&KNOWLEDGE_FACTS_TOTAL);
    LazyLock::force(&KNOWLEDGE_EXTRACTIONS_TOTAL);
    LazyLock::force(&RECALL_DURATION_SECONDS);
    LazyLock::force(&EMBEDDING_DURATION_SECONDS);
}

/// Record a fact insertion.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn record_fact_inserted(nous_id: &str) {
    KNOWLEDGE_FACTS_TOTAL.with_label_values(&[nous_id]).inc();
}

/// Record a knowledge extraction operation.
#[cfg_attr(not(test), expect(dead_code, reason = "metric recording called from extraction pipeline"))]
pub(crate) fn record_extraction(nous_id: &str, success: bool) {
    let status = if success { "ok" } else { "error" };
    KNOWLEDGE_EXTRACTIONS_TOTAL
        .with_label_values(&[nous_id, status])
        .inc();
}

/// Record recall duration.
pub(crate) fn record_recall_duration(nous_id: &str, duration_secs: f64) {
    RECALL_DURATION_SECONDS
        .with_label_values(&[nous_id])
        .observe(duration_secs);
}

/// Record embedding computation duration.
#[cfg_attr(not(test), expect(dead_code, reason = "metric recording called from embedding pipeline"))]
pub(crate) fn record_embedding_duration(provider: &str, duration_secs: f64) {
    EMBEDDING_DURATION_SECONDS
        .with_label_values(&[provider])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read a Prometheus counter value by metric name and label values.
    fn read_counter(name: &str, label_values: &[&str]) -> u64 {
        let families = prometheus::default_registry().gather();
        for family in &families {
            if family.name() == name {
                for metric in family.get_metric() {
                    let labels = metric.get_label();
                    let matches = label_values.iter().enumerate().all(|(i, expected)| {
                        labels.get(i).is_some_and(|l| l.value() == *expected)
                    });
                    if matches {
                        #[expect(
                            clippy::as_conversions,
                            reason = "prometheus counter values are non-negative integers stored as f64; truncation is safe for test comparison"
                        )]
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "prometheus counter values are small integers in tests; no truncation risk"
                        )]
                        #[expect(
                            clippy::cast_sign_loss,
                            reason = "prometheus counters are non-negative; sign loss cannot occur"
                        )]
                        return metric.get_counter().value() as u64;
                    }
                }
            }
        }
        0
    }

    /// Read a Prometheus histogram count by metric name and label values.
    fn read_histogram_count(name: &str, label_values: &[&str]) -> u64 {
        let families = prometheus::default_registry().gather();
        for family in &families {
            if family.name() == name {
                for metric in family.get_metric() {
                    let labels = metric.get_label();
                    let matches = label_values.iter().enumerate().all(|(i, expected)| {
                        labels.get(i).is_some_and(|l| l.value() == *expected)
                    });
                    if matches {
                        return metric.get_histogram().sample_count();
                    }
                }
            }
        }
        0
    }

    #[test]
    fn init_registers_all_metrics() {
        // WHY: prometheus::Registry::gather() filters out IntCounterVec /
        // HistogramVec families that have zero observed label sets. init()
        // only forces the LazyLocks, so we also have to emit at least one
        // label set per metric for them to appear in gather() output.
        // Tracked in #2988 — the earlier `init_does_not_panic` test was
        // looser and didn't catch this.
        init();
        record_fact_inserted("init-test-nous");
        record_extraction("init-test-nous", true);
        record_recall_duration("init-test-nous", 0.001);
        record_embedding_duration("init-test-provider", 0.001);

        let families = prometheus::default_registry().gather();
        let metric_names: std::collections::HashSet<_> =
            families.iter().map(prometheus::proto::MetricFamily::name).collect();
        assert!(metric_names.contains("aletheia_knowledge_facts_total"));
        assert!(metric_names.contains("aletheia_knowledge_extractions_total"));
        assert!(metric_names.contains("aletheia_recall_duration_seconds"));
        assert!(metric_names.contains("aletheia_embedding_duration_seconds"));
    }

    #[test]
    fn record_fact_inserted_increments_counter() {
        let nous_id = "test-nous-fact";
        let before = read_counter("aletheia_knowledge_facts_total", &[nous_id]);
        record_fact_inserted(nous_id);
        let after = read_counter("aletheia_knowledge_facts_total", &[nous_id]);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn record_extraction_increments_counters() {
        let nous_id = "test-nous-extraction";
        let before_ok =
            read_counter("aletheia_knowledge_extractions_total", &[nous_id, "ok"]);
        let before_error =
            read_counter("aletheia_knowledge_extractions_total", &[nous_id, "error"]);

        record_extraction(nous_id, true);
        record_extraction(nous_id, false);

        let after_ok =
            read_counter("aletheia_knowledge_extractions_total", &[nous_id, "ok"]);
        let after_error =
            read_counter("aletheia_knowledge_extractions_total", &[nous_id, "error"]);

        assert_eq!(after_ok, before_ok + 1);
        assert_eq!(after_error, before_error + 1);
    }

    #[test]
    fn record_recall_duration_records_observation() {
        let nous_id = "test-nous-recall";
        let before = read_histogram_count("aletheia_recall_duration_seconds", &[nous_id]);
        record_recall_duration(nous_id, 0.05);
        let after = read_histogram_count("aletheia_recall_duration_seconds", &[nous_id]);
        assert_eq!(after, before + 1);
    }

    #[test]
    fn record_embedding_duration_records_observation() {
        let provider = "candle";
        let before =
            read_histogram_count("aletheia_embedding_duration_seconds", &[provider]);
        record_embedding_duration(provider, 0.1);
        let after =
            read_histogram_count("aletheia_embedding_duration_seconds", &[provider]);
        assert_eq!(after, before + 1);
    }
}
