//! Recall@k metrics for measuring knowledge retrieval quality.

use std::collections::HashSet;

use serde::Serialize;
use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, ScenarioRunOutcome,
    ScenarioSubResult, assert_eval,
};

type DocId = String;

/// Standard k values for recall benchmarks.
pub(crate) const RECALL_K_VALUES: [usize; 4] = [1, 5, 10, 20];

/// Result of a recall@k computation at a specific k value.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecallScore {
    /// The k value used.
    pub k: usize,
    /// Fraction of relevant items found in top-k (0.0 to 1.0).
    pub score: f64,
    /// Fraction of retrieved items at k that are relevant (0.0 to 1.0).
    pub precision: f64,
    /// Count of relevant items found in top-k.
    pub found: usize,
    /// Total number of relevant items.
    pub total_relevant: usize,
}

/// Compute recall@k: fraction of relevant items found in the top-k retrieved results.
///
/// Returns a score of 0.0 if `relevant` is empty.
#[must_use]
pub(crate) fn compute_recall_at_k(
    relevant: &HashSet<String>,
    retrieved: &[String],
    k: usize,
) -> RecallScore {
    let total_relevant = relevant.len();
    if total_relevant == 0 {
        return RecallScore {
            k,
            score: 0.0,
            precision: 0.0,
            found: 0,
            total_relevant: 0,
        };
    }

    // WHY: deduplicate retrieved items so duplicates don't inflate the score
    let top_k: HashSet<&str> = retrieved.iter().take(k).map(String::as_str).collect();
    let found = relevant
        .iter()
        .filter(|item| top_k.contains(item.as_str()))
        .count();

    let score = usize_to_f64(found) / usize_to_f64(total_relevant);

    let actual_k = retrieved.len().min(k);
    let precision = if actual_k == 0 {
        0.0
    } else {
        usize_to_f64(found) / usize_to_f64(actual_k)
    };

    RecallScore {
        k,
        score,
        precision,
        found,
        total_relevant,
    }
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// Compute recall at all standard k values (1, 5, 10, 20).
#[must_use]
pub(crate) fn compute_recall_benchmark(
    relevant: &HashSet<String>,
    retrieved: &[String],
) -> Vec<RecallScore> {
    RECALL_K_VALUES
        .iter()
        .map(|&k| compute_recall_at_k(relevant, retrieved, k))
        .collect()
}

fn recall_has_signal(scores: &[RecallScore]) -> bool {
    scores.iter().any(|score| score.found > 0)
}

fn recall_sub_result(score: &RecallScore) -> ScenarioSubResult {
    let in_range = (0.0..=1.0).contains(&score.score) && (0.0..=1.0).contains(&score.precision);
    ScenarioSubResult {
        sub_id: format!("recall-at-{}", score.k),
        classification: ScenarioClassification::Informational,
        passed: in_range,
        criteria: Some(format!(
            "score={:.3}; precision={:.3}; found={}/{}",
            score.score, score.precision, score.found, score.total_relevant
        )),
        response_excerpt: None,
        violation_ids: if in_range {
            Vec::new()
        } else {
            vec!["recall_metric_out_of_range".to_owned()]
        },
    }
}

fn recall_signal_sub_result(has_signal: bool) -> ScenarioSubResult {
    ScenarioSubResult {
        sub_id: "ground-truth-retrieval-signal".to_owned(),
        classification: ScenarioClassification::Assertive,
        passed: has_signal,
        criteria: Some(
            "at least one configured ground-truth document id must appear in retrieved results"
                .to_owned(),
        ),
        response_excerpt: None,
        violation_ids: if has_signal {
            Vec::new()
        } else {
            vec!["no_ground_truth_hits".to_owned()]
        },
    }
}

/// Scenario that benchmarks recall@k against the knowledge search API.
struct RecallBenchmarkScenario {
    relevant_set: Vec<DocId>,
}

impl RecallBenchmarkScenario {
    const RELEVANT_IDS_ENV: &'static str = "ALETHEIA_RECALL_RELEVANT_IDS";

    fn from_operator_config() -> Self {
        let relevant_set = std::env::var(Self::RELEVANT_IDS_ENV)
            .ok()
            .map(|raw| parse_relevant_set(&raw))
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(Self::default_ground_truth);
        Self { relevant_set }
    }

    fn default_ground_truth() -> Vec<DocId> {
        vec![
            "recall-benchmark-alpha".to_owned(),
            "recall-benchmark-beta".to_owned(),
            "recall-benchmark-gamma".to_owned(),
        ]
    }
}

fn parse_relevant_set(raw: &str) -> Vec<DocId> {
    raw.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

impl Scenario for RecallBenchmarkScenario {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "recall-at-k-benchmark",
            description: "Compute recall@k against configured ground-truth document IDs",
            category: "cognitive",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,

            classification: ScenarioClassification::Assertive,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let mut sub_results = Vec::new();
                let result: crate::error::Result<()> = async {
                    let nous_list = client.list_nous().await?;
                    let nous = nous_list
                        .first()
                        .context(crate::error::NoAgentsAvailableSnafu)?;

                    let results = client
                        .search_knowledge("recall benchmark", &nous.id, 20)
                        .await?;

                    let retrieved: Vec<String> =
                        results.facts.iter().map(|f| f.id.clone()).collect();

                    let relevant: HashSet<String> = self.relevant_set.iter().cloned().collect();

                    let scores = compute_recall_benchmark(&relevant, &retrieved);
                    sub_results.extend(scores.iter().map(recall_sub_result));
                    let has_signal = recall_has_signal(&scores);
                    sub_results.push(recall_signal_sub_result(has_signal));

                    for score in &scores {
                        tracing::info!(
                            k = score.k,
                            recall = score.score,
                            precision = score.precision,
                            found = score.found,
                            total = score.total_relevant,
                            "recall@k"
                        );
                    }

                    assert_eval(
                        scores.iter().all(|s| s.score >= 0.0 && s.score <= 1.0),
                        "recall scores must be in [0.0, 1.0]",
                    )?;
                    assert_eval(
                        has_signal,
                        "no configured ground-truth document IDs were retrieved",
                    )?;

                    Ok(())
                }
                .await;
                ScenarioRunOutcome::from(result).with_sub_results(sub_results)
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "recall-at-k-benchmark"
            )),
        )
    }
}

pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![Box::new(RecallBenchmarkScenario::from_operator_config())]
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests {
    use super::*;

    #[test]
    fn recall_empty_relevant_returns_zero() {
        let relevant = HashSet::new();
        let retrieved = vec!["a".to_owned(), "b".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 5);
        assert!(
            (score.score).abs() < f64::EPSILON,
            "empty relevant set should yield 0.0"
        );
        assert!(
            (score.precision).abs() < f64::EPSILON,
            "empty relevant set should yield 0.0 precision"
        );
        assert_eq!(score.total_relevant, 0, "total_relevant should be 0");
    }

    #[test]
    fn recall_empty_retrieved_returns_zero() {
        let relevant: HashSet<String> = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let retrieved: Vec<String> = vec![];
        let score = compute_recall_at_k(&relevant, &retrieved, 5);
        assert!(
            (score.score).abs() < f64::EPSILON,
            "empty retrieved set should yield 0.0"
        );
        assert!(
            (score.precision).abs() < f64::EPSILON,
            "empty retrieved set should yield 0.0 precision"
        );
    }

    #[test]
    fn recall_perfect_score() {
        let relevant: HashSet<String> =
            HashSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let retrieved = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 3);
        assert!(
            (score.score - 1.0).abs() < f64::EPSILON,
            "all relevant items retrieved should yield 1.0"
        );
        assert_eq!(score.found, 3, "all 3 should be found");
        assert!(
            (score.precision - 1.0).abs() < f64::EPSILON,
            "all retrieved items are relevant"
        );
    }

    #[test]
    fn recall_ground_truth_two_of_three_found() {
        let relevant: HashSet<String> =
            HashSet::from(["1".to_owned(), "2".to_owned(), "3".to_owned()]);
        let retrieved = vec!["1".to_owned(), "2".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 3);
        assert!(
            (score.score - 2.0 / 3.0).abs() < f64::EPSILON,
            "2 of 3 relevant docs should produce 2/3 recall"
        );
    }

    #[test]
    fn relevant_set_parser_uses_operator_ids() {
        let ids = parse_relevant_set(" doc-1,doc-2, ,doc-3 ");
        assert_eq!(ids, vec!["doc-1", "doc-2", "doc-3"]);
    }

    #[test]
    fn recall_ground_truth_empty_retrieval_zero() {
        let relevant: HashSet<String> =
            HashSet::from(["1".to_owned(), "2".to_owned(), "3".to_owned()]);
        let retrieved = vec![];
        let score = compute_recall_at_k(&relevant, &retrieved, 3);
        assert!(
            (score.score).abs() < f64::EPSILON,
            "no retrieved docs should produce zero recall"
        );
    }

    #[test]
    fn recall_signal_requires_at_least_one_ground_truth_hit() {
        let relevant: HashSet<String> =
            HashSet::from(["1".to_owned(), "2".to_owned(), "3".to_owned()]);
        let no_hits = compute_recall_benchmark(&relevant, &["4".to_owned(), "5".to_owned()]);
        assert!(
            !recall_has_signal(&no_hits),
            "benchmark should not accept all-zero recall as a signal"
        );

        let with_hit = compute_recall_benchmark(&relevant, &["4".to_owned(), "1".to_owned()]);
        assert!(
            recall_has_signal(&with_hit),
            "benchmark should accept at least one retrieved ground-truth document"
        );
    }

    #[test]
    fn recall_ground_truth_extra_retrieval_keeps_recall_and_precision_honest() {
        let relevant: HashSet<String> =
            HashSet::from(["1".to_owned(), "2".to_owned(), "3".to_owned()]);
        let retrieved = vec![
            "1".to_owned(),
            "2".to_owned(),
            "3".to_owned(),
            "4".to_owned(),
        ];
        let score = compute_recall_at_k(&relevant, &retrieved, 4);
        assert!(
            (score.score - 1.0).abs() < f64::EPSILON,
            "all relevant docs retrieved should produce perfect recall"
        );
        assert!(
            (score.precision - 0.75).abs() < f64::EPSILON,
            "3 relevant of 4 retrieved docs should produce 0.75 precision"
        );
    }

    #[test]
    fn recall_partial_score() {
        let relevant: HashSet<String> = HashSet::from([
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
            "e".to_owned(),
        ]);
        let retrieved = vec![
            "a".to_owned(),
            "x".to_owned(),
            "b".to_owned(),
            "y".to_owned(),
            "z".to_owned(),
        ];
        let score = compute_recall_at_k(&relevant, &retrieved, 5);
        assert!(
            (score.score - 0.4).abs() < f64::EPSILON,
            "2 out of 5 relevant found should yield 0.4, got {}",
            score.score
        );
    }

    #[test]
    fn recall_k_limits_retrieval_window() {
        let relevant: HashSet<String> =
            HashSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let retrieved = vec![
            "x".to_owned(),
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
        ];

        let at_1 = compute_recall_at_k(&relevant, &retrieved, 1);
        assert!(
            (at_1.score).abs() < f64::EPSILON,
            "recall@1 should be 0.0 when first item is irrelevant"
        );

        let at_2 = compute_recall_at_k(&relevant, &retrieved, 2);
        assert!(
            (at_2.score - 1.0 / 3.0).abs() < f64::EPSILON,
            "recall@2 should be 1/3, got {}",
            at_2.score
        );

        let at_4 = compute_recall_at_k(&relevant, &retrieved, 4);
        assert!(
            (at_4.score - 1.0).abs() < f64::EPSILON,
            "recall@4 should be 1.0 when all relevant items within k"
        );
    }

    #[test]
    fn recall_k_larger_than_retrieved_list() {
        let relevant: HashSet<String> = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let retrieved = vec!["a".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 10);
        assert!(
            (score.score - 0.5).abs() < f64::EPSILON,
            "recall@10 with 1 of 2 relevant should be 0.5"
        );
    }

    #[test]
    fn recall_benchmark_computes_all_k_values() {
        let relevant: HashSet<String> =
            HashSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let retrieved = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let scores = compute_recall_benchmark(&relevant, &retrieved);
        assert_eq!(
            scores.len(),
            RECALL_K_VALUES.len(),
            "should compute score for each standard k value"
        );
        for (score, &expected_k) in scores.iter().zip(RECALL_K_VALUES.iter()) {
            assert_eq!(score.k, expected_k, "k values should match");
        }
    }

    #[test]
    fn recall_benchmark_monotonically_nondecreasing() {
        let relevant: HashSet<String> = HashSet::from([
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
            "e".to_owned(),
        ]);
        let mut retrieved: Vec<String> = (0..20).map(|i| format!("noise-{i}")).collect();
        // WHY: place relevant items at positions that span the k boundaries
        retrieved[0] = "a".to_owned();
        retrieved[4] = "b".to_owned();
        retrieved[9] = "c".to_owned();
        retrieved[14] = "d".to_owned();
        retrieved[19] = "e".to_owned();

        let scores = compute_recall_benchmark(&relevant, &retrieved);
        for window in scores.windows(2) {
            assert!(
                window[0].score <= window[1].score,
                "recall should be non-decreasing: recall@{} ({}) > recall@{} ({})",
                window[0].k,
                window[0].score,
                window[1].k,
                window[1].score
            );
        }
    }

    #[test]
    fn recall_score_fields_populated() {
        let relevant: HashSet<String> = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let retrieved = vec!["a".to_owned(), "x".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 2);
        assert_eq!(score.k, 2, "k should be 2");
        assert_eq!(score.found, 1, "found should be 1");
        assert_eq!(score.total_relevant, 2, "total_relevant should be 2");
    }

    #[test]
    fn recall_score_serializes() {
        let score = RecallScore {
            k: 5,
            score: 0.6,
            precision: 0.75,
            found: 3,
            total_relevant: 5,
        };
        let json = serde_json::to_string(&score);
        assert!(json.is_ok(), "RecallScore should serialize to JSON");
    }

    #[test]
    fn recall_no_overlap_yields_zero() {
        let relevant: HashSet<String> = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let retrieved = vec!["x".to_owned(), "y".to_owned(), "z".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 3);
        assert!(
            (score.score).abs() < f64::EPSILON,
            "no overlap should yield 0.0"
        );
        assert_eq!(score.found, 0, "found should be 0");
    }

    #[test]
    fn recall_duplicate_retrieved_items() {
        let relevant: HashSet<String> = HashSet::from(["a".to_owned(), "b".to_owned()]);
        let retrieved = vec!["a".to_owned(), "a".to_owned(), "a".to_owned()];
        let score = compute_recall_at_k(&relevant, &retrieved, 3);
        // WHY: duplicates in retrieved still only count as one match against the relevant set
        assert!(
            (score.score - 0.5).abs() < f64::EPSILON,
            "duplicate retrieved items: 1 unique match out of 2 relevant = 0.5, got {}",
            score.score
        );
        assert!(
            (score.precision - 1.0 / 3.0).abs() < f64::EPSILON,
            "duplicate retrieved items: 1 unique match in 3 retrieved slots = 1/3 precision, got {}",
            score.precision
        );
    }
}
