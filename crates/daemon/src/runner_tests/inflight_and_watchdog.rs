//! In-flight timeout, bridge cancellation, and watchdog tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::super::*;
use crate::bridge::DaemonBridge;
use crate::runner::ExecutionResult;

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
                outcome: TaskOutcome::Success,
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
                outcome: TaskOutcome::Failed,
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

/// Bridge returning a canned turn outcome immediately, for end-to-end
/// runner tests of turn-outcome → task-outcome mapping (#7252).
struct CannedOutcomeBridge {
    result: ExecutionResult,
}

impl DaemonBridge for CannedOutcomeBridge {
    fn send_prompt(
        &self,
        _nous_id: &str,
        _session_key: &str,
        _prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>> {
        let result = self.result.clone();
        Box::pin(async move { Ok(result) })
    }
}

/// Register a due prosoche task on a bridge-backed runner, run one
/// tick + in-flight sweep, and return the runner for assertions.
async fn run_prosoche_once(bridge: Arc<dyn DaemonBridge>, mode: DaemonOutputMode) -> TaskRunner {
    let shutdown = CancellationToken::new();
    let mut runner = TaskRunner::with_bridge("test-nous", shutdown, bridge).with_output_mode(mode);

    let task = TaskDef {
        id: "test-prosoche".to_owned(),
        name: "Prosoche attention check".to_owned(),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(45)),
        action: TaskAction::Builtin(BuiltinTask::Prosoche),
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
        runner.in_flight.contains_key("test-prosoche"),
        "task should be in flight after tick"
    );

    // Wait for the spawned task to finish, then collect the outcome.
    for _ in 0..100 {
        if runner
            .in_flight
            .get("test-prosoche")
            .is_some_and(|task| task.handle.is_finished())
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    runner.check_in_flight().await;
    runner
}

/// WHY(#7252): the regression this issue reports — a daemon prosoche run
/// whose turn failed (e.g. history `load_failed`) logged `task completed`
/// and kept a clean failure count. The turn's failure must reach the task
/// record: `consecutive_failures` increments and the error is surfaced.
#[tokio::test]
async fn prosoche_failed_turn_records_task_failure() {
    let bridge: Arc<dyn DaemonBridge> = Arc::new(CannedOutcomeBridge {
        result: ExecutionResult::failed(Some("turn failed: history load_failed".to_owned())),
    });
    let runner = run_prosoche_once(bridge, DaemonOutputMode::Brief).await;

    assert_eq!(
        runner.tasks[0].consecutive_failures, 1,
        "a failed turn must record a failed task run"
    );
    let last_error = runner.tasks[0]
        .last_error
        .as_deref()
        .expect("failed run must surface the turn's error");
    assert!(
        last_error.contains("turn failed: history load_failed"),
        "brief mode carries the redacted error excerpt, got: {last_error}"
    );
    assert_eq!(
        runner.tasks[0].run_count, 0,
        "a failed run must not inflate the completion count"
    );
}

/// WHY(#7252 + #4948): in the default Summary output mode the failure is
/// still recorded honestly — outcome failed, failure count increments — but
/// the task record carries only the metadata digest of the output, never
/// raw turn content. The readable error is in the daemon's WARN logs.
#[tokio::test]
async fn prosoche_failed_turn_records_failure_without_leaking_output() {
    let bridge: Arc<dyn DaemonBridge> = Arc::new(CannedOutcomeBridge {
        result: ExecutionResult::failed(Some("turn failed: history load_failed".to_owned())),
    });
    let runner = run_prosoche_once(bridge, DaemonOutputMode::Summary).await;

    assert_eq!(runner.tasks[0].consecutive_failures, 1);
    let last_error = runner.tasks[0]
        .last_error
        .as_deref()
        .expect("failed run must record an error reference");
    assert!(
        last_error.starts_with("output summary:"),
        "summary mode digests the output, got: {last_error}"
    );
    assert!(
        !last_error.contains("load_failed"),
        "summary mode must not leak raw output text, got: {last_error}"
    );
}

#[tokio::test]
async fn prosoche_successful_turn_records_task_completion() {
    let bridge: Arc<dyn DaemonBridge> = Arc::new(CannedOutcomeBridge {
        result: ExecutionResult::success(Some("prosoche report".to_owned())),
    });
    let runner = run_prosoche_once(bridge, DaemonOutputMode::Summary).await;

    assert_eq!(
        runner.tasks[0].consecutive_failures, 0,
        "a successful turn must not record a failure"
    );
    assert_eq!(runner.tasks[0].run_count, 1);
    assert_eq!(runner.tasks[0].last_error, None);
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
        schedule: Schedule::Interval(Duration::from_mins(1)),
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
            timeout: Duration::from_mins(1),
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
