#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]
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
    AutoDreamConfig, DbMonitor, DbMonitoringConfig, DbStatus, DriftDetectionConfig, DriftDetector,
    KnowledgeMaintenanceConfig, MaintenanceConfig, MaintenanceReport, ProposeRulesConfig,
    RetentionConfig, RetentionExecutor, RetentionSummary, SerendipityMaintenanceConfig,
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

// ── MaintenanceConfig defaults ──

#[test]
fn maintenance_config_default_composes_sensible_sub_defaults() {
    let cfg = MaintenanceConfig::default();

    // WHY: the aggregate config delegates each field to the sub-config's
    // Default impl. A regression that flipped these defaults would silently
    // re-enable or disable maintenance tasks on upgrade.
    assert!(
        cfg.trace_rotation.enabled,
        "trace rotation is enabled by default"
    );
    assert!(
        cfg.drift_detection.enabled,
        "drift detection is enabled by default"
    );
    assert!(
        cfg.db_monitoring.enabled,
        "db monitoring is enabled by default"
    );
    assert!(
        !cfg.retention.enabled,
        "retention is disabled by default (binary crate owns the executor)"
    );
    assert!(
        !cfg.knowledge_maintenance.enabled,
        "knowledge maintenance is disabled by default"
    );
    assert!(
        !cfg.propose_rules.enabled,
        "rule proposal is disabled by default"
    );
    // cron sub-configs: all disabled
    assert!(
        !cfg.cron.evolution.enabled,
        "evolution cron is disabled by default"
    );
    assert!(
        !cfg.cron.reflection.enabled,
        "reflection cron is disabled by default"
    );
    assert!(
        !cfg.cron.graph_cleanup.enabled,
        "graph cleanup cron is disabled by default"
    );
}

#[test]
fn trace_rotation_config_default_uses_logs_paths_and_two_week_age() {
    let cfg = TraceRotationConfig::default();

    assert!(cfg.enabled);
    assert_eq!(cfg.trace_dir, PathBuf::from("logs/traces"));
    assert_eq!(cfg.archive_dir, PathBuf::from("logs/traces/archive"));
    assert_eq!(
        cfg.max_age_days, 14,
        "default retention window is two weeks"
    );
    assert_eq!(cfg.max_total_size_mb, 500);
    assert!(cfg.compress, "compression is on by default");
    assert_eq!(cfg.max_archives, 30);
}

#[test]
fn drift_detection_config_default_points_at_instance_example() {
    let cfg = DriftDetectionConfig::default();

    assert!(cfg.enabled);
    assert_eq!(cfg.instance_root, PathBuf::from("instance"));
    assert_eq!(cfg.example_root, PathBuf::from("instance.example"));
    assert!(cfg.alert_on_missing);
}

#[test]
fn drift_detection_default_ignore_and_optional_patterns_populated() {
    let cfg = DriftDetectionConfig::default();

    // Ignored: runtime data and database files must never be flagged as drift.
    assert!(
        cfg.ignore_patterns.iter().any(|p| p == "data/"),
        "default ignore must include data/, got {:?}",
        cfg.ignore_patterns
    );
    assert!(
        cfg.ignore_patterns.iter().any(|p| p == "*.db"),
        "default ignore must include *.db, got {:?}",
        cfg.ignore_patterns
    );
    assert!(cfg.ignore_patterns.iter().any(|p| p == ".gitkeep"));

    // Optional scaffolding: must be tracked distinctly from required files.
    assert!(cfg.optional_patterns.iter().any(|p| p == "packs/"));
    assert!(cfg.optional_patterns.iter().any(|p| p == "services/"));
    assert!(cfg.optional_patterns.iter().any(|p| p == "README.md"));
}

#[test]
fn db_monitoring_config_default_warn_alert_thresholds() {
    let cfg = DbMonitoringConfig::default();

    assert!(cfg.enabled);
    assert_eq!(cfg.data_dir, PathBuf::from("data"));
    assert_eq!(
        cfg.warn_threshold_mb, 100,
        "default warn threshold is 100MB"
    );
    assert_eq!(
        cfg.alert_threshold_mb, 500,
        "default alert threshold is 500MB"
    );
    assert!(
        cfg.warn_threshold_mb < cfg.alert_threshold_mb,
        "warn must fire before alert"
    );
}

