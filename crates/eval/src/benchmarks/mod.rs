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
/// Dataset validation primitives shared by benchmark loaders.
pub mod validation;

pub use self::runner::{BenchmarkMode, BenchmarkRunner, BenchmarkRunnerConfig};

/// Re-export of [`EvalClient`](crate::client::EvalClient) for external use.
///
/// External consumers of the benchmark runner need this to construct a
/// runner. The rest of the client API surface is not stable.
pub type EvalClient = crate::client::EvalClient;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::provenance::EvalProvenance;

pub use self::metrics::{BenchmarkScore, score_answer};
pub use self::validation::{
    BenchmarkValidationIssue, BenchmarkValidationOptions, BenchmarkValidationReport,
};

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
    /// Expected evidence or fact references supplied by the source dataset.
    pub expected_evidence_refs: Vec<String>,
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
    /// SHA-256 hash of the dataset file, when the runner can read it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_hash: Option<String>,
    /// Git SHA of the build or invocation, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub git_sha: Option<String>,
    /// Whether malformed or incomplete dataset records were allowed.
    #[serde(default)]
    pub dataset_best_effort: bool,
    /// Dataset validation diagnostics captured before execution.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_validation: Option<BenchmarkValidationReport>,
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
            dataset_hash: None,
            git_sha: None,
            dataset_best_effort: false,
            dataset_validation: None,
        }
    }
}

/// Per-question execution status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QuestionStatus {
    /// The question produced an answer and was included in scored-only diagnostics.
    #[default]
    Scored,
    /// The benchmark pipeline failed before a scorable answer was available,
    /// outside the transcript ingestion path.
    Error,
    /// Transcript ingestion failed before the benchmark question was asked.
    IngestionError,
    /// The benchmark question exceeded its configured timeout.
    Timeout,
    /// The model returned an empty answer.
    NoAnswer,
}

impl QuestionStatus {
    /// Whether this status should be included in correctness denominators.
    #[must_use]
    pub fn is_scored(self) -> bool {
        matches!(self, Self::Scored)
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
    /// Execution status for this question.
    #[serde(default)]
    pub status: QuestionStatus,
    /// Error or timeout detail when the question was not scorable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error_message: Option<String>,
    /// The answer produced by aletheia.
    pub actual_answer: String,
    /// The expected answers (ground truth, may have multiple valid forms).
    pub expected_answers: Vec<String>,
    /// Expected evidence/fact references from the dataset, when available.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub expected_evidence_refs: Vec<String>,
    /// Best score across all expected answers.
    pub score: BenchmarkScore,
    /// Facts inserted by transcript ingestion before asking the question.
    #[serde(default, skip_serializing_if = "is_default")]
    pub ingestion_inserted_facts: usize,
    /// Facts skipped by transcript ingestion before asking the question.
    #[serde(default, skip_serializing_if = "is_default")]
    pub ingestion_skipped_facts: usize,
    /// Empty source turns filtered out before transcript ingestion.
    #[serde(default, skip_serializing_if = "is_default")]
    pub filtered_turns: usize,
    /// Optional LLM-as-judge score (populated when judge is configured).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_score: Option<judge::JudgeScore>,
    /// Optional retrieval metrics: facts retrieved for the question.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieved_facts: Option<Vec<RetrievedFact>>,
    /// Retrieval scoring basis and relevant refs used for metrics.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retrieval_scoring: Option<RetrievalScoring>,
    /// Optional retrieval metric: Recall@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recall_at_k: Option<f64>,
    /// Optional retrieval metric: NDCG@k.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ndcg_at_k: Option<f64>,
}

/// One retrieved fact serialized with retrieval metric provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedFact {
    /// Fact ID returned by the knowledge API, when present.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    /// Stable display reference for this retrieved fact.
    pub reference: String,
    /// Knowledge API relevance score.
    pub score: f64,
    /// Stored fact confidence.
    pub confidence: f64,
    /// SHA-256 hash of the fact content.
    pub content_sha256: String,
}

