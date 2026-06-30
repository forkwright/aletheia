use super::*;
use crate::provenance::EvalProvenance;

fn result(id: &str, category: &str, actual: &str, expected: &[&str]) -> QuestionResult {
    QuestionResult {
        id: id.to_owned(),
        category: category.to_owned(),
        status: QuestionStatus::Scored,
        error_message: None,
        actual_answer: actual.to_owned(),
        expected_answers: expected.iter().map(|answer| (*answer).to_owned()).collect(),
        expected_evidence_refs: Vec::new(),
        score: score_answer(
            actual,
            &expected
                .iter()
                .map(|answer| (*answer).to_owned())
                .collect::<Vec<_>>(),
        ),
        judge_score: None,
        retrieved_facts: None,
        retrieval_scoring: None,
        recall_at_k: None,
        ndcg_at_k: None,
    }
}

fn judge_score(correct: bool, status: judge::JudgeStatus) -> judge::JudgeScore {
    judge::JudgeScore {
        correct,
        reasoning: "judge result".to_owned(),
        status,
        error_message: matches!(status, judge::JudgeStatus::Error)
            .then(|| "judge failed".to_owned()),
        provenance: judge::JudgeProvenance {
            endpoint: "http://judge.test".to_owned(),
            model: "judge-model".to_owned(),
            prompt_sha256: "abc".to_owned(),
            raw_response_sha256: None,
            raw_response_body_ref: None,
            request_id: None,
            usage: None,
            provider_status: Some(200),
            parse_status: if status.is_scored() {
                judge::JudgeParseStatus::Parsed
            } else {
                judge::JudgeParseStatus::MalformedJson
            },
        },
    }
}

#[test]
fn empty_report_has_zero_rates() {
    let report = BenchmarkReport::default();
    assert!(report.exact_match_rate().abs() < f64::EPSILON);
    assert!(report.mean_f1().abs() < f64::EPSILON);
    assert_eq!(report.total, 0);
}

