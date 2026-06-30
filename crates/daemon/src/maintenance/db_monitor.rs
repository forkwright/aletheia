//! Database size monitoring with configurable thresholds.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mneme::store::SessionStore;
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
    /// Human-readable alert messages for databases exceeding thresholds or failing health probes.
    pub alerts: Vec<String>,
}

/// Information about a single database.
#[derive(Debug, Clone, Serialize)]
pub struct DbInfo {
    /// Database file or legacy directory name.
    pub name: String,
    /// Absolute path to the database file or directory.
    pub path: PathBuf,
    /// Filesystem shape of the database path.
    pub shape: DbShape,
    /// Total size in bytes.
    pub size_bytes: u64,
    /// Health classification based on configured thresholds.
    pub status: DbStatus,
    /// Lightweight store open/read health, when this monitor knows how to verify it.
    pub health: DbHealth,
}

/// Filesystem shape for a database path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbShape {
    /// Single-file database or legacy store.
    File,
    /// Directory-backed store.
    Directory,
}

impl std::fmt::Display for DbShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Directory => write!(f, "directory"),
        }
    }
}

/// Store-level health for a monitored database path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "detail", rename_all = "kebab-case")]
pub enum DbHealth {
    /// Store opened and a lightweight partition read succeeded.
    Healthy,
    /// File-shaped `sessions.db` is present for legacy compatibility and was not opened as fjall.
    LegacyFile,
    /// The database has no known store-level health probe.
    NotChecked,
    /// The store is currently locked by an active writer.
    Locked(String),
    /// The store-level health probe failed.
    Unhealthy(String),
}

impl std::fmt::Display for DbHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::LegacyFile => write!(f, "legacy-file"),
            Self::NotChecked => write!(f, "not-checked"),
            Self::Locked(reason) => write!(f, "locked: {reason}"),
            Self::Unhealthy(reason) => write!(f, "unhealthy: {reason}"),
        }
    }
}

/// Runtime hook for checking the already-open session store.
pub trait SessionStoreHealthProbe: std::fmt::Debug + Send + Sync {
    /// Return lightweight session-store health from the active runtime handle.
    fn check_session_store(&self) -> DbHealth;
}

/// Health status of a database based on size thresholds.
#[non_exhaustive]
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
    session_store_health_probe: Option<Arc<dyn SessionStoreHealthProbe>>,
}

impl DbMonitor {
    /// Create a monitor with the given threshold configuration.
    #[must_use]
    pub fn new(config: DbMonitoringConfig) -> Self {
        Self {
            config,
            session_store_health_probe: None,
        }
    }

    /// Attach a runtime session-store health probe.
    #[must_use]
    pub fn with_session_store_health_probe(
        mut self,
        probe: Option<Arc<dyn SessionStoreHealthProbe>>,
    ) -> Self {
        self.session_store_health_probe = probe;
        self
    }

    /// Check all database files and return a size report.
    pub fn check(&self) -> error::Result<DbSizeReport> {
        let mut report = DbSizeReport::default();

        if !self.config.data_dir.exists() {
            return Ok(report);
        }

        self.check_sessions_store(&mut report)?;
        self.check_path("planning.db", DbHealth::NotChecked, &mut report)?;

        self.scan_db_files(&mut report)?;

        // WHY: Retain visibility into stale pre-Krites data directories during
        // operator cleanup without presenting them as current storage.
        let legacy_cozo_dir = self.config.data_dir.join("cozo");
        if legacy_cozo_dir.exists() {
            let size = dir_size(&legacy_cozo_dir)?;
            let status = self.classify(size);
            let name = "legacy-cozo/".to_owned();
            if status != DbStatus::Ok {
                report
                    .alerts
                    .push(format!("{name} {}MB ({status})", size / (1024 * 1024)));
            }
            report.databases.push(DbInfo {
                name,
                path: legacy_cozo_dir,
                shape: DbShape::Directory,
                size_bytes: size,
                status,
                health: DbHealth::NotChecked,
            });
        }

        report.total_size_bytes = report.databases.iter().map(|d| d.size_bytes).sum();

        Ok(report)
    }

    fn check_sessions_store(&self, report: &mut DbSizeReport) -> error::Result<()> {
        let name = "sessions.db";
        let path = self.config.data_dir.join(name);
        if !path.exists() {
            return Ok(());
        }

        let (shape, size) = path_shape_size(&path)?;
        let health = match shape {
            DbShape::File => DbHealth::LegacyFile,
            DbShape::Directory => self.check_session_store_health(&path),
        };

        self.push_db_info(name.to_owned(), path, shape, size, health, report);

        Ok(())
    }

    fn check_session_store_health(&self, path: &Path) -> DbHealth {
        if let Some(probe) = &self.session_store_health_probe {
            return probe.check_session_store();
        }

        if !path.join("version").is_file() {
            return DbHealth::Unhealthy("missing fjall version marker".to_owned());
        }

        let result = SessionStore::open(path).and_then(|store| {
            store.ping()?;
            store.find_session_by_id("__aletheia_diagnostic_probe__")?;
            Ok(())
        });

        match result {
            Ok(()) => DbHealth::Healthy,
            Err(err) => {
                let reason = err.to_string();
                if reason.to_ascii_lowercase().contains("locked") {
                    DbHealth::Locked(reason)
                } else {
                    DbHealth::Unhealthy(reason)
                }
            }
        }
    }

