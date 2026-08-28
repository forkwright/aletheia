//! Prompt audit log retention (#3411).
//!
//! Prunes daily JSONL files older than `retention_days`. The audit log itself
//! is append-only and rotates per-day by filename (`YYYY-MM-DD.jsonl`); this
//! task enforces the retention window.

use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::error;
#[cfg(test)]
use crate::maintenance::dated_file_retention::SECONDS_PER_DAY;
use crate::maintenance::dated_file_retention::{DatedFileRetention, DatedFileRetentionReport};

/// Configuration for prompt audit log retention.
#[derive(Debug, Clone)]
pub struct PromptAuditRetentionConfig {
    /// Whether pruning is active.
    pub enabled: bool,
    /// Directory holding daily JSONL files.
    pub log_dir: PathBuf,
    /// Files older than this many days are deleted.
    pub retention_days: u32,
}

impl Default for PromptAuditRetentionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_dir: PathBuf::from("logs/prompt-audit"),
            retention_days: 90,
        }
    }
}

/// Outcome of a retention run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptAuditRetentionReport {
    /// Number of daily files deleted.
    pub files_pruned: u32,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Number of valid daily files retained by filename date.
    pub files_retained: u32,
    /// Number of malformed JSONL files retained by mtime fallback.
    pub malformed_files_skipped: u32,
    /// Number of malformed JSONL files deleted by mtime fallback.
    pub fallback_files_pruned: u32,
}

impl From<DatedFileRetentionReport> for PromptAuditRetentionReport {
    fn from(report: DatedFileRetentionReport) -> Self {
        Self {
            files_pruned: report.files_pruned,
            bytes_freed: report.bytes_freed,
            files_retained: report.files_retained,
            malformed_files_skipped: report.malformed_files_skipped,
            fallback_files_pruned: report.fallback_files_pruned,
        }
    }
}

/// Prunes prompt-audit daily files past the retention window.
pub struct PromptAuditRotator {
    config: PromptAuditRetentionConfig,
}

impl PromptAuditRotator {
    /// Create a rotator with the given configuration.
    #[must_use]
    pub fn new(config: PromptAuditRetentionConfig) -> Self {
        Self { config }
    }

    /// Run retention. Delete daily `*.jsonl` files whose filename date is
    /// older than `retention_days`; malformed names use mtime fallback.
    ///
    /// # Errors
    ///
    /// Returns an error if the log directory cannot be read or a file cannot
    /// be deleted. Missing log directory is treated as an empty report, not
    /// an error, so operators can enable the feature before any requests
    /// have been logged.
    pub fn prune(&self) -> error::Result<PromptAuditRetentionReport> {
        if !self.config.enabled {
            return Ok(PromptAuditRetentionReport::default());
        }

        let report = DatedFileRetention {
            dir: &self.config.log_dir,
            extension: "jsonl",
            retention_days: self.config.retention_days,
            parse_date: parse_audit_date,
            label: "prompt audit",
        }
        .prune()?;

        Ok(report.into())
    }
}

fn parse_audit_date(path: &Path) -> Option<Date> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != "YYYY-MM-DD".len() {
        return None;
    }
    stem.parse::<Date>().ok()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;
    use std::time::SystemTime;

    use jiff::ToSpan;

    use super::*;
    use crate::maintenance::test_support::{utc_today, write_fixture};

    /// Set a file's mtime to `days_ago` days in the past.
    ///
    /// WHY: tests rely on rewriting mtime to simulate aging without waiting.
    /// MSRV 1.94 provides `File::set_modified`.
    fn set_old_mtime(path: &std::path::Path, days_ago: u64) {
        let age = std::time::Duration::from_secs(days_ago * SECONDS_PER_DAY);
        let mtime = SystemTime::now()
            .checked_sub(age)
            .expect("subtract duration");
        let file = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_modified(mtime).expect("set mtime");
    }

    fn date_days_ago(days: u32) -> Date {
        utc_today()
            .checked_sub(i64::from(days).days())
            .expect("date subtract")
    }

    fn date_days_ahead(days: u32) -> Date {
        utc_today()
            .checked_add(i64::from(days).days())
            .expect("date add")
    }

    fn audit_file(dir: &std::path::Path, date: Date) -> PathBuf {
        dir.join(format!("{date}.jsonl"))
    }

    #[test]
    fn disabled_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PromptAuditRetentionConfig {
            enabled: false,
            log_dir: tmp.path().to_path_buf(),
            retention_days: 1,
        };
        let path = tmp.path().join("2020-01-01.jsonl");
        write_fixture(&path, "{}\n");
        set_old_mtime(&path, 365);

        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 0);
        assert_eq!(report.files_retained, 0);
        assert!(path.exists(), "disabled rotator must not touch files");
    }

    #[test]
    fn missing_dir_is_empty_report() {
        let config = PromptAuditRetentionConfig {
            enabled: true,
            log_dir: PathBuf::from("/tmp/does-not-exist-xyz-prompt-audit-12345"),
            retention_days: 90,
        };
        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 0);
        assert_eq!(report.files_retained, 0);
    }

    #[test]
    fn valid_files_use_filename_date_not_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PromptAuditRetentionConfig {
            enabled: true,
            log_dir: tmp.path().to_path_buf(),
            retention_days: 7,
        };

        let old = audit_file(tmp.path(), date_days_ago(8));
        let recent = audit_file(tmp.path(), date_days_ago(1));
        write_fixture(&old, "{}\n");
        write_fixture(&recent, "{}\n");
        set_old_mtime(&recent, 365);

        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 1);
        assert_eq!(report.files_retained, 1);
        assert!(!old.exists(), "old file must be pruned");
        assert!(recent.exists(), "recent file must be kept");
    }

    #[test]
    fn future_dated_file_is_retained() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PromptAuditRetentionConfig {
            enabled: true,
            log_dir: tmp.path().to_path_buf(),
            retention_days: 7,
        };

        let future = audit_file(tmp.path(), date_days_ahead(2));
        write_fixture(&future, "{}\n");
        set_old_mtime(&future, 365);

        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 0);
        assert_eq!(report.files_retained, 1);
        assert!(future.exists(), "future-dated audit file must be kept");
    }

    #[test]
    fn malformed_jsonl_uses_mtime_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PromptAuditRetentionConfig {
            enabled: true,
            log_dir: tmp.path().to_path_buf(),
            retention_days: 7,
        };
        let stale = tmp.path().join("restored-copy.jsonl");
        let fresh = tmp.path().join("operator-note.jsonl");
        write_fixture(&stale, "{}\n");
        write_fixture(&fresh, "{}\n");
        set_old_mtime(&stale, 365);

        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 1);
        assert_eq!(report.fallback_files_pruned, 1);
        assert_eq!(report.malformed_files_skipped, 1);
        assert!(!stale.exists(), "stale malformed file should use fallback");
        assert!(
            fresh.exists(),
            "fresh malformed file should be reported and kept"
        );
    }

    #[test]
    fn non_jsonl_files_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PromptAuditRetentionConfig {
            enabled: true,
            log_dir: tmp.path().to_path_buf(),
            retention_days: 1,
        };
        let note = tmp.path().join("README.txt");
        write_fixture(&note, "operator notes\n");
        set_old_mtime(&note, 365);

        let report = PromptAuditRotator::new(config).prune().unwrap();
        assert_eq!(report.files_pruned, 0, "non-jsonl file must be skipped");
        assert!(note.exists(), "non-jsonl file must remain");
    }
}
