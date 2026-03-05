//! Database size monitoring with configurable thresholds.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error;

/// Configuration for database size monitoring.
#[derive(Debug, Clone)]
pub struct DbMonitoringConfig {
    /// Whether database monitoring is active.
    pub enabled: bool,
    /// Directory containing database files to monitor.
    pub data_dir: PathBuf,
    /// Size in MB above which a warning is reported.
    pub warn_threshold_mb: u64,
    /// Size in MB above which an alert is raised.
    pub alert_threshold_mb: u64,
}

impl Default for DbMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            data_dir: PathBuf::from("data"),
            warn_threshold_mb: 100,
            alert_threshold_mb: 500,
        }
    }
}

/// Outcome of a database size check.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DbSizeReport {
    /// Individual database entries with size and status.
    pub databases: Vec<DbInfo>,
    /// Sum of all database sizes in bytes.
    pub total_size_bytes: u64,
    /// Human-readable alert messages for databases exceeding thresholds.
    pub alerts: Vec<String>,
}

/// Information about a single database.
#[derive(Debug, Clone, Serialize)]
pub struct DbInfo {
    /// Database file or directory name (e.g., `"sessions.db"`, `"cozo/"`).
    pub name: String,
    /// Absolute path to the database file or directory.
    pub path: PathBuf,
    /// Total size in bytes.
    pub size_bytes: u64,
    /// Health classification based on configured thresholds.
    pub status: DbStatus,
}

/// Health status of a database based on size thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbStatus {
    /// Size is within normal bounds.
    Ok,
    /// Size exceeds the warning threshold but is below the alert threshold.
    Warning,
    /// Size exceeds the alert threshold and needs attention.
    Alert,
}

impl std::fmt::Display for DbStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warning => write!(f, "warning"),
            Self::Alert => write!(f, "alert"),
        }
    }
}

/// Monitors database file sizes and reports against thresholds.
pub struct DbMonitor {
    config: DbMonitoringConfig,
}

impl DbMonitor {
    /// Create a monitor with the given threshold configuration.
    pub fn new(config: DbMonitoringConfig) -> Self {
        Self { config }
    }

    /// Check all database files and return a size report.
    pub fn check(&self) -> error::Result<DbSizeReport> {
        let mut report = DbSizeReport::default();

        if !self.config.data_dir.exists() {
            return Ok(report);
        }

        // Check known databases.
        self.check_file("sessions.db", &mut report)?;
        self.check_file("planning.db", &mut report)?;

        // Scan for any other .db files.
        self.scan_db_files(&mut report)?;

        // Check cozo/ directory total size.
        let cozo_dir = self.config.data_dir.join("cozo");
        if cozo_dir.exists() {
            let size = dir_size(&cozo_dir)?;
            let status = self.classify(size);
            let name = "cozo/".to_owned();
            if status != DbStatus::Ok {
                report
                    .alerts
                    .push(format!("{name} {}MB ({status})", size / (1024 * 1024)));
            }
            report.databases.push(DbInfo {
                name,
                path: cozo_dir,
                size_bytes: size,
                status,
            });
        }

        report.total_size_bytes = report.databases.iter().map(|d| d.size_bytes).sum();

        Ok(report)
    }

    fn check_file(&self, name: &str, report: &mut DbSizeReport) -> error::Result<()> {
        let path = self.config.data_dir.join(name);
        if !path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(&path).context(error::MaintenanceIoSnafu {
            context: format!("reading metadata for {}", path.display()),
        })?;

        let size = metadata.len();
        let status = self.classify(size);

        if status != DbStatus::Ok {
            report
                .alerts
                .push(format!("{name} {}MB ({status})", size / (1024 * 1024)));
        }

        report.databases.push(DbInfo {
            name: name.to_owned(),
            path,
            size_bytes: size,
            status,
        });

        Ok(())
    }

