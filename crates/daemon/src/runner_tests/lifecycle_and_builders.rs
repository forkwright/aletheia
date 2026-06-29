//! Lifecycle + builder pattern + basic failure handling tests (split from `runner_tests.rs`).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::super::*;
use super::make_echo_task;
use crate::bridge::DaemonBridge;
use crate::execution::execute_builtin;
use crate::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceReport};
use crate::runner::ExecutionResult;

/// Knowledge executor that reports non-fatal errors for decay refresh.
struct DecayErrorKnowledge;

impl KnowledgeMaintenanceExecutor for DecayErrorKnowledge {
    fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> crate::error::Result<()> {
        Ok(())
    }

    fn refresh_decay_scores(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            items_processed: 5,
            items_modified: 1,
            errors: 2,
            duration_ms: 42,
            detail: Some("decay refresh: 2 partial failures".to_owned()),
        })
    }

    fn deduplicate_entities(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn recompute_graph_scores(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn refresh_embeddings(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn garbage_collect(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn maintain_indexes(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn health_check(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn run_skill_decay(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn materialize_derived_facts(&self) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn discover_serendipitous_facts(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
}

#[test]
fn register_shows_in_status() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("task-1"));
    runner.register(make_echo_task("task-2"));

    let statuses = runner.status();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].id, "task-1");
    assert_eq!(statuses[1].id, "task-2");
    assert!(statuses[0].enabled);
}

#[test]
fn runner_honors_daemon_behavior_output_limits() {
    let token = CancellationToken::new();
    let behavior = taxis::config::DaemonBehaviorConfig {
        runner_output_brief_head_lines: 2,
        runner_output_brief_tail_lines: 1,
        ..taxis::config::DaemonBehaviorConfig::default()
    };
    let runner = TaskRunner::new("test-nous", token)
        .with_output_mode(DaemonOutputMode::Brief)
        .with_daemon_behavior(behavior);

    let long = "a\nb\nc\nd\ne";
    let truncated = truncate_output(
        long,
        Some(runner.daemon_behavior.runner_output_brief_head_lines),
        Some(runner.daemon_behavior.runner_output_brief_tail_lines),
    );

    assert_eq!(truncated, "a\nb\n... (2 lines omitted)\ne");
}

#[tokio::test]
async fn shutdown_exits_run_loop() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token.clone());

    let handle = tokio::spawn(
        async move {
            runner.run().await;
        }
        .instrument(tracing::info_span!("test_runner")),
    );

    token.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(result.is_ok(), "runner should exit on shutdown signal");
}

#[tokio::test]
async fn task_disabled_after_consecutive_failures() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "failing-task".to_owned(),
        name: "Failing task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_millis(10)),
        action: TaskAction::Command("exit 1".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    for _ in 0..3 {
        runner.record_task_failure("failing-task", "exit code 1");
    }

    let statuses = runner.status();
    assert!(
        !statuses[0].enabled,
        "task should be disabled after 3 failures"
    );
    assert_eq!(statuses[0].consecutive_failures, 3);
}

#[tokio::test]
async fn successful_command_resets_failures() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "echo-task".to_owned(),
        name: "Echo task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Command("echo ok".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].consecutive_failures = 2;
    runner.record_task_completion("echo-task", Duration::from_millis(10), 0);

    let statuses = runner.status();
    assert_eq!(statuses[0].consecutive_failures, 0);
    assert_eq!(statuses[0].run_count, 1);
    assert!(statuses[0].enabled);
}

#[tokio::test]
async fn builtin_prosoche_executes() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "prosoche".to_owned(),
        name: "Prosoche check".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Builtin(BuiltinTask::Prosoche),
        enabled: true,
        catch_up: false,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(-1))
            .expect("past timestamp arithmetic should succeed"),
    );

    runner.tick();

    // kanon:ignore TESTING/sleep-in-test reason = "prosoche spawns a real child process; tokio::time::pause cannot advance OS-level process execution"
    tokio::time::sleep(Duration::from_millis(100)).await;
    runner.check_in_flight().await;

    let statuses = runner.status();
    assert_eq!(statuses[0].run_count, 1);
    assert_eq!(statuses[0].consecutive_failures, 0);
}

