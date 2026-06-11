//! Memory benchmark harness: `LongMemEval`, `LoCoMo`, `HaluMem` scoring against aletheia.
//!
//! Each benchmark provides a standardized dataset of long
//! conversations + question/answer pairs that measure a specific memory
//! ability (factual recall, temporal reasoning, cross-session consolidation).
//! This module provides:
//!
//! - Dataset format parsers (`LongMemEvalDataset`, `LocomoDataset`)
//! - A common [`MemoryBenchmark`] trait the runner can execute
//! - Metric computation ([`BenchmarkScore`]: exact match, F1, contains)
//! - A runner that ingests conversations through the aletheia API, asks
//!   questions, and compares the answers against ground truth
//!
//! # Why this exists
//!
//! The memory pipeline has several research-backed features (admission
//! control, staleness, probe QA, evidence gap, surprise, anomaly detection)
//! — each with heuristic implementations. Without a benchmark loop, every
//! change is unmeasured and may regress recall quality. The harness closes
//! that loop.
//!
//! # Running
//!
//! ```text
//! cargo run -p dokimion --bin dokimion -- benchmark longmemeval \
//!     --dataset /path/to/longmemeval.json \
//!     --instance http://localhost:8080 \
//!     --output results.json
//! ```
//!
//! Datasets must be downloaded separately — see [`download_instructions`].
//! The harness does not commit dataset files to the repo.

/// Published peer baseline scores for contextualizing results.
pub mod baselines;
/// LLM-as-judge scorer for binary answer correctness.
pub mod judge;
/// Dataset loader + question iterator for the `LoCoMo` benchmark.
pub mod locomo;
/// Dataset loader + question iterator for the `LongMemEval` benchmark.
pub mod longmemeval;
/// Benchmark scoring (exact match, F1, contains, recall@k, NDCG@k).
pub mod metrics;
/// Live benchmark runner: executes a benchmark against an aletheia instance.
pub mod runner;

pub use self::runner::{BenchmarkRunner, BenchmarkRunnerConfig};

/// Re-export of [`EvalClient`](crate::client::EvalClient) for external use.
///
/// External consumers of the benchmark runner need this to construct a
/// runner. The rest of the client API surface is not stable.
pub type EvalClient = crate::client::EvalClient;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use self::metrics::{BenchmarkScore, score_answer};

/// A single question/answer pair backed by prior conversation context.
#[derive(Debug, Clone)]
pub struct BenchmarkQuestion {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark question id from external dataset JSON, not a domain newtype
    /// Unique identifier for this question within the benchmark.
    pub id: String,
    /// The conversations (sessions) to ingest before asking this question.
    ///
    /// Each session is a list of turns; each turn is (role, content).
    pub sessions: Vec<Vec<(String, String)>>,
    /// The question text to ask after ingestion.
    pub question: String,
    /// The ground-truth answer(s). Multiple acceptable answers may be listed.
    pub expected_answers: Vec<String>,
    /// Category label for per-ability scoring (e.g. "temporal", "multi-session").
    pub category: String,
}

/// A memory benchmark dataset: a collection of questions.
pub trait MemoryBenchmark {
    /// Human-readable benchmark name (e.g. "`LongMemEval`", "`LoCoMo`").
    fn name(&self) -> &'static str;

    /// Iterator over all questions in the dataset.
    fn questions(&self) -> Box<dyn Iterator<Item = BenchmarkQuestion> + '_>;

    /// Total question count (for progress reporting).
    fn len(&self) -> usize;

    /// Whether the dataset has no questions.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// System and run metadata captured alongside a benchmark report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkMetadata {
    /// ISO-8601 timestamp when the benchmark run started.
    pub timestamp: String,
    /// Aletheia version string from `/api/health`.
    pub aletheia_version: String,
    // kanon:ignore RUST/primitive-for-domain-id — nous_id deserialized from API response; newtype would require custom Deserialize
    /// Nous agent ID used for the benchmark.
    pub nous_id: String,
    /// Model identifier from the nous agent configuration.
    pub model: String,
    /// Name of the benchmark dataset.
    pub benchmark: String,
    /// Total questions in the dataset.
    pub total_questions: usize,
    /// Number of questions actually evaluated (after `max_questions` limit).
    pub evaluated_questions: usize,
    /// Per-question timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for BenchmarkMetadata {
    fn default() -> Self {
        Self {
            timestamp: "1970-01-01T00:00:00Z".to_owned(),
            aletheia_version: "unknown".to_owned(),
            nous_id: "benchmark".to_owned(),
            model: "unknown".to_owned(),
            benchmark: "unknown".to_owned(),
            total_questions: 0,
            evaluated_questions: 0,
            timeout_secs: 120,
        }
    }
}

