//! JSONL persistence for evaluation results as training data.

use std::io::Write;
use std::path::Path;

use mneme::meta::Stamped as _;
use serde::Serialize;
use snafu::ResultExt;

use crate::coverage::{self, Policy, SkipClass, SkipKind, Summary};
use crate::error::{self, Result};
use crate::provenance::EvalProvenance;
use crate::runner::RunReport;
use crate::scenario::ScenarioOutcome;
use crate::tags::tag_eval_result;

/// A single evaluation record for JSONL training data output.
#[derive(Debug, Clone, Serialize)]
pub struct EvalRecord {
    /// ISO 8601 timestamp of when the evaluation was run.
    pub timestamp: String,
    /// Stable identifier for the eval run.
    pub eval_run_id: String,
    /// Provenance envelope for the run.
    pub provenance: EvalProvenance,
    /// Evaluation category (e.g., "health", "cognitive", "session").
    pub eval_type: String,
    // kanon:ignore RUST/primitive-for-domain-id — scenario_id for JSONL training data output, mirrors external scenario ids
    /// Scenario identifier.
    pub scenario_id: String,
    /// Whether the scenario passed.
    pub passed: bool,
    /// Duration in milliseconds (0 for skipped scenarios).
    pub duration_ms: u64,
    /// Outcome kind: "passed", "failed", or "skipped".
    pub outcome: String,
    /// Error message or skip reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Whether this scenario is part of the required coverage denominator.
    pub required_for_coverage: bool,
    /// Machine-readable skip reason, when skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_kind: Option<SkipKind>,
    /// Machine-readable skip class, when skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_class: Option<SkipClass>,
    /// Run-level coverage denominators, present for stamped CLI writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<EvalRecordCoverage>,
    /// Structured sub-results for multi-probe scenarios.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sub_results: Vec<crate::scenario::ScenarioSubResult>,
}

/// Run-level coverage context persisted beside each JSONL record.
#[derive(Debug, Clone, Serialize)]
pub struct EvalRecordCoverage {
    /// Policy used for the run.
    pub policy: Policy,
    /// Required selected scenarios.
    pub required_scenarios: usize,
    /// Required scenarios that passed.
    pub passed_required: usize,
    /// Required scenarios that failed.
    pub failed_required: usize,
    /// Required scenarios that were skipped.
    pub skipped_required: usize,
    /// Required pass rate in basis points.
    pub required_pass_rate_bps: u32,
    /// Required skip ratio in basis points.
    pub required_skip_ratio_bps: u32,
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
#[must_use]
pub fn records_from_report(report: &RunReport) -> Vec<EvalRecord> {
    records_from_report_with_coverage(report, None)
}

/// Convert a run report into JSONL records with optional coverage context.
#[must_use]
pub fn records_from_report_with_coverage(
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
pub fn append_jsonl(path: &Path, records: &[EvalRecord]) -> Result<()> {
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

/// Append evaluation records from a `RunReport` to a JSONL file and write a
/// sibling `<path>.meta.json` file with provenance metadata.
///
/// The `.meta.json` file is always overwritten (not appended) because it
/// reflects the provenance of the *most recent* batch of records written to
/// the JSONL file.
///
/// # Errors
///
/// Returns `Io` if either file cannot be opened or written to.
/// Returns `Json` if serialization of records or metadata fails.
pub fn append_jsonl_stamped(path: &Path, report: &RunReport) -> Result<()> {
    append_jsonl_stamped_with_coverage(path, report, None)
}

/// Append evaluation records and stamped metadata with coverage denominators.
///
/// # Errors
///
/// Returns `Io` if any output file cannot be opened or written to.
/// Returns `Json` if serialization of records or metadata fails.
pub fn append_jsonl_stamped_with_coverage(
    path: &Path,
    report: &RunReport,
    coverage: Option<&Summary>,
) -> Result<()> {
    let records = records_from_report_with_coverage(report, coverage);
    append_jsonl(path, &records)?;

    // Write the sibling .meta.json alongside the JSONL output.
    let meta = stamp_with_coverage(report, coverage);
    let meta_path = sibling_path(path, "meta.json");
    let meta_json = serde_json::to_vec_pretty(&meta).context(error::JsonSnafu)?;
    let mut meta_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&meta_path)
        .context(error::IoSnafu)?;
    meta_file.write_all(&meta_json).context(error::IoSnafu)?;

    // Write the sibling .tags.json for fast set-membership filtering.
    let tags = tag_eval_result(report);
    let tags_path = sibling_path(path, "tags.json");
    let tags_json = serde_json::to_vec_pretty(&tags).context(error::JsonSnafu)?;
    let mut tags_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tags_path)
        .context(error::IoSnafu)?;
    tags_file.write_all(&tags_json).context(error::IoSnafu)?;

    if let Some(coverage) = coverage {
        let coverage_path = sibling_path(path, "coverage.json");
        let coverage_json = serde_json::to_vec_pretty(coverage).context(error::JsonSnafu)?;
        let mut coverage_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&coverage_path)
            .context(error::IoSnafu)?;
        coverage_file
            .write_all(&coverage_json)
            .context(error::IoSnafu)?;
    }

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

fn millis_from_duration(duration: &std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::scenario::{ScenarioClassification, ScenarioMeta, ScenarioResult};

    fn sample_meta(id: &'static str, category: &'static str) -> ScenarioMeta {
        ScenarioMeta {
            id,
            description: "test scenario",
            category,
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
            classification: ScenarioClassification::Assertive,
        }
    }

    fn sample_provenance() -> EvalProvenance {
        EvalProvenance::new("er-test", "http://localhost")
    }

    #[test]
    fn records_from_report_passed() {
        let report = RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: Duration::from_millis(100),
            results: vec![ScenarioResult {
                meta: sample_meta("test-pass", "health"),
                outcome: ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
                sub_results: vec![],
            }],
            provenance: sample_provenance(),
        };
        let records = records_from_report(&report);
        assert_eq!(records.len(), 1, "should produce one record");
        assert!(records[0].passed, "should be passed");
        assert_eq!(records[0].outcome, "passed", "outcome should be 'passed'");
        assert_eq!(records[0].duration_ms, 50, "duration should be 50ms");
        assert!(
            records[0].message.is_none(),
            "passed should have no message"
        );
        assert_eq!(records[0].eval_run_id, "er-test");
    }