#[test]
fn register_maintenance_tasks_respects_enabled() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.trace_rotation.enabled = true;
    config.drift_detection.enabled = false;
    config.db_monitoring.enabled = true;
    config.retention.enabled = false;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"trace-rotation"));
    assert!(!ids.contains(&"drift-detection"));
    assert!(ids.contains(&"db-monitor"));
    assert!(!ids.contains(&"retention-execution"));
}

#[test]
fn register_maintenance_tasks_includes_instance_backup_status() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.instance_backup.enabled = true;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"instance-backup"),
        "status ids should include canonical instance-backup"
    );
    assert!(
        !ids.contains(&"fjall-backup"),
        "status ids should not include legacy fjall-backup"
    );
}

#[test]
fn serendipity_discovery_task_registers_at_daily_seven_utc() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.knowledge_maintenance.enabled = true;
    config.knowledge_maintenance.serendipity.enabled = true;
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(MockKnowledgeExecutor);

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner = runner.with_knowledge_maintenance(executor);
    runner.register_maintenance_tasks();

    let task = runner
        .tasks
        .iter()
        .find(|task| task.def.id == "serendipity-discovery")
        .expect("serendipity task should be scheduled");
    assert!(matches!(
        &task.def.schedule,
        Schedule::Cron(expr) if expr == "0 0 7 * * *"
    ));
}

#[test]
fn serendipity_discovery_task_skips_when_disabled() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.knowledge_maintenance.enabled = true;
    config.knowledge_maintenance.serendipity.enabled = false;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(!ids.contains(&"serendipity-discovery"));
}

#[test]
fn routing_store_refresh_registers_when_store_is_attached() {
    let token = CancellationToken::new();
    let config = MaintenanceConfig::default();
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(aletheia_routing::AfterActionStore::new(
        tmp.path().to_owned(),
    ));

    let mut runner = TaskRunner::new("system", token)
        .with_maintenance(config)
        .with_after_action_store(store);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let refresh = statuses
        .iter()
        .find(|status| status.id == "routing-store-refresh")
        .expect("routing refresh task should be scheduled");
    assert_eq!(refresh.name, "Routing after-action store refresh");
    assert!(refresh.enabled);
}

#[test]
fn bridge_dependent_cron_tasks_skip_without_bridge() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.cron.evolution.enabled = true;
    config.cron.reflection.enabled = true;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let ids: Vec<String> = runner
        .status()
        .into_iter()
        .map(|status| status.id)
        .collect();
    assert!(!ids.iter().any(|id| id == "cron-evolution"));
    assert!(!ids.iter().any(|id| id == "cron-reflection"));
}

#[test]
fn bridge_dependent_cron_tasks_register_with_bridge() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let mut config = MaintenanceConfig::default();
    config.cron.evolution.enabled = true;
    config.cron.reflection.enabled = true;

    let mut runner = TaskRunner::with_bridge("test-nous", token, bridge).with_maintenance(config);
    runner.register_maintenance_tasks();

    let ids: Vec<String> = runner
        .status()
        .into_iter()
        .map(|status| status.id)
        .collect();
    assert!(ids.iter().any(|id| id == "cron-evolution"));
    assert!(ids.iter().any(|id| id == "cron-reflection"));
}

#[test]
fn register_maintenance_tasks_skips_without_config() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("system", token);
    runner.register_maintenance_tasks();
    assert!(runner.status().is_empty());
}

#[test]
fn retention_requires_executor() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.retention.enabled = true;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(
        !ids.contains(&"retention-execution"),
        "retention should not register without executor"
    );
}

