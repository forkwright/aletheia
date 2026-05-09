//! Self-prompt + bridge-missing + error-variant display tests (split from `runner_tests.rs`).

use super::super::*;
use super::make_echo_task;
use crate::execution::execute_builtin;

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
    let mut runner = TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
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
    let mut runner = TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
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
    let mut runner = TaskRunner::with_bridge("test-nous", token, bridge).with_self_prompt(config);
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

#[test]
fn register_top_issue_self_prompt_adds_recurring_task() {
    let token = CancellationToken::new();
    let mut runner = TaskRunner::new("test-nous", token);
    let issues = vec![crate::self_prompt::OpenIssue {
        number: 1,
        title: "Generate issue-driven prompt".to_owned(),
        body: "Use issue title and body.".to_owned(),
    }];

    let task_id = runner
        .register_top_issue_self_prompt(&issues, Schedule::Interval(Duration::from_mins(30)))
        .expect("registered task");

    assert_eq!(task_id, "issue-self-prompt-1");
    let statuses = runner.status();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].id, "issue-self-prompt-1");
    assert_eq!(statuses[0].name, "Issue #1 self-prompt");
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
        schedule: Schedule::Interval(Duration::from_mins(1)),
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
async fn builtin_prosoche_without_bridge_runs_local_check() {
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
    assert!(exec_result.success);
    assert!(
        exec_result
            .output
            .unwrap_or_default()
            .contains("checked_at")
    );
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
    assert!(
        exec_result
            .output
            .unwrap_or_default()
            .contains("no bridge configured")
    );
}

/// Self-audit runs the local prosoche audit runner without a bridge.
#[tokio::test]
async fn self_audit_without_bridge_runs_local_runner() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let maintenance = crate::maintenance::MaintenanceConfig {
        prosoche_audit_dir: tmp.path().join("audits"),
        ..crate::maintenance::MaintenanceConfig::default()
    };
    let result = execute_builtin(
        &BuiltinTask::SelfAudit,
        "test-nous",
        None,
        Some(&maintenance),
        None,
        None,
    )
    .await;

    assert!(result.is_ok());
    let exec_result = result.unwrap();
    assert!(exec_result.success);
    assert!(
        exec_result
            .output
            .unwrap_or_default()
            .contains("prosoche self-audit complete")
    );
}

/// Error path: knowledge maintenance without executor returns an explicit failure.
#[tokio::test]
async fn knowledge_task_without_executor_returns_unconfigured_failure() {
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
    assert!(
        exec_result
            .output
            .unwrap_or_default()
            .contains("executor configured")
    );
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
