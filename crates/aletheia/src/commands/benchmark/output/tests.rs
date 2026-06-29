#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::io::Write as _;

use dokimion::benchmarks::{BenchmarkReport, BenchmarkScore, QuestionResult, QuestionStatus};

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

#[test]
fn reward_surface_uses_real_report_and_baseline_file() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("baseline.json");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(
        serde_json::json!({
            "benchmark": "LongMemEval",
            "exact_match_rate": 0.35,
            "mean_f1": 0.40
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();

    let report = sample_report();
    let surface = load_reward_surface(&report, &path).unwrap();

    assert!((surface.outcome.exact_match_rate - 0.5).abs() < f64::EPSILON);
    assert!((surface.reward - 0.15).abs() < f64::EPSILON);
    assert_eq!(
        format_reward_surface(&surface),
        "Reward vs baseline: +0.150 (EM 50.0% vs baseline 35.0%)"
    );
}