#[test]
fn retention_registers_when_enabled_with_executor() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.retention.enabled = true;
    let executor: Arc<dyn crate::maintenance::RetentionExecutor> = Arc::new(MockRetentionExecutor);

    let mut runner = TaskRunner::new("system", token)
        .with_maintenance(config)
        .with_retention(executor);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let retention = statuses
        .iter()
        .find(|s| s.id == "retention-execution")
        .expect("retention task should be scheduled");
    assert_eq!(retention.name, "Data retention cleanup");
    assert!(retention.enabled);
}

#[test]
fn knowledge_maintenance_registers_only_implemented_tasks() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.knowledge_maintenance.enabled = true;
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(MockKnowledgeExecutor);

    let mut runner = TaskRunner::new("system", token)
        .with_maintenance(config)
        .with_knowledge_maintenance(executor);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"decay-refresh"));
    assert!(ids.contains(&"entity-dedup"));
    assert!(ids.contains(&"graph-recompute"));
    assert!(ids.contains(&"skill-decay"));
    assert!(!ids.contains(&"embedding-refresh"));
    assert!(!ids.contains(&"knowledge-gc"));
    assert!(!ids.contains(&"index-maintenance"));
    assert!(!ids.contains(&"graph-health-check"));
}

#[tokio::test]
async fn retention_without_executor_skips() {
    let result = execute_builtin(
        &BuiltinTask::RetentionExecution,
        "system",
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());
    let output = result
        .expect("retention execution should not error")
        .output
        .unwrap_or_default();
    assert!(output.contains("skipped"));
}

#[test]
fn status_empty_runner() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token);
    assert!(
        runner.status().is_empty(),
        "new runner should have no tasks"
    );
}

#[test]
fn register_startup_task_immediate() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "startup-task".to_owned(),
        name: "Startup".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Startup,
        action: TaskAction::Command("echo boot".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    let before = jiff::Timestamp::now();
    runner.register(task);

    let statuses = runner.status();
    let next_run_str = statuses[0]
        .next_run
        .as_ref()
        .expect("startup should have next_run");
    let next_run: jiff::Timestamp = next_run_str.parse().expect("valid timestamp");
    assert!(
        next_run >= before,
        "startup task next_run should be >= time before registration"
    );
}

#[test]
fn with_bridge_stores_bridge() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let runner = TaskRunner::with_bridge("test-nous", token, bridge);
    assert!(runner.status().is_empty());
}

#[test]
fn with_maintenance_builder_pattern() {
    let token = CancellationToken::new();
    let config = MaintenanceConfig::default();
    let runner = TaskRunner::new("test-nous", token).with_maintenance(config);
    assert!(runner.status().is_empty());
}

#[test]
fn with_retention_builder_pattern() {
    let token = CancellationToken::new();
    let executor: Arc<dyn crate::maintenance::RetentionExecutor> = Arc::new(MockRetentionExecutor);
    let runner = TaskRunner::new("test-nous", token).with_retention(executor);
    assert!(runner.status().is_empty());
}

struct MockRetentionExecutor;

impl crate::maintenance::RetentionExecutor for MockRetentionExecutor {
    fn execute_retention(&self) -> crate::error::Result<crate::maintenance::RetentionSummary> {
        Ok(crate::maintenance::RetentionSummary::default())
    }
}

struct MockKnowledgeExecutor;

impl crate::maintenance::KnowledgeMaintenanceExecutor for MockKnowledgeExecutor {
    fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> crate::error::Result<()> {
        Ok(())
    }

