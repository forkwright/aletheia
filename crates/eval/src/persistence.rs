//! JSONL persistence for evaluation results as training data.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use snafu::ResultExt;

use crate::error::{self, Result};
use crate::runner::RunReport;
use crate::scenario::ScenarioOutcome;

/// A single evaluation record for JSONL training data output.
#[derive(Debug, Clone, Serialize)]
pub struct EvalRecord {
    /// ISO 8601 timestamp of when the evaluation was run.
    pub timestamp: String,
    /// Evaluation category (e.g., "health", "cognitive", "session").
    pub eval_type: String,
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
}

/// Convert a run report into evaluation records for JSONL output.
#[must_use]
pub fn records_from_report(report: &RunReport) -> Vec<EvalRecord> {
    let timestamp = now_iso8601();

    report
        .results
        .iter()
        .map(|result| {
            let (passed, duration_ms, outcome, message) = match &result.outcome {
                ScenarioOutcome::Passed { duration } => (
                    true,
                    millis_from_duration(duration),
                    "passed".to_owned(),
                    None,
                ),
                ScenarioOutcome::Failed { duration, error } => (
                    false,
                    millis_from_duration(duration),
                    "failed".to_owned(),
                    Some(error.to_string()),
                ),
                ScenarioOutcome::Skipped { reason } => {
                    (false, 0, "skipped".to_owned(), Some(reason.clone()))
                }
            };

            EvalRecord {
                timestamp: timestamp.clone(),
                eval_type: result.meta.category.to_owned(),
                scenario_id: result.meta.id.to_owned(),
                passed,
                duration_ms,
                outcome,
                message,
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

fn now_iso8601() -> String {
    // WHY: avoid pulling in jiff/chrono for a single timestamp format.
    // Epoch seconds are unambiguous and lightweight.
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    format!("{secs}")
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
    use crate::scenario::{ScenarioMeta, ScenarioResult};

    fn sample_meta(id: &'static str, category: &'static str) -> ScenarioMeta {
        ScenarioMeta {
            id,
            description: "test scenario",
            category,
            requires_auth: false,
            requires_nous: false,
            expected_contains: None,
            expected_pattern: None,
        }
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
            }],
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
            }],
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
                    reason: "no auth".to_owned(),
                },
            }],
        };
        let records = records_from_report(&report);
        assert_eq!(records.len(), 1, "should produce one record");
        assert!(!records[0].passed, "should not be passed");
        assert_eq!(records[0].outcome, "skipped", "outcome should be 'skipped'");
        assert_eq!(records[0].duration_ms, 0, "skipped should have 0 duration");
    }

    #[test]
    fn eval_record_serializes_to_json() {
        let record = EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_type: "health".to_owned(),
            scenario_id: "health-ok".to_owned(),
            passed: true,
            duration_ms: 50,
            outcome: "passed".to_owned(),
            message: None,
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(
            json.contains("health-ok"),
            "JSON should contain scenario ID"
        );
        assert!(!json.contains("message"), "None message should be skipped");
    }

    #[test]
    fn eval_record_with_message_serializes() {
        let record = EvalRecord {
            timestamp: "1234567890".to_owned(),
            eval_type: "cognitive".to_owned(),
            scenario_id: "test-fail".to_owned(),
            passed: false,
            duration_ms: 100,
            outcome: "failed".to_owned(),
            message: Some("assertion failed".to_owned()),
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(
            json.contains("assertion failed"),
            "JSON should contain error message"
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
            eval_type: "test".to_owned(),
            scenario_id: "test-1".to_owned(),
            passed: true,
            duration_ms: 10,
            outcome: "passed".to_owned(),
            message: None,
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
            eval_type: "test".to_owned(),
            scenario_id: "first".to_owned(),
            passed: true,
            duration_ms: 10,
            outcome: "passed".to_owned(),
            message: None,
        }];
        let record2 = vec![EvalRecord {
            timestamp: "2".to_owned(),
            eval_type: "test".to_owned(),
            scenario_id: "second".to_owned(),
            passed: true,
            duration_ms: 20,
            outcome: "passed".to_owned(),
            message: None,
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
    fn millis_from_duration_converts() {
        let d = Duration::from_millis(42);
        assert_eq!(millis_from_duration(&d), 42, "should convert to 42ms");
    }
}
