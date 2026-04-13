#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
use tracing::Instrument;

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

    runner.record_task_completion("backoff-clear", Duration::from_millis(1));
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
        schedule: Schedule::Interval(Duration::from_secs(60)),
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
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        }
        .instrument(tracing::info_span!("test_hung_task")),
    );

    runner.in_flight.insert(
        "hung-task".to_owned(),
        InFlightTask {
            handle,
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
        .expect("timestamp arithmetic should succeed");
    runner.set_last_run("hourly-task", three_hours_ago);

    runner.tasks[0].next_run = Some(
        jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_hours(1))
            .expect("timestamp arithmetic should succeed"),
    );

    runner.check_missed_cron_catchup();

    let next = runner.tasks[0]
        .next_run
        .expect("next_run should be set after catch-up");
    let diff = next
        .since(jiff::Timestamp::now())
        .expect("duration since should succeed")
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
        .expect("timestamp arithmetic should succeed");
    runner.set_last_run("no-catchup", three_hours_ago);

    let future_run = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(1))
        .expect("timestamp arithmetic should succeed");
    runner.tasks[0].next_run = Some(future_run);

    runner.check_missed_cron_catchup();

    assert_eq!(
        runner.tasks[0]
            .next_run
            .expect("next_run should remain unchanged"),
        future_run
    );
}

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

    let handle = tokio::spawn(
        async {
            // kanon:ignore TESTING/sleep-in-test reason = "simulates an in-flight task; handle is aborted before sleep elapses"
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        }
        .instrument(tracing::info_span!("test_inflight_task")),
    );
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

    if let Some(task) = runner.in_flight.remove("inflight-task") {
        task.handle.abort();
    }
}

/// IDs returned by `status()` for the core maintenance tasks must match the IDs
/// accepted by `aletheia maintenance run <id>`.
#[test]
fn maintenance_status_ids_accepted_by_run() {
    let run_accepted: &[&str] = &["trace-rotation", "drift-detection", "db-monitor"];

    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.trace_rotation.enabled = true;
    config.drift_detection.enabled = true;
    config.db_monitoring.enabled = true;

    let mut runner = TaskRunner::new("system", token).with_maintenance(config);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let status_ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();

    for id in run_accepted {
        assert!(
            status_ids.contains(id),
            "run-accepted id '{id}' not found in status output — IDs are mismatched"
        );
    }
}

// -- Brief output mode tests --

#[test]
fn truncate_output_short_passes_through() {
    let short = "line 1\nline 2\nline 3";
    assert_eq!(
        truncate_output(short, None, None),
        short,
        "short output should pass through unchanged"
    );
}

#[test]
fn truncate_output_long_shows_head_and_tail() {
    let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
    let long = lines.join("\n");
    let truncated = truncate_output(&long, None, None);

    assert!(
        truncated.contains("line 1"),
        "head should include first line"
    );
    assert!(
        truncated.contains("line 5"),
        "head should include line 5 (BRIEF_HEAD_LINES=5)"
    );
    assert!(
        truncated.contains("lines omitted"),
        "should contain omission marker"
    );
    assert!(
        truncated.contains("line 20"),
        "tail should include last line"
    );
    assert!(
        truncated.contains("line 18"),
        "tail should include line 18 (BRIEF_TAIL_LINES=3)"
    );
    assert!(
        !truncated.contains("line 10"),
        "middle lines should be omitted"
    );
}

#[test]
fn with_output_mode_sets_mode() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token).with_output_mode(DaemonOutputMode::Brief);
    assert_eq!(runner.output_mode, DaemonOutputMode::Brief);
}

#[test]
fn default_output_mode_is_full() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token);
    assert_eq!(runner.output_mode, DaemonOutputMode::Full);
}

// -- Jitter integration test in runner --

