//! JSONL persistence for evaluation results as training data.

use std::io::Write;
use std::path::{Path, PathBuf};

use mneme::meta::Stamped as _;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::coverage::{self, Policy, SkipClass, SkipKind, Summary};
use crate::error::{self, Result};
use crate::provenance::EvalProvenance;
use crate::runner::RunReport;
use crate::scenario::ScenarioOutcome;
use crate::tags::tag_eval_result;

/// A single evaluation record for JSONL training data output.
#[derive(Debug, Clone, Serialize)]
struct EvalRecord {
    /// ISO 8601 timestamp of when the evaluation was run.
    timestamp: String,
    /// Stable identifier for the eval run.
    eval_run_id: String,
    /// Provenance envelope for the run.
    provenance: EvalProvenance,
    /// Evaluation category (e.g., "health", "cognitive", "session").
    eval_type: String,
    // kanon:ignore RUST/primitive-for-domain-id — scenario_id for JSONL training data output, mirrors external scenario ids
    /// Scenario identifier.
    scenario_id: String,
    /// Whether the scenario passed.
    passed: bool,
    /// Duration in milliseconds (0 for skipped scenarios).
    duration_ms: u64,
    /// Outcome kind: "passed", "failed", or "skipped".
    outcome: String,
    /// Error message or skip reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// Whether this scenario is part of the required coverage denominator.
    required_for_coverage: bool,
    /// Machine-readable skip reason, when skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_kind: Option<SkipKind>,
    /// Machine-readable skip class, when skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_class: Option<SkipClass>,
    /// Run-level coverage denominators, present for stamped CLI writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<EvalRecordCoverage>,
    /// Structured sub-results for multi-probe scenarios.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    sub_results: Vec<crate::scenario::ScenarioSubResult>,
}