    #[test]
    fn records_from_report_failed() {
        let report = RunReport {
            passed: 0,
            failed: 1,
            skipped: 0,
            total_duration: Duration::from_millis(200),
            results: vec![ScenarioResult {
                meta: sample_meta("test-fail", "session"),
                outcome: ScenarioOutcome::Failed {
                    duration: Duration::from_millis(150),
                    error: crate::error::AssertionSnafu {
                        message: "test failure",
                    }
                    .build(),
                },
                sub_results: vec![],
            }],
            provenance: sample_provenance(),
        };
        let records = records_from_report(&report);
        assert_eq!(records.len(), 1, "should produce one record");
        assert!(!records[0].passed, "should not be passed");
        assert_eq!(records[0].outcome, "failed", "outcome should be 'failed'");
        assert!(records[0].message.is_some(), "failed should have message");
    }

    #[test]
    fn records_from_report_skipped() {
        let report = RunReport {
            passed: 0,
            failed: 0,
            skipped: 1,
            total_duration: Duration::from_millis(10),
            results: vec![ScenarioResult {
                meta: sample_meta("test-skip", "cognitive"),
                outcome: ScenarioOutcome::Skipped {
                    reason: coverage::SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
                },
                sub_results: vec![],
            }],
            provenance: sample_provenance(),
        };
        let records = records_from_report(&report);
        assert_eq!(records.len(), 1, "should produce one record");
        assert!(!records[0].passed, "should not be passed");
        assert_eq!(records[0].outcome, "skipped", "outcome should be 'skipped'");
        assert_eq!(records[0].duration_ms, 0, "skipped should have 0 duration");
        assert_eq!(records[0].skip_kind, Some(SkipKind::MissingAuthToken));
        assert_eq!(records[0].skip_class, Some(SkipClass::Environmental));
    }

    #[test]
    fn eval_record_serializes_to_json() {
        let record = EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance: sample_provenance(),
            eval_type: "health".to_owned(),
            scenario_id: "health-ok".to_owned(),
            passed: true,
            duration_ms: 50,
            outcome: "passed".to_owned(),
            message: None,
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(
            json.contains("health-ok"),
            "JSON should contain scenario ID"
        );
        assert!(
            json.contains("eval_run_id"),
            "JSON should contain eval_run_id"
        );
        assert!(!json.contains("message"), "None message should be skipped");
    }

