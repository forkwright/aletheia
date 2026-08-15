//! Self-history baseline comparison: diff a benchmark run against its own
//! prior run.
//!
//! Distinct from [`super::baselines`], which compares against static
//! peer-published numbers. This module diffs a candidate
//! [`BenchmarkReport`] against *this benchmark's own* previous run,
//! persisted on disk, reusing the same statistically-rigorous comparison
//! machinery ([`BenchmarkReport::with_comparisons_against`] -- bootstrap
//! confidence intervals, FDR-adjusted p-values) rather than a bespoke diff.

use std::path::Path;

use snafu::ResultExt;

use crate::error::{self, Result};

use super::BenchmarkReport;

/// Load a previously-persisted [`BenchmarkReport`] from disk, if present.
///
/// Returns `Ok(None)` when the path does not exist yet -- there is no
/// self-history for this benchmark, which is the normal state for the
/// first run, not an error condition.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read, or its content
/// is not a valid `BenchmarkReport` JSON document.
pub fn load_prior_report(path: impl AsRef<Path>) -> Result<Option<BenchmarkReport>> {
    match std::fs::read_to_string(path.as_ref()) {
        Ok(content) => {
            let report: BenchmarkReport =
                serde_json::from_str(&content).context(error::JsonSnafu)?;
            Ok(Some(report))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source).context(error::IoSnafu),
    }
}

/// Persist a [`BenchmarkReport`] to disk so a later run can load it as
/// self-history via [`load_prior_report`].
///
/// # Errors
///
/// Returns an error if the report cannot be serialized or the file cannot
/// be written.
pub fn save_report(path: impl AsRef<Path>, report: &BenchmarkReport) -> Result<()> {
    let json = serde_json::to_vec_pretty(report).context(error::JsonSnafu)?;
    std::fs::write(path.as_ref(), json).context(error::IoSnafu)
}

/// Attach self-history comparisons to `report` against the prior run
/// persisted at `history_path`, then persist `report` as the new history
/// for the next run.
///
/// A missing `history_path` (first run for this benchmark) leaves
/// `report.comparisons` empty rather than erroring -- there is nothing yet
/// to compare against. Every call persists `report` as the new baseline
/// for the *next* call, so history is always the immediately-prior run.
///
/// # Errors
///
/// Returns an error if the prior report exists but cannot be read/parsed,
/// or if the updated report cannot be persisted.
pub fn compare_against_self_history(
    mut report: BenchmarkReport,
    history_path: impl AsRef<Path>,
) -> Result<BenchmarkReport> {
    let history_path = history_path.as_ref();
    if let Some(prior) = load_prior_report(history_path)? {
        report = report.with_comparisons_against(&prior, "self-history");
    }
    save_report(history_path, &report)?;
    Ok(report)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::benchmarks::BenchmarkComparisonStatus;

    fn sample_report(benchmark: &str) -> BenchmarkReport {
        BenchmarkReport {
            benchmark: benchmark.to_owned(),
            total: 1,
            scored: 1,
            ..Default::default()
        }
    }

    #[test]
    fn load_prior_report_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("does-not-exist.json");
        assert!(
            load_prior_report(&missing)
                .expect("a missing file is not an error")
                .is_none()
        );
    }

    #[test]
    fn first_run_has_no_prior_history_and_leaves_comparisons_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let history_path = dir.path().join("history.json");

        let report = compare_against_self_history(sample_report("locomo"), &history_path)
            .expect("first run persists without a prior");

        assert!(
            report.comparisons.is_empty(),
            "no prior history -- nothing to compare against yet"
        );
        assert!(
            history_path.exists(),
            "first run must persist history for the next run"
        );
    }

    #[test]
    fn second_run_compares_against_persisted_first_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let history_path = dir.path().join("history.json");

        compare_against_self_history(sample_report("locomo"), &history_path).expect("first run");
        let second = compare_against_self_history(sample_report("locomo"), &history_path)
            .expect("second run compares against the persisted first run");

        assert_eq!(
            second.comparisons.len(),
            2,
            "F1 and ExactMatch comparisons must both be produced against self-history"
        );
    }

    #[test]
    fn mismatched_benchmark_name_produces_incomparable_comparisons() {
        let dir = tempfile::tempdir().expect("temp dir");
        let history_path = dir.path().join("history.json");

        compare_against_self_history(sample_report("locomo"), &history_path)
            .expect("first run seeds locomo history");
        let second = compare_against_self_history(sample_report("longmemeval"), &history_path)
            .expect("second run against mismatched history must not error");

        assert_eq!(second.comparisons.len(), 2);
        for comparison in &second.comparisons {
            assert_eq!(
                comparison.status,
                BenchmarkComparisonStatus::Incomparable,
                "comparing against a different benchmark's history must not be silently accepted"
            );
        }
    }
}