    fn check_path(
        &self,
        name: &str,
        health: DbHealth,
        report: &mut DbSizeReport,
    ) -> error::Result<()> {
        let path = self.config.data_dir.join(name);
        if !path.exists() {
            return Ok(());
        }

        let (shape, size) = path_shape_size(&path)?;
        self.push_db_info(name.to_owned(), path, shape, size, health, report);

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

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                // kanon:ignore RUST/no-result-unwrap-or-default — fallback to empty string is safe: extension check below skips unnamed files
                .unwrap_or_default();

            if !std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
                || known.contains(&name)
            {
                continue;
            }

            let (shape, size) = path_shape_size(&path)?;
            self.push_db_info(
                name.to_owned(),
                path,
                shape,
                size,
                DbHealth::NotChecked,
                report,
            );
        }

        Ok(())
    }

    fn push_db_info(
        &self,
        name: String,
        path: PathBuf,
        shape: DbShape,
        size: u64,
        health: DbHealth,
        report: &mut DbSizeReport,
    ) {
        let status = self.classify(size);

        if status != DbStatus::Ok {
            report
                .alerts
                .push(format!("{name} {}MB ({status})", size / (1024 * 1024)));
        }
        if let DbHealth::Unhealthy(reason) = &health {
            report
                .alerts
                .push(format!("{name} health unhealthy: {reason}"));
        }

        report.databases.push(DbInfo {
            name,
            path,
            shape,
            size_bytes: size,
            status,
            health,
        });
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

fn path_shape_size(path: &Path) -> error::Result<(DbShape, u64)> {
    let metadata = fs::metadata(path).context(error::MaintenanceIoSnafu {
        context: format!("reading metadata for {}", path.display()),
    })?;

    if metadata.is_dir() {
        Ok((DbShape::Directory, dir_size(path)?))
    } else {
        Ok((DbShape::File, metadata.len()))
    }
}

fn dir_size(path: &Path) -> error::Result<u64> {
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
mod tests {
    use super::*;
    use mneme::store::SessionStore;

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
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("sessions.db"), "small").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases.len(), 1);
        assert_eq!(report.databases[0].status, DbStatus::Ok);
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn sessions_db_directory_reports_recursive_size_and_health() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();
        let sessions_path = config.data_dir.join("sessions.db");
        {
            let store = SessionStore::open(&sessions_path).expect("open session store");
            store
                .create_session("ses-1", "alice", "main", None, None)
                .expect("create session");
        }

        let expected_size = dir_size(&sessions_path).expect("directory size");
        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        let sessions = report
            .databases
            .iter()
            .find(|d| d.name == "sessions.db")
            .expect("sessions.db present");
        assert_eq!(sessions.shape, DbShape::Directory);
        assert_eq!(sessions.health, DbHealth::Healthy);
        assert_eq!(sessions.size_bytes, expected_size);
        assert_eq!(report.total_size_bytes, expected_size);
    }

    #[test]
    fn sessions_db_legacy_file_is_explicit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("sessions.db"), "legacy").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        let sessions = report
            .databases
            .iter()
            .find(|d| d.name == "sessions.db")
            .expect("sessions.db present");
        assert_eq!(sessions.shape, DbShape::File);
        assert_eq!(sessions.health, DbHealth::LegacyFile);
        assert_eq!(sessions.size_bytes, 6);
    }

    #[test]
    fn db_above_warn_returns_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        let data = vec![0u8; 2 * 1024 * 1024];
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
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

        let data = vec![0u8; 6 * 1024 * 1024];
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("sessions.db"), &data1).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
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

        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("custom.db"), "data").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        assert_eq!(report.databases.len(), 1);
        assert_eq!(report.databases[0].name, "custom.db");
    }

    #[test]
    fn extra_db_directories_are_detected_with_recursive_size() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(config.data_dir.join("custom.db/nested")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("custom.db/root.dat"), "abc").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("custom.db/nested/leaf.dat"), "de").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        let custom = report
            .databases
            .iter()
            .find(|d| d.name == "custom.db")
            .expect("custom.db directory present");
        assert_eq!(custom.shape, DbShape::Directory);
        assert_eq!(custom.size_bytes, 5);
        assert_eq!(custom.health, DbHealth::NotChecked);
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

    #[test]
    fn default_config_values() {
        let config = DbMonitoringConfig::default();
        assert!(config.enabled);
        assert_eq!(config.data_dir, PathBuf::from("data"));
        assert_eq!(config.warn_threshold_mb, 100);
        assert_eq!(config.alert_threshold_mb, 500);
    }

    #[test]
    fn db_status_display() {
        assert_eq!(DbStatus::Ok.to_string(), "ok");
        assert_eq!(DbStatus::Warning.to_string(), "warning");
        assert_eq!(DbStatus::Alert.to_string(), "alert");
    }

    #[test]
    fn legacy_cozo_directory_tracked() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(config.data_dir.join("cozo")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("cozo/shard1.dat"), "aaaa").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("cozo/shard2.dat"), "bbbb").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        let legacy_cozo = report
            .databases
            .iter()
            .find(|d| d.name == "legacy-cozo/")
            .expect("should have legacy cozo entry");
        assert_eq!(legacy_cozo.size_bytes, 8, "sum of two 4-byte files");
    }

    #[test]
    fn multiple_known_and_extra_dbs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("sessions.db"), "s").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("planning.db"), "p").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        fs::write(config.data_dir.join("custom.db"), "c").unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");

        let names: Vec<&str> = report.databases.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"sessions.db"));
        assert!(names.contains(&"planning.db"));
        assert!(names.contains(&"custom.db"));
        assert_eq!(report.databases.len(), 3);
    }

    #[test]
    fn empty_data_dir_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = make_config(tmp.path());
        fs::create_dir_all(&config.data_dir).unwrap();

        let monitor = DbMonitor::new(config);
        let report = monitor.check().expect("check succeeds");
        assert!(report.databases.is_empty());
        assert_eq!(report.total_size_bytes, 0);
    }
}