/// Retrieval relevance basis used for a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RetrievalScoringMode {
    /// Dataset evidence/fact refs were used as the relevance set.
    EvidenceId,
    /// Dataset lacks evidence refs; normalized content hashes were used.
    NormalizedContent,
}

/// Retrieval scoring metadata for a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalScoring {
    /// Relevance basis used for Recall@k and NDCG@k.
    pub mode: RetrievalScoringMode,
    /// Whether the normalized-content fallback was used.
    pub fallback_used: bool,
    /// Relevant refs compared against retrieved facts.
    pub relevant_refs: Vec<String>,
}

/// Aggregate report for a benchmark run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Benchmark name.
    pub benchmark: String,
    /// Total questions attempted.
    pub total: usize,
    /// Questions that produced non-empty answers.
    pub scored: usize,
    /// Questions that failed before producing a scorable answer, excluding ingestion failures.
    pub errors: usize,
    /// Questions whose transcript ingestion failed before the question was asked.
    #[serde(default)]
    pub ingestion_errors: usize,
    /// Questions that exceeded the per-question timeout.
    pub timeouts: usize,
    /// Questions that returned an empty answer.
    pub no_answers: usize,
    /// Facts inserted by transcript ingestion across all attempted questions.
    #[serde(default)]
    pub ingestion_inserted_facts: usize,
    /// Facts skipped by transcript ingestion across all attempted questions.
    #[serde(default)]
    pub ingestion_skipped_facts: usize,
    /// Empty source turns filtered out across all attempted questions.
    #[serde(default)]
    pub filtered_turns: usize,
    /// Aggregate answer and reliability metrics with explicit denominators.
    #[serde(default)]
    pub metrics: BenchmarkMetrics,
    /// Optional reliability gate evaluation used by publishing commands.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reliability_gate: Option<BenchmarkReliabilityGate>,
    /// LLM judge denominator summary, when judge scoring was attempted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub judge_summary: Option<JudgeSummary>,
    /// Per-question results.
    pub questions: Vec<QuestionResult>,
    /// Shared provenance envelope for this benchmark run.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provenance: Option<EvalProvenance>,
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

/// Aggregate benchmark metrics with denominators made explicit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMetrics {
    /// Denominator for primary attempted-question answer metrics.
    pub attempted_denominator: usize,
    /// Denominator for secondary scored-only answer diagnostics.
    pub scored_denominator: usize,
    /// Primary exact-match rate over every attempted question.
    pub attempted_exact_match_rate: f64,
    /// Primary mean F1 over every attempted question.
    pub attempted_mean_f1: f64,
    /// Secondary exact-match diagnostic over questions with non-empty answers.
    pub scored_exact_match_rate: f64,
    /// Secondary mean-F1 diagnostic over questions with non-empty answers.
    pub scored_mean_f1: f64,
    /// Fraction of attempted questions that failed outside transcript ingestion.
    pub error_rate: f64,
    /// Fraction of attempted questions whose transcript ingestion failed.
    pub ingestion_error_rate: f64,
    /// Fraction of attempted questions that timed out.
    pub timeout_rate: f64,
    /// Fraction of attempted questions that returned an empty answer.
    pub no_answer_rate: f64,
}

/// Per-category metrics with primary attempted denominators and scored-only diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkCategoryMetrics {
    /// Category label from the benchmark dataset.
    pub category: String,
    /// Attempted questions in this category.
    pub attempted: usize,
    /// Questions in this category that produced non-empty answers.
    pub scored: usize,
    /// Primary exact-match rate over every attempted question in this category.
    pub attempted_exact_match_rate: f64,
    /// Primary mean F1 over every attempted question in this category.
    pub attempted_mean_f1: f64,
    /// Secondary exact-match diagnostic over scored questions in this category.
    pub scored_exact_match_rate: f64,
    /// Secondary mean-F1 diagnostic over scored questions in this category.
    pub scored_mean_f1: f64,
}