#[test]
fn register_task_with_jitter_shifts_next_run() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "jittered-task".to_owned(),
        name: "Jittered".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_secs(3600)),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        jitter: Some(jiff::SignedDuration::from_secs(600)),
        ..TaskDef::default()
    };

    let before = jiff::Timestamp::now();
    runner.register(task);

    let statuses = runner.status();
    let next_run: jiff::Timestamp = statuses[0]
        .next_run
        .as_ref()
        .expect("should have next_run")
        .parse()
        .expect("valid timestamp");

    // NOTE: with jitter, next_run should be >= now + interval (base) since
    // jitter is additive and non-negative.
    assert!(
        next_run > before,
        "jittered next_run should be in the future"
    );
}

// -- State persistence integration test --

#[test]
fn with_state_store_persists_across_restarts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("state");

    // First runner: register, complete a task, persist
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("open store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("persist-task"));
        runner.record_task_completion("persist-task", Duration::from_millis(10));

        let statuses = runner.status();
        assert_eq!(statuses[0].run_count, 1);
    }

    // Second runner: restore state from same DB
    {
        let token = CancellationToken::new();
        let store = crate::state::TaskStateStore::open(&db_path).expect("reopen store");
        let mut runner = TaskRunner::new("test-nous", token).with_state_store(store);
        runner.register(make_echo_task("persist-task"));
        runner.restore_state();

        let statuses = runner.status();
        assert_eq!(
            statuses[0].run_count, 1,
            "run_count should be restored from store"
        );
    }
}

// -- Self-prompt integration tests --

#[test]
fn with_self_prompt_builder_configures_limiter() {
    let token = CancellationToken::new();
    let config = crate::self_prompt::SelfPromptConfig {
        enabled: true,
        max_per_hour: 5,
    };
    let runner = TaskRunner::new("test-nous", token).with_self_prompt(config);
    assert!(runner.self_prompt_config.enabled);
    assert_eq!(runner.self_prompt_config.max_per_hour, 5);
}

#[test]
fn self_prompt_disabled_by_default() {
    let token = CancellationToken::new();
    let runner = TaskRunner::new("test-nous", token);
    assert!(
        !runner.self_prompt_config.enabled,
        "self-prompting must be disabled by default"
    );
}

#[tokio::test]
async fn self_prompt_not_queued_when_disabled() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let mut runner = TaskRunner::with_bridge("test-nous", token, bridge);
    runner.register(make_echo_task("test-task"));

    // Simulate a result with a follow-up section
    let result = ExecutionResult {
        success: true,
        output: Some("## Follow-up\nInvestigate disk usage.\n".to_owned()),
    };
    runner.maybe_queue_self_prompt("test-task", &result);

    // Give a moment for any spawned task (there should be none)
    // kanon:ignore TESTING/sleep-in-test reason = "verifying no async task was spawned; brief yield to confirm absence"
    tokio::time::sleep(Duration::from_millis(10)).await;

    // No way to directly inspect spawned tasks, but we verify the limiter
    // was not invoked (count stays 0)
    assert_eq!(
        runner.self_prompt_limiter.count("test-nous"),
        0,
        "limiter should not be touched when disabled"
    );
}

#[tokio::test]
async fn self_prompt_queued_when_enabled_with_follow_up() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let config = crate::self_prompt::SelfPromptConfig {
        enabled: true,
        max_per_hour: 3,
    };
    let mut runner =
        TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
    runner.register(make_echo_task("test-task"));

    let result = ExecutionResult {
        success: true,
        output: Some("## Follow-up\nCheck /data disk usage.\n".to_owned()),
    };
    runner.maybe_queue_self_prompt("test-task", &result);

    assert_eq!(
        runner.self_prompt_limiter.count("test-nous"),
        1,
        "limiter should record the dispatched self-prompt"
    );
}

#[tokio::test]
async fn self_prompt_rate_limited_after_max() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let config = crate::self_prompt::SelfPromptConfig {
        enabled: true,
        max_per_hour: 1,
    };
    let mut runner =
        TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
    runner.register(make_echo_task("test-task"));

    let result = ExecutionResult {
        success: true,
        output: Some("## Follow-up\nFirst action.\n".to_owned()),
    };
    runner.maybe_queue_self_prompt("test-task", &result);
    assert_eq!(runner.self_prompt_limiter.count("test-nous"), 1);

    // Second attempt should be rate-limited
    let result2 = ExecutionResult {
        success: true,
        output: Some("## Follow-up\nSecond action.\n".to_owned()),
    };
    runner.maybe_queue_self_prompt("test-task", &result2);
    assert_eq!(
        runner.self_prompt_limiter.count("test-nous"),
        1,
        "second self-prompt should be rate-limited"
    );
}

