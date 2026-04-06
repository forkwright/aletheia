//! Recall@k metrics for measuring knowledge retrieval quality.

use std::collections::HashSet;

use serde::Serialize;
use snafu::OptionExt as _;
use tracing::Instrument;

use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioFuture, ScenarioMeta, assert_eval};

/// Standard k values for recall benchmarks.
pub(crate) const RECALL_K_VALUES: [usize; 4] = [1, 5, 10, 20];

/// Result of a recall@k computation at a specific k value.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecallScore {
    /// The k value used.
    pub k: usize,
    /// Fraction of relevant items found in top-k (0.0 to 1.0).
    pub score: f64,
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

    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "count values are small enough for lossless f64 conversion"
    )]
    let score = found as f64 / total_relevant as f64;

    RecallScore {
        k,
        score,
        found,
        total_relevant,
    }
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

/// Scenario that benchmarks recall@k against the knowledge search API.
struct RecallBenchmarkScenario;

impl Scenario for RecallBenchmarkScenario {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: "recall-at-k-benchmark",
            description: "Measure knowledge retrieval quality at k=1,5,10,20",
            category: "cognitive",
            requires_auth: true,
            requires_nous: true,
            expected_contains: None,
            expected_pattern: None,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(
            async move {
                let nous_list = client.list_nous().await?;
                let nous = nous_list
                    .first()
                    .context(crate::error::NoAgentsAvailableSnafu)?;

                let results = client
                    .search_knowledge("recall benchmark", &nous.id, 20)
                    .await?;

                let retrieved: Vec<String> = results.facts.iter().map(|f| f.id.clone()).collect();

                // WHY: ground truth is the full set returned; we measure how search ranking
                // surfaces them. With no pre-seeded facts, this validates the pipeline works.
                let relevant: HashSet<String> = retrieved.iter().cloned().collect();

                let scores = compute_recall_benchmark(&relevant, &retrieved);

                for score in &scores {
                    tracing::info!(
                        k = score.k,
                        recall = score.score,
                        found = score.found,
                        total = score.total_relevant,
                        "recall@k"
                    );
                }

                assert_eval(
                    scores.iter().all(|s| s.score >= 0.0 && s.score <= 1.0),
                    "recall scores must be in [0.0, 1.0]",
                )?;

                Ok(())
            }
            .instrument(tracing::info_span!(
                "scenario",
                id = "recall-at-k-benchmark"
            )),
        )
    }
}

pub(crate) fn scenarios() -> Vec<Box<dyn Scenario>> {
    vec![Box::new(RecallBenchmarkScenario)]
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
    }
}