/// Result of scoring a single benchmark question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResult {
    // kanon:ignore RUST/primitive-for-domain-id — benchmark result id mirrors external dataset question id
    /// Question id.
    pub id: String,
    /// Category.
    pub category: String,
    /// The answer produced by aletheia.
    pub actual_answer: String,
    /// The expected answers (ground truth, may have multiple valid forms).
    pub expected_answers: Vec<String>,
    /// Best score across all expected answers.
    pub score: BenchmarkScore,
    /// Optional LLM-as-judge score (populated when judge is configured).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_score: Option<judge::JudgeScore>,
    /// Optional retrieval metrics: facts retrieved for the question.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieved_facts: Option<Vec<String>>,
    /// Optional retrieval metric: Recall@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recall_at_k: Option<f64>,
    /// Optional retrieval metric: NDCG@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ndcg_at_k: Option<f64>,
}

/// Aggregate report for a benchmark run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Benchmark name.
    pub benchmark: String,
    /// Total questions scored.
    pub total: usize,
    /// Per-question results.
    pub questions: Vec<QuestionResult>,
    /// System and run metadata.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<BenchmarkMetadata>,
    /// Statistical summary with 95% CI for key metrics.
    ///
    /// Populated by calling [`BenchmarkReport::with_statistics`].
    /// Absent in reports produced without statistical analysis.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub statistics: Option<BenchmarkStatistics>,
}

/// Statistical summary for a benchmark run.
///
/// Carries the key aggregate metrics with 95% bootstrap CIs, absorbed from
/// the quantified-self pipeline's statistical discipline. These numbers make
/// results honest: a point estimate without a CI is not publishable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStatistics {
    /// 95% bootstrap CI lower bound for mean F1 across all questions.
    pub f1_ci_low: f64,
    /// 95% bootstrap CI upper bound for mean F1 across all questions.
    pub f1_ci_high: f64,
    /// 95% bootstrap CI lower bound for exact-match rate.
    pub em_ci_low: f64,
    /// 95% bootstrap CI upper bound for exact-match rate.
    pub em_ci_high: f64,
    /// Number of bootstrap resamples used to compute CI.
    pub n_resamples: usize,
    /// Tool + version string for provenance.
    pub method: String,
}

impl BenchmarkReport {
    /// Build an aggregate report from individual question results.
    #[must_use]
    pub fn new(benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self {
        Self {
            benchmark: benchmark.into(),
            total: questions.len(),
            questions,
            metadata: None,
            statistics: None,
        }
    }

    /// Build a report with metadata.
    #[must_use]
    pub fn with_metadata(
        benchmark: impl Into<String>,
        questions: Vec<QuestionResult>,
        metadata: BenchmarkMetadata,
    ) -> Self {
        Self {
            benchmark: benchmark.into(),
            total: questions.len(),
            questions,
            metadata: Some(metadata),
            statistics: None,
        }
    }

    /// Attach bootstrap CIs for F1 and exact-match rate to the report.
    ///
    /// Computes 95% percentile bootstrap CIs using `n_resamples` draws.
    /// Call this after all questions have been collected, before publishing
    /// results. Returns the same report unchanged if fewer than 2 questions
    /// are present (CI requires n ≥ 2).
    ///
    /// # Provenance
    ///
    /// Absorbed from `shared/stats.py` in the quantified-self pipeline.
    /// Reference: Efron & Hastie (2021) *Computer Age Statistical Inference*.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10K); f64 handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn with_statistics(mut self, n_resamples: usize) -> Self {
        use crate::stats::bootstrap::bootstrap_ci;

        if self.questions.len() < 2 {
            return self;
        }
        let f1_scores: Vec<f64> = self.questions.iter().map(|q| q.score.f1).collect();
        let em_scores: Vec<f64> = self
            .questions
            .iter()
            .map(|q| if q.score.exact_match { 1.0 } else { 0.0 })
            .collect();

        let mean_fn = |data: &[f64]| {
            if data.is_empty() {
                0.0
            } else {
                data.iter().sum::<f64>() / data.len() as f64 // SAFETY: bootstrap resample count; small usize within f64 mantissa
            }
        };

        let f1_ci = bootstrap_ci(&f1_scores, mean_fn, n_resamples, 42, 0.95);
        let em_ci = bootstrap_ci(&em_scores, mean_fn, n_resamples, 42, 0.95);

        if let (Ok(f1), Ok(em)) = (f1_ci, em_ci) {
            self.statistics = Some(BenchmarkStatistics {
                f1_ci_low: f1.ci_low,
                f1_ci_high: f1.ci_high,
                em_ci_low: em.ci_low,
                em_ci_high: em.ci_high,
                n_resamples: f1.n_resamples,
                method: "percentile bootstrap (Efron & Hastie 2021)".to_owned(),
            });
        }
        self
    }