    fn refresh_decay_scores(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn deduplicate_entities(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn recompute_graph_scores(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn refresh_embeddings(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn garbage_collect(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn maintain_indexes(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn health_check(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn run_skill_decay(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn materialize_derived_facts(
        &self,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }

    fn discover_serendipitous_facts(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<crate::maintenance::MaintenanceReport> {
        Ok(crate::maintenance::MaintenanceReport::default())
    }
}

#[test]
fn execution_result_serialization() {
    let result = ExecutionResult {
        success: true,
        errors: 0,
        output: Some("hello".to_owned()),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    let back: ExecutionResult = serde_json::from_str(&json).expect("deserialize");
    assert!(back.success);
    assert_eq!(back.output.as_deref(), Some("hello"));
}

#[tokio::test]
async fn disabled_task_not_in_tick() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "disabled-task".to_owned(),
        name: "Disabled".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Command("echo should-not-run".to_owned()),
        enabled: false,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(-10))
            .expect("past timestamp arithmetic should succeed"),
    );

    runner.tick();

    assert!(runner.in_flight.is_empty());
    let statuses = runner.status();
    assert_eq!(
        statuses[0].run_count, 0,
        "disabled task should not have run"
    );
}

#[tokio::test]
async fn child_token_cancelled_by_parent() {
    let parent = CancellationToken::new();
    let child = parent.child_token();
    let mut runner = TaskRunner::new("test-nous", child);

    let handle = tokio::spawn(
        async move {
            runner.run().await;
        }
        .instrument(tracing::info_span!("test_runner")),
    );

    parent.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(
        result.is_ok(),
        "child runner should exit when parent token is cancelled"
    );
}

#[tokio::test]
async fn dropped_token_stops_runner() {
    let token = CancellationToken::new();
    let child = token.child_token();
    let mut runner = TaskRunner::new("test-nous", child);

    let handle = tokio::spawn(
        async move {
            runner.run().await;
        }
        .instrument(tracing::info_span!("test_runner")),
    );

    token.cancel();
    drop(token);

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(result.is_ok(), "runner should exit when token is cancelled");
}

#[tokio::test]
async fn shutdown_completes_within_timeout() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token.clone());

    let handle = tokio::spawn(
        async move {
            runner.run().await;
        }
        .instrument(tracing::info_span!("test_runner")),
    );

    token.cancel();
    let timeout = Duration::from_secs(2);
    let result = tokio::time::timeout(timeout, handle).await;
    assert!(
        result.is_ok(),
        "shutdown should complete well within {timeout:?}"
    );
}

/// Multiple runners with independent child tokens: cancelling one does not affect others.
#[tokio::test]
async fn independent_child_tokens_isolated() {
    let parent = CancellationToken::new();
    let child_a = parent.child_token();
    let child_b = parent.child_token();

    let mut runner_a = TaskRunner::new("nous-a", child_a.clone());
    let mut runner_b = TaskRunner::new("nous-b", child_b);

    let handle_a = tokio::spawn(
        async move { runner_a.run().await }.instrument(tracing::info_span!("test_runner_a")),
    );
    let handle_b = tokio::spawn(
        async move { runner_b.run().await }.instrument(tracing::info_span!("test_runner_b")),
    );

    child_a.cancel();

    let result_a = tokio::time::timeout(Duration::from_secs(2), handle_a).await;
    assert!(
        result_a.is_ok(),
        "runner_a should stop when its token is cancelled"
    );

    assert!(!handle_b.is_finished(), "runner_b should still be running");

    parent.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle_b).await;
}

#[test]
fn backoff_applied_on_failure() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("backoff-task"));

    runner.record_task_failure("backoff-task", "test error");
    assert_eq!(runner.tasks[0].consecutive_failures, 1);
    assert!(runner.tasks[0].backoff_until.is_some());

    let backoff = runner.tasks[0]
        .backoff_until
        .expect("backoff should be set after first failure");
    let expected_min = Instant::now() + Duration::from_secs(55);
    assert!(
        backoff > expected_min,
        "1st failure should have ~60s backoff"
    );

    runner.record_task_failure("backoff-task", "test error 2");
    assert_eq!(runner.tasks[0].consecutive_failures, 2);
    let backoff = runner.tasks[0]
        .backoff_until
        .expect("backoff should be set after second failure");
    let expected_min = Instant::now() + Duration::from_secs(295);
    assert!(
        backoff > expected_min,
        "2nd failure should have ~300s backoff"
    );
}

#[test]
fn backoff_cleared_on_success() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("backoff-clear"));

