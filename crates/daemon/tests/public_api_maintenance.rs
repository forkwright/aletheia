#![expect(
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs files share the same import block; not every file uses every item"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use oikonomos::bridge::{DaemonBridge, NoopBridge};
use oikonomos::coordination::Coordinator;
use oikonomos::cron::{
    CronConfig, CronEvolutionConfig, CronGraphCleanupConfig, CronReflectionConfig,
};
use oikonomos::error::Error as DaemonError;
use oikonomos::maintenance::{
    AutoDreamConfig, DbHealth, DbMonitor, DbMonitoringConfig, DbShape, DbStatus,
    DriftDetectionConfig, DriftDetector, KnowledgeMaintenanceConfig, MaintenanceConfig,
    MaintenanceReport, ProposeRulesConfig, RetentionConfig, RetentionExecutor, RetentionSummary,
    TraceRotationConfig, TraceRotator,
};
use oikonomos::probe::{
    Probe, ProbeAuditConfig, ProbeAuditSummary, ProbeCategory, ProbeResult, ProbeSet,
    build_probe_audit_prompt,
};
use oikonomos::runner::{DaemonOutputMode, ExecutionResult, TaskRunner};
use oikonomos::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus};
use oikonomos::self_prompt::{SELF_PROMPT_SESSION_KEY, SelfPromptConfig};
use oikonomos::state::{AllowedTriggers, DaemonConfig, WorkspaceGuard};
use oikonomos::triggers::TriggerRouter;

mod common;
use common::{make_runner, write_fixture};

// ── Real-implementation maintenance tasks: TraceRotator, DriftDetector, DbMonitor ──

#[test]
fn trace_rotator_rotates_old_files_via_public_api() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let trace_dir = tmp.path().join("traces");
    let archive_dir = trace_dir.join("archive");
    fs::create_dir_all(&trace_dir).expect("create trace dir");

    write_fixture(trace_dir.join("session-1.log"), b"old log data");
    write_fixture(trace_dir.join("session-2.log"), b"more old log data");

    let config = TraceRotationConfig {
        enabled: true,
        trace_dir: trace_dir.clone(),
        archive_dir: archive_dir.clone(),
        max_age_days: 0,          // WHY: treat every file as past retention
        max_total_size_mb: 9_999, // WHY: disable size-triggered rotation
        compress: false,
        max_archives: 100,
    };

    let rotator = TraceRotator::new(config);
    let report = rotator.rotate().expect("rotation succeeds");

    assert_eq!(report.files_rotated, 2, "both fixtures should be rotated");
    assert!(
        report.bytes_freed > 0,
        "rotation must report non-zero bytes freed"
    );
    assert!(
        archive_dir.join("session-1.log").exists(),
        "session-1 should be in archive"
    );
    assert!(
        archive_dir.join("session-2.log").exists(),
        "session-2 should be in archive"
    );
}

#[test]
fn trace_rotator_nonexistent_dir_returns_empty_report() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = TraceRotationConfig {
        enabled: true,
        trace_dir: tmp.path().join("does-not-exist"),
        archive_dir: tmp.path().join("archive"),
        max_age_days: 1,
        max_total_size_mb: 1,
        compress: false,
        max_archives: 1,
    };

    let rotator = TraceRotator::new(config);
    let report = rotator.rotate().expect("missing dir must not error");

    assert_eq!(report.files_rotated, 0);
    assert_eq!(report.files_pruned, 0);
    assert_eq!(report.bytes_freed, 0);
}

#[test]
fn drift_detector_reports_missing_files_via_public_api() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    let example_root = tmp.path().join("example");

    fs::create_dir_all(example_root.join("config")).expect("mkdir config");
    write_fixture(example_root.join("config/aletheia.toml"), b"# template");
    fs::create_dir_all(&instance_root).expect("mkdir instance");

    let config = DriftDetectionConfig {
        enabled: true,
        instance_root: instance_root.clone(),
        example_root: example_root.clone(),
        alert_on_missing: true,
        ignore_patterns: vec!["data/".to_owned(), "*.db".to_owned()],
        optional_patterns: Vec::new(),
    };

    let detector = DriftDetector::new(config);
    let report = detector.check().expect("drift check succeeds");

    assert!(
        report
            .missing_files
            .contains(&PathBuf::from("config/aletheia.toml")),
        "must flag missing config file, got {:?}",
        report.missing_files
    );
    assert!(
        report.checked_at.is_some(),
        "checked_at must be populated on success"
    );
    assert!(report.template_available);
    assert_eq!(report.template_root, example_root);
}