    #[test]
    fn eval_record_with_message_serializes() {
        let record = EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance: sample_provenance(),
            eval_type: "cognitive".to_owned(),
            scenario_id: "test-fail".to_owned(),
            passed: false,
            duration_ms: 100,
            outcome: "failed".to_owned(),
            message: Some("assertion failed".to_owned()),
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(
            json.contains("assertion failed"),
            "JSON should contain error message"
        );
    }

    #[test]
    fn eval_record_does_not_serialize_token() {
        let provenance = EvalProvenance::new("er-1", "http://localhost")
            .with_redacted_args(&["--token".to_owned(), "secret-token".to_owned()]);
        let record = EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance,
            eval_type: "health".to_owned(),
            scenario_id: "health-ok".to_owned(),
            passed: true,
            duration_ms: 50,
            outcome: "passed".to_owned(),
            message: None,
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(
            !json.contains("secret-token"),
            "token must not leak into JSONL"
        );
        assert!(
            json.contains("[REDACTED]"),
            "redacted placeholder should appear"
        );
    }

    #[test]
    fn append_jsonl_creates_file() {
        let dir = std::env::temp_dir().join("aletheia-eval-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-output.jsonl");
        let _ = std::fs::remove_file(&path);

        let records = vec![EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance: sample_provenance(),
            eval_type: "test".to_owned(),
            scenario_id: "test-1".to_owned(),
            passed: true,
            duration_ms: 10,
            outcome: "passed".to_owned(),
            message: None,
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        }];

        let result = append_jsonl(&path, &records);
        assert!(result.is_ok(), "append_jsonl should succeed");

