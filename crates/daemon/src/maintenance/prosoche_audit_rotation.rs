//! Prosoche audit report retention (#5667).
//!
//! `AuditStorage::persist` writes one timestamped JSON report per `SelfAudit`
//! run and never removes one, so the directory grows without bound and is
//! carried into every instance backup. This task enforces a retention window
//! over `prosoche-audit-<timestamp>.json` files.

use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::error;
use crate::maintenance::dated_file_retention::{DatedFileRetention, DatedFileRetentionReport};

/// Filename prefix written by `AuditStorage::persist`.
const FILENAME_PREFIX: &str = "prosoche-audit-";

/// Length of the `YYYY-MM-DD` date encoded at the start of the timestamp.
const DATE_LEN: usize = "YYYY-MM-DD".len();

/// Configuration for prosoche audit report retention.
///
/// WHY: the report directory is not repeated here — it is owned by
/// `MaintenanceConfig::prosoche_audit_dir`, the same field `AuditStorage` is
/// built from, so the pruner and the writer cannot drift onto two paths.
#[derive(Debug, Clone)]
pub struct ProsocheAuditRetentionConfig {
    /// Whether pruning is active.
    pub enabled: bool,
    /// Reports older than this many days are deleted.
    pub retention_days: u32,
}

impl Default for ProsocheAuditRetentionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
        }
    }
}

/// Prunes prosoche audit reports past the retention window.
pub struct ProsocheAuditRotator {
    config: ProsocheAuditRetentionConfig,
    report_dir: PathBuf,
}

impl ProsocheAuditRotator {
    /// Create a rotator for the directory `AuditStorage` writes to.
    #[must_use]
    pub fn new(config: ProsocheAuditRetentionConfig, report_dir: impl Into<PathBuf>) -> Self {
        Self {
            config,
            report_dir: report_dir.into(),
        }
    }

    /// Run retention over the report directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the report directory cannot be read or a report
    /// cannot be deleted. A missing directory is an empty report, not an
    /// error.
    pub fn prune(&self) -> error::Result<DatedFileRetentionReport> {
        if !self.config.enabled {
            return Ok(DatedFileRetentionReport::default());
        }

        DatedFileRetention {
            dir: &self.report_dir,
            extension: "json",
            retention_days: self.config.retention_days,
            parse_date: parse_report_date,
            label: "prosoche audit",
        }
        .prune()
    }
}

/// Extract the calendar date from a `prosoche-audit-<timestamp>.json` name.
///
/// `AuditStorage::persist` builds the stem by replacing `:` and `.` in an
/// RFC 3339 timestamp with `-`, so the date portion is the leading
/// `YYYY-MM-DD` and survives that substitution unchanged.
fn parse_report_date(path: &Path) -> Option<Date> {
    let stem = path.file_stem()?.to_str()?;
    let ts = stem.strip_prefix(FILENAME_PREFIX)?;
    if ts.len() < DATE_LEN {
        return None;
    }
    // WHY: a bare `parse` on the whole stem cannot work — the time portion has
    // had its separators rewritten and is no longer a valid RFC 3339 string.
    ts.get(..DATE_LEN)?.parse::<Date>().ok()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use jiff::ToSpan;

    use super::*;
    use crate::maintenance::test_support::utc_today;

    fn report_name(date: Date) -> String {
        format!("{FILENAME_PREFIX}{date}T04-05-06-123456789Z.json")
    }

    fn write_report(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        #[expect(
            clippy::disallowed_methods,
            reason = "test fixture: synchronous write in non-async test context"
        )]
        std::fs::write(&path, "{}").expect("write fixture");
        path
    }

    fn rotator(dir: &Path, retention_days: u32) -> ProsocheAuditRotator {
        ProsocheAuditRotator::new(
            ProsocheAuditRetentionConfig {
                enabled: true,
                retention_days,
            },
            dir,
        )
    }

    #[test]
    fn parses_date_from_persisted_filename_shape() {
        // WHY: `AuditStorage::persist` rewrites `:` and `.` to `-`, so the
        // stem is not a valid RFC 3339 string and only the leading date parses.
        let date = Date::constant(2026, 3, 14);
        let path = PathBuf::from(report_name(date));
        assert_eq!(parse_report_date(&path), Some(date));
    }

    #[test]
    fn unprefixed_json_is_not_dated() {
        assert_eq!(parse_report_date(Path::new("2026-03-14.json")), None);
    }

    #[test]
    fn prunes_reports_past_the_window_and_retains_recent_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = utc_today().checked_sub(120.days()).expect("old date");
        let recent = utc_today().checked_sub(3.days()).expect("recent date");
        let old_path = write_report(dir.path(), &report_name(old));
        let recent_path = write_report(dir.path(), &report_name(recent));

        let report = rotator(dir.path(), 90).prune().expect("prune");

        // This assertion fails against the pre-fix code, where nothing pruned.
        assert_eq!(report.files_pruned, 1);
        assert_eq!(report.files_retained, 1);
        assert!(!old_path.exists());
        assert!(recent_path.exists());
    }

    #[test]
    fn disabled_config_prunes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = utc_today().checked_sub(120.days()).expect("old date");
        let path = write_report(dir.path(), &report_name(old));

        let report = ProsocheAuditRotator::new(
            ProsocheAuditRetentionConfig {
                enabled: false,
                retention_days: 90,
            },
            dir.path(),
        )
        .prune()
        .expect("prune");

        assert_eq!(report.files_pruned, 0);
        assert!(path.exists());
    }

    #[test]
    fn missing_dir_is_an_empty_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        let report = rotator(&dir.path().join("absent"), 90)
            .prune()
            .expect("prune");
        assert_eq!(report.files_pruned, 0);
        assert_eq!(report.files_retained, 0);
    }

    #[test]
    fn non_json_siblings_are_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let note = write_report(dir.path(), "operator-notes.md");
        let report = rotator(dir.path(), 0).prune().expect("prune");
        assert_eq!(report.files_pruned, 0);
        assert!(note.exists());
    }

    #[test]
    fn future_dated_report_is_retained() {
        let dir = tempfile::tempdir().expect("tempdir");
        let future = utc_today().checked_add(5.days()).expect("future date");
        let path = write_report(dir.path(), &report_name(future));
        let report = rotator(dir.path(), 90).prune().expect("prune");
        assert_eq!(report.files_pruned, 0);
        assert_eq!(report.files_retained, 1);
        assert!(path.exists());
    }
}
