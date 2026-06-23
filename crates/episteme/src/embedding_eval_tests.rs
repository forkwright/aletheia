#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with known length"
)]

use std::collections::HashMap;

use super::*;
use crate::embedding::{EmbeddingProvider, EmbeddingResult, MockEmbeddingProvider};

const STATIC_DIM: usize = 2;

fn mock() -> MockEmbeddingProvider {
    MockEmbeddingProvider::new(64)
}

fn simple_corpus() -> Vec<(String, String)> {
    vec![
        ("id-alice".into(), "alice prefers tea over coffee".into()),
        ("id-bob".into(), "bob enjoys cycling on weekends".into()),
        (
            "id-carol".into(),
            "carol studies distributed systems".into(),
        ),
    ]
}

fn simple_dataset() -> EvalDataset {
    EvalDataset::from_jsonl_str(
        r#"{"query":"alice tea coffee","relevant_ids":["id-alice"]}
{"query":"distributed systems","relevant_ids":["id-carol"]}
"#,
    )
    .expect("simple dataset must parse")
}

#[derive(Debug)]
struct StaticEmbeddingProvider {
    model_name: &'static str,
    vectors: HashMap<&'static str, Vec<f32>>,
    fallback: Vec<f32>,
}

impl StaticEmbeddingProvider {
    fn new(model_name: &'static str, vectors: &[(&'static str, [f32; STATIC_DIM])]) -> Self {
        let vectors = vectors
            .iter()
            .map(|(text, vector)| (*text, vector.to_vec()))
            .collect();
        Self {
            model_name,
            vectors,
            fallback: vec![0.0, 1.0],
        }
    }
}

impl EmbeddingProvider for StaticEmbeddingProvider {
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        Ok(self
            .vectors
            .get(text)
            .cloned()
            .unwrap_or_else(|| self.fallback.clone()))
    }

    fn dimension(&self) -> usize {
        STATIC_DIM
    }

    fn model_name(&self) -> &str {
        self.model_name
    }
}

fn gate_corpus() -> Vec<(String, String)> {
    vec![
        ("id-good".into(), "good document".into()),
        ("id-bad".into(), "bad document".into()),
    ]
}

