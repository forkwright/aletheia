use koina::metrics::MetricsRegistry;

use crate::extract::{
    ConversationMessage, ExtractionConfig, ExtractionEngine, ExtractionError, ExtractionProvider,
};

struct QualityMetricProvider;

impl ExtractionProvider for QualityMetricProvider {
    fn complete<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    > {
        Box::pin(async {
            // WHY: 5 facts at 0.99 triggers confidence inflation (>80% >= 0.95),
            // plus one empty-field fact, one self-reference fact, and one
            // low-confidence fact so the test can assert every rejection reason.
            Ok(r#"{"entities":[],"relationships":[],"facts":[
                {"subject":"Alice","predicate":"likes","object":"Rust","confidence":0.99},
                {"subject":"Bob","predicate":"likes","object":"Python","confidence":0.99},
                {"subject":"Carol","predicate":"uses","object":"Aletheia","confidence":0.99},
                {"subject":"","predicate":"is","object":"empty","confidence":0.99},
                {"subject":"I","predicate":"like","object":"Rust","confidence":0.99},
                {"subject":"Dave","predicate":"likes","object":"Go","confidence":0.2}
            ]}"#
            .to_owned())
        })
    }
}

fn encode(registry: &MetricsRegistry) -> String {
    let mut buf = String::new();
    #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
    registry.encode(&mut buf).unwrap();
    buf
}

#[expect(clippy::expect_used, reason = "test assertions")]
#[tokio::test]
async fn extract_refined_records_extraction_quality_metrics() {
    let registry = MetricsRegistry::new();
    registry.with_registry(crate::metrics::register);

    let engine = ExtractionEngine::new(ExtractionConfig {
        extract_self_facts: false,
        min_message_length: 1,
        ..ExtractionConfig::default()
    });
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "quality metrics test".to_owned(),
        tool_calls: None,
        reasoning: None,
    }];

    let refined = engine
        .extract_refined(&messages, &QualityMetricProvider, "_test_qm", "test")
        .await
        .expect("extraction should succeed");

    // Sanity: only the three high-confidence non-self facts should survive.
    assert_eq!(
        refined.extraction.facts.len(),
        3,
        "expected three accepted facts"
    );

    let out = encode(&registry);

    assert!(
        out.contains("aletheia_extraction_confidence_inflation_total"),
        "confidence inflation metric missing; got: {out}"
    );
    assert!(
        out.contains(
            "aletheia_extraction_quality_total{nous_id=\"_test_qm\",producer=\"test\",provider=\"unknown\",model=\"unknown\",status=\"rejected\",reason=\"empty_field\"} 1"
        ),
        "empty_field rejection metric missing; got: {out}"
    );
    assert!(
        out.contains(
            "aletheia_extraction_quality_total{nous_id=\"_test_qm\",producer=\"test\",provider=\"unknown\",model=\"unknown\",status=\"rejected\",reason=\"self_reference\"} 1"
        ),
        "self_reference rejection metric missing; got: {out}"
    );
    assert!(
        out.contains(
            "aletheia_extraction_quality_total{nous_id=\"_test_qm\",producer=\"test\",provider=\"unknown\",model=\"unknown\",status=\"rejected\",reason=\"low_confidence\"} 1"
        ),
        "low_confidence rejection metric missing; got: {out}"
    );
    assert!(
        out.contains(
            "aletheia_extraction_quality_total{nous_id=\"_test_qm\",producer=\"test\",provider=\"unknown\",model=\"unknown\",status=\"accepted\",reason=\"\"} 3"
        ),
        "accepted quality metric missing; got: {out}"
    );
    assert!(
        out.contains(
            "aletheia_extraction_confidence_count{nous_id=\"_test_qm\",producer=\"test\",provider=\"unknown\",model=\"unknown\",status=\"extracted\"} 6"
        ),
        "extracted confidence histogram missing; got: {out}"
    );
}