/// Run-level coverage context persisted beside each JSONL record.
#[derive(Debug, Clone, Serialize)]
struct EvalRecordCoverage {
    /// Policy used for the run.
    policy: Policy,
    /// Required selected scenarios.
    required_scenarios: usize,
    /// Required scenarios that passed.
    passed_required: usize,
    /// Required scenarios that failed.
    failed_required: usize,
    /// Required scenarios that were skipped.
    skipped_required: usize,
    /// Required pass rate in basis points.
    required_pass_rate_bps: u32,
    /// Required skip ratio in basis points.
    required_skip_ratio_bps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StampedJsonlManifest {
    schema_version: u32,
    artifact_path: String,
    runs: Vec<StampedJsonlRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StampedJsonlRun {
    eval_run_id: String,
    record_count: usize,
    jsonl_path: String,
    meta_path: String,
    tags_path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    coverage_path: Option<String>,
    recovered_from_partial_write: bool,
}

impl StampedJsonlManifest {
    fn new(path: &Path) -> Self {
        Self {
            schema_version: 1,
            artifact_path: path.display().to_string(),
            runs: Vec::new(),
        }
    }

    fn contains_run(&self, eval_run_id: &str) -> bool {
        self.runs
            .iter()
            .any(|entry| entry.eval_run_id == eval_run_id)
    }

    fn upsert_run(&mut self, entry: StampedJsonlRun) {
        if let Some(existing) = self
            .runs
            .iter_mut()
            .find(|run| run.eval_run_id == entry.eval_run_id)
        {
            *existing = entry;
        } else {
            self.runs.push(entry);
        }
    }
}

impl From<&Summary> for EvalRecordCoverage {
    fn from(summary: &Summary) -> Self {
        Self {
            policy: summary.policy,
            required_scenarios: summary.required_scenarios,
            passed_required: summary.passed_required,
            failed_required: summary.failed_required,
            skipped_required: summary.skipped_required,
            required_pass_rate_bps: summary.required_pass_rate_bps,
            required_skip_ratio_bps: summary.required_skip_ratio_bps,
        }
    }
}

/// Convert a run report into evaluation records for JSONL output.
#[cfg(test)]
#[must_use]
fn records_from_report(report: &RunReport) -> Vec<EvalRecord> {
    records_from_report_with_coverage(report, None)
}

/// Convert a run report into JSONL records with optional coverage context.
#[must_use]
fn records_from_report_with_coverage(
    report: &RunReport,
    coverage: Option<&Summary>,
) -> Vec<EvalRecord> {
    let timestamp = now_iso8601();
    let eval_run_id = report.provenance.eval_run_id.clone();
    let provenance = report.provenance.clone();
    let coverage_record = coverage.map(EvalRecordCoverage::from);

    report
        .results
        .iter()
        .map(|result| {
            let (passed, duration_ms, outcome, message, skip_kind, skip_class) =
                match &result.outcome {
                    ScenarioOutcome::Passed { duration } => (
                        true,
                        millis_from_duration(duration),
                        "passed".to_owned(),
                        None,
                        None,
                        None,
                    ),
                    ScenarioOutcome::Failed { duration, error } => (
                        false,
                        millis_from_duration(duration),
                        "failed".to_owned(),
                        Some(error.to_string()),
                        None,
                        None,
                    ),
                    ScenarioOutcome::Skipped { reason } => {
                        let kind = coverage::classify_skip(reason);
                        (
                            false,
                            0,
                            "skipped".to_owned(),
                            Some(reason.clone()),
                            Some(kind),
                            Some(kind.class()),
                        )
                    }
                };

            EvalRecord {
                timestamp: timestamp.clone(),
                eval_run_id: eval_run_id.clone(),
                provenance: provenance.clone(),
                eval_type: result.meta.category.to_owned(),
                scenario_id: result.meta.id.to_owned(),
                passed,
                duration_ms,
                outcome,
                message,
                required_for_coverage: coverage::required_for_coverage(&result.meta),
                skip_kind,
                skip_class,
                coverage: coverage_record.clone(),
                sub_results: result.sub_results.clone(),
            }
        })
        .collect()
}

/// Append evaluation records to a JSONL file, creating it if necessary.
///
/// # Errors
///
/// Returns `Io` if the file cannot be opened or written to.
/// Returns `Json` if a record cannot be serialized.
#[cfg(test)]
fn append_jsonl(path: &Path, records: &[EvalRecord]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context(error::IoSnafu)?;

    for record in records {
        let line = serde_json::to_string(record).context(error::JsonSnafu)?;
        writeln!(file, "{line}").context(error::IoSnafu)?;
    }

    Ok(())
}

/// Append evaluation records from a `RunReport` to a JSONL file and commit
/// sibling metadata, tags, and manifest sidecars for the same run ID.
///
/// # Errors
///
/// Returns `Io` if either file cannot be opened or written to.
/// Returns `Json` if serialization of records or metadata fails.
#[cfg(test)]
fn append_jsonl_stamped(path: &Path, report: &RunReport) -> Result<()> {
    append_jsonl_stamped_with_coverage(path, report, None)
}

/// Append evaluation records and stamped metadata with coverage denominators.
///
/// # Errors
///
/// Returns `Io` if any output file cannot be opened or written to.
/// Returns `Json` if serialization of records or metadata fails.
// kanon:ignore RUST/pub-visibility — used by the aletheia CLI to persist stamped eval JSONL artifacts
pub fn append_jsonl_stamped_with_coverage(
    path: &Path,
    report: &RunReport,
    coverage: Option<&Summary>,
) -> Result<()> {
    let records = records_from_report_with_coverage(report, coverage);
    let eval_run_id = report.provenance.eval_run_id.clone();
    let meta = stamp_with_coverage(report, coverage);
    let meta_path = sibling_path(path, "meta.json");
    let tags = tag_eval_result(report);
    let tags_path = sibling_path(path, "tags.json");
    let coverage_path = coverage.map(|_| sibling_path(path, "coverage.json"));
    let manifest_path = sibling_path(path, "manifest.json");
    let mut manifest = read_manifest(path, &manifest_path)?;
    let manifest_has_run = manifest.contains_run(&eval_run_id);
    let jsonl_has_run = jsonl_contains_run(path, &eval_run_id)?;
    let sidecars_complete = meta_path.exists()
        && tags_path.exists()
        && coverage_path
            .as_ref()
            .is_none_or(|coverage_path| coverage_path.exists());
    if manifest_has_run && jsonl_has_run && sidecars_complete {
        return Ok(());
    }

    let recovered_from_partial_write = manifest_has_run || jsonl_has_run;

    write_json_atomic(&meta_path, &meta)?;
    write_json_atomic(&tags_path, &tags)?;
    if let (Some(coverage), Some(coverage_path)) = (coverage, coverage_path.as_ref()) {
        write_json_atomic(coverage_path, coverage)?;
    }

    if !jsonl_has_run {
        append_jsonl_durable(path, &records)?;
    }

    manifest.upsert_run(StampedJsonlRun {
        eval_run_id,
        record_count: records.len(),
        jsonl_path: path.display().to_string(),
        meta_path: meta_path.display().to_string(),
        tags_path: tags_path.display().to_string(),
        coverage_path: coverage_path.map(|path| path.display().to_string()),
        recovered_from_partial_write,
    });
    write_json_atomic(&manifest_path, &manifest)?;

    Ok(())
}

fn stamp_with_coverage(
    report: &RunReport,
    coverage: Option<&Summary>,
) -> mneme::meta::ArtefactMeta {
    let mut meta = report.stamp();
    if let Some(coverage) = coverage {
        meta = meta
            .with_count(
                "required",
                u64::try_from(coverage.required_scenarios).unwrap_or(u64::MAX),
            )
            .with_count(
                "required_passed",
                u64::try_from(coverage.passed_required).unwrap_or(u64::MAX),
            )
            .with_count(
                "required_failed",
                u64::try_from(coverage.failed_required).unwrap_or(u64::MAX),
            )
            .with_count(
                "required_skipped",
                u64::try_from(coverage.skipped_required).unwrap_or(u64::MAX),
            )
            .with_count(
                "required_pass_rate_bps",
                u64::from(coverage.required_pass_rate_bps),
            )
            .with_count(
                "required_skip_ratio_bps",
                u64::from(coverage.required_skip_ratio_bps),
            );
    }
    meta
}

pub(crate) fn now_iso8601() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%dT%H:%M:%S%.3f%:z")
        .to_string()
}

fn sibling_path(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut p = path.to_owned();
    let ext = match p.extension() {
        Some(e) => format!("{}.{suffix}", e.to_string_lossy()),
        None => suffix.to_owned(),
    };
    p.set_extension(ext);
    p
}

fn read_manifest(path: &Path, manifest_path: &Path) -> Result<StampedJsonlManifest> {
    match std::fs::read_to_string(manifest_path) {
        Ok(content) => serde_json::from_str(&content).context(error::JsonSnafu),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(StampedJsonlManifest::new(path))
        }
        Err(source) => Err(source).context(error::IoSnafu),
    }
}

