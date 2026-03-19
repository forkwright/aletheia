//! Disk space monitoring for proactive write protection.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const BYTES_PER_MB: u64 = 1024 * 1024;

/// Default warning threshold: 1 GB.
pub const DEFAULT_WARNING_BYTES: u64 = 1024 * BYTES_PER_MB;

/// Default critical threshold: 100 MB.
pub const DEFAULT_CRITICAL_BYTES: u64 = 100 * BYTES_PER_MB;

/// Disk space status relative to configured thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskStatus {
    /// Available space is above the warning threshold.
    Ok {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
    /// Available space is below the warning threshold but above critical.
    Warning {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
    /// Available space is below the critical threshold.
    Critical {
        /// Bytes available on the filesystem.
        available_bytes: u64,
    },
}

impl DiskStatus {
    /// Returns the available bytes regardless of status level.
    #[must_use]
    pub fn available_bytes(self) -> u64 {
        match self {
            Self::Ok { available_bytes }
            | Self::Warning { available_bytes }
            | Self::Critical { available_bytes } => available_bytes,
        }
    }

    /// Returns `true` when space is at the critical level.
    #[must_use]
    pub fn is_critical(self) -> bool {
        matches!(self, Self::Critical { .. })
    }
}

impl fmt::Display for DiskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mb = self.available_bytes() / BYTES_PER_MB;
        match self {
            Self::Ok { .. } => write!(f, "ok ({mb} MB available)"),
            Self::Warning { .. } => write!(f, "warning ({mb} MB available)"),
            Self::Critical { .. } => write!(f, "critical ({mb} MB available)"),
        }
    }
}

/// Query available disk space for the filesystem containing `path`.
///
/// # Errors
///
/// Returns an I/O error if the `statvfs` syscall fails (e.g. path does not
/// exist or is not accessible).
pub fn available_space(path: &Path) -> std::io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path contains interior null byte: {e}"),
        )
    })?;

    let stat = rustix::fs::statvfs(c_path.as_c_str())
        .map_err(|e| std::io::Error::from_raw_os_error(e.raw_os_error()))?;

    // WHY: f_bavail (blocks available to unprivileged users) * f_frsize
    // (fragment size) gives the bytes available for non-root writes.
    Ok(stat.f_bavail * stat.f_frsize)
}

/// Check disk space and classify against thresholds.
pub fn check_disk_space(
    path: &Path,
    warning_bytes: u64,
    critical_bytes: u64,
) -> std::io::Result<DiskStatus> {
    let avail = available_space(path)?;
    Ok(classify(avail, warning_bytes, critical_bytes))
}

/// Classify an available-bytes value against thresholds.
fn classify(available_bytes: u64, warning_bytes: u64, critical_bytes: u64) -> DiskStatus {
    if available_bytes < critical_bytes {
        DiskStatus::Critical { available_bytes }
    } else if available_bytes < warning_bytes {
        DiskStatus::Warning { available_bytes }
    } else {
        DiskStatus::Ok { available_bytes }
    }
}

/// Shared disk space monitor backed by an [`AtomicU64`].
///
/// The monitor caches the last-known available bytes so that write paths can
/// check disk status without issuing a syscall on every operation. A
/// background task should call [`DiskSpaceMonitor::refresh`] periodically.
#[derive(Clone)]
pub struct DiskSpaceMonitor {
    cached_available: Arc<AtomicU64>,
    warn_threshold: u64,
    critical_threshold: u64,
}

impl DiskSpaceMonitor {
    /// Create a new monitor with the given thresholds (in bytes).
    ///
    /// The initial cached value is `u64::MAX` (assumes space is available
    /// until the first [`refresh`](Self::refresh) completes).
    #[must_use]
    pub fn new(warning_bytes: u64, critical_bytes: u64) -> Self {
        Self {
            cached_available: Arc::new(AtomicU64::new(u64::MAX)),
            warn_threshold: warning_bytes,
            critical_threshold: critical_bytes,
        }
    }

    /// Refresh the cached value by querying the filesystem at `path`.
    ///
    /// Returns the new [`DiskStatus`] after updating the cache.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `statvfs` fails.
    pub fn refresh(&self, path: &Path) -> std::io::Result<DiskStatus> {
        let avail = available_space(path)?;
        self.cached_available.store(avail, Ordering::Relaxed);
        Ok(classify(
            avail,
            self.warn_threshold,
            self.critical_threshold,
        ))
    }

    /// Current disk status based on the last cached value.
    #[must_use]
    pub fn status(&self) -> DiskStatus {
        let avail = self.cached_available.load(Ordering::Relaxed);
        classify(avail, self.warn_threshold, self.critical_threshold)
    }

    /// Returns `true` if non-essential writes (logs, caches, backups) should
    /// proceed. Returns `false` when disk space is at the critical level.
    #[must_use]
    pub fn allow_non_essential_write(&self) -> bool {
        !self.status().is_critical()
    }