    /// Fraction of questions with exact-match score >= 1.0.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn exact_match_rate(&self) -> f64 {
        if self.questions.is_empty() {
            return 0.0;
        }
        let hits = self
            .questions
            .iter()
            .filter(|q| q.score.exact_match)
            .count();
        hits as f64 / self.questions.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean F1 score across all questions.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn mean_f1(&self) -> f64 {
        if self.questions.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.questions.iter().map(|q| q.score.f1).sum();
        sum / self.questions.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean LLM-as-judge accuracy across all questions that have a judge score.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn judge_accuracy(&self) -> Option<f64> {
        let scored: Vec<_> = self
            .questions
            .iter()
            .filter_map(|q| q.judge_score.as_ref())
            .collect();
        if scored.is_empty() {
            return None;
        }
        let correct = scored.iter().filter(|j| j.correct).count() as f64; // SAFETY: question counts <10_000 per function-level #[expect]
        Some(correct / scored.len() as f64) // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean Recall@k across all questions that have retrieval metrics.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "value counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — value counts are bounded and small"
    )]
    pub fn mean_recall_at_k(&self) -> Option<f64> {
        let values: Vec<f64> = self
            .questions
            .iter()
            .filter_map(|q| q.recall_at_k)
            .collect();
        if values.is_empty() {
            return None;
        }
        Some(values.iter().sum::<f64>() / values.len() as f64) // SAFETY: value counts <10_000 per function-level #[expect]
    }

    /// Mean NDCG@k across all questions that have retrieval metrics.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "value counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — value counts are bounded and small"
    )]
    pub fn mean_ndcg_at_k(&self) -> Option<f64> {
        let values: Vec<f64> = self.questions.iter().filter_map(|q| q.ndcg_at_k).collect();
        if values.is_empty() {
            return None;
        }
        Some(values.iter().sum::<f64>() / values.len() as f64) // SAFETY: value counts <10_000 per function-level #[expect]
    }

    /// Group questions by category and return a (category, `exact_match_rate`, f1) tuple per category.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "bucket counts are small; f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — bucket counts are bounded and small"
    )]
    pub fn per_category(&self) -> Vec<(String, f64, f64)> {
        use std::collections::BTreeMap;
        let mut buckets: BTreeMap<String, Vec<&QuestionResult>> = BTreeMap::new();
        for q in &self.questions {
            buckets.entry(q.category.clone()).or_default().push(q);
        }
        buckets
            .into_iter()
            .map(|(cat, results)| {
                let total = results.len() as f64; // SAFETY: bucket counts small per function-level #[expect]
                let em = results.iter().filter(|r| r.score.exact_match).count() as f64; // SAFETY: bucket counts small per function-level #[expect]
                let f1_sum: f64 = results.iter().map(|r| r.score.f1).sum();
                (cat, em / total, f1_sum / total)
            })
            .collect()
    }
}

/// Dataset download instructions (not wired to any downloader, just for docs).
#[must_use]
pub fn download_instructions() -> &'static str {
    "Benchmark datasets are not committed to this repo. Download separately:

LongMemEval:
    https://github.com/xiaowu0162/LongMemEval
    (LongMemEval-M is ~115k token histories, 500 questions)

LoCoMo:
    https://github.com/snap-research/locomo
    (50 conversations with ~200 QA each, ~27 sessions per conversation)

HaluMem:
    https://arxiv.org/abs/2511.03506 (data release pending)

Place JSON files under benchmark-data/ (gitignored) and point the runner
at them via --dataset."
}

/// Load a `LongMemEval` dataset from a JSON file on disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is not in the
/// expected `LongMemEval` format.
pub async fn load_longmemeval(
    path: impl AsRef<Path> + Send,
) -> std::io::Result<longmemeval::LongMemEvalDataset> {
    longmemeval::LongMemEvalDataset::from_path(path).await
}

