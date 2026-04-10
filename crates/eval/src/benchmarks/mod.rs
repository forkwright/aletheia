//! Memory benchmark harness: `LongMemEval`, `LoCoMo`, `HaluMem` scoring against aletheia.
//!
//! Based on #2854. Each benchmark provides a standardized dataset of long
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
//! The memory pipeline has grown several research-backed features
//! (admission control #2853, staleness #2848, probe QA #2846, evidence gap
//! #2851, surprise #2852, anomaly detection #2847) — each with heuristic
//! implementations. Without a benchmark loop, every change is unmeasured and
//! may regress recall quality. The harness closes that loop.
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

/// Dataset loader + question iterator for the `LongMemEval` benchmark.
pub mod longmemeval;
/// Dataset loader + question iterator for the `LoCoMo` benchmark.
pub mod locomo;
/// Benchmark scoring (exact match, F1, contains).
pub mod metrics;

use std::path::Path;

pub use self::metrics::{BenchmarkScore, score_answer};

/// A single question/answer pair backed by prior conversation context.
#[derive(Debug, Clone)]
pub struct BenchmarkQuestion {
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

/// Result of scoring a single benchmark question.
#[derive(Debug, Clone)]
pub struct QuestionResult {
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
}

/// Aggregate report for a benchmark run.
#[derive(Debug, Clone, Default)]
pub struct BenchmarkReport {
    /// Benchmark name.
    pub benchmark: String,
    /// Total questions scored.
    pub total: usize,
    /// Per-question results.
    pub questions: Vec<QuestionResult>,
}

impl BenchmarkReport {
    /// Build an aggregate report from individual question results.
    #[must_use]
    pub fn new(benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self {
        Self {
            benchmark: benchmark.into(),
            total: questions.len(),
            questions,
        }
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
        hits as f64 / self.questions.len() as f64
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
        sum / self.questions.len() as f64
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
                let total = results.len() as f64;
                let em = results.iter().filter(|r| r.score.exact_match).count() as f64;
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
pub async fn load_locomo(
    path: impl AsRef<Path> + Send,
) -> std::io::Result<locomo::LocomoDataset> {
    locomo::LocomoDataset::from_path(path).await
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "tests assert against known-length vectors"
)]
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
    fn download_instructions_mentions_datasets() {
        let instructions = download_instructions();
        assert!(instructions.contains("LongMemEval"));
        assert!(instructions.contains("LoCoMo"));
    }
}