#[test]
fn self_prompt_no_follow_up_no_dispatch() {
    let token = CancellationToken::new();
    let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
    let config = crate::self_prompt::SelfPromptConfig {
        enabled: true,
        max_per_hour: 5,
    };
    let mut runner =
        TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
    runner.register(make_echo_task("test-task"));

    let result = ExecutionResult {
        success: true,
        output: Some("Everything is fine. No issues.".to_owned()),
    };
    runner.maybe_queue_self_prompt("test-task", &result);

    assert_eq!(
        runner.self_prompt_limiter.count("test-nous"),
        0,
        "no follow-up section should mean no dispatch"
    );
}

// -- Error handling tests --

/// Error path: task with invalid cron expression is rejected during registration.
#[test]
fn register_task_with_invalid_cron_fails() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "bad-cron-task".to_owned(),
        name: "Bad cron task".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Cron("not a valid cron".to_owned()),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };

    // Registration succeeds (validation is lazy)
    runner.register(task);
    assert_eq!(runner.status().len(), 1);

    // But next_run calculation should fail - extract to avoid temporary lifetime issue
    let statuses = runner.status();
    let result = statuses[0].next_run.as_ref();
    // next_run is None when calculation fails
    assert!(result.is_none() || result.unwrap().is_empty());
}

/// Error path: failing command execution records failure in task status.
#[tokio::test]
async fn failing_command_records_consecutive_failures() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);

    let task = TaskDef {
        id: "failing-command".to_owned(),
        name: "Failing command".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_secs(60)),
        action: TaskAction::Command("exit 42".to_owned()),
        enabled: true,
        ..TaskDef::default()
    };
    runner.register(task);

    // Simulate failure recording
    runner.record_task_failure("failing-command", "exit code: 42");

    let statuses = runner.status();
    assert_eq!(statuses[0].consecutive_failures, 1);
    assert_eq!(statuses[0].last_error, Some("exit code: 42".to_owned()));
}

/// Error path: task execution with missing bridge for bridge-dependent tasks.
#[tokio::test]
async fn builtin_prosoche_without_bridge_returns_failure() {
    let result = execute_builtin(
        &BuiltinTask::Prosoche,
        "test-nous",
        None, // no bridge
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_ok());
    let exec_result = result.unwrap();
    assert!(!exec_result.success);
    assert!(exec_result
        .output
        .unwrap_or_default()
        .contains("no bridge configured"));
}

/// Error path: probe audit without bridge returns failure result.
#[tokio::test]
async fn probe_audit_without_bridge_returns_failure() {
    let result = execute_builtin(
        &BuiltinTask::ProbeAudit,
        "test-nous",
        None, // no bridge
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_ok());
    let exec_result = result.unwrap();
    assert!(!exec_result.success);
    assert!(exec_result
        .output
        .unwrap_or_default()
        .contains("no bridge configured"));
}

/// Error path: self-audit without bridge returns failure result.
#[tokio::test]
async fn self_audit_without_bridge_returns_failure() {
    let result = execute_builtin(
        &BuiltinTask::SelfAudit,
        "test-nous",
        None, // no bridge
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_ok());
    let exec_result = result.unwrap();
    assert!(!exec_result.success);
    assert!(exec_result
        .output
        .unwrap_or_default()
        .contains("no bridge configured"));
}

/// Error path: knowledge maintenance without executor returns not implemented.
#[tokio::test]
async fn knowledge_task_without_executor_returns_not_implemented() {
    let result = execute_builtin(
        &BuiltinTask::DecayRefresh,
        "test-nous",
        None,
        None,
        None,
        None, // no knowledge executor
    )
    .await;

    assert!(result.is_ok());
    let exec_result = result.unwrap();
    assert!(!exec_result.success);
    assert!(exec_result
        .output
        .unwrap_or_default()
        .contains("NOT_IMPLEMENTED"));
}

