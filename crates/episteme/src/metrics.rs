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

/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&KNOWLEDGE_FACTS_TOTAL);
    LazyLock::force(&KNOWLEDGE_EXTRACTIONS_TOTAL);
    LazyLock::force(&RECALL_DURATION_SECONDS);
    LazyLock::force(&EMBEDDING_DURATION_SECONDS);
}

/// Record a fact insertion.
pub(crate) fn record_fact_inserted(nous_id: &str) {
    KNOWLEDGE_FACTS_TOTAL.with_label_values(&[nous_id]).inc();
}

/// Record a knowledge extraction operation.
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
pub(crate) fn record_embedding_duration(provider: &str, duration_secs: f64) {
    EMBEDDING_DURATION_SECONDS
        .with_label_values(&[provider])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_fact_inserted_does_not_panic() {
        record_fact_inserted("test-nous");
    }

    #[test]
    fn record_extraction_does_not_panic() {
        record_extraction("test-nous", true);
        record_extraction("test-nous", false);
    }

    #[test]
    fn record_recall_duration_does_not_panic() {
        record_recall_duration("test-nous", 0.05);
    }

    #[test]
    fn record_embedding_duration_does_not_panic() {
        record_embedding_duration("candle", 0.1);
    }
}
