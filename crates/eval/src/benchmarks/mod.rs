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

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::provenance::EvalProvenance;

pub use self::metrics::{BenchmarkScore, score_answer};
pub use self::validation::{
    BenchmarkValidationIssue, BenchmarkValidationOptions, BenchmarkValidationReport,
};

/// Default bootstrap resample count used by benchmark report generation.
pub const BENCHMARK_STAT_RESAMPLES: usize = 2000;

/// Minimum scored question count required for publishable bootstrap CIs.
pub const MIN_PUBLISHABLE_SCORED_QUESTIONS: usize = 2;

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
    fn score(self, question: &QuestionResult) -> f64 {
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
    fn unavailable(
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

    fn complete(
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

#[derive(Debug, Clone, Copy, Default)]
struct BenchmarkCounts {
    scored: usize,
    ingestion_errors: usize,
    ingestion_partials: usize,
    errors: usize,
    timeouts: usize,
    no_answers: usize,
}

impl BenchmarkCounts {
    fn from_questions(questions: &[QuestionResult]) -> Self {
        let mut counts = Self::default();
        for question in questions {
            match question.status {
                QuestionStatus::Scored => counts.scored += 1,
                QuestionStatus::IngestionError => counts.ingestion_errors += 1,
                QuestionStatus::IngestionPartial => counts.ingestion_partials += 1,
                QuestionStatus::Error => counts.errors += 1,
                QuestionStatus::Timeout => counts.timeouts += 1,
                QuestionStatus::NoAnswer => counts.no_answers += 1,
            }
        }
        counts
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
            ingestion_summary: BenchmarkIngestionSummary::default(),
            ingestion_errors: counts.ingestion_errors,
            ingestion_partials: counts.ingestion_partials,
            errors: counts.errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
            metadata: None,
            statistics: None,
            comparisons: Vec::new(),
            publishability: None,
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
            ingestion_summary: BenchmarkIngestionSummary::default(),
            ingestion_errors: counts.ingestion_errors,
            ingestion_partials: counts.ingestion_partials,
            errors: counts.errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
            metadata: Some(metadata),
            statistics: None,
            comparisons: Vec::new(),
            publishability: None,
        }
    }

    /// Attach the shared provenance envelope for this benchmark report.
    #[must_use]
    pub fn with_provenance(mut self, provenance: EvalProvenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Attach aggregate haystack ingestion counters to the report.
    #[must_use]
    pub(crate) fn with_ingestion_summary(mut self, summary: BenchmarkIngestionSummary) -> Self {
        self.ingestion_summary = summary;
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

    /// Attach the standard benchmark statistical summary and publishability assessment.
    #[must_use]
    pub fn with_standard_statistics(self) -> Self {
        self.with_statistics(BENCHMARK_STAT_RESAMPLES)
            .with_publishability_assessment()
    }

    /// Attach baseline/candidate comparisons against a prior full benchmark report.
    ///
    /// The current report is treated as the candidate and `baseline` as the
    /// baseline. Comparisons are matched by question id and include FDR
    /// correction across the F1 and exact-match metric comparisons.
    #[must_use]
    pub fn with_comparisons_against(mut self, baseline: &Self, label: impl Into<String>) -> Self {
        self.comparisons = self.comparison_reports_against(baseline, label);
        self.with_publishability_assessment()
    }

    /// Build baseline/candidate statistical comparisons without mutating the report.
    #[must_use]
    pub fn comparison_reports_against(
        &self,
        baseline: &Self,
        label: impl Into<String>,
    ) -> Vec<BenchmarkComparisonReport> {
        let label = label.into();
        if self.benchmark != baseline.benchmark {
            return [
                BenchmarkComparisonMetric::F1,
                BenchmarkComparisonMetric::ExactMatch,
            ]
            .into_iter()
            .map(|metric| {
                BenchmarkComparisonReport::unavailable(
                    metric,
                    format!("{label} {metric}"),
                    BenchmarkComparisonStatus::Incomparable,
                    0,
                    format!(
                        "benchmark mismatch: baseline={} candidate={}",
                        baseline.benchmark, self.benchmark
                    ),
                )
            })
            .collect();
        }

        let mut comparisons = [
            BenchmarkComparisonMetric::F1,
            BenchmarkComparisonMetric::ExactMatch,
        ]
        .into_iter()
        .map(|metric| self.comparison_for_metric(baseline, metric, &label))
        .collect::<Vec<_>>();

        let raw_p_values = comparisons
            .iter()
            .filter_map(|comparison| comparison.statistics.as_ref())
            .map(|statistics| statistics.p_raw)
            .filter(|p| p.is_finite())
            .collect::<Vec<_>>();

        if let Ok(adjusted) =
            crate::stats::fdr_correct(&raw_p_values, crate::stats::FdrMethod::BenjaminiHochberg)
        {
            let mut adjusted_iter = adjusted.into_iter();
            for comparison in &mut comparisons {
                if let Some(statistics) = comparison.statistics.as_mut()
                    && statistics.p_raw.is_finite()
                    && let Some(adjusted_p) = adjusted_iter.next()
                {
                    statistics.set_adjusted_p(adjusted_p);
                }
            }
        }

        comparisons
    }

    /// Attach an explicit publishability assessment.
    #[must_use]
    pub fn with_publishability_assessment(mut self) -> Self {
        self.publishability = Some(self.assess_publishability());
        self
    }

    /// Assess whether this report has enough statistical context for publication.
    #[must_use]
    pub fn assess_publishability(&self) -> BenchmarkPublishability {
        let mut reasons = Vec::new();
        if self.scored < MIN_PUBLISHABLE_SCORED_QUESTIONS {
            reasons.push(format!(
                "requires at least {MIN_PUBLISHABLE_SCORED_QUESTIONS} scored questions for bootstrap CIs; got {}",
                self.scored
            ));
        }
        if self.statistics.is_none() {
            reasons.push("missing bootstrap confidence intervals for EM and F1".to_owned());
        }

        match &self.provenance {
            Some(provenance) => {
                if provenance.config_hash.is_none() {
                    reasons.push("missing benchmark configuration hash".to_owned());
                }
                if provenance.scenario_suite_hash.is_none() {
                    reasons.push("missing dataset hash in provenance".to_owned());
                }
                if provenance.redacted_args.is_empty() {
                    reasons.push("missing redacted CLI provenance".to_owned());
                }
            }
            None => reasons.push("missing eval provenance".to_owned()),
        }

        match &self.metadata {
            Some(metadata) => {
                if metadata.dataset_hash.is_none() {
                    reasons.push("missing dataset hash in benchmark metadata".to_owned());
                }
                if let Some(validation) = &metadata.dataset_validation
                    && !validation.errors.is_empty()
                {
                    reasons.push(format!(
                        "dataset validation has {} error(s)",
                        validation.errors.len()
                    ));
                }
            }
            None => reasons.push("missing benchmark metadata".to_owned()),
        }

        for comparison in &self.comparisons {
            if comparison.status != BenchmarkComparisonStatus::Complete {
                let reason = comparison
                    .reason
                    .as_deref()
                    .unwrap_or("comparison statistics are incomplete");
                reasons.push(format!(
                    "{} comparison is not publishable: {reason}",
                    comparison.metric
                ));
            } else if comparison
                .statistics
                .as_ref()
                .and_then(|statistics| statistics.p_adjusted)
                .is_none()
            {
                reasons.push(format!(
                    "{} comparison is missing FDR-adjusted p-value",
                    comparison.metric
                ));
            }
        }

        BenchmarkPublishability {
            publishable: reasons.is_empty(),
            minimum_scored_questions: MIN_PUBLISHABLE_SCORED_QUESTIONS,
            reasons,
        }
    }

    fn scored_questions(&self) -> Vec<&QuestionResult> {
        self.questions
            .iter()
            .filter(|q| q.status.is_scored())
            .collect()
    }

    fn attempted_questions_by_id(&self) -> BTreeMap<&str, &QuestionResult> {
        self.questions.iter().map(|q| (q.id.as_str(), q)).collect()
    }

    fn comparison_for_metric(
        &self,
        baseline: &Self,
        metric: BenchmarkComparisonMetric,
        label: &str,
    ) -> BenchmarkComparisonReport {
        let (baseline_scores, candidate_scores) = self.paired_scores(baseline, metric);
        let matched_questions = baseline_scores.len();
        let comparison_label = format!("{label} {metric}");

        if matched_questions < MIN_PUBLISHABLE_SCORED_QUESTIONS {
            return BenchmarkComparisonReport::unavailable(
                metric,
                comparison_label,
                BenchmarkComparisonStatus::InsufficientSamples,
                matched_questions,
                format!(
                    "requires at least {MIN_PUBLISHABLE_SCORED_QUESTIONS} matched attempted questions; got {matched_questions}"
                ),
            );
        }

        match crate::stats::comparison_report(
            &baseline_scores,
            &candidate_scores,
            comparison_label.clone(),
            None,
        ) {
            Ok(statistics) => BenchmarkComparisonReport::complete(
                metric,
                comparison_label,
                matched_questions,
                statistics,
            ),
            Err(error) => BenchmarkComparisonReport::unavailable(
                metric,
                comparison_label,
                BenchmarkComparisonStatus::Error,
                matched_questions,
                error.to_string(),
            ),
        }
    }

    fn paired_scores(
        &self,
        baseline: &Self,
        metric: BenchmarkComparisonMetric,
    ) -> (Vec<f64>, Vec<f64>) {
        let baseline_by_id = baseline.attempted_questions_by_id();
        let candidate_by_id = self.attempted_questions_by_id();
        let mut baseline_scores = Vec::new();
        let mut candidate_scores = Vec::new();

        for (id, candidate) in candidate_by_id {
            if let Some(baseline_question) = baseline_by_id.get(id) {
                baseline_scores.push(metric.score(baseline_question));
                candidate_scores.push(metric.score(candidate));
            }
        }

        (baseline_scores, candidate_scores)
    }

    /// Fraction of attempted questions with exact-match score >= 1.0.
    ///
    /// Non-scorable outcomes remain in the denominator and contribute zero.
    /// Use [`BenchmarkReport::scored_only_exact_match_rate`] for the
    /// diagnostic score over answered questions only.
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

    /// Exact-match rate over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn scored_only_exact_match_rate(&self) -> f64 {
        let scored = self.scored_questions();
        if scored.is_empty() {
            return 0.0;
        }
        let hits = scored.iter().filter(|q| q.score.exact_match).count();
        hits as f64 / scored.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean F1 score across attempted questions.
    ///
    /// Non-scorable outcomes remain in the denominator and contribute zero.
    /// Use [`BenchmarkReport::scored_only_mean_f1`] for the diagnostic score
    /// over answered questions only.
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

    /// Mean F1 over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn scored_only_mean_f1(&self) -> f64 {
        let scored = self.scored_questions();
        if scored.is_empty() {
            return 0.0;
        }
        let sum: f64 = scored.iter().map(|q| q.score.f1).sum();
        sum / scored.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
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

    /// Group attempted questions by category and return `(category, EM, F1)`.
    ///
    /// Non-scorable outcomes remain in each category denominator and
    /// contribute zero. Use [`BenchmarkReport::scored_only_per_category`] for
    /// the diagnostic answered-question denominator.
    #[must_use]
    pub fn per_category(&self) -> Vec<(String, f64, f64)> {
        Self::per_category_for_questions(self.questions.iter())
    }

    /// Group answered questions by category and return `(category, EM, F1)`.
    #[must_use]
    pub fn scored_only_per_category(&self) -> Vec<(String, f64, f64)> {
        Self::per_category_for_questions(self.questions.iter().filter(|q| q.status.is_scored()))
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "bucket counts are small; f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — bucket counts are bounded and small"
    )]
    fn per_category_for_questions<'a>(
        questions: impl Iterator<Item = &'a QuestionResult>,
    ) -> Vec<(String, f64, f64)> {
        use std::collections::BTreeMap;
        let mut buckets: BTreeMap<String, Vec<&QuestionResult>> = BTreeMap::new();
        for q in questions {
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
        let baseline =
            BenchmarkReport::new("Test", vec![result("q1", "factual", "wrong", &["blue"])]);
        let candidate =
            BenchmarkReport::new("Test", vec![result("q1", "factual", "blue", &["blue"])])
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
}
