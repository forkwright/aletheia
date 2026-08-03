//! Shared retention primitive for directories of date-stamped files.
//!
//! Several maintenance tasks prune a flat directory whose filenames encode the
//! day the file was written: prompt audit logs (`YYYY-MM-DD.jsonl`, #3411) and
//! prosoche audit reports (`prosoche-audit-<timestamp>.json`, #5667). Both need
//! the same rules — delete past the retention window by filename date, fall back
//! to mtime when the name does not parse, and leave unrelated extensions alone.
//!
//! WHY: this module owns that behaviour once. Callers supply the directory, the
//! extension they own, and a filename-to-date parser; they do not reimplement
//! the walk, the cutoff arithmetic, or the mtime fallback.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use jiff::civil::Date;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error;

pub(crate) const SECONDS_PER_DAY: u64 = 86_400;

/// Outcome of a dated-file retention run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatedFileRetentionReport {
    /// Number of files deleted, by filename date and by mtime fallback.
    pub files_pruned: u32,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Number of files retained because their filename date is within the window.
    pub files_retained: u32,
    /// Number of files with an unparseable name retained by mtime fallback.
    pub malformed_files_skipped: u32,
    /// Number of files with an unparseable name deleted by mtime fallback.
    pub fallback_files_pruned: u32,
}

/// Parameters for a single retention run.
pub struct DatedFileRetention<'a, F> {
    /// Directory to prune. A missing directory yields an empty report.
    pub dir: &'a Path,
    /// Only files with this extension are considered.
    pub extension: &'a str,
    /// Files older than this many days are deleted.
    pub retention_days: u32,
    /// Maps a path to the date encoded in its filename, if it parses.
    pub parse_date: F,
    /// Label used in trace output to identify the calling task.
    pub label: &'a str,
}

impl<F> DatedFileRetention<'_, F>
where
    F: Fn(&Path) -> Option<Date>,
{
    /// Delete files past the retention window.
    ///
    /// Files whose name parses to a date are judged on that date. Files whose
    /// name does not parse fall back to mtime, so a corrupted or hand-renamed
    /// file is still eventually reclaimed rather than pinned forever.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read or a file cannot be
    /// deleted. A missing directory is an empty report, not an error, so the
    /// task can be enabled before the producing code has written anything.
    pub fn prune(&self) -> error::Result<DatedFileRetentionReport> {
        let mut report = DatedFileRetentionReport::default();
        if !self.dir.exists() {
            return Ok(report);
        }

        let now = SystemTime::now();
        let today = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
        let cutoff_date = today
            .checked_sub(i64::from(self.retention_days).days())
            .unwrap_or(today);
        let max_age =
            std::time::Duration::from_secs(u64::from(self.retention_days) * SECONDS_PER_DAY);

        let dir = fs::read_dir(self.dir).context(error::MaintenanceIoSnafu {
            context: format!("reading {} dir {}", self.label, self.dir.display()),
        })?;

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: format!("reading {} directory entry", self.label),
            })?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            // WHY: only prune the extension this task owns — leave any sidecar
            // files alone so operators can drop notes or reports next to the
            // directory without the daemon deleting them.
            if path.extension().and_then(|e| e.to_str()) != Some(self.extension) {
                continue;
            }

            if let Some(file_date) = (self.parse_date)(&path) {
                if file_date < cutoff_date {
                    self.prune_file(&path, &mut report, "by filename date")?;
                } else {
                    report.files_retained += 1;
                }
            } else if Self::prune_malformed_by_mtime(&entry, now, max_age, &mut report)? {
                tracing::debug!(
                    path = %path.display(),
                    label = self.label,
                    "pruned malformed file by mtime fallback"
                );
            } else {
                tracing::debug!(
                    path = %path.display(),
                    label = self.label,
                    "retained malformed file by mtime fallback"
                );
            }
        }

        Ok(report)
    }

    fn prune_file(
        &self,
        path: &Path,
        report: &mut DatedFileRetentionReport,
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
        tracing::debug!(path = %path.display(), label = self.label, reason, "pruned file");
        Ok(())
    }

    fn prune_malformed_by_mtime(
        entry: &fs::DirEntry,
        now: SystemTime,
        max_age: std::time::Duration,
        report: &mut DatedFileRetentionReport,
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
}