    fn scan_db_files(&self, report: &mut DbSizeReport) -> error::Result<()> {
        let dir = fs::read_dir(&self.config.data_dir).context(error::MaintenanceIoSnafu {
            context: format!("reading data dir {}", self.config.data_dir.display()),
        })?;

        let known = ["sessions.db", "planning.db"];

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading data dir entry",
            })?;
            let path = entry.path();

            if path.is_dir() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            if !std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
                || known.contains(&name)
            {
                continue;
            }

            let metadata = entry.metadata().context(error::MaintenanceIoSnafu {
                context: format!("reading metadata for {}", path.display()),
            })?;

            let size = metadata.len();
            let status = self.classify(size);

            if status != DbStatus::Ok {
                report
                    .alerts
                    .push(format!("{name} {}MB ({status})", size / (1024 * 1024)));
            }

            report.databases.push(DbInfo {
                name: name.to_owned(),
                path,
                size_bytes: size,
                status,
            });
        }

        Ok(())
    }

    fn classify(&self, size_bytes: u64) -> DbStatus {
        let size_mb = size_bytes / (1024 * 1024);
        if size_mb >= self.config.alert_threshold_mb {
            DbStatus::Alert
        } else if size_mb >= self.config.warn_threshold_mb {
            DbStatus::Warning
        } else {
            DbStatus::Ok
        }
    }
}

fn dir_size(path: &std::path::Path) -> error::Result<u64> {
    let mut total = 0u64;
    let dir = fs::read_dir(path).context(error::MaintenanceIoSnafu {
        context: format!("reading dir size {}", path.display()),
    })?;

    for entry in dir {
        let entry = entry.context(error::MaintenanceIoSnafu {
            context: "reading dir entry",
        })?;
        let p = entry.path();
        if p.is_dir() {
            total += dir_size(&p)?;
        } else {
            total += entry
                .metadata()
                .context(error::MaintenanceIoSnafu {
                    context: format!("reading file size {}", p.display()),
                })?
                .len();
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(tmp: &std::path::Path) -> DbMonitoringConfig {
        DbMonitoringConfig {
            enabled: true,
            data_dir: tmp.join("data"),
            warn_threshold_mb: 1,
            alert_threshold_mb: 5,
        }
    }

    #[test]
    fn small_db_returns_ok() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();
        fs::write(config.data_dir.join("sessions.db"), "small").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases.len(), 1);
        assert_eq!(report.databases[0].status, DbStatus::Ok);
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn db_above_warn_returns_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        // Create a file > 1MB but < 5MB.
        let data = vec![0u8; 2 * 1024 * 1024];
        fs::write(config.data_dir.join("sessions.db"), &data).unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases[0].status, DbStatus::Warning);
        assert!(!report.alerts.is_empty());
    }

    #[test]
    fn db_above_alert_returns_alert() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        // Create a file > 5MB.
        let data = vec![0u8; 6 * 1024 * 1024];
        fs::write(config.data_dir.join("sessions.db"), &data).unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases[0].status, DbStatus::Alert);
        assert!(!report.alerts.is_empty());
    }

    #[test]
    fn total_size_calculated_correctly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        let data1 = vec![0u8; 100];
        let data2 = vec![0u8; 200];
        fs::write(config.data_dir.join("sessions.db"), &data1).unwrap();
        fs::write(config.data_dir.join("planning.db"), &data2).unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases.len(), 2);
        assert_eq!(report.total_size_bytes, 300);
    }

    #[test]
    fn extra_db_files_detected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        fs::write(config.data_dir.join("custom.db"), "data").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases.len(), 1);
        assert_eq!(report.databases[0].name, "custom.db");
    }

    #[test]
    fn nonexistent_data_dir_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = DbMonitoringConfig {
            data_dir: tmp.path().join("nonexistent"),
            ..make_config(tmp.path())
        };

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("should not error");
        assert!(report.databases.is_empty());
        assert_eq!(report.total_size_bytes, 0);
    }
}