/// Load a `LoCoMo` dataset from a JSON file on disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is not in the
/// expected `LoCoMo` format.
pub async fn load_locomo(path: impl AsRef<Path> + Send) -> std::io::Result<locomo::LocomoDataset> {
    locomo::LocomoDataset::from_path(path).await
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "tests assert against known-length vectors"
)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

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
            QuestionResult {
                id: "q1".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "blue".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                score: BenchmarkScore {
                    exact_match: true,
                    f1: 1.0,
                    contains: true,
                },
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
            QuestionResult {
                id: "q2".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "green".to_owned(),
                expected_answers: vec!["red".to_owned()],
                score: BenchmarkScore {
                    exact_match: false,
                    f1: 0.0,
                    contains: false,
                },
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
        ];
        let report = BenchmarkReport::new("Test", questions);
        assert_eq!(report.total, 2);
        assert!((report.exact_match_rate() - 0.5).abs() < f64::EPSILON);
        assert!((report.mean_f1() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn per_category_groups_results() {
        let questions = vec![
            QuestionResult {
                id: "q1".to_owned(),
                category: "temporal".to_owned(),
                actual_answer: "yes".to_owned(),
                expected_answers: vec!["yes".to_owned()],
                score: BenchmarkScore {
                    exact_match: true,
                    f1: 1.0,
                    contains: true,
                },
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
            QuestionResult {
                id: "q2".to_owned(),
                category: "temporal".to_owned(),
                actual_answer: "no".to_owned(),
                expected_answers: vec!["yes".to_owned()],
                score: BenchmarkScore {
                    exact_match: false,
                    f1: 0.0,
                    contains: false,
                },
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
            QuestionResult {
                id: "q3".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "42".to_owned(),
                expected_answers: vec!["42".to_owned()],
                score: BenchmarkScore {
                    exact_match: true,
                    f1: 1.0,
                    contains: true,
                },
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
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
    fn judge_accuracy_computed_correctly() {
        let questions = vec![
            QuestionResult {
                id: "q1".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "blue".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                score: BenchmarkScore::zero(),
                judge_score: Some(judge::JudgeScore {
                    correct: true,
                    reasoning: "ok".to_owned(),
                }),
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
            QuestionResult {
                id: "q2".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "red".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                score: BenchmarkScore::zero(),
                judge_score: Some(judge::JudgeScore {
                    correct: false,
                    reasoning: "wrong".to_owned(),
                }),
                retrieved_facts: None,
                recall_at_k: None,
                ndcg_at_k: None,
            },
        ];
        let report = BenchmarkReport::new("Test", questions);
        assert!((report.judge_accuracy().unwrap() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn judge_accuracy_none_when_no_scores() {
        let questions = vec![QuestionResult {
            id: "q1".to_owned(),
            category: "factual".to_owned(),
            actual_answer: "blue".to_owned(),
            expected_answers: vec!["blue".to_owned()],
            score: BenchmarkScore::zero(),
            judge_score: None,
            retrieved_facts: None,
            recall_at_k: None,
            ndcg_at_k: None,
        }];
        let report = BenchmarkReport::new("Test", questions);
        assert!(report.judge_accuracy().is_none());
    }

    #[test]
    fn mean_recall_and_ndcg_computed_correctly() {
        let questions = vec![
            QuestionResult {
                id: "q1".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "blue".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                score: BenchmarkScore::zero(),
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: Some(1.0),
                ndcg_at_k: Some(1.0),
            },
            QuestionResult {
                id: "q2".to_owned(),
                category: "factual".to_owned(),
                actual_answer: "red".to_owned(),
                expected_answers: vec!["blue".to_owned()],
                score: BenchmarkScore::zero(),
                judge_score: None,
                retrieved_facts: None,
                recall_at_k: Some(0.0),
                ndcg_at_k: Some(0.0),
            },
        ];
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
        };
        let report = BenchmarkReport::with_metadata("LongMemEval", vec![], meta);
        let json = serde_json::to_string(&report).expect("serialize");
        let deserialized: BenchmarkReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.metadata, report.metadata);
    }

    #[test]
    fn download_instructions_mentions_datasets() {
        let instructions = download_instructions();
        assert!(instructions.contains("LongMemEval"));
        assert!(instructions.contains("LoCoMo"));
    }
}
