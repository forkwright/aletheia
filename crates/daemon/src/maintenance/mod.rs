//! Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.

/// Database size monitoring with configurable warning and alert thresholds.
pub(crate) mod db_monitor;
/// Instance drift detection: compare a live instance against the example template.
pub(crate) mod drift_detection;
/// Knowledge graph maintenance bridge trait and report types.
pub(crate) mod knowledge;
/// Data retention policy execution trait and summary types.
pub(crate) mod retention;
/// Trace file rotation, gzip compression, and archive pruning.
pub(crate) mod trace_rotation;

pub use db_monitor::{DbInfo, DbMonitor, DbMonitoringConfig, DbSizeReport, DbStatus};
pub use drift_detection::{DriftDetectionConfig, DriftDetector, DriftReport};
pub use knowledge::{
    AutoDreamConfig, KnowledgeMaintenanceConfig, KnowledgeMaintenanceExecutor, MaintenanceReport,
};
pub use retention::{RetentionConfig, RetentionExecutor, RetentionSummary};
pub use trace_rotation::{RotationReport, TraceRotationConfig, TraceRotator};

/// Configuration for the rule proposal generation task.
#[derive(Debug, Clone)]
pub struct ProposeRulesConfig {
    /// Whether the rule proposal task is enabled.
    pub enabled: bool,
    /// Directory where `rule_proposals.toml` is written.
    ///
    /// Defaults to `instance/data` resolved from `ALETHEIA_ROOT`.
    pub data_dir: std::path::PathBuf,
}

impl Default for ProposeRulesConfig {
    fn default() -> Self {
        let root = std::env::var("ALETHEIA_ROOT").map_or_else(
            |_e| std::path::PathBuf::from("instance"),
            std::path::PathBuf::from,
        );
        Self {
            enabled: false,
            data_dir: root.join("data"),
        }
    }
}

/// Aggregated maintenance configuration for all daemon tasks.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceConfig {
    /// Trace file rotation and compression settings.
    pub trace_rotation: TraceRotationConfig,
    /// Instance drift detection settings.
    pub drift_detection: DriftDetectionConfig,
    /// Database size monitoring thresholds.
    pub db_monitoring: DbMonitoringConfig,
    /// Data retention policy settings.
    pub retention: RetentionConfig,
    /// Knowledge graph maintenance settings.
    pub knowledge_maintenance: KnowledgeMaintenanceConfig,
    /// Cron task configuration (evolution, reflection, graph cleanup).
    pub cron: crate::cron::CronConfig,
    /// Rule proposal generation from observed patterns.
    pub propose_rules: ProposeRulesConfig,
}
