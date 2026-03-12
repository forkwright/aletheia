use super::*;
use crate::execution::execute_builtin;

fn make_echo_task(id: &str) -> TaskDef {
    TaskDef {
        id: id.to_owned(),
        name: format!("Test task {id}"),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        ..TaskDef::default()
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

#[tokio::test]
async fn shutdown_exits_run_loop() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token.clone());

    let handle = tokio::spawn(async move {
        runner.run().await;
    });

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
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("echo ok".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].consecutive_failures = 2;
    runner.record_task_completion("echo-task", Duration::from_millis(10));

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
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Builtin(BuiltinTask::Prosoche),
        enabled: true,
        catch_up: false,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(-1))
            .unwrap(),
    );

    runner.tick();

    // Wait for the spawned task to complete.
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
    assert!(ids.contains(&"db-size-monitor"));
    assert!(!ids.contains(&"retention-execution"));
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
    let output = result.unwrap().output.unwrap_or_default();
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
    let executor: Arc<dyn crate::maintenance::RetentionExecutor> =
        Arc::new(MockRetentionExecutor);
    let runner = TaskRunner::new("test-nous", token).with_retention(executor);
    assert!(runner.status().is_empty());
}

struct MockRetentionExecutor;

impl crate::maintenance::RetentionExecutor for MockRetentionExecutor {
    fn execute_retention(&self) -> crate::error::Result<crate::maintenance::RetentionSummary> {
        Ok(crate::maintenance::RetentionSummary::default())
    }
}

#[test]
fn execution_result_serialization() {
    let result = ExecutionResult {
        success: true,
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
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("echo should-not-run".to_owned()),
        enabled: false,
        ..TaskDef::default()
    };
    runner.register(task);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(-10))
            .unwrap(),
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

    let handle = tokio::spawn(async move {
        runner.run().await;
    });

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

    let handle = tokio::spawn(async move {
        runner.run().await;
    });

    token.cancel();
    drop(token);

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(result.is_ok(), "runner should exit when token is cancelled");
}

#[tokio::test]
async fn shutdown_completes_within_timeout() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token.clone());

    let handle = tokio::spawn(async move {
        runner.run().await;
    });

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

    let handle_a = tokio::spawn(async move { runner_a.run().await });
    let handle_b = tokio::spawn(async move { runner_b.run().await });

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

// --- Exponential backoff tests ---

#[test]
fn backoff_applied_on_failure() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("backoff-task"));

    // First failure → 60s backoff.
    runner.record_task_failure("backoff-task", "test error");
    assert_eq!(runner.tasks[0].consecutive_failures, 1);
    assert!(runner.tasks[0].backoff_until.is_some());

    let backoff = runner.tasks[0].backoff_until.unwrap();
    let expected_min = Instant::now() + Duration::from_secs(55);
    assert!(
        backoff > expected_min,
        "1st failure should have ~60s backoff"
    );

    // Second failure → 300s backoff.
    runner.record_task_failure("backoff-task", "test error 2");
    assert_eq!(runner.tasks[0].consecutive_failures, 2);
    let backoff = runner.tasks[0].backoff_until.unwrap();
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

    runner.record_task_completion("backoff-clear", Duration::from_millis(1));
    assert!(runner.tasks[0].backoff_until.is_none());
    assert_eq!(runner.tasks[0].consecutive_failures, 0);
}

// --- Hung task detection tests ---

#[tokio::test]
async fn hung_task_cancelled_after_2x_timeout() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "hung-task".to_owned(),
        name: "Hung task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("echo ok".to_owned()),
        timeout: Duration::from_millis(50),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    // Simulate a hung task by spawning a long sleep.
    let handle = tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(ExecutionResult {
            success: true,
            output: None,
        })
    });

    runner.in_flight.insert(
        "hung-task".to_owned(),
        InFlightTask {
            handle,
            started_at: Instant::now()
                .checked_sub(Duration::from_millis(150))
                .unwrap(),
            timeout: Duration::from_millis(50),
            warned: false,
        },
    );

    runner.check_in_flight().await;

    assert!(!runner.in_flight.contains_key("hung-task"));
    assert_eq!(runner.tasks[0].consecutive_failures, 1);
}

// --- Missed cron catch-up tests ---

#[test]
fn missed_cron_catchup_fires_on_startup() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "hourly-task".to_owned(),
        name: "Hourly task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Cron("0 0 * * * *".to_owned()),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        catch_up: true,
        ..TaskDef::default()
    };
    runner.register(task);

    let three_hours_ago = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(3))
        .unwrap();
    runner.set_last_run("hourly-task", three_hours_ago);

    // Set next_run far in the future.
    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_hours(1))
            .unwrap(),
    );

    runner.check_missed_cron_catchup();

    let next = runner.tasks[0].next_run.unwrap();
    let diff = next
        .since(jiff::Timestamp::now())
        .unwrap()
        .get_seconds()
        .abs();
    assert!(diff < 5, "catch-up should set next_run to ~now");
}

#[test]
fn missed_cron_catchup_skips_disabled_catch_up() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "no-catchup".to_owned(),
        name: "No catch-up".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Cron("0 0 * * * *".to_owned()),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        catch_up: false,
        ..TaskDef::default()
    };
    runner.register(task);

    let three_hours_ago = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(3))
        .unwrap();
    runner.set_last_run("no-catchup", three_hours_ago);

    let future_run = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(1))
        .unwrap();
    runner.tasks[0].next_run = Some(future_run);

    runner.check_missed_cron_catchup();

    assert_eq!(runner.tasks[0].next_run.unwrap(), future_run);
}

// --- Task metrics tests ---

#[test]
fn task_metrics_on_success() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("metrics-task"));

    runner.record_task_completion("metrics-task", Duration::from_millis(42));

    let statuses = runner.status();
    assert_eq!(statuses[0].run_count, 1);
    assert_eq!(statuses[0].consecutive_failures, 0);
}

#[test]
fn task_metrics_on_failure() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("metrics-fail"));

    runner.record_task_failure("metrics-fail", "boom");

    let statuses = runner.status();
    assert_eq!(statuses[0].consecutive_failures, 1);
    assert_eq!(statuses[0].run_count, 0);
}

#[tokio::test]
async fn in_flight_reported_in_status() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    runner.register(make_echo_task("inflight-task"));

    let handle = tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(ExecutionResult {
            success: true,
            output: None,
        })
    });
    runner.in_flight.insert(
        "inflight-task".to_owned(),
        InFlightTask {
            handle,
            started_at: Instant::now(),
            timeout: Duration::from_secs(300),
            warned: false,
        },
    );

    let statuses = runner.status();
    assert!(statuses[0].in_flight);

    // Clean up — abort so the test doesn't hang.
    if let Some(task) = runner.in_flight.remove("inflight-task") {
        task.handle.abort();
    }
}