#[test]
fn retention_config_default_disabled() {
    let cfg = RetentionConfig::default();
    assert!(
        !cfg.enabled,
        "retention is opt-in because the binary owns the SessionStore"
    );
}

#[test]
fn knowledge_maintenance_config_default_disabled_with_default_auto_dream() {
    let cfg = KnowledgeMaintenanceConfig::default();
    assert!(!cfg.enabled);
    assert!(!cfg.auto_dream.enabled);
    assert!(!cfg.serendipity.enabled);
    assert_eq!(cfg.serendipity.cadence, "0 0 7 * * *");
}

#[test]
fn auto_dream_config_default_min_hours_and_sessions() {
    let cfg = AutoDreamConfig::default();

    assert!(!cfg.enabled);
    assert_eq!(cfg.min_hours, 24);
    assert_eq!(cfg.min_sessions, 5);
    assert_eq!(cfg.scan_interval_secs, 600);
    assert_eq!(cfg.stale_threshold_secs, 3_600);
}

#[test]
fn propose_rules_config_default_disabled_and_has_data_dir() {
    let cfg = ProposeRulesConfig::default();

    assert!(!cfg.enabled);
    // The data_dir resolves from ALETHEIA_ROOT (if set) else "instance".
    // We cannot set env vars safely from parallel tests, so assert only that
    // the path ends with "data".
    assert!(
        cfg.data_dir.ends_with("data"),
        "data_dir should be <instance>/data, got {}",
        cfg.data_dir.display()
    );
}

#[test]
fn cron_config_default_all_tasks_disabled() {
    let cfg = CronConfig::default();

    assert!(!cfg.evolution.enabled);
    assert_eq!(cfg.evolution.interval, Duration::from_hours(24));

    assert!(!cfg.reflection.enabled);
    assert_eq!(cfg.reflection.interval, Duration::from_hours(24));

    assert!(!cfg.graph_cleanup.enabled);
    assert_eq!(
        cfg.graph_cleanup.interval,
        Duration::from_hours(168),
        "graph cleanup defaults to a weekly cadence"
    );

    // Individual sub-config defaults must match the aggregate.
    let evo = CronEvolutionConfig::default();
    assert_eq!(evo.enabled, cfg.evolution.enabled);
    assert_eq!(evo.interval, cfg.evolution.interval);
    let refl = CronReflectionConfig::default();
    assert_eq!(refl.interval, cfg.reflection.interval);
    let gc = CronGraphCleanupConfig::default();
    assert_eq!(gc.interval, cfg.graph_cleanup.interval);
}

// ── DbStatus and report types ──

#[test]
fn db_status_display_renders_lowercase_labels() {
    assert_eq!(DbStatus::Ok.to_string(), "ok");
    assert_eq!(DbStatus::Warning.to_string(), "warning");
    assert_eq!(DbStatus::Alert.to_string(), "alert");
}

#[test]
fn db_status_serde_roundtrips_through_json() {
    for status in [DbStatus::Ok, DbStatus::Warning, DbStatus::Alert] {
        let json = serde_json::to_string(&status).expect("serialize DbStatus");
        let back: DbStatus = serde_json::from_str(&json).expect("deserialize DbStatus");
        assert_eq!(back, status, "{status} must round-trip through JSON");
    }
}

#[test]
fn retention_summary_default_is_zeroed() {
    let s = RetentionSummary::default();
    assert_eq!(s.sessions_cleaned, 0);
    assert_eq!(s.messages_cleaned, 0);
    assert_eq!(s.blackboard_entries_cleaned, 0);
    assert_eq!(s.bytes_freed, 0);
}

#[test]
fn retention_summary_serde_roundtrips_through_json() {
    let original = RetentionSummary {
        sessions_cleaned: 12,
        messages_cleaned: 345,
        blackboard_entries_cleaned: 6,
        bytes_freed: 67_890,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: RetentionSummary = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.sessions_cleaned, original.sessions_cleaned);
    assert_eq!(back.messages_cleaned, original.messages_cleaned);
    assert_eq!(
        back.blackboard_entries_cleaned,
        original.blackboard_entries_cleaned
    );
    assert_eq!(back.bytes_freed, original.bytes_freed);
}

