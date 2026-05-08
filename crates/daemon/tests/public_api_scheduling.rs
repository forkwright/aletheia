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
    RetentionConfig, RetentionExecutor, RetentionSummary, TraceRotationConfig, TraceRotator,
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

// Split: Cron scheduling via Schedule::Cron + TaskRunner + mock RetentionExecutor.

// Section 4: Cron scheduling via Schedule::Cron + TaskRunner
// ---------------------------------------------------------------------------

/// Build a minimal registration for a single shell command with a specific schedule.
fn builtin_probe_task(id: &str, schedule: Schedule) -> TaskDef {
    TaskDef {
        id: id.to_owned(),
        name: format!("probe {id}"),
        nous_id: "test-nous".to_owned(),
        schedule,
        action: TaskAction::Builtin(BuiltinTask::ProbeAudit),
        enabled: true,
        active_window: None,
        timeout: Duration::from_secs(30),
        catch_up: false,
        jitter: None,
    }
}

#[test]
fn register_cron_valid_expression_populates_next_run_in_status() {
    let mut runner = make_runner("test-nous");
    // Every hour on the hour (6-field).
    runner.register(builtin_probe_task(
        "hourly",
        Schedule::Cron("0 0 * * * *".to_owned()),
    ));

    let statuses = runner.status();
    let hourly = statuses
        .iter()
        .find(|s| s.id == "hourly")
        .expect("hourly task registered");
    assert!(
        hourly.next_run.is_some(),
        "valid cron expression must yield Some(next_run) in status, got {:?}",
        hourly.next_run
    );
    assert!(
        hourly.enabled,
        "newly registered task must be enabled by default"
    );
    assert_eq!(hourly.run_count, 0);
    assert_eq!(hourly.consecutive_failures, 0);
    assert!(!hourly.in_flight);
}

#[test]
fn register_cron_invalid_expression_yields_no_next_run() {
    // Regression test: an invalid cron expression must not panic, must not
    // silently fire, and must surface observably as next_run = None.
    let mut runner = make_runner("test-nous");
    runner.register(builtin_probe_task(
        "broken",
        Schedule::Cron("this is definitely not cron".to_owned()),
    ));

    let statuses = runner.status();
    let broken = statuses
        .iter()
        .find(|s| s.id == "broken")
        .expect("broken task still registers");
    assert!(
        broken.next_run.is_none(),
        "invalid cron expression must produce next_run=None, got {:?}",
        broken.next_run
    );
}

#[test]
fn register_cron_5_field_expression_parses_like_6_field() {
    // 5-field cron is also supported (defaults sec=0).
    let mut runner = make_runner("test-nous");
    runner.register(builtin_probe_task(
        "five-field",
        Schedule::Cron("*/15 * * * *".to_owned()),
    ));

    let statuses = runner.status();
    let task = statuses.iter().find(|s| s.id == "five-field").unwrap();
    assert!(
        task.next_run.is_some(),
        "5-field cron must parse (got next_run = {:?})",
        task.next_run
    );
}

#[test]
fn register_cron_day_of_week_names_parse() {
    let mut runner = make_runner("test-nous");
    runner.register(builtin_probe_task(
        "weekdays",
        Schedule::Cron("0 */15 9-17 * * MON-FRI".to_owned()),
    ));

    let statuses = runner.status();
    let task = statuses.iter().find(|s| s.id == "weekdays").unwrap();
    assert!(
        task.next_run.is_some(),
        "weekday cron with names must parse"
    );
}

#[test]
fn register_schedule_startup_fires_immediately() {
    let mut runner = make_runner("test-nous");
    runner.register(builtin_probe_task("boot", Schedule::Startup));

    let statuses = runner.status();
    let boot = statuses.iter().find(|s| s.id == "boot").unwrap();
    assert!(
        boot.next_run.is_some(),
        "Startup schedule must populate next_run so the loop fires it"
    );
}

#[test]
fn register_schedule_once_future_preserves_target_timestamp() {
    let mut runner = make_runner("test-nous");
    let future = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_secs(3_600))
        .unwrap();
    runner.register(builtin_probe_task("once-future", Schedule::Once(future)));

    let statuses = runner.status();
    let task = statuses.iter().find(|s| s.id == "once-future").unwrap();
    let next = task.next_run.as_ref().expect("once(future) has next_run");
    assert_eq!(
        next,
        &future.to_string(),
        "Once schedule must preserve its target timestamp in status"
    );
}

#[test]
fn register_schedule_once_past_yields_no_next_run() {
    let mut runner = make_runner("test-nous");
    let past = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(1))
        .unwrap();
    runner.register(builtin_probe_task("stale", Schedule::Once(past)));

    let statuses = runner.status();
    let stale = statuses.iter().find(|s| s.id == "stale").unwrap();
    assert!(
        stale.next_run.is_none(),
        "Once(past) must not schedule further runs, got {:?}",
        stale.next_run
    );
}

#[test]
fn task_runner_status_returns_all_registered_tasks() {
    let mut runner = make_runner("test-nous");
    runner.register(builtin_probe_task(
        "a",
        Schedule::Interval(Duration::from_mins(1)),
    ));
    runner.register(builtin_probe_task(
        "b",
        Schedule::Interval(Duration::from_mins(2)),
    ));
    runner.register(builtin_probe_task(
        "c",
        Schedule::Cron("0 0 4 * * *".to_owned()),
    ));

    let statuses = runner.status();
    assert_eq!(statuses.len(), 3);
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"b"));
    assert!(ids.contains(&"c"));
}

#[test]
fn task_runner_with_maintenance_and_output_mode_chains() {
    // Exercise the builder chain to lock in the return type and argument order.
    let runner = TaskRunner::new("test-nous", CancellationToken::new())
        .with_maintenance(MaintenanceConfig::default())
        .with_output_mode(DaemonOutputMode::Brief)
        .with_self_prompt(SelfPromptConfig {
            enabled: true,
            max_per_hour: 5,
        });

    // Nothing registered yet so status is empty.
    assert!(runner.status().is_empty());
}

// ---------------------------------------------------------------------------
// Section 11: Mock RetentionExecutor as DynTrait — verifies the trait
// contract from outside the crate without reaching into internals.
// ---------------------------------------------------------------------------

struct InMemoryRetention {
    summary: RetentionSummary,
}

impl RetentionExecutor for InMemoryRetention {
    fn execute_retention(&self) -> oikonomos::error::Result<RetentionSummary> {
        Ok(self.summary.clone())
    }
}

#[test]
fn retention_executor_trait_is_implementable_from_integration_tests() {
    // WHY: the retention trait is the bridge between the daemon crate (which
    // defines the interface) and the binary crate (which implements it over
    // the SessionStore). Ensuring the trait is implementable FROM outside the
    // crate guards against accidentally sealing it or referencing crate-private
    // types in its signature.
    let executor: Arc<dyn RetentionExecutor> = Arc::new(InMemoryRetention {
        summary: RetentionSummary {
            sessions_cleaned: 11,
            messages_cleaned: 22,
            blackboard_entries_cleaned: 44,
            bytes_freed: 33,
        },
    });

    let result = executor
        .execute_retention()
        .expect("mock executor succeeds");
    assert_eq!(result.sessions_cleaned, 11);
    assert_eq!(result.messages_cleaned, 22);
    assert_eq!(result.blackboard_entries_cleaned, 44);
    assert_eq!(result.bytes_freed, 33);

    // And the runner accepts it through the public builder.
    let _runner = TaskRunner::new("test-nous", CancellationToken::new())
        .with_maintenance(MaintenanceConfig::default())
        .with_retention(executor);
}