        let content = std::fs::read_to_string(&path).expect("should read file");
        assert!(content.contains("test-1"), "file should contain record");
        assert!(content.ends_with('\n'), "file should end with newline");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_jsonl_appends_to_existing() {
        let dir = std::env::temp_dir().join("aletheia-eval-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-append.jsonl");
        let _ = std::fs::remove_file(&path);

        let record1 = vec![EvalRecord {
            timestamp: "1".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance: sample_provenance(),
            eval_type: "test".to_owned(),
            scenario_id: "first".to_owned(),
            passed: true,
            duration_ms: 10,
            outcome: "passed".to_owned(),
            message: None,
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        }];
        let record2 = vec![EvalRecord {
            timestamp: "2".to_owned(),
            eval_run_id: "er-1".to_owned(),
            provenance: sample_provenance(),
            eval_type: "test".to_owned(),
            scenario_id: "second".to_owned(),
            passed: true,
            duration_ms: 20,
            outcome: "passed".to_owned(),
            message: None,
            required_for_coverage: true,
            skip_kind: None,
            skip_class: None,
            coverage: None,
            sub_results: vec![],
        }];

        append_jsonl(&path, &record1).expect("first append");
        append_jsonl(&path, &record2).expect("second append");

        let content = std::fs::read_to_string(&path).expect("should read file");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "should have two lines");
        assert!(
            lines[0].contains("first"),
            "first line should be first record"
        );
        assert!(
            lines[1].contains("second"),
            "second line should be second record"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn now_iso8601_returns_nonempty() {
        let ts = now_iso8601();
        assert!(!ts.is_empty(), "timestamp should not be empty");
    }

    #[test]
    fn now_iso8601_roundtrips_as_jiff_zoned() {
        let ts = now_iso8601();
        let parsed = jiff::Zoned::strptime("%Y-%m-%dT%H:%M:%S%.3f%:z", &ts)
            .expect("timestamp should parse as ISO 8601 UTC");
        assert_eq!(
            parsed.strftime("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
            ts,
            "formatted timestamp should round-trip through jiff"
        );
    }

    #[test]
    fn millis_from_duration_converts() {
        let d = Duration::from_millis(42);
        assert_eq!(millis_from_duration(&d), 42, "should convert to 42ms");
    }

    #[test]
    fn append_jsonl_stamped_writes_meta_sibling() {
        let dir = std::env::temp_dir().join("aletheia-eval-stamped-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("stamped-test.jsonl");
        let meta_path = dir.join("stamped-test.jsonl.meta.json");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&meta_path);

        let report = RunReport {
            passed: 2,
            failed: 1,
            skipped: 0,
            total_duration: std::time::Duration::from_millis(300),
            results: vec![],
            provenance: sample_provenance(),
        };

        append_jsonl_stamped(&path, &report).expect("stamped append should succeed");

        assert!(path.exists(), "JSONL file should exist after stamped write");
        assert!(
            meta_path.exists(),
            "meta JSON sibling should exist after stamped write"
        );

        let meta_content = std::fs::read_to_string(&meta_path).expect("should read meta file");
        let meta: mneme::meta::ArtefactMeta =
            serde_json::from_str(&meta_content).expect("meta should be valid JSON");
        assert!(
            meta.producer.starts_with("dokimion@"),
            "producer must start with 'dokimion@'"
        );
        assert_eq!(
            meta.row_counts.get("passed").copied(),
            Some(2),
            "meta should carry passed count"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&meta_path);
    }

    #[test]
    fn append_jsonl_stamped_with_coverage_writes_denominators() {
        let dir = std::env::temp_dir().join("aletheia-eval-coverage-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("stamped-coverage.jsonl");
        let meta_path = dir.join("stamped-coverage.jsonl.meta.json");
        let coverage_path = dir.join("stamped-coverage.jsonl.coverage.json");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_file(&coverage_path);

        let report = RunReport {
            passed: 1,
            failed: 0,
            skipped: 1,
            total_duration: std::time::Duration::from_millis(100),
            results: vec![
                ScenarioResult {
                    meta: sample_meta("coverage-pass", "health"),
                    outcome: ScenarioOutcome::Passed {
                        duration: std::time::Duration::from_millis(50),
                    },
                    sub_results: vec![],
                },
                ScenarioResult {
                    meta: sample_meta("coverage-skip", "session"),
                    outcome: ScenarioOutcome::Skipped {
                        reason: coverage::SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
                    },
                    sub_results: vec![],
                },
            ],
            provenance: sample_provenance(),
        };
        let coverage = coverage::Policy::Ci.evaluate(&report);

        append_jsonl_stamped_with_coverage(&path, &report, Some(&coverage))
            .expect("stamped append should succeed");

        let meta_content = std::fs::read_to_string(&meta_path).expect("should read meta file");
        let meta: mneme::meta::ArtefactMeta =
            serde_json::from_str(&meta_content).expect("meta should be valid JSON");
        assert_eq!(
            meta.row_counts.get("required").copied(),
            Some(2),
            "meta should carry required denominator"
        );
        assert_eq!(
            meta.row_counts.get("required_skipped").copied(),
            Some(1),
            "meta should carry skipped required count"
        );

        let coverage_content =
            std::fs::read_to_string(&coverage_path).expect("should read coverage file");
        assert!(coverage_content.contains("\"policy\": \"ci\""));
        assert!(coverage_content.contains("\"required_scenarios\": 2"));

        let jsonl_content = std::fs::read_to_string(&path).expect("should read JSONL file");
        assert!(jsonl_content.contains("\"skip_kind\":\"missing_auth_token\""));
        assert!(jsonl_content.contains("\"coverage\""));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&meta_path);
        let _ = std::fs::remove_file(&coverage_path);
    }

    #[test]
    fn append_jsonl_stamped_writes_tags_sibling() {
        let dir = std::env::temp_dir().join("aletheia-eval-tags-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("stamped-tags.jsonl");
        let tags_path = dir.join("stamped-tags.jsonl.tags.json");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tags_path);

        let report = RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: std::time::Duration::from_millis(100),
            results: vec![ScenarioResult {
                meta: sample_meta("tag-test", "health"),
                outcome: ScenarioOutcome::Passed {
                    duration: std::time::Duration::from_millis(50),
                },
                sub_results: vec![],
            }],
            provenance: sample_provenance(),
        };

        append_jsonl_stamped(&path, &report).expect("stamped append should succeed");

        assert!(path.exists(), "JSONL file should exist after stamped write");
        assert!(
            tags_path.exists(),
            "tags JSON sibling should exist after stamped write"
        );

        let tags_content = std::fs::read_to_string(&tags_path).expect("should read tags file");
        let tags: Vec<crate::tags::TagId> =
            serde_json::from_str(&tags_content).expect("tags should be valid JSON");
        assert!(
            tags.iter().any(|t| matches!(
                t,
                crate::tags::TagId::Outcome(crate::tags::OutcomeTag::Passed)
            )),
            "tags should contain passed outcome"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&tags_path);
    }
}
