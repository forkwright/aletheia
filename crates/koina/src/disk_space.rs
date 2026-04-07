//! Disk space monitoring for proactive write protection.

use std::fmt;
use std::path::Path;


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

impl DiskStatus {
    /// Human-readable status level name.
    #[must_use]
    fn level_name(self) -> &'static str {
        match self {
            Self::Ok { .. } => "ok",
            Self::Warning { .. } => "warning",
            Self::Critical { .. } => "critical",
        }
    }
}

impl fmt::Display for DiskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} MB available)",
            self.level_name(),
            self.available_bytes() / BYTES_PER_MB
        )
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
    fn disk_status_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DiskStatus>();
    }
}
