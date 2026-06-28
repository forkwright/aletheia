//! Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.

use std::path::PathBuf;
use std::sync::Arc;

use koina::system::{Environment, RealSystem};

/// Database size monitoring with configurable warning and alert thresholds.
pub(crate) mod db_monitor;
/// Instance drift detection: compare a live instance against the example template.
pub(crate) mod drift_detection;
/// Fjall knowledge store file-level backup with timestamped snapshots.
pub mod fjall_backup;
/// Whole-instance backup set covering knowledge, sessions, working checkpoints,
/// config, and workspace data.
pub mod instance_backup;
/// Knowledge graph maintenance bridge trait and report types.
pub(crate) mod knowledge;
/// Prompt audit log retention pruning (#3411).
pub(crate) mod prompt_audit_rotation;
/// Canonical maintenance task registry.
pub mod registry;
/// Data retention policy execution trait and summary types.
pub(crate) mod retention;
/// Trace file rotation, gzip compression, and archive pruning.
pub(crate) mod trace_rotation;

pub use db_monitor::{DbInfo, DbMonitor, DbMonitoringConfig, DbSizeReport, DbStatus};
pub use drift_detection::{DriftDetectionConfig, DriftDetector, DriftReport};
pub use fjall_backup::{FjallBackup, FjallBackupConfig, FjallBackupReport, FjallVerifyResult};
pub use instance_backup::{
    BackupManifest, InstanceBackup, InstanceBackupConfig, InstanceBackupReport,
    InstanceVerifyResult, StoreEntry, WorkspaceOmission,
};
pub use knowledge::{
    AutoDreamConfig, DerivedRulesConfig, KnowledgeMaintenanceConfig, KnowledgeMaintenanceExecutor,
    MaintenanceOutcome, MaintenanceReport, SerendipityMaintenanceConfig,
};
pub use prompt_audit_rotation::{
    PromptAuditRetentionConfig, PromptAuditRetentionReport, PromptAuditRotator,
};
pub use registry::{
    MaintenanceConfigSection, MaintenanceRuntimeCapabilities, MaintenanceTaskDefinition,
    MaintenanceTaskImplementationStatus, MaintenanceTaskOwner, ManualMaintenanceTask,
    SkippedMaintenanceWarning, maintenance_task_by_id, maintenance_task_registry,
    manual_maintenance_task_ids, manual_maintenance_tasks,
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
        let root = RealSystem.var("ALETHEIA_ROOT").map_or_else(
            || std::path::PathBuf::from("instance"),
            std::path::PathBuf::from,
        );
        Self {
            enabled: false,
            data_dir: root.join("data"),
        }
    }
}

/// Records backup observability metrics from the runtime crate.
pub trait BackupMetricsRecorder: std::fmt::Debug + Send + Sync {
    /// Record one backup attempt duration.
    fn record_backup_duration(&self, duration_secs: f64, success: bool);
}

/// Aggregated maintenance configuration for all daemon tasks.
#[derive(Debug, Clone)]
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
    /// Whole-instance backup settings.
    pub instance_backup: InstanceBackupConfig,
    /// Runtime metrics hook for backup freshness alerting.
    pub backup_metrics: Option<Arc<dyn BackupMetricsRecorder>>,
    /// Directory where prosoche self-audit reports are written.
    pub prosoche_audit_dir: PathBuf,
    /// Cron task configuration (evolution, reflection, graph cleanup).
    pub cron: crate::cron::CronConfig,
    /// Rule proposal generation from observed patterns.
    pub propose_rules: ProposeRulesConfig,
    /// Prompt audit log retention pruning (#3411).
    pub prompt_audit: PromptAuditRetentionConfig,
    /// Runtime handle for refreshing empirical routing after-action stats.
    pub after_action_store: Option<Arc<aletheia_routing::AfterActionStore>>,
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        let root = RealSystem.var("ALETHEIA_ROOT").map_or_else(
            || std::path::PathBuf::from("instance"),
            std::path::PathBuf::from,
        );
        Self {
            trace_rotation: TraceRotationConfig::default(),
            drift_detection: DriftDetectionConfig::default(),
            db_monitoring: DbMonitoringConfig::default(),
            retention: RetentionConfig::default(),
            knowledge_maintenance: KnowledgeMaintenanceConfig::default(),
            instance_backup: InstanceBackupConfig::default(),
            backup_metrics: None,
            prosoche_audit_dir: root.join("data").join("prosoche-audits"),
            cron: crate::cron::CronConfig::default(),
            propose_rules: ProposeRulesConfig::default(),
            prompt_audit: PromptAuditRetentionConfig::default(),
            after_action_store: None,
        }
    }
}
