//! Prompt audit log retention (#3411).
//!
//! Prunes daily JSONL files older than `retention_days`. The audit log itself
//! is append-only and rotates per-day by filename (`YYYY-MM-DD.jsonl`); this
//! task enforces the retention window.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::civil::Date;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error;

const SECONDS_PER_DAY: u64 = 86_400;

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
        if !self.config.log_dir.exists() {
            return Ok(PromptAuditRetentionReport::default());
        }

        let now = SystemTime::now();
        let today = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
        let cutoff_date = today
            .checked_sub(i64::from(self.config.retention_days).days())
            .unwrap_or(today);
        let max_age =
            std::time::Duration::from_secs(u64::from(self.config.retention_days) * SECONDS_PER_DAY);

        let dir = fs::read_dir(&self.config.log_dir).context(error::MaintenanceIoSnafu {
            context: format!("reading prompt audit dir {}", self.config.log_dir.display()),
        })?;

        let mut report = PromptAuditRetentionReport::default();

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading prompt audit directory entry",
            })?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            // WHY: only prune `*.jsonl` files — leave any accidental sidecar
            // files alone so operators can drop notes or reports next to the
            // log directory without the daemon deleting them.
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            if let Some(audit_date) = parse_audit_date(&path) {
                if audit_date < cutoff_date {
                    prune_file(&path, &mut report, "pruning prompt audit file by date")?;
                } else {
                    report.files_retained += 1;
                }
            } else if prune_malformed_by_mtime(&entry, now, max_age, &mut report)? {
                tracing::debug!(
                    path = %path.display(),
                    "pruned malformed prompt audit file by mtime fallback"
                );
            } else {
                tracing::debug!(
                    path = %path.display(),
                    "retained malformed prompt audit file by mtime fallback"
                );
            }
        }

        Ok(report)
    }
}

fn parse_audit_date(path: &Path) -> Option<Date> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != "YYYY-MM-DD".len() {
        return None;
    }
    stem.parse::<Date>().ok()
}

fn prune_file(
    path: &Path,
    report: &mut PromptAuditRetentionReport,
    reason: &str,
) -> error::Result<()> {
    let metadata = fs::metadata(path).context(error::MaintenanceIoSnafu {
        context: format!("reading metadata for {}", path.display()),
    })?;
    let size = metadata.len();
    fs::remove_file(path).context(error::MaintenanceIoSnafu {
        context: format!("pruning {}", path.display()),
    })?;
    report.files_pruned += 1;
    report.bytes_freed += size;
    tracing::debug!(path = %path.display(), reason, "pruned prompt audit file");
    Ok(())
}

fn prune_malformed_by_mtime(
    entry: &fs::DirEntry,
    now: SystemTime,
    max_age: std::time::Duration,
    report: &mut PromptAuditRetentionReport,
) -> error::Result<bool> {
    let path = entry.path();
    let metadata = entry.metadata().context(error::MaintenanceIoSnafu {
        context: format!("reading metadata for {}", path.display()),
    })?;
    let modified = metadata.modified().context(error::MaintenanceIoSnafu {
        context: format!("reading mtime for {}", path.display()),
    })?;

    // kanon:ignore RUST/no-result-unwrap-or-default — future mtime is treated as not expired; zero duration correctly skips pruning
    let age = now.duration_since(modified).unwrap_or_default();
    if age > max_age {
        let size = metadata.len();
        fs::remove_file(&path).context(error::MaintenanceIoSnafu {
            context: format!("pruning {}", path.display()),
        })?;
        report.files_pruned += 1;
        report.fallback_files_pruned += 1;
        report.bytes_freed += size;
        return Ok(true);
    }

    report.malformed_files_skipped += 1;
    Ok(false)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn write_fixture(path: &std::path::Path, content: &str) {
        #[expect(
            clippy::disallowed_methods,
            reason = "test fixture: synchronous write in non-async test context"
        )]
        fs::write(path, content).expect("write");
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(path, perms).unwrap();
    }

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

    fn utc_today() -> Date {
        Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date()
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