/// Reliability ceilings used to gate benchmark publication.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReliabilityThresholds {
    /// Maximum allowed generic error rate.
    pub max_error_rate: f64,
    /// Maximum allowed transcript-ingestion error rate.
    pub max_ingestion_error_rate: f64,
    /// Maximum allowed timeout rate.
    pub max_timeout_rate: f64,
    /// Maximum allowed no-answer rate.
    pub max_no_answer_rate: f64,
}

impl BenchmarkReliabilityThresholds {
    /// Strict publishing gate: no failed attempts allowed.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            max_error_rate: 0.0,
            max_ingestion_error_rate: 0.0,
            max_timeout_rate: 0.0,
            max_no_answer_rate: 0.0,
        }
    }

    /// Evaluate this gate against a benchmark report.
    #[must_use]
    pub fn evaluate(self, report: &BenchmarkReport) -> BenchmarkReliabilityGate {
        BenchmarkReliabilityGate {
            passed: report.error_rate() <= self.max_error_rate
                && report.ingestion_error_rate() <= self.max_ingestion_error_rate
                && report.timeout_rate() <= self.max_timeout_rate
                && report.no_answer_rate() <= self.max_no_answer_rate,
            thresholds: self,
            error_rate: report.error_rate(),
            ingestion_error_rate: report.ingestion_error_rate(),
            timeout_rate: report.timeout_rate(),
            no_answer_rate: report.no_answer_rate(),
        }
    }
}

impl Default for BenchmarkReliabilityThresholds {
    fn default() -> Self {
        Self::strict()
    }
}

/// Result of evaluating reliability gates for a benchmark run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReliabilityGate {
    /// Whether every observed reliability rate was within its threshold.
    pub passed: bool,
    /// Configured reliability ceilings.
    pub thresholds: BenchmarkReliabilityThresholds,
    /// Observed generic error rate.
    pub error_rate: f64,
    /// Observed transcript-ingestion error rate.
    pub ingestion_error_rate: f64,
    /// Observed timeout rate.
    pub timeout_rate: f64,
    /// Observed no-answer rate.
    pub no_answer_rate: f64,
}

/// Aggregate denominator semantics for LLM-as-judge scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeSummary {
    /// Questions for which judge scoring was attempted.
    pub attempted: usize,
    /// Judge attempts that returned a parsed judgment.
    pub scored: usize,
    /// Judge attempts that failed, timed out, refused, or returned malformed data.
    pub errors: usize,
    /// Parsed judge judgments marked correct.
    pub correct: usize,
}

/// Statistical summary for a benchmark run.
///
/// Carries the key aggregate metrics with 95% bootstrap CIs, absorbed from
/// the quantified-self pipeline's statistical discipline. These numbers make
/// results honest: a point estimate without a CI is not publishable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStatistics {
    /// Denominator for the primary attempted-question CIs.
    #[serde(default)]
    pub attempted_denominator: usize,
    /// Denominator for secondary scored-only diagnostic CIs.
    #[serde(default)]
    pub scored_denominator: usize,
    /// 95% bootstrap CI lower bound for primary attempted-question mean F1.
    pub f1_ci_low: f64,
    /// 95% bootstrap CI upper bound for primary attempted-question mean F1.
    pub f1_ci_high: f64,
    /// 95% bootstrap CI lower bound for primary attempted-question exact-match rate.
    pub em_ci_low: f64,
    /// 95% bootstrap CI upper bound for primary attempted-question exact-match rate.
    pub em_ci_high: f64,
    /// 95% bootstrap CI lower bound for scored-only mean F1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scored_f1_ci_low: Option<f64>,
    /// 95% bootstrap CI upper bound for scored-only mean F1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scored_f1_ci_high: Option<f64>,
    /// 95% bootstrap CI lower bound for scored-only exact-match rate.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scored_em_ci_low: Option<f64>,
    /// 95% bootstrap CI upper bound for scored-only exact-match rate.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scored_em_ci_high: Option<f64>,
    /// Number of bootstrap resamples used to compute CI.
    pub n_resamples: usize,
    /// Tool + version string for provenance.
    pub method: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct BenchmarkCounts {
    scored: usize,
    errors: usize,
    ingestion_errors: usize,
    timeouts: usize,
    no_answers: usize,
    ingestion_inserted_facts: usize,
    ingestion_skipped_facts: usize,
    filtered_turns: usize,
}