#[test]
fn drift_detector_missing_example_root_when_enabled_reports_unavailable() {
    // WHY(#5143): an enabled drift check with no template cannot assess drift.
    // The detector reports `template_available: false`; the execution layer
    // surfaces this as an unsuccessful task rather than the detector throwing.
    let tmp = tempfile::tempdir().expect("tempdir");
    let example_root = tmp.path().join("does-not-exist");
    let config = DriftDetectionConfig {
        enabled: true,
        instance_root: tmp.path().join("instance"),
        example_root: example_root.clone(),
        alert_on_missing: true,
        ignore_patterns: Vec::new(),
        optional_patterns: Vec::new(),
    };

    let detector = DriftDetector::new(config);
    let report = detector
        .check()
        .expect("enabled drift check with missing template must not error");
    assert!(
        !report.template_available,
        "report must flag the template as unavailable"
    );
    assert_eq!(report.template_root, example_root);
}

#[test]
fn drift_detector_missing_example_root_when_disabled_returns_unavailable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = DriftDetectionConfig {
        enabled: false,
        instance_root: tmp.path().join("instance"),
        example_root: tmp.path().join("does-not-exist"),
        alert_on_missing: true,
        ignore_patterns: Vec::new(),
        optional_patterns: Vec::new(),
    };

    let example_root = config.example_root.clone();
    let detector = DriftDetector::new(config);
    let report = detector
        .check()
        .expect("disabled drift check with missing template is not an error");

    assert!(report.missing_files.is_empty());
    assert!(report.optional_missing_files.is_empty());
    assert!(
        report.checked_at.is_some(),
        "checked_at must still be stamped"
    );
    assert!(
        !report.template_available,
        "report must flag the template as unavailable"
    );
    assert_eq!(report.template_root, example_root);
}

#[test]
fn db_monitor_classifies_sizes_above_thresholds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).expect("mkdir data");

    // 2 MiB file -> warning (warn=1, alert=5)
    let two_mb = vec![0u8; 2 * 1024 * 1024];
    write_fixture(data_dir.join("sessions.db"), &two_mb);

    // 6 MiB file -> alert
    let six_mb = vec![0u8; 6 * 1024 * 1024];
    write_fixture(data_dir.join("planning.db"), &six_mb);

    let config = DbMonitoringConfig {
        enabled: true,
        data_dir: data_dir.clone(),
        warn_threshold_mb: 1,
        alert_threshold_mb: 5,
    };
    let monitor = DbMonitor::new(config);
    let report = monitor.check().expect("monitor check succeeds");

    assert_eq!(report.databases.len(), 2);
    let sessions = report
        .databases
        .iter()
        .find(|d| d.name == "sessions.db")
        .expect("sessions.db present");
    assert_eq!(sessions.status, DbStatus::Warning);
    assert_eq!(sessions.shape, DbShape::File);
    assert_eq!(sessions.health, DbHealth::LegacyFile);
    let planning = report
        .databases
        .iter()
        .find(|d| d.name == "planning.db")
        .expect("planning.db present");
    assert_eq!(planning.status, DbStatus::Alert);
    assert_eq!(planning.shape, DbShape::File);
    assert_eq!(planning.health, DbHealth::NotChecked);
    assert_eq!(
        report.total_size_bytes,
        sessions.size_bytes + planning.size_bytes,
        "total must equal sum of databases"
    );
    assert_eq!(
        report.alerts.len(),
        2,
        "both above-threshold dbs must raise an alert"
    );
}