    runner.record_task_failure("backoff-clear", "fail");
    assert!(runner.tasks[0].backoff_until.is_some());

    runner.record_task_completion("backoff-clear", Duration::from_millis(1), 0);
    assert!(runner.tasks[0].backoff_until.is_none());
    assert_eq!(runner.tasks[0].consecutive_failures, 0);
}

#[tokio::test]
async fn hung_task_cancelled_after_2x_timeout() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "hung-task".to_owned(),
        name: "Hung task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Command("echo ok".to_owned()),
        timeout: Duration::from_millis(50),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    // NOTE: Simulate a hung task by spawning a long sleep.
    let handle = tokio::spawn(
        async {
            // kanon:ignore TESTING/sleep-in-test reason = "simulates a hung task; the runner cancels the handle before the sleep elapses"
            tokio::time::sleep(Duration::from_mins(1)).await;
            Ok(ExecutionResult {
                success: true,
                errors: 0,
                output: None,
            })
        }
        .instrument(tracing::info_span!("test_hung_task")),
    );

    runner.in_flight.insert(
        "hung-task".to_owned(),
        InFlightTask {
            handle,
            cancel: CancellationToken::new(),
            started_at: Instant::now()
                .checked_sub(Duration::from_millis(150))
                .expect("subtracting 150ms from now should succeed"),
            timeout: Duration::from_millis(50),
            warned: false,
        },
    );

    runner.check_in_flight().await;

    assert!(!runner.in_flight.contains_key("hung-task"));
    assert_eq!(runner.tasks[0].consecutive_failures, 1);
}

/// Bridge that captures the cancellation token passed to a prompt dispatch and
/// never returns, so the runner must cancel the token and abort the task.
struct CancelCapturingBridge {
    captured: Arc<Mutex<Option<CancellationToken>>>,
    ready: Arc<Notify>,
}

impl CancelCapturingBridge {
    fn new() -> (Self, Arc<Mutex<Option<CancellationToken>>>, Arc<Notify>) {
        let captured = Arc::new(Mutex::new(None));
        let ready = Arc::new(Notify::new());
        (
            Self {
                captured: Arc::clone(&captured),
                ready: Arc::clone(&ready),
            },
            captured,
            ready,
        )
    }
}

impl DaemonBridge for CancelCapturingBridge {
    fn send_prompt(
        &self,
        _nous_id: &str,
        _session_key: &str,
        _prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>> {
        Box::pin(async {
            Ok(ExecutionResult {
                success: false,
                errors: 0,
                output: Some("send_prompt not expected".to_owned()),
            })
        })
    }

    fn send_prompt_with_cancel(
        &self,
        _nous_id: &str,
        _session_key: &str,
        _prompt: &str,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>> {
        *self.captured.lock().expect("lock poisoned") = Some(cancel.clone());
        self.ready.notify_one();

        Box::pin(async move {
            // Wait until cancellation propagates, then yield forever so the
            // runner's timeout path cancels the stored token before aborting.
            cancel.cancelled().await;
            loop {
                tokio::task::yield_now().await;
            }
        })
    }
}

#[tokio::test]
async fn hung_bridge_task_cancels_token_before_abort() {
    let shutdown = CancellationToken::new();
    let (bridge, captured, ready) = CancelCapturingBridge::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(bridge);
    let mut runner = TaskRunner::with_bridge("test-nous", shutdown, bridge);

    let task = TaskDef {
        id: "bridge-task".to_owned(),
        name: "Bridge task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::SelfPrompt("hello".to_owned()),
        timeout: Duration::from_millis(50),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);
    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs(10))
            .expect("past timestamp arithmetic should succeed"),
    );

    runner.tick();
    assert!(
        runner.in_flight.contains_key("bridge-task"),
        "task should be in flight after tick"
    );