#[test]
fn maintenance_report_serde_roundtrips_through_json() {
    let original = MaintenanceReport {
        items_processed: 1_000,
        items_modified: 42,
        errors: 3,
        duration_ms: 5_678,
        detail: Some("skill-decay: 40 active, 2 retired".to_owned()),
    };
    let json = serde_json::to_string(&original).expect("serialize MaintenanceReport");
    let back: MaintenanceReport =
        serde_json::from_str(&json).expect("deserialize MaintenanceReport");

    assert_eq!(back.items_processed, original.items_processed);
    assert_eq!(back.items_modified, original.items_modified);
    assert_eq!(back.errors, original.errors);
    assert_eq!(back.duration_ms, original.duration_ms);
    assert_eq!(back.detail, original.detail);
}

// ── DaemonConfig TOML round-trip, SelfPromptConfig JSON round-trip ──

#[test]
fn daemon_config_default_is_disabled_with_three_children() {
    let cfg = DaemonConfig::default();
    assert!(
        !cfg.enabled,
        "daemon must be opt-in per workspace — default is disabled"
    );
    assert_eq!(cfg.max_children, 3);
    assert!(cfg.allowed_tasks.is_empty());
    assert!(cfg.watch_paths.is_empty());
    assert!(cfg.webhook_port.is_none());
    assert!(!cfg.brief_output);
    assert!(!cfg.self_prompt.enabled);
    assert!(!cfg.allowed_triggers.file_watch);
    assert!(!cfg.allowed_triggers.webhook);
}

#[test]
fn daemon_config_toml_roundtrip_preserves_fields() {
    let original = DaemonConfig {
        enabled: true,
        max_children: 8,
        allowed_triggers: AllowedTriggers {
            file_watch: true,
            webhook: true,
        },
        allowed_tasks: vec!["trace-rotation".to_owned(), "db-monitor".to_owned()],
        webhook_port: Some(18_789),
        watch_paths: vec!["instance/nous".to_owned()],
        brief_output: true,
        self_prompt: SelfPromptConfig {
            enabled: true,
            max_per_hour: 2,
        },
    };

    let toml_text = toml::to_string(&original).expect("serialize DaemonConfig to TOML");
    let back: DaemonConfig = toml::from_str(&toml_text).expect("deserialize DaemonConfig");

    assert_eq!(back.enabled, original.enabled);
    assert_eq!(back.max_children, original.max_children);
    assert!(back.allowed_triggers.file_watch);
    assert!(back.allowed_triggers.webhook);
    assert_eq!(back.allowed_tasks, original.allowed_tasks);
    assert_eq!(back.webhook_port, original.webhook_port);
    assert_eq!(back.watch_paths, original.watch_paths);
    assert_eq!(back.brief_output, original.brief_output);
    assert_eq!(back.self_prompt.enabled, original.self_prompt.enabled);
    assert_eq!(
        back.self_prompt.max_per_hour,
        original.self_prompt.max_per_hour
    );
}

#[test]
fn daemon_config_is_task_allowed_empty_allow_list_permits_any_task() {
    let cfg = DaemonConfig::default();
    assert!(
        cfg.is_task_allowed("trace-rotation"),
        "empty allow_list must permit all tasks"
    );
    assert!(cfg.is_task_allowed("arbitrary-task-id"));
}

#[test]
fn daemon_config_is_task_allowed_filters_when_list_populated() {
    let cfg = DaemonConfig {
        allowed_tasks: vec!["trace-rotation".to_owned(), "drift-detection".to_owned()],
        ..DaemonConfig::default()
    };
    assert!(cfg.is_task_allowed("trace-rotation"));
    assert!(cfg.is_task_allowed("drift-detection"));
    assert!(
        !cfg.is_task_allowed("db-monitor"),
        "unlisted task must be denied when allow_list is populated"
    );
}

#[test]
fn self_prompt_session_key_uses_daemon_prefix() {
    // WHY: session keys in daemon code must start with "daemon:" so users can
    // filter self-prompt sessions apart from user-driven ones.
    assert!(
        SELF_PROMPT_SESSION_KEY.starts_with("daemon:"),
        "self-prompt session key must use daemon: prefix, got {SELF_PROMPT_SESSION_KEY}"
    );
}