impl BenchmarkCounts {
    fn from_questions(questions: &[QuestionResult]) -> Self {
        let mut counts = Self::default();
        for question in questions {
            match question.status {
                QuestionStatus::Scored => counts.scored += 1,
                QuestionStatus::Error => counts.errors += 1,
                QuestionStatus::IngestionError => counts.ingestion_errors += 1,
                QuestionStatus::Timeout => counts.timeouts += 1,
                QuestionStatus::NoAnswer => counts.no_answers += 1,
            }
            counts.ingestion_inserted_facts += question.ingestion_inserted_facts;
            counts.ingestion_skipped_facts += question.ingestion_skipped_facts;
            counts.filtered_turns += question.filtered_turns;
        }
        counts
    }
}

impl BenchmarkMetrics {
    fn from_questions(questions: &[QuestionResult], counts: BenchmarkCounts) -> Self {
        let attempted_denominator = questions.len();
        let scored_denominator = counts.scored;
        Self {
            attempted_denominator,
            scored_denominator,
            attempted_exact_match_rate: exact_match_rate_for_questions(
                questions,
                attempted_denominator,
            ),
            attempted_mean_f1: mean_f1_for_questions(questions, attempted_denominator),
            scored_exact_match_rate: scored_exact_match_rate_for_questions(questions),
            scored_mean_f1: scored_mean_f1_for_questions(questions),
            error_rate: fraction(counts.errors, attempted_denominator),
            ingestion_error_rate: fraction(counts.ingestion_errors, attempted_denominator),
            timeout_rate: fraction(counts.timeouts, attempted_denominator),
            no_answer_rate: fraction(counts.no_answers, attempted_denominator),
        }
    }
}

impl JudgeSummary {
    fn from_questions(questions: &[QuestionResult]) -> Option<Self> {
        let mut summary = Self {
            attempted: 0,
            scored: 0,
            errors: 0,
            correct: 0,
        };
        for score in questions.iter().filter_map(|q| q.judge_score.as_ref()) {
            summary.attempted += 1;
            if score.status.is_scored() {
                summary.scored += 1;
                if score.correct {
                    summary.correct += 1;
                }
            } else {
                summary.errors += 1;
            }
        }
        (summary.attempted > 0).then_some(summary)
    }
}