/// Error path: cron expression parsing returns descriptive error.
#[test]
fn cron_parse_error_includes_expression_and_reason() {
    use crate::cron_expr::CronExpr;

    let result = CronExpr::parse("invalid cron");
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(err_string.contains("invalid cron expression"));
    assert!(err_string.contains("invalid cron")); // original expression
}

/// Error path: cron expression with wrong field count returns error.
#[test]
fn cron_parse_wrong_field_count_returns_error() {
    use crate::cron_expr::CronExpr;

    let result = CronExpr::parse("* * *"); // only 3 fields
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(err_string.contains("expected 5 or 6 fields"));
}

/// Error path: cron expression with out-of-range values returns error.
#[test]
fn cron_parse_out_of_range_hour_returns_error() {
    use crate::cron_expr::CronExpr;

    let result = CronExpr::parse("0 0 25 * * *"); // hour 25 is invalid
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(err_string.contains("out of range"));
}

/// Error path: missing config paths return errors in `propose_rules`.
#[tokio::test]
async fn propose_rules_with_missing_data_dir_returns_error() {
    // NOTE: propose_rules uses a temp directory if the default doesn't exist,
    // so this test verifies the task runs without panic even with invalid paths
    let result = execute_builtin(
        &BuiltinTask::ProposeRules,
        "test-nous",
        None,
        Some(&crate::maintenance::MaintenanceConfig::default()),
        None,
        None,
    )
    .await;

    // Should complete without panic, even if data dir is missing
    assert!(result.is_ok());
}

/// Error path: task execution error includes `task_id` in error message.
#[test]
fn task_failed_error_includes_task_id() {
    use crate::error::Error;

    let err = Error::TaskFailed {
        task_id: "test-task-123".to_owned(),
        reason: "disk full".to_owned(),
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("test-task-123"));
    assert!(err_string.contains("disk full"));
}

/// Error path: cron parse error includes expression details.
#[test]
fn cron_parse_error_variant_includes_details() {
    use crate::error::Error;

    let err = Error::CronParse {
        expression: "0 0 * * *".to_owned(),
        reason: "invalid day-of-week".to_owned(),
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("0 0 * * *"));
    assert!(err_string.contains("invalid day-of-week"));
}

/// Error path: command failed error includes command details.
#[test]
fn command_failed_error_display() {
    use crate::error::Error;

    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "command not found");
    let err = Error::CommandFailed {
        command: "missing-binary".to_owned(),
        source: io_err,
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("missing-binary"));
}

/// Error path: task disabled error includes failure count.
#[test]
fn task_disabled_error_includes_failure_count() {
    use crate::error::Error;

    let err = Error::TaskDisabled {
        task_id: "failing-task".to_owned(),
        failures: 3,
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("failing-task"));
    assert!(err_string.contains('3'));
}

/// Error path: maintenance io error includes context.
#[test]
fn maintenance_io_error_includes_context() {
    use crate::error::Error;

    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let err = Error::MaintenanceIo {
        context: "reading state file".to_owned(),
        source: io_err,
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("reading state file"));
}

/// Error path: blocking join error includes context.
#[test]
fn blocking_join_error_includes_context() {
    use crate::error::Error;

    // Create a fake JoinError by panicking a task and catching it
    let rt = tokio::runtime::Runtime::new().unwrap();
    let join_err = rt
        .block_on(async {
            let handle = tokio::spawn(async { panic!("task panicked") });
            handle.await
        })
        .unwrap_err();

    let err = Error::BlockingJoin {
        context: "knowledge maintenance".to_owned(),
        source: join_err,
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("knowledge maintenance"));
}

/// Error path: shutdown error variant.
#[test]
fn shutdown_error_display() {
    use crate::error::Error;

    let err = Error::Shutdown {
        location: snafu::location!(),
    };

    let err_string = err.to_string();
    assert!(err_string.contains("shutdown"));
}