#[test]
fn report_aggregates_em_and_f1() {
    let questions = vec![
        result("q1", "factual", "blue", &["blue"]),
        result("q2", "factual", "green", &["red"]),
    ];
    let report = BenchmarkReport::new("Test", questions);
    assert_eq!(report.total, 2);
    assert!((report.exact_match_rate() - 0.5).abs() < f64::EPSILON);
    assert!((report.mean_f1() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn per_category_groups_results() {
    let questions = vec![
        result("q1", "temporal", "yes", &["yes"]),
        result("q2", "temporal", "no", &["yes"]),
        result("q3", "factual", "42", &["42"]),
    ];
    let report = BenchmarkReport::new("Test", questions);
    let per_cat = report.per_category();
    assert_eq!(per_cat.len(), 2);
    // BTreeMap sorts alphabetically: factual, temporal
    assert_eq!(per_cat[0].0, "factual");
    assert!((per_cat[0].1 - 1.0).abs() < f64::EPSILON);
    assert_eq!(per_cat[1].0, "temporal");
    assert!((per_cat[1].1 - 0.5).abs() < f64::EPSILON);
}

#[test]
fn judge_accuracy_counts_errors_in_denominator() {
    let mut q1 = result("q1", "factual", "blue", &["blue"]);
    q1.judge_score = Some(judge_score(true, judge::JudgeStatus::Scored));
    let mut q2 = result("q2", "factual", "red", &["blue"]);
    q2.judge_score = Some(judge_score(false, judge::JudgeStatus::Scored));
    let mut q3 = result("q3", "factual", "green", &["green"]);
    q3.judge_score = Some(judge_score(false, judge::JudgeStatus::Error));

    let questions = vec![q1, q2, q3];
    let report = BenchmarkReport::new("Test", questions);
    assert_eq!(
        report.judge_summary,
        Some(JudgeSummary {
            attempted: 3,
            scored: 2,
            errors: 1,
            correct: 1,
        })
    );
    assert!((report.judge_accuracy().unwrap() - (1.0 / 3.0)).abs() < f64::EPSILON);
}

#[test]
fn judge_accuracy_none_when_no_scores() {
    let questions = vec![result("q1", "factual", "blue", &["blue"])];
    let report = BenchmarkReport::new("Test", questions);
    assert!(report.judge_accuracy().is_none());
}

#[test]
fn mean_recall_and_ndcg_computed_correctly() {
    let mut q1 = result("q1", "factual", "blue", &["blue"]);
    q1.recall_at_k = Some(1.0);
    q1.ndcg_at_k = Some(1.0);
    let mut q2 = result("q2", "factual", "red", &["blue"]);
    q2.recall_at_k = Some(0.0);
    q2.ndcg_at_k = Some(0.0);
    let questions = vec![q1, q2];
    let report = BenchmarkReport::new("Test", questions);
    assert!((report.mean_recall_at_k().unwrap() - 0.5).abs() < f64::EPSILON);
    assert!((report.mean_ndcg_at_k().unwrap() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn metadata_defaults() {
    let meta = BenchmarkMetadata::default();
    assert_eq!(meta.nous_id, "benchmark");
    assert_eq!(meta.model, "unknown");
}

#[test]
fn report_with_metadata_roundtrips_via_json() {
    let meta = BenchmarkMetadata {
        timestamp: "2026-04-17T12:00:00Z".to_owned(),
        aletheia_version: "1.0.0".to_owned(),
        nous_id: "benchmark".to_owned(),
        model: "claude-opus-4".to_owned(),
        benchmark: "LongMemEval".to_owned(),
        total_questions: 500,
        evaluated_questions: 50,
        timeout_secs: 120,
        dataset_hash: Some("sha256:abc".to_owned()),
        git_sha: Some("deadbeef".to_owned()),
        dataset_best_effort: true,
        dataset_validation: Some(BenchmarkValidationReport {
            dataset: "LongMemEval".to_owned(),
            dataset_path: Some("/tmp/dataset.json".to_owned()),
            best_effort: true,
            require_retrieval_evidence: false,
            errors: Vec::new(),
            warnings: Vec::new(),
        }),
    };
    let report = BenchmarkReport::with_metadata("LongMemEval", vec![], meta);
    let json = serde_json::to_string(&report).expect("serialize");
    let deserialized: BenchmarkReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.metadata, report.metadata);
}

#[test]
fn non_scored_statuses_count_as_zero_in_headline_denominator() {
    let scored = result("q1", "factual", "blue", &["blue"]);
    let mut timeout = result("q2", "factual", "", &["red"]);
    timeout.status = QuestionStatus::Timeout;
    timeout.error_message = Some("timed out".to_owned());
    timeout.score = BenchmarkScore::zero();
    let mut error = result("q3", "factual", "", &["green"]);
    error.status = QuestionStatus::Error;
    error.error_message = Some("request failed".to_owned());
    error.score = BenchmarkScore::zero();
    let mut no_answer = result("q4", "factual", "", &["yellow"]);
    no_answer.status = QuestionStatus::NoAnswer;
    no_answer.error_message = Some("empty answer".to_owned());
    no_answer.score = BenchmarkScore::zero();

    let questions = vec![scored, timeout, error, no_answer];
    let report = BenchmarkReport::new("Test", questions);
    assert_eq!(report.total, 4);
    assert_eq!(report.scored, 1);
    assert_eq!(report.timeouts, 1);
    assert_eq!(report.errors, 1);
    assert_eq!(report.no_answers, 1);
    assert_eq!(
        report.reliability,
        BenchmarkReliabilitySummary {
            attempted: 4,
            scored: 1,
            ingestion_errors: 0,
            ingestion_partials: 0,
            errors: 1,
            timeouts: 1,
            no_answers: 1,
            error_rate: 0.25,
            timeout_rate: 0.25,
            no_answer_rate: 0.25,
            ingestion_error_rate: 0.0,
            ingestion_partial_rate: 0.0,
        }
    );
    assert_eq!(report.reliability_summary(), report.reliability);
    assert!((report.exact_match_rate() - 0.25).abs() < f64::EPSILON);
    assert!((report.mean_f1() - 0.25).abs() < f64::EPSILON);
    assert!((report.scored_only_exact_match_rate() - 1.0).abs() < f64::EPSILON);
    assert!((report.scored_only_mean_f1() - 1.0).abs() < f64::EPSILON);

    let per_category = report.per_category();
    assert_eq!(per_category.len(), 1);
    assert!((per_category[0].1 - 0.25).abs() < f64::EPSILON);
    assert!((per_category[0].2 - 0.25).abs() < f64::EPSILON);
    let scored_only_per_category = report.scored_only_per_category();
    assert_eq!(scored_only_per_category.len(), 1);
    assert!((scored_only_per_category[0].1 - 1.0).abs() < f64::EPSILON);
    assert!((scored_only_per_category[0].2 - 1.0).abs() < f64::EPSILON);
}

fn publishable_metadata() -> BenchmarkMetadata {
    BenchmarkMetadata {
        timestamp: "2026-04-17T12:00:00Z".to_owned(),
        aletheia_version: "1.0.0".to_owned(),
        nous_id: "benchmark".to_owned(),
        model: "claude-opus-4".to_owned(),
        benchmark: "Test".to_owned(),
        total_questions: 2,
        evaluated_questions: 2,
        timeout_secs: 120,
        dataset_hash: Some("sha256:dataset".to_owned()),
        git_sha: Some("abc123".to_owned()),
        dataset_best_effort: false,
        dataset_validation: Some(BenchmarkValidationReport {
            dataset: "Test".to_owned(),
            dataset_path: Some("/tmp/dataset.json".to_owned()),
            best_effort: false,
            require_retrieval_evidence: false,
            errors: Vec::new(),
            warnings: Vec::new(),
        }),
    }
}

fn publishable_provenance() -> EvalProvenance {
    let args = vec![
        "aletheia".to_owned(),
        "benchmark".to_owned(),
        "longmemeval".to_owned(),
    ];
    EvalProvenance::new("er-test", "http://localhost")
        .with_redacted_args(&args)
        .with_config_hash("sha256:config")
        .with_scenario_suite_hash("sha256:dataset")
        .finished()
}

#[test]
fn standard_statistics_populates_ci_and_publishability() {
    let questions = vec![
        result("q1", "factual", "blue", &["blue"]),
        result("q2", "factual", "green", &["red"]),
    ];
    let report = BenchmarkReport::with_metadata("Test", questions, publishable_metadata())
        .with_provenance(publishable_provenance())
        .with_standard_statistics();

    let statistics = report.statistics.expect("statistics populated");
    assert_eq!(statistics.n_resamples, BENCHMARK_STAT_RESAMPLES);
    let publishability = report.publishability.expect("publishability populated");
    assert!(
        publishability.publishable,
        "expected publishable report, got reasons: {:?}",
        publishability.reasons
    );
}

#[test]
fn publishability_rejects_operational_failure_rates() {
    let mut timeout = result("q3", "factual", "", &["red"]);
    timeout.status = QuestionStatus::Timeout;
    timeout.error_message = Some("timed out".to_owned());
    timeout.score = BenchmarkScore::zero();
    let questions = vec![
        result("q1", "factual", "blue", &["blue"]),
        result("q2", "factual", "green", &["green"]),
        timeout,
    ];

    let report = BenchmarkReport::with_metadata("Test", questions, publishable_metadata())
        .with_provenance(publishable_provenance())
        .with_standard_statistics();

    assert!(report.statistics.is_some());
    let publishability = report.publishability.expect("publishability populated");
    assert!(!publishability.publishable);
    assert!(
        publishability
            .reasons
            .iter()
            .any(|reason| reason.contains("timeout rate")),
        "got reasons: {:?}",
        publishability.reasons
    );
}

#[test]
fn insufficient_samples_are_explicitly_non_publishable() {
    let report = BenchmarkReport::new("Test", vec![result("q1", "factual", "blue", &["blue"])])
        .with_standard_statistics();

    assert!(report.statistics.is_none());
    let publishability = report.publishability.expect("publishability populated");
    assert!(!publishability.publishable);
    assert!(
        publishability
            .reasons
            .iter()
            .any(|reason| reason.contains("requires at least 2 scored questions")),
        "got reasons: {:?}",
        publishability.reasons
    );
}

#[test]
fn baseline_candidate_comparisons_include_fdr_adjusted_p_values() {
    let baseline = BenchmarkReport::new(
        "Test",
        vec![
            result("q1", "factual", "wrong", &["blue"]),
            result("q2", "factual", "wrong", &["red"]),
            result("q3", "factual", "wrong", &["green"]),
        ],
    );
    let candidate = BenchmarkReport::new(
        "Test",
        vec![
            result("q1", "factual", "blue", &["blue"]),
            result("q2", "factual", "red", &["red"]),
            result("q3", "factual", "green", &["green"]),
        ],
    )
    .with_comparisons_against(&baseline, "baseline_vs_candidate");

    assert_eq!(candidate.comparisons.len(), 2);
    for comparison in &candidate.comparisons {
        assert_eq!(comparison.status, BenchmarkComparisonStatus::Complete);
        let statistics = comparison.statistics.as_ref().expect("statistics");
        assert_eq!(statistics.n_a, 3);
        assert_eq!(statistics.n_b, 3);
        assert!(
            statistics.p_adjusted.is_some(),
            "FDR-adjusted p-value must be present"
        );
    }
}

#[test]
fn comparison_reports_insufficient_matches_explicitly() {
    let baseline = BenchmarkReport::new("Test", vec![result("q1", "factual", "wrong", &["blue"])]);
    let candidate = BenchmarkReport::new("Test", vec![result("q1", "factual", "blue", &["blue"])])
        .with_comparisons_against(&baseline, "baseline_vs_candidate");

    assert_eq!(candidate.comparisons.len(), 2);
    for comparison in &candidate.comparisons {
        assert_eq!(
            comparison.status,
            BenchmarkComparisonStatus::InsufficientSamples
        );
        assert_eq!(comparison.matched_questions, 1);
        assert!(comparison.reason.is_some());
    }
}

#[test]
fn download_instructions_mentions_datasets() {
    let instructions = download_instructions();
    assert!(instructions.contains("LongMemEval"));
    assert!(instructions.contains("LoCoMo"));
}
