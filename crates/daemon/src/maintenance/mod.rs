//! Instance maintenance services — trace rotation, drift detection, DB monitoring, retention.

pub mod db_monitor;
pub mod drift_detection;
pub mod retention;
pub mod trace_rotation;

pub use db_monitor::{DbInfo, DbMonitor, DbMonitoringConfig, DbSizeReport, DbStatus};
pub use drift_detection::{DriftDetectionConfig, DriftDetector, DriftReport};
pub use retention::{RetentionConfig, RetentionExecutor, RetentionSummary};
pub use trace_rotation::{RotationReport, TraceRotationConfig, TraceRotator};

/// Aggregated maintenance configuration for all daemon tasks.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceConfig {
    pub trace_rotation: TraceRotationConfig,
    pub drift_detection: DriftDetectionConfig,
    pub db_monitoring: DbMonitoringConfig,
    pub retention: RetentionConfig,
}