fn gate_dataset() -> EvalDataset {
    EvalDataset::from_jsonl_str(r#"{"query":"target query","relevant_ids":["id-good"]}"#)
        .expect("gate dataset must parse")
}

#[test]
fn parse_valid_jsonl() {
    let ds = simple_dataset();
    assert_eq!(ds.len(), 2, "parse valid jsonl: values should be equal");
    assert_eq!(
        ds.queries[0].query, "alice tea coffee",
        "parse valid jsonl: values should be equal"
    );
    assert_eq!(
        ds.queries[0].relevant_ids,
        ["id-alice"],
        "parse valid jsonl: values should be equal"
    );
}

#[test]
fn parse_skips_blank_lines() {
    let ds = EvalDataset::from_jsonl_str("\n{\"query\":\"x\",\"relevant_ids\":[\"y\"]}\n\n")
        .expect("blank lines should be skipped");
    assert_eq!(
        ds.len(),
        1,
        "parse skips blank lines: values should be equal"
    );
}

#[test]
fn parse_bad_json_returns_error() {
    let result = EvalDataset::from_jsonl_str("not json at all");
    assert!(result.is_err(), "bad json should return error");
}

#[test]
fn missing_jsonl_file_returns_io_error_not_parse_error() {
    let result =
        EvalDataset::from_jsonl_file(std::path::Path::new("/tmp/__no_such_eval_dataset__"));
    let err = result.expect_err("missing file must error");
    assert!(
        matches!(err, EvalError::IoFailed { .. }),
        "expected IoFailed for missing file, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.starts_with("cannot read eval dataset "),
        "io error message should start with 'cannot read eval dataset', got: {msg}"
    );
    // The misleading "parse … line 0" framing must no longer appear.
    assert!(
        !msg.contains("parse"),
        "io error must not mention parsing, got: {msg}"
    );
}

#[test]
fn evaluate_returns_recall_and_mrr() {
    let provider = mock();
    let corpus = simple_corpus();
    let dataset = simple_dataset();
    let metrics = evaluate_model(&provider, &dataset, &corpus, 3)
        .expect("evaluate_model must succeed on valid inputs");
    // recall_at_k is in [0, 1]
    assert!(
        metrics.recall_at_k >= 0.0 && metrics.recall_at_k <= 1.0,
        "recall must be in [0,1]"
    );
    assert!(
        metrics.mrr >= 0.0 && metrics.mrr <= 1.0,
        "mrr must be in [0,1]"
    );
    assert_eq!(
        metrics.per_query.len(),
        2,
        "per_query count must match dataset size"
    );
}

#[test]
fn evaluate_empty_corpus_errors() {
    let provider = mock();
    let dataset = simple_dataset();
    let err = evaluate_model(&provider, &dataset, &[], 5).expect_err("empty corpus must error");
    assert!(
        matches!(err, EvalError::EmptyCorpus { .. }),
        "expected EmptyCorpus, got {err:?}"
    );
}

#[test]
fn evaluate_empty_dataset_errors() {
    let provider = mock();
    let corpus = simple_corpus();
    let dataset = EvalDataset {
        queries: vec![],
        permissive: false,
    };
    let err =
        evaluate_model(&provider, &dataset, &corpus, 5).expect_err("empty dataset must error");
    assert!(
        matches!(err, EvalError::EmptyDataset { .. }),
        "expected EmptyDataset, got {err:?}"
    );
}

#[test]
fn measure_baseline_records_metrics_without_gate() {
    let provider = mock();
    let corpus = simple_corpus();
    let dataset = simple_dataset();
    let run = measure_baseline(&provider, &dataset, &corpus, 3)
        .expect("measure_baseline must succeed");
    assert_eq!(
        run.mode,
        EvalRunMode::Measurement,
        "baseline-only run must be measurement mode"
    );
    assert!(run.passed, "measurement mode should complete successfully");
    assert!(run.candidate.is_none(), "measurement has no candidate");
    assert!(
        run.failure_reason.is_none(),
        "measurement should not carry a gate failure"
    );
}

#[test]
fn compare_no_candidate_fails_closed() {
    let provider = mock();
    let corpus = simple_corpus();
    let dataset = simple_dataset();
    let run = compare_models(&provider, None, &dataset, &corpus, 3)
        .expect("compare_models with no candidate should return a failed gate result");
    assert_eq!(run.mode, EvalRunMode::Gate, "compare_models is gate mode");
    assert!(!run.passed, "gate without candidate must fail closed");
    assert!(run.candidate.is_none(), "no candidate means None in result");
    assert_eq!(
        run.failure_reason.as_deref(),
        Some("candidate provider missing for embedding regression gate"),
        "missing candidate should explain the fail-closed gate result"
    );
}

#[test]
fn compare_same_model_passes() {
    let a = mock();
    let b = mock();
    let corpus = simple_corpus();
    let dataset = simple_dataset();
    let run = compare_models(&a, Some(&b), &dataset, &corpus, 3)
        .expect("compare_models same model must succeed");
    // Same model: candidate recall == baseline recall, so it passes.
    assert_eq!(
        run.mode,
        EvalRunMode::Gate,
        "candidate comparison is a gate"
    );
    assert!(run.passed, "identical models must pass");
    assert!(run.candidate.is_some(), "candidate metrics must be present");
    assert!(
        run.failure_reason.is_none(),
        "passing gate should not carry a failure reason"
    );
}

#[test]
fn compare_regressing_candidate_fails() {
    let baseline = StaticEmbeddingProvider::new(
        "baseline-static",
        &[
            ("target query", [1.0, 0.0]),
            ("good document", [1.0, 0.0]),
            ("bad document", [0.0, 1.0]),
        ],
    );
    let candidate = StaticEmbeddingProvider::new(
        "candidate-static",
        &[
            ("target query", [0.0, 1.0]),
            ("good document", [1.0, 0.0]),
            ("bad document", [0.0, 1.0]),
        ],
    );
    let corpus = gate_corpus();
    let dataset = gate_dataset();
    let run = compare_models(&baseline, Some(&candidate), &dataset, &corpus, 1)
        .expect("compare_models should evaluate candidate regression");

    assert_eq!(
        run.mode,
        EvalRunMode::Gate,
        "candidate comparison is a gate"
    );
    assert!(!run.passed, "regressing candidate must fail");
    assert!(
        (run.baseline.recall_at_k - 1.0).abs() < f64::EPSILON,
        "baseline should retrieve the relevant document"
    );
    assert!(
        run.candidate
            .as_ref()
            .expect("candidate metrics must be present")
            .recall_at_k
            .abs()
            < f64::EPSILON,
        "candidate should miss the relevant document"
    );
    assert!(
        run.failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("below required baseline threshold")),
        "regression failure should explain the threshold miss"
    );
}

