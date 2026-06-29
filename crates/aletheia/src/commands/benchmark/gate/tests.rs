#![expect(clippy::unwrap_used, reason = "test assertions")]

use dokimion::benchmarks::{
    BenchmarkMetadata, BenchmarkReport, BenchmarkScore, QuestionResult, QuestionStatus,
};

use super::*;

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

fn first_two_questions_mut(
    report: &mut BenchmarkReport,
) -> (&mut QuestionResult, &mut QuestionResult) {
    let Some((first, rest)) = report.questions.split_first_mut() else {
        panic!("sample report must contain a first question");
    };
    let Some(second) = rest.first_mut() else {
        panic!("sample report must contain a second question");
    };
    (first, second)
}

fn sample_report_with_gate_metadata() -> BenchmarkReport {
    let mut report = sample_report();
    report.metadata = Some(BenchmarkMetadata {
        timestamp: "2026-06-01T00:00:00Z".to_owned(),
        aletheia_version: "0.1.0".to_owned(),
        nous_id: "benchmark".to_owned(),
        model: "fixture-model".to_owned(),
        benchmark: "LongMemEval".to_owned(),
        total_questions: 2,
        evaluated_questions: 2,
        timeout_secs: 120,
        dataset_hash: Some(
            "sha256:a6ecd7d5dadb9734f9cf28a590b99bfe527d906c08aaf3d356e7861661e74f10".to_owned(),
        ),
        git_sha: Some("0123456789abcdef".to_owned()),
        dataset_best_effort: false,
        dataset_validation: None,
    });
    let (first, second) = first_two_questions_mut(&mut report);
    first.recall_at_k = Some(1.0);
    second.recall_at_k = Some(0.5);
    first.ndcg_at_k = Some(1.0);
    second.ndcg_at_k = Some(0.5);
    report.judge_summary = Some(dokimion::benchmarks::JudgeSummary {
        attempted: 2,
        scored: 2,
        errors: 0,
        correct: 1,
    });
    report
}

fn sample_gate_baseline() -> BenchmarkGateBaseline {
    BenchmarkGateBaseline {
        version: 1,
        benchmark: "LongMemEval".to_owned(),
        provenance: BenchmarkGateProvenance {
            dataset_hash: "sha256:a6ecd7d5dadb9734f9cf28a590b99bfe527d906c08aaf3d356e7861661e74f10"
                .to_owned(),
            dataset_version: "fixture-smoke-v1".to_owned(),
            model: "fixture-model".to_owned(),
            source_report: "crates/aletheia/testdata/benchmarks/smoke-report.json".to_owned(),
            reviewed_at: "2026-06-01T00:00:00Z".to_owned(),
            reviewed_by: "benchmark-maintainers".to_owned(),
            git_sha: Some("0123456789abcdef".to_owned()),
        },
        metrics: BenchmarkGateMetrics {
            exact_match_rate: 0.5,
            mean_f1: 0.5,
            error_rate: 0.0,
            timeout_rate: 0.0,
            no_answer_rate: 0.0,
            recall_at_k: Some(0.75),
            ndcg_at_k: Some(0.75),
            judge_accuracy: Some(0.5),
            judge_error_rate: Some(0.0),
        },
        allowed_regression: BenchmarkGateMetrics {
            exact_match_rate: 0.0,
            mean_f1: 0.0,
            error_rate: 0.0,
            timeout_rate: 0.0,
            no_answer_rate: 0.0,
            recall_at_k: Some(0.0),
            ndcg_at_k: Some(0.0),
            judge_accuracy: Some(0.0),
            judge_error_rate: Some(0.0),
        },
        minimums: BenchmarkGateMinimums {
            scored_questions: 2,
            exact_match_rate: 0.5,
            mean_f1: 0.5,
            recall_at_k: Some(0.75),
            ndcg_at_k: Some(0.75),
            judge_accuracy: Some(0.5),
        },
        maximums: BenchmarkGateMaximums {
            errors: 0.0,
            timeouts: 0.0,
            no_answers: 0.0,
            judge_errors: Some(0.0),
        },
        require_retrieval: true,
        require_judge: true,
    }
}

#[test]
fn benchmark_gate_passes_reviewed_baseline() {
    let report = sample_report_with_gate_metadata();
    let baseline = sample_gate_baseline();

    let gate = benchmark_gate_report(&report, &baseline).unwrap();

    assert!(gate.passed);
    assert!(gate.checks.iter().all(|check| check.passed));
}

#[test]
fn benchmark_gate_fails_quality_regression() {
    let mut report = sample_report_with_gate_metadata();
    let Some(first) = report.questions.first_mut() else {
        panic!("sample report must contain a first question");
    };
    first.score.exact_match = false;
    first.score.f1 = 0.0;
    let baseline = sample_gate_baseline();

    let gate = benchmark_gate_report(&report, &baseline).unwrap();
    let err = require_gate_passed(&gate).unwrap_err();
    let message = err.to_string();

    assert!(!gate.passed);
    assert!(message.contains("exact_match_rate"), "got: {message}");
    assert!(message.contains("mean_f1"), "got: {message}");
}

#[test]
fn benchmark_gate_fails_stale_dataset_provenance() {
    let mut report = sample_report_with_gate_metadata();
    report.metadata.as_mut().unwrap().dataset_hash = Some("sha256:new-dataset".to_owned());
    let baseline = sample_gate_baseline();

    let gate = benchmark_gate_report(&report, &baseline).unwrap();
    let failed_metrics = gate
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| check.metric.as_str())
        .collect::<Vec<_>>();

    assert!(!gate.passed);
    assert!(failed_metrics.contains(&"dataset_hash"));
}

#[test]
fn benchmark_gate_fails_when_required_retrieval_missing() {
    let mut report = sample_report_with_gate_metadata();
    for question in &mut report.questions {
        question.recall_at_k = None;
        question.ndcg_at_k = None;
    }
    let baseline = sample_gate_baseline();

    let gate = benchmark_gate_report(&report, &baseline).unwrap();
    let failed_metrics = gate
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| check.metric.as_str())
        .collect::<Vec<_>>();

    assert!(!gate.passed);
    assert!(failed_metrics.contains(&"recall_at_k"));
    assert!(failed_metrics.contains(&"ndcg_at_k"));
}