    /// Warning threshold in bytes.
    #[must_use]
    pub fn warning_bytes(&self) -> u64 {
        self.warn_threshold
    }

    /// Critical threshold in bytes.
    #[must_use]
    pub fn critical_bytes(&self) -> u64 {
        self.critical_threshold
    }
}

impl fmt::Debug for DiskSpaceMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiskSpaceMonitor")
            .field("status", &self.status())
            .field("warn_threshold_mb", &(self.warn_threshold / BYTES_PER_MB))
            .field(
                "critical_threshold_mb",
                &(self.critical_threshold / BYTES_PER_MB),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn classify_returns_ok_above_warning() {
        let status = classify(2_000_000_000, DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            matches!(status, DiskStatus::Ok { .. }),
            "2 GB should be Ok, got {status}"
        );
    }

    #[test]
    fn classify_returns_warning_between_thresholds() {
        let status = classify(500_000_000, DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            matches!(status, DiskStatus::Warning { .. }),
            "500 MB should be Warning, got {status}"
        );
    }

    #[test]
    fn classify_returns_critical_below_critical() {
        let status = classify(50_000_000, DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            matches!(status, DiskStatus::Critical { .. }),
            "50 MB should be Critical, got {status}"
        );
    }

    #[test]
    fn classify_at_exact_warning_boundary_is_ok() {
        let status = classify(
            DEFAULT_WARNING_BYTES,
            DEFAULT_WARNING_BYTES,
            DEFAULT_CRITICAL_BYTES,
        );
        assert!(
            matches!(status, DiskStatus::Ok { .. }),
            "exactly at warning threshold should be Ok, got {status}"
        );
    }

    #[test]
    fn classify_at_exact_critical_boundary_is_warning() {
        let status = classify(
            DEFAULT_CRITICAL_BYTES,
            DEFAULT_WARNING_BYTES,
            DEFAULT_CRITICAL_BYTES,
        );
        assert!(
            matches!(status, DiskStatus::Warning { .. }),
            "exactly at critical threshold should be Warning, got {status}"
        );
    }

    #[test]
    fn classify_zero_bytes_is_critical() {
        let status = classify(0, DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            matches!(status, DiskStatus::Critical { .. }),
            "0 bytes should be Critical, got {status}"
        );
    }

    #[test]
    fn monitor_initial_status_is_ok() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            matches!(monitor.status(), DiskStatus::Ok { .. }),
            "initial status should be Ok (u64::MAX cached)"
        );
    }

    #[test]
    fn monitor_status_reflects_cached_value() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        monitor
            .cached_available
            .store(50_000_000, Ordering::Relaxed);
        assert!(
            monitor.status().is_critical(),
            "50 MB cached should produce Critical status"
        );
    }

    #[test]
    fn monitor_allow_non_essential_write_at_ok() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        assert!(
            monitor.allow_non_essential_write(),
            "should allow non-essential writes when Ok"
        );
    }

    #[test]
    fn monitor_blocks_non_essential_write_at_critical() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        monitor
            .cached_available
            .store(50_000_000, Ordering::Relaxed);
        assert!(
            !monitor.allow_non_essential_write(),
            "should block non-essential writes when Critical"
        );
    }

    #[test]
    fn monitor_allows_non_essential_write_at_warning() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        monitor
            .cached_available
            .store(500_000_000, Ordering::Relaxed);
        assert!(
            monitor.allow_non_essential_write(),
            "should allow non-essential writes when Warning"
        );
    }

    #[test]
    fn available_bytes_accessor_returns_value() {
        let status = DiskStatus::Warning {
            available_bytes: 42,
        };
        assert_eq!(status.available_bytes(), 42);
    }

    #[test]
    fn disk_status_display_includes_mb() {
        let status = DiskStatus::Warning {
            available_bytes: 500 * BYTES_PER_MB,
        };
        let display = status.to_string();
        assert!(
            display.contains("500"),
            "display should contain MB value: {display}"
        );
        assert!(
            display.contains("warning"),
            "display should contain level: {display}"
        );
    }

    #[test]
    fn check_disk_space_returns_valid_status() {
        let status = check_disk_space(
            Path::new("/"),
            DEFAULT_WARNING_BYTES,
            DEFAULT_CRITICAL_BYTES,
        )
        .unwrap();
        // NOTE: On any real filesystem "/" should have some space.
        assert!(status.available_bytes() > 0, "root fs should have space");
    }

    #[test]
    fn refresh_updates_cached_value() {
        let monitor = DiskSpaceMonitor::new(DEFAULT_WARNING_BYTES, DEFAULT_CRITICAL_BYTES);
        let status = monitor.refresh(Path::new("/")).unwrap();
        assert!(
            status.available_bytes() > 0,
            "root fs should report available space"
        );
        assert_eq!(
            monitor.status().available_bytes(),
            status.available_bytes(),
            "cached value should match refresh result"
        );
    }

    #[test]
    fn monitor_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DiskSpaceMonitor>();
    }

    #[test]
    fn disk_status_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DiskStatus>();
    }
}
