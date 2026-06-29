#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::path::PathBuf;

use dokimion::benchmarks::{BenchmarkReport, BenchmarkScore, QuestionResult, QuestionStatus};

use super::*;

fn base_args() -> RunArgs {
    RunArgs {
        dataset: PathBuf::from("/tmp/does-not-matter.json"),
        url: "http://127.0.0.1:18789".to_owned(),
        token: None,
        nous_id: "benchmark".to_owned(),
        max_questions: None,
        timeout: 120,
        json: false,
        output: None,
        baseline_out: None,
        baseline_in: None,
        baseline_report: None,
        gate_baseline: None,
        publishable: false,
        retrieval_k: None,
        best_effort_dataset: false,
        judge_endpoint: None,
        judge_model: "gpt-4o".to_owned(),
        judge_api_key: None,
    }
}

#[test]
fn validate_rejects_timeout_zero() {
    let mut a = base_args();
    a.timeout = 0;
    let err = validate_args(&a).unwrap_err();
    assert!(
        err.to_string().contains("--timeout must be greater than 0"),
        "got: {err}"
    );
}

#[test]
fn validate_rejects_max_questions_zero() {
    let mut a = base_args();
    a.max_questions = Some(0);
    let err = validate_args(&a).unwrap_err();
    assert!(err.to_string().contains("--max-questions"), "got: {err}");
}

#[test]
fn validate_rejects_retrieval_k_zero() {
    let mut a = base_args();
    a.retrieval_k = Some(0);
    let err = validate_args(&a).unwrap_err();
    assert!(err.to_string().contains("--retrieval-k"), "got: {err}");
}

#[test]
fn validate_rejects_empty_nous_id() {
    let mut a = base_args();
    a.nous_id = String::new();
    let err = validate_args(&a).unwrap_err();
    assert!(err.to_string().contains("--nous-id"), "got: {err}");
}

#[test]
fn validate_rejects_whitespace_only_nous_id() {
    let mut a = base_args();
    a.nous_id = "   ".to_owned();
    let err = validate_args(&a).unwrap_err();
    assert!(err.to_string().contains("--nous-id"), "got: {err}");
}

#[test]
fn validate_rejects_malformed_url() {
    let mut a = base_args();
    a.url = "not a url".to_owned();
    let err = validate_args(&a).unwrap_err();
    assert!(
        err.to_string().contains("--url is not a valid URL"),
        "got: {err}"
    );
}

#[test]
fn validate_accepts_well_formed_args() {
    validate_args(&base_args()).unwrap();
    let mut a = base_args();
    a.url = "https://example.com:8443/path".to_owned();
    a.max_questions = Some(5);
    a.retrieval_k = Some(10);
    a.timeout = 1;
    validate_args(&a).unwrap();
}

#[test]
fn validate_requires_gate_baseline_for_publishable_runs() {
    let mut a = base_args();
    a.publishable = true;
    let err = validate_args(&a).unwrap_err();
    assert!(
        err.to_string()
            .contains("--publishable requires --gate-baseline"),
        "got: {err}"
    );

    a.gate_baseline = Some(PathBuf::from("reviewed-gate.json"));
    validate_args(&a).unwrap();
}

fn sample_report() -> BenchmarkReport {
    BenchmarkReport::new(
        "LongMemEval",
        vec![
            QuestionResult {
                id: "q1".to_owned(),
                category: "factual".to_owned(),
                status: QuestionStatus::Scored,
                error_message: None,
                actual_answer: "blue".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                expected_evidence_refs: Vec::new(),
                score: BenchmarkScore {
                    exact_match: true,
                    f1: 1.0,
                    contains: true,
                },
                judge_score: None,
                retrieved_facts: None,
                retrieval_scoring: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
            QuestionResult {
                id: "q2".to_owned(),
                category: "factual".to_owned(),
                status: QuestionStatus::Scored,
                error_message: None,
                actual_answer: "green".to_owned(),
                expected_answers: vec!["red".to_owned()],
                expected_evidence_refs: Vec::new(),
                score: BenchmarkScore {
                    exact_match: false,
                    f1: 0.0,
                    contains: false,
                },
                judge_score: None,
                retrieved_facts: None,
                retrieval_scoring: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
        ],
    )
}

#[test]
fn publishable_mode_rejects_point_estimate_only_report() {
    let report = sample_report();
    let err = require_publishable_report(&report).unwrap_err();

    assert!(
        err.to_string()
            .contains("missing bootstrap confidence intervals"),
        "got: {err}"
    );
}