#[test]
fn query_result_fields_populated() {
    let provider = mock();
    let corpus = simple_corpus();
    let dataset = simple_dataset();
    let metrics =
        evaluate_model(&provider, &dataset, &corpus, 2).expect("evaluate_model must succeed");
    for qr in &metrics.per_query {
        assert!(!qr.query.is_empty(), "query must not be empty");
        assert!(qr.top_k_ids.len() <= 2, "top_k_ids capped at k=2");
        assert!(
            qr.reciprocal_rank >= 0.0 && qr.reciprocal_rank <= 1.0,
            "rr must be in [0,1]"
        );
    }
}

#[test]
fn k_larger_than_corpus_clamps_to_corpus_size() {
    let provider = mock();
    let corpus = simple_corpus(); // 3 items
    let dataset = simple_dataset();
    let metrics = evaluate_model(&provider, &dataset, &corpus, 100)
        .expect("evaluate_model with large k must succeed");
    // Effective k = min(100, 3) = 3
    assert_eq!(metrics.k, 3, "k must be clamped to corpus size");
    for qr in &metrics.per_query {
        assert!(
            qr.top_k_ids.len() <= 3,
            "top_k_ids cannot exceed corpus size"
        );
    }
}

#[test]
fn cosine_similarity_unit_vectors() {
    let a = vec![1.0_f32, 0.0];
    let b = vec![0.0_f32, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!(
        (sim - 0.0).abs() < 1e-6,
        "orthogonal unit vectors => similarity 0"
    );

    let c = vec![1.0_f32, 0.0];
    let same = cosine_similarity(&a, &c);
    assert!(
        (same - 1.0).abs() < 1e-6,
        "identical unit vectors => similarity 1"
    );
}

#[test]
fn parse_with_optional_description() {
    let ds = EvalDataset::from_jsonl_str(
        r#"{"query":"foo","relevant_ids":["bar"],"description":"a test query"}"#,
    )
    .expect("parse with optional description must succeed");
    assert_eq!(
        ds.queries[0].description.as_deref(),
        Some("a test query"),
        "description must round-trip"
    );
}

#[test]
fn validate_rejects_unknown_relevant_id() {
    let ds = EvalDataset::from_jsonl_str(r#"{"query":"missing","relevant_ids":["id-ghost"]}"#)
        .expect("dataset must parse");
    let err = ds
        .validate_against_corpus(&simple_corpus())
        .expect_err("unknown relevant id must fail closed");
    assert!(
        matches!(err, EvalError::UnknownRelevantId { .. }),
        "expected UnknownRelevantId, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("id-ghost"),
        "error should name the unknown id: {msg}"
    );
    assert!(
        msg.contains("line Some(1)"),
        "error should include source line: {msg}"
    );
}

#[test]
fn validate_rejects_duplicate_relevant_id() {
    let ds = EvalDataset::from_jsonl_str(
        r#"{"query":"dup","relevant_ids":["id-alice","id-alice"]}"#,
    )
    .expect("dataset must parse");
    let err = ds
        .validate_against_corpus(&simple_corpus())
        .expect_err("duplicate relevant id must fail closed");
    assert!(
        matches!(err, EvalError::DuplicateRelevantId { .. }),
        "expected DuplicateRelevantId, got {err:?}"
    );
}

#[test]
fn evaluate_accepts_empty_relevant_ids() {
    let ds = EvalDataset::from_jsonl_str(r#"{"query":"no relevant","relevant_ids":[]}"#)
        .expect("dataset must parse");
    let provider = mock();
    let metrics = evaluate_model(&provider, &ds, &simple_corpus(), 3)
        .expect("empty relevant_ids should not fail validation");
    assert_eq!(
        metrics.recall_at_k, 0.0,
        "empty relevant_ids cannot produce a hit"
    );
    assert_eq!(metrics.mrr, 0.0, "empty relevant_ids yield zero MRR");
}

#[test]
fn permissive_mode_continues_with_unknown_ids() {
    let ds = EvalDataset::from_jsonl_str(
        r#"{"query":"missing","relevant_ids":["id-ghost"]}
{"query":"valid","relevant_ids":["id-alice"]}"#,
    )
    .expect("dataset must parse")
    .permissive(true);

    // Validation itself should not error in permissive mode.
    ds.validate_against_corpus(&simple_corpus())
        .expect("permissive validation should not fail");

    let provider = mock();
    let metrics = evaluate_model(&provider, &ds, &simple_corpus(), 3)
        .expect("permissive evaluation should continue");
    assert!(
        metrics.recall_at_k >= 0.0 && metrics.recall_at_k <= 1.0,
        "recall must remain bounded"
    );
}