fn jsonl_contains_run(path: &Path, eval_run_id: &str) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            for line in content.lines().filter(|line| !line.trim().is_empty()) {
                let value: serde_json::Value =
                    serde_json::from_str(line).context(error::JsonSnafu)?;
                if value.get("eval_run_id").and_then(serde_json::Value::as_str) == Some(eval_run_id)
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source).context(error::IoSnafu),
    }
}

fn append_jsonl_durable(path: &Path, records: &[EvalRecord]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context(error::IoSnafu)?;

    for record in records {
        let line = serde_json::to_string(record).context(error::JsonSnafu)?;
        writeln!(file, "{line}").context(error::IoSnafu)?;
    }
    file.sync_all().context(error::IoSnafu)?;
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(value).context(error::JsonSnafu)?;
    write_bytes_atomic(path, &json)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp_path = temp_sibling_path(path);
    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .context(error::IoSnafu)?;
        file.write_all(bytes).context(error::IoSnafu)?;
        file.sync_all().context(error::IoSnafu)?;
    }
    std::fs::rename(&tmp_path, path).context(error::IoSnafu)?;
    if let Some(parent) = path.parent() {
        let dir = std::fs::OpenOptions::new()
            .read(true)
            .open(parent)
            .context(error::IoSnafu)?;
        dir.sync_all().context(error::IoSnafu)?;
    }
    Ok(())
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let mut tmp_path = path.to_owned();
    let extension = path.extension().map_or_else(String::new, |extension| {
        extension.to_string_lossy().into_owned()
    });
    let suffix = std::process::id();
    let tmp_extension = if extension.is_empty() {
        format!("tmp-{suffix}")
    } else {
        format!("{extension}.tmp-{suffix}")
    };
    tmp_path.set_extension(tmp_extension);
    tmp_path
}

fn millis_from_duration(duration: &std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests;