impl BenchmarkReport {
    /// Build an aggregate report from individual question results.
    #[must_use]
    pub fn new(benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self {
        let counts = BenchmarkCounts::from_questions(&questions);
        Self {
            benchmark: benchmark.into(),
            total: questions.len(),
            scored: counts.scored,
            errors: counts.errors,
            ingestion_errors: counts.ingestion_errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            ingestion_inserted_facts: counts.ingestion_inserted_facts,
            ingestion_skipped_facts: counts.ingestion_skipped_facts,
            filtered_turns: counts.filtered_turns,
            metrics: BenchmarkMetrics::from_questions(&questions, counts),
            reliability_gate: None,
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
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
        let counts = BenchmarkCounts::from_questions(&questions);
        Self {
            benchmark: benchmark.into(),
            total: questions.len(),
            scored: counts.scored,
            errors: counts.errors,
            ingestion_errors: counts.ingestion_errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            ingestion_inserted_facts: counts.ingestion_inserted_facts,
            ingestion_skipped_facts: counts.ingestion_skipped_facts,
            filtered_turns: counts.filtered_turns,
            metrics: BenchmarkMetrics::from_questions(&questions, counts),
            reliability_gate: None,
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
            metadata: Some(metadata),
            statistics: None,
        }
    }

    /// Attach the shared provenance envelope for this benchmark report.
    #[must_use]
    pub fn with_provenance(mut self, provenance: EvalProvenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Attach a reliability gate evaluation for publishing workflows.
    #[must_use]
    pub fn with_reliability_gate(mut self, thresholds: BenchmarkReliabilityThresholds) -> Self {
        self.reliability_gate = Some(thresholds.evaluate(&self));
        self
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
        let f1_scores: Vec<f64> = self.questions.iter().map(attempted_f1).collect();
        let em_scores: Vec<f64> = self
            .questions
            .iter()
            .map(|q| if attempted_exact_match(q) { 1.0 } else { 0.0 })
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
            let scored = self.scored_questions();
            let (scored_f1_ci_low, scored_f1_ci_high, scored_em_ci_low, scored_em_ci_high) =
                if scored.len() < 2 {
                    (None, None, None, None)
                } else {
                    let scored_f1_scores: Vec<f64> = scored.iter().map(|q| q.score.f1).collect();
                    let scored_em_scores: Vec<f64> = scored
                        .iter()
                        .map(|q| if q.score.exact_match { 1.0 } else { 0.0 })
                        .collect();
                    let scored_f1 = bootstrap_ci(&scored_f1_scores, mean_fn, n_resamples, 42, 0.95);
                    let scored_em = bootstrap_ci(&scored_em_scores, mean_fn, n_resamples, 42, 0.95);
                    if let (Ok(scored_f1), Ok(scored_em)) = (scored_f1, scored_em) {
                        (
                            Some(scored_f1.ci_low),
                            Some(scored_f1.ci_high),
                            Some(scored_em.ci_low),
                            Some(scored_em.ci_high),
                        )
                    } else {
                        (None, None, None, None)
                    }
                };
            self.statistics = Some(BenchmarkStatistics {
                attempted_denominator: self.total,
                scored_denominator: self.scored,
                f1_ci_low: f1.ci_low,
                f1_ci_high: f1.ci_high,
                em_ci_low: em.ci_low,
                em_ci_high: em.ci_high,
                scored_f1_ci_low,
                scored_f1_ci_high,
                scored_em_ci_low,
                scored_em_ci_high,
                n_resamples: f1.n_resamples,
                method: "percentile bootstrap (Efron & Hastie 2021)".to_owned(),
            });
        }
        self
    }

    fn scored_questions(&self) -> Vec<&QuestionResult> {
        self.questions
            .iter()
            .filter(|q| q.status.is_scored())
            .collect()
    }

    /// Primary exact-match rate over every attempted question.
    #[must_use]
    pub fn exact_match_rate(&self) -> f64 {
        self.metrics.attempted_exact_match_rate
    }

    /// Primary mean F1 over every attempted question.
    #[must_use]
    pub fn mean_f1(&self) -> f64 {
        self.metrics.attempted_mean_f1
    }

    /// Secondary exact-match diagnostic over questions with non-empty answers.
    #[must_use]
    pub fn scored_exact_match_rate(&self) -> f64 {
        self.metrics.scored_exact_match_rate
    }

    /// Secondary mean-F1 diagnostic over questions with non-empty answers.
    #[must_use]
    pub fn scored_mean_f1(&self) -> f64 {
        self.metrics.scored_mean_f1
    }

    /// Fraction of attempted questions that failed outside transcript ingestion.
    #[must_use]
    pub fn error_rate(&self) -> f64 {
        self.metrics.error_rate
    }

    /// Fraction of attempted questions whose transcript ingestion failed.
    #[must_use]
    pub fn ingestion_error_rate(&self) -> f64 {
        self.metrics.ingestion_error_rate
    }

    /// Fraction of attempted questions that exceeded the per-question timeout.
    #[must_use]
    pub fn timeout_rate(&self) -> f64 {
        self.metrics.timeout_rate
    }

    /// Fraction of attempted questions that returned an empty answer.
    #[must_use]
    pub fn no_answer_rate(&self) -> f64 {
        self.metrics.no_answer_rate
    }

    /// Mean LLM-as-judge accuracy across all attempted judge calls.
    ///
    /// Judge errors stay in the denominator and count as incorrect. Use
    /// [`BenchmarkReport::judge_summary`] to distinguish parsed judgments from
    /// failed/refused/malformed judge attempts.
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
        let summary = self.judge_summary?;
        Some(summary.correct as f64 / summary.attempted as f64) // SAFETY: question counts <10_000 per function-level #[expect]
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

    /// Group questions by category with explicit attempted and scored-only denominators.
    #[must_use]
    pub fn per_category(&self) -> Vec<BenchmarkCategoryMetrics> {
        use std::collections::BTreeMap;
        let mut buckets: BTreeMap<String, Vec<&QuestionResult>> = BTreeMap::new();
        for q in &self.questions {
            buckets.entry(q.category.clone()).or_default().push(q);
        }
        buckets
            .into_iter()
            .map(|(cat, results)| {
                let attempted = results.len();
                let scored = results.iter().filter(|q| q.status.is_scored()).count();
                BenchmarkCategoryMetrics {
                    category: cat,
                    attempted,
                    scored,
                    attempted_exact_match_rate: exact_match_rate_for_refs(&results, attempted),
                    attempted_mean_f1: mean_f1_for_refs(&results, attempted),
                    scored_exact_match_rate: scored_exact_match_rate_for_refs(&results),
                    scored_mean_f1: scored_mean_f1_for_refs(&results),
                }
            })
            .collect()
    }
}

fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}

fn attempted_exact_match(question: &QuestionResult) -> bool {
    question.status.is_scored() && question.score.exact_match
}

fn attempted_f1(question: &QuestionResult) -> f64 {
    if question.status.is_scored() {
        question.score.f1
    } else {
        0.0
    }
}

fn exact_match_rate_for_questions(questions: &[QuestionResult], denominator: usize) -> f64 {
    let hits = questions
        .iter()
        .filter(|question| attempted_exact_match(question))
        .count();
    fraction(hits, denominator)
}

fn mean_f1_for_questions(questions: &[QuestionResult], denominator: usize) -> f64 {
    mean(questions.iter().map(attempted_f1).sum(), denominator)
}

fn scored_exact_match_rate_for_questions(questions: &[QuestionResult]) -> f64 {
    let scored = questions
        .iter()
        .filter(|question| question.status.is_scored())
        .collect::<Vec<_>>();
    scored_exact_match_rate_for_refs(&scored)
}

fn scored_mean_f1_for_questions(questions: &[QuestionResult]) -> f64 {
    let scored = questions
        .iter()
        .filter(|question| question.status.is_scored())
        .collect::<Vec<_>>();
    scored_mean_f1_for_refs(&scored)
}

fn exact_match_rate_for_refs(questions: &[&QuestionResult], denominator: usize) -> f64 {
    let hits = questions
        .iter()
        .filter(|question| attempted_exact_match(question))
        .count();
    fraction(hits, denominator)
}

fn mean_f1_for_refs(questions: &[&QuestionResult], denominator: usize) -> f64 {
    mean(
        questions
            .iter()
            .map(|question| attempted_f1(question))
            .sum(),
        denominator,
    )
}

fn scored_exact_match_rate_for_refs(questions: &[&QuestionResult]) -> f64 {
    let scored = questions
        .iter()
        .filter(|question| question.status.is_scored())
        .collect::<Vec<_>>();
    let hits = scored
        .iter()
        .filter(|question| question.score.exact_match)
        .count();
    fraction(hits, scored.len())
}

fn scored_mean_f1_for_refs(questions: &[&QuestionResult]) -> f64 {
    let scored = questions
        .iter()
        .filter(|question| question.status.is_scored())
        .collect::<Vec<_>>();
    mean(
        scored.iter().map(|question| question.score.f1).sum(),
        scored.len(),
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "question counts are small (<10000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — question counts are bounded and small"
)]
fn fraction(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "question counts are small (<10000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — question counts are bounded and small"
)]
fn mean(sum: f64, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    sum / denominator as f64
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

/// Load and validate a `LongMemEval` dataset from a JSON file on disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or validated.
pub async fn load_longmemeval_with_options(
    path: impl AsRef<Path> + Send,
    options: BenchmarkValidationOptions,
) -> std::io::Result<(longmemeval::LongMemEvalDataset, BenchmarkValidationReport)> {
    longmemeval::LongMemEvalDataset::from_path_with_options(path, options).await
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

/// Load and validate a `LoCoMo` dataset from a JSON file on disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or validated.
pub async fn load_locomo_with_options(
    path: impl AsRef<Path> + Send,
    options: BenchmarkValidationOptions,
) -> std::io::Result<(locomo::LocomoDataset, BenchmarkValidationReport)> {
    locomo::LocomoDataset::from_path_with_options(path, options).await
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
            ingestion_inserted_facts: 0,
            ingestion_skipped_facts: 0,
            filtered_turns: 0,
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
        assert_eq!(per_cat[0].category, "factual");
        assert_eq!(per_cat[0].attempted, 1);
        assert!((per_cat[0].attempted_exact_match_rate - 1.0).abs() < f64::EPSILON);
        assert_eq!(per_cat[1].category, "temporal");
        assert_eq!(per_cat[1].attempted, 2);
        assert!((per_cat[1].attempted_exact_match_rate - 0.5).abs() < f64::EPSILON);
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
    fn non_scored_statuses_count_as_zero_in_attempted_denominator() {
        let mut timeout = result("q2", "factual", "", &["red"]);
        timeout.status = QuestionStatus::Timeout;
        timeout.error_message = Some("timed out".to_owned());
        let questions = vec![result("q1", "factual", "blue", &["blue"]), timeout];
        let report = BenchmarkReport::new("Test", questions);
        assert_eq!(report.total, 2);
        assert_eq!(report.scored, 1);
        assert_eq!(report.timeouts, 1);
        assert_eq!(report.metrics.attempted_denominator, 2);
        assert_eq!(report.metrics.scored_denominator, 1);
        assert!((report.exact_match_rate() - 0.5).abs() < f64::EPSILON);
        assert!((report.mean_f1() - 0.5).abs() < f64::EPSILON);
        assert!((report.scored_exact_match_rate() - 1.0).abs() < f64::EPSILON);
        assert!((report.scored_mean_f1() - 1.0).abs() < f64::EPSILON);
        assert!((report.timeout_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn report_tracks_ingestion_and_filtered_turn_reliability() {
        let mut ingestion = result("q2", "factual", "", &["red"]);
        ingestion.status = QuestionStatus::IngestionError;
        ingestion.error_message = Some("transcript ingestion failed".to_owned());
        ingestion.ingestion_inserted_facts = 3;
        ingestion.ingestion_skipped_facts = 2;
        ingestion.filtered_turns = 1;
        let report = BenchmarkReport::new(
            "Test",
            vec![result("q1", "factual", "blue", &["blue"]), ingestion],
        )
        .with_reliability_gate(BenchmarkReliabilityThresholds {
            max_error_rate: 0.0,
            max_ingestion_error_rate: 0.25,
            max_timeout_rate: 0.0,
            max_no_answer_rate: 0.0,
        });

        assert_eq!(report.ingestion_errors, 1);
        assert_eq!(report.ingestion_inserted_facts, 3);
        assert_eq!(report.ingestion_skipped_facts, 2);
        assert_eq!(report.filtered_turns, 1);
        assert!((report.ingestion_error_rate() - 0.5).abs() < f64::EPSILON);
        assert_eq!(
            report.reliability_gate,
            Some(BenchmarkReliabilityGate {
                passed: false,
                thresholds: BenchmarkReliabilityThresholds {
                    max_error_rate: 0.0,
                    max_ingestion_error_rate: 0.25,
                    max_timeout_rate: 0.0,
                    max_no_answer_rate: 0.0,
                },
                error_rate: 0.0,
                ingestion_error_rate: 0.5,
                timeout_rate: 0.0,
                no_answer_rate: 0.0,
            })
        );
    }

    #[test]
    fn download_instructions_mentions_datasets() {
        let instructions = download_instructions();
        assert!(instructions.contains("LongMemEval"));
        assert!(instructions.contains("LoCoMo"));
    }
}
