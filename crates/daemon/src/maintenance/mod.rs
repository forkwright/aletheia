//! Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.

/// Database size monitoring with configurable warning and alert thresholds.
pub mod db_monitor;
/// Instance drift detection: compare a live instance against the example template.
pub mod drift_detection;
/// Knowledge graph maintenance bridge trait and report types.
pub mod knowledge;
/// Data retention policy execution trait and summary types.
pub mod retention;
/// Trace file rotation, gzip compression, and archive pruning.
pub mod trace_rotation;

pub use db_monitor::{DbInfo, DbMonitor, DbMonitoringConfig, DbSizeReport, DbStatus};
pub use drift_detection::{DriftDetectionConfig, DriftDetector, DriftReport};
pub use knowledge::{KnowledgeMaintenanceConfig, KnowledgeMaintenanceExecutor, MaintenanceReport};
pub use retention::{RetentionConfig, RetentionExecutor, RetentionSummary};
pub use trace_rotation::{RotationReport, TraceRotationConfig, TraceRotator};

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
}