#[test]
fn self_prompt_config_default_disabled_one_per_hour() {
    let cfg = SelfPromptConfig::default();
    assert!(!cfg.enabled, "self-prompt is opt-in");
    assert_eq!(cfg.max_per_hour, 1, "conservative default rate limit");
}

#[test]
fn self_prompt_config_serde_roundtrips_and_supplies_defaults() {
    // Round-trip a fully populated config.
    let original = SelfPromptConfig {
        enabled: true,
        max_per_hour: 5,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: SelfPromptConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.enabled, original.enabled);
    assert_eq!(back.max_per_hour, original.max_per_hour);

    // Empty object must populate serde defaults (disabled, 1/hr).
    let empty: SelfPromptConfig =
        serde_json::from_str("{}").expect("empty JSON object parses via serde defaults");
    assert!(!empty.enabled);
    assert_eq!(empty.max_per_hour, 1);
}

// ── ExecutionResult, TaskStatus, BuiltinTask, Schedule serde ──

#[test]
fn execution_result_serde_roundtrips_through_json() {
    let original = ExecutionResult {
        success: true,
        output: Some("ok".to_owned()),
        errors: 0,
    };
    let json = serde_json::to_string(&original).expect("serialize ExecutionResult");
    let back: ExecutionResult = serde_json::from_str(&json).expect("deserialize ExecutionResult");
    assert!(back.success);
    assert_eq!(back.output.as_deref(), Some("ok"));
}

#[test]
fn task_status_serde_roundtrips_through_json() {
    let original = TaskStatus {
        id: "trace-rotation".to_owned(),
        name: "Trace rotation".to_owned(),
        enabled: true,
        next_run: Some("2026-04-10T03:00:00Z".to_owned()),
        last_run: None,
        run_count: 14,
        consecutive_failures: 0,
        in_flight: false,
        last_error: None,
        last_errors: 0,
        available: true,
        reason: None,
    };
    let json = serde_json::to_string(&original).expect("serialize TaskStatus");
    let back: TaskStatus = serde_json::from_str(&json).expect("deserialize TaskStatus");
    assert_eq!(back.id, original.id);
    assert_eq!(back.name, original.name);
    assert_eq!(back.enabled, original.enabled);
    assert_eq!(back.next_run, original.next_run);
    assert_eq!(back.run_count, original.run_count);
}

#[test]
fn builtin_task_serde_roundtrips_through_json() {
    for task in [
        BuiltinTask::TraceRotation,
        BuiltinTask::DriftDetection,
        BuiltinTask::DbSizeMonitor,
        BuiltinTask::ProbeAudit,
        BuiltinTask::SelfPrompt,
        BuiltinTask::SerendipityDiscovery,
    ] {
        let json = serde_json::to_string(&task).expect("serialize BuiltinTask");
        let back: BuiltinTask = serde_json::from_str(&json).expect("deserialize BuiltinTask");
        let json2 = serde_json::to_string(&back).expect("re-serialize BuiltinTask");
        assert_eq!(json, json2, "BuiltinTask round-trip must be stable");
    }
}

#[test]
fn schedule_serde_roundtrips_cron_interval_once_startup() {
    // Cron
    let cron = Schedule::Cron("0 0 4 * * *".to_owned());
    let back: Schedule = serde_json::from_str(&serde_json::to_string(&cron).unwrap()).unwrap();
    assert!(matches!(back, Schedule::Cron(expr) if expr == "0 0 4 * * *"));

    // Interval
    let interval = Schedule::Interval(Duration::from_mins(2));
    let back: Schedule = serde_json::from_str(&serde_json::to_string(&interval).unwrap()).unwrap();
    assert!(
        matches!(back, Schedule::Interval(d) if d == Duration::from_mins(2)),
        "Interval must round-trip"
    );

    // Startup
    let startup = Schedule::Startup;
    let back: Schedule = serde_json::from_str(&serde_json::to_string(&startup).unwrap()).unwrap();
    assert!(matches!(back, Schedule::Startup));
}