    // Wait for the spawned task to enter the bridge and hand us its token.
    tokio::time::timeout(Duration::from_secs(5), ready.notified())
        .await
        .expect("bridge should receive the prompt");

    let task_token = captured
        .lock()
        .expect("lock poisoned")
        .clone()
        .expect("token should be captured");
    assert!(!task_token.is_cancelled(), "token should start uncancelled");

    // Simulate the task running well past its 2x timeout threshold.
    let inflight = runner
        .in_flight
        .get_mut("bridge-task")
        .expect("task still in flight");
    inflight.started_at = Instant::now()
        .checked_sub(Duration::from_millis(150))
        .expect("test duration should fit before now");

    runner.check_in_flight().await;

    assert!(
        !runner.in_flight.contains_key("bridge-task"),
        "runner should remove the hung task"
    );
    assert!(
        task_token.is_cancelled(),
        "runner should cancel the token passed to the bridge-dispatched task"
    );
    assert_eq!(
        runner.tasks[0].consecutive_failures, 1,
        "hung task should be recorded as a failure"
    );
}

#[tokio::test]
async fn watchdog_enabled_restarts_hung_inflight_task() {
    let token = CancellationToken::new();
    let settings = taxis::config::WatchdogSettings {
        enabled: true,
        heartbeat_timeout_secs: 0,
        check_interval_secs: 1,
        max_restarts: 5,
    };
    let mut runner = TaskRunner::new("test-nous", token).with_watchdog_settings(&settings);

    let task = TaskDef {
        id: "watchdog-task".to_owned(),
        name: "Watchdog task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("sleep 60".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    let task_cancel = CancellationToken::new();
    let handle = tokio::spawn(async {
        std::future::pending::<crate::error::Result<ExecutionResult>>().await
    });
    runner.in_flight.insert(
        "watchdog-task".to_owned(),
        InFlightTask {
            handle,
            cancel: task_cancel.clone(),
            started_at: Instant::now(),
            timeout: Duration::from_secs(60),
            warned: false,
        },
    );
    runner.register_watchdog_process("watchdog-task");

    runner.check_task_watchdog().await;

    assert!(
        !runner.in_flight.contains_key("watchdog-task"),
        "watchdog should remove the hung task from in-flight tracking"
    );
    assert!(
        task_cancel.is_cancelled(),
        "watchdog kill should cancel the task token"
    );
    assert_eq!(
        runner.tasks[0].consecutive_failures, 1,
        "watchdog kill should record a task failure"
    );
    assert_eq!(
        runner.watchdog_restart_count(),
        1,
        "watchdog should record the restart event"
    );
    let next_run = runner.tasks[0]
        .next_run
        .expect("watchdog restart should schedule an immediate run");
    assert!(
        next_run <= jiff::Timestamp::now(),
        "watchdog restart should be due immediately"
    );
}

/// Regression (#5132): a knowledge maintenance report with errors > 0 must be
/// routed through the runner's failure path so status/metrics see a non-success.
#[tokio::test]
async fn knowledge_partial_error_records_task_failure() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token)
        .with_knowledge_maintenance(Arc::new(DecayErrorKnowledge));

    let task = TaskDef {
        id: "decay-refresh".to_owned(),
        name: "Decay refresh".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Builtin(BuiltinTask::DecayRefresh),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);
    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs(10))
            .expect("past timestamp arithmetic should succeed"),
    );

    runner.tick();
    assert!(
        runner.in_flight.contains_key("decay-refresh"),
        "task should be spawned"
    );

    while runner.in_flight.contains_key("decay-refresh") {
        runner.check_in_flight().await;
        tokio::task::yield_now().await;
    }

    let statuses = runner.status();
    assert_eq!(
        statuses[0].consecutive_failures, 1,
        "partial-error maintenance must increment consecutive failures"
    );
    assert!(
        statuses[0]
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("2 errors"),
        "last_error should surface the error count: {:?}",
        statuses[0].last_error
    );
}
