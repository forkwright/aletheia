//! Benchmark report data types and serialized status records.

use serde::{Deserialize, Serialize};

use crate::provenance::EvalProvenance;

use super::{BenchmarkScore, BenchmarkValidationReport, judge};

/// Default bootstrap resample count used by benchmark report generation.
pub const BENCHMARK_STAT_RESAMPLES: usize = 2000;

/// Minimum scored question count required for publishable bootstrap CIs.
pub const MIN_PUBLISHABLE_SCORED_QUESTIONS: usize = 2;
/// Maximum generic error rate allowed for publishable benchmark reports.
pub const MAX_PUBLISHABLE_ERROR_RATE: f64 = 0.0;
/// Maximum timeout rate allowed for publishable benchmark reports.
pub const MAX_PUBLISHABLE_TIMEOUT_RATE: f64 = 0.0;
/// Maximum no-answer rate allowed for publishable benchmark reports.
pub const MAX_PUBLISHABLE_NO_ANSWER_RATE: f64 = 0.0;
/// Maximum complete ingestion-error rate allowed for publishable reports.
pub const MAX_PUBLISHABLE_INGESTION_ERROR_RATE: f64 = 0.0;
/// Maximum partial-ingestion rate allowed for publishable reports.
pub const MAX_PUBLISHABLE_INGESTION_PARTIAL_RATE: f64 = 0.0;

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
    /// The question produced an answer and was included in score denominators.
    #[default]
    Scored,
    /// Haystack ingestion failed before the question could be asked.
    IngestionError,
    /// Haystack ingestion inserted some context but also reported errors.
    IngestionPartial,
    /// The benchmark pipeline failed before a scorable answer was available.
    Error,
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
    /// Questions included in score denominators.
    pub scored: usize,
    /// Aggregate haystack ingestion counters for this run.
    #[serde(default)]
    pub ingestion_summary: BenchmarkIngestionSummary,
    /// Questions blocked by complete haystack ingestion failure.
    #[serde(default)]
    pub ingestion_errors: usize,
    /// Questions blocked by partial haystack ingestion.
    #[serde(default)]
    pub ingestion_partials: usize,
    /// Questions that failed before producing a scorable answer.
    pub errors: usize,
    /// Questions that exceeded the per-question timeout.
    pub timeouts: usize,
    /// Questions that returned an empty answer.
    pub no_answers: usize,
    /// Explicit reliability counts and rates for benchmark gates.
    #[serde(default)]
    pub reliability: BenchmarkReliabilitySummary,
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
    /// Baseline/candidate statistical comparisons, when requested.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub comparisons: Vec<BenchmarkComparisonReport>,
    /// Explicit assessment of whether this report is publishable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publishability: Option<BenchmarkPublishability>,
}

/// Aggregate haystack ingestion counters for a benchmark run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkIngestionSummary {
    /// Haystack ingestion requests attempted.
    pub attempts: usize,
    /// Haystack ingestion failures reported by request or per-fact errors.
    pub failures: usize,
    /// Facts inserted across haystack ingestion requests.
    pub inserted: usize,
    /// Facts skipped across haystack ingestion requests.
    pub skipped: usize,
}

/// Operational reliability counts and rates for a benchmark report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReliabilitySummary {
    /// Total attempted questions.
    pub attempted: usize,
    /// Questions that produced scored answers.
    pub scored: usize,
    /// Questions blocked by complete haystack ingestion failure.
    pub ingestion_errors: usize,
    /// Questions blocked by partial haystack ingestion.
    pub ingestion_partials: usize,
    /// Questions that failed before producing a scorable answer.
    pub errors: usize,
    /// Questions that exceeded the per-question timeout.
    pub timeouts: usize,
    /// Questions that returned an empty answer.
    pub no_answers: usize,
    /// Generic error rate over attempted questions.
    pub error_rate: f64,
    /// Timeout rate over attempted questions.
    pub timeout_rate: f64,
    /// Empty-answer rate over attempted questions.
    pub no_answer_rate: f64,
    /// Complete ingestion-error rate over attempted questions.
    pub ingestion_error_rate: f64,
    /// Partial-ingestion rate over attempted questions.
    pub ingestion_partial_rate: f64,
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

/// Publishability assessment for a benchmark report.
///
/// This is intentionally serialized next to point estimates so archived JSON
/// can distinguish "publishable with statistical context" from "exploratory
/// point estimates only".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkPublishability {
    /// Whether the report meets the benchmark publication requirements.
    pub publishable: bool,
    /// Minimum scored question count required for bootstrap CIs.
    pub minimum_scored_questions: usize,
    /// Reasons the report is not publishable. Empty when `publishable` is true.
    pub reasons: Vec<String>,
}

/// Metric used in a baseline/candidate benchmark comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BenchmarkComparisonMetric {
    /// Exact-match rate represented as per-question 0/1 scores.
    ExactMatch,
    /// Token-level F1 score.
    F1,
}

impl std::fmt::Display for BenchmarkComparisonMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExactMatch => write!(f, "exact_match"),
            Self::F1 => write!(f, "f1"),
        }
    }
}

impl BenchmarkComparisonMetric {
    pub(super) fn score(self, question: &QuestionResult) -> f64 {
        match self {
            Self::ExactMatch => {
                if question.score.exact_match {
                    1.0
                } else {
                    0.0
                }
            }
            Self::F1 => question.score.f1,
        }
    }
}

/// Completeness status for a baseline/candidate statistical comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BenchmarkComparisonStatus {
    /// Comparison contains CI, effect size, raw p-value, and FDR-adjusted p-value.
    Complete,
    /// Fewer than two matched attempted questions were available.
    InsufficientSamples,
    /// Reports could not be compared, for example because benchmark names differ.
    Incomparable,
    /// Statistical calculation failed even though inputs looked comparable.
    Error,
}

impl std::fmt::Display for BenchmarkComparisonStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete => write!(f, "complete"),
            Self::InsufficientSamples => write!(f, "insufficient_samples"),
            Self::Incomparable => write!(f, "incomparable"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Baseline/candidate comparison for one benchmark metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparisonReport {
    /// Metric being compared.
    pub metric: BenchmarkComparisonMetric,
    /// Human-readable comparison label.
    pub label: String,
    /// Whether the comparison is statistically complete.
    pub status: BenchmarkComparisonStatus,
    /// Number of attempted question ids present in both reports.
    pub matched_questions: usize,
    /// Full statistical comparison when available.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub statistics: Option<crate::stats::ComparisonReport>,
    /// Reason a complete comparison could not be produced.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
}

impl BenchmarkComparisonReport {
    pub(super) fn unavailable(
        metric: BenchmarkComparisonMetric,
        label: String,
        status: BenchmarkComparisonStatus,
        matched_questions: usize,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            metric,
            label,
            status,
            matched_questions,
            statistics: None,
            reason: Some(reason.into()),
        }
    }

    pub(super) fn complete(
        metric: BenchmarkComparisonMetric,
        label: String,
        matched_questions: usize,
        statistics: crate::stats::ComparisonReport,
    ) -> Self {
        Self {
            metric,
            label,
            status: BenchmarkComparisonStatus::Complete,
            matched_questions,
            statistics: Some(statistics),
            reason: None,
        }
    }
}
