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

mod question;
mod report;
#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "tests assert against known-length vectors"
)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod report_tests;
mod types;

use std::path::Path;

pub use self::metrics::{BenchmarkScore, score_answer};
pub use self::question::{BenchmarkQuestion, MemoryBenchmark};
pub use self::types::{
    BENCHMARK_STAT_RESAMPLES, BenchmarkComparisonMetric, BenchmarkComparisonReport,
    BenchmarkComparisonStatus, BenchmarkIngestionSummary, BenchmarkMetadata,
    BenchmarkPublishability, BenchmarkReliabilitySummary, BenchmarkReport, BenchmarkStatistics,
    JudgeSummary, MAX_PUBLISHABLE_ERROR_RATE, MAX_PUBLISHABLE_INGESTION_ERROR_RATE,
    MAX_PUBLISHABLE_INGESTION_PARTIAL_RATE, MAX_PUBLISHABLE_NO_ANSWER_RATE,
    MAX_PUBLISHABLE_TIMEOUT_RATE, MIN_PUBLISHABLE_SCORED_QUESTIONS, QuestionResult, QuestionStatus,
    RetrievalScoring, RetrievalScoringMode, RetrievedFact,
};
pub use self::validation::{
    BenchmarkValidationIssue, BenchmarkValidationOptions, BenchmarkValidationReport,
};

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
