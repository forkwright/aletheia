#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid by construction after asserting len/call_count"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use crate::bridge::DaemonBridge;
use crate::maintenance::{MaintenanceReport, RetentionSummary};

use super::*;

/// Configurable test bridge: records every `send_prompt` call and returns
/// either a canned `Ok` or a canned error from a snafu builder.
///
/// WHY: `execute_builtin`'s `Prosoche`/`SelfAudit`/`ProbeAudit` branches each
/// take separate code paths for `bridge.send_prompt(...) → Ok` vs `Err`.
/// We need a controllable mock to exercise both.
struct TestBridge {
    result: Mutex<std::result::Result<ExecutionResult, ()>>,
    calls: Mutex<Vec<(String, String, String)>>,
}

impl TestBridge {
    fn ok(output: &str) -> Self {
        Self {
            result: Mutex::new(Ok(ExecutionResult {
                success: true,
                output: Some(output.to_owned()),
            })),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn err() -> Self {
        Self {
            result: Mutex::new(Err(())),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().expect("not poisoned").len()
    }
}

impl DaemonBridge for TestBridge {
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ExecutionResult>> + Send + '_>> {
        self.calls
            .lock()
            .expect("not poisoned")
            .push((nous_id.to_owned(), session_key.to_owned(), prompt.to_owned()));
        // WHY: clone the canned outcome out of the mutex so the future
        // doesn't borrow self past the lock guard. Snafu errors aren't
        // Clone, so we synthesize a fresh one for each Err invocation.
        let res: Result<ExecutionResult> =
            match &*self.result.lock().expect("not poisoned") {
                Ok(r) => Ok(r.clone()),
                Err(()) => error::TaskFailedSnafu {
                    task_id: "test".to_owned(),
                    reason: "simulated bridge error".to_owned(),
                }
                .fail(),
            };
        Box::pin(async move { res })
    }
}

/// Mock retention executor that returns a canned summary.
struct MockRetention {
    summary: RetentionSummary,
}

impl crate::maintenance::RetentionExecutor for MockRetention {
    fn execute_retention(&self) -> Result<RetentionSummary> {
        Ok(self.summary.clone())
    }
}

/// Mock knowledge executor that returns canned reports for every method.
struct MockKnowledge;

impl crate::maintenance::KnowledgeMaintenanceExecutor for MockKnowledge {
    fn refresh_decay_scores(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport {
            items_processed: 7,
            items_modified: 2,
            ..Default::default()
        })
    }
    fn deduplicate_entities(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn recompute_graph_scores(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn refresh_embeddings(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn garbage_collect(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn maintain_indexes(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn health_check(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
    fn run_skill_decay(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
}

// --- execute_command ---

#[tokio::test]
async fn execute_command_success_captures_stdout() {
    let result = execute_command("echo hello").await.expect("should succeed");
    assert!(result.success);
    let output = result.output.expect("should have output");
    assert!(
        output.contains("hello"),
        "stdout should contain 'hello', got: {output}"
    );
}

#[tokio::test]
async fn execute_command_failure_returns_error() {
    let err = execute_command("exit 7")
        .await
        .expect_err("non-zero exit should fail");
    let msg = err.to_string();
    // Either an exit-code message or stderr is captured.
    assert!(
        msg.contains('7') || msg.contains("exit"),
        "expected exit-code in error, got: {msg}"
    );
}

#[tokio::test]
async fn execute_command_failure_uses_stderr_in_reason() {
    // WHY: When a command writes to stderr and exits non-zero, the error
    // reason should contain the stderr output rather than the bare exit code.
    let err = execute_command("echo 'something failed' >&2; exit 1")
        .await
        .expect_err("should fail");
    assert!(
        err.to_string().contains("something failed"),
        "expected stderr in reason, got: {err}"
    );
}

// --- execute_action dispatch ---

#[tokio::test]
async fn execute_action_dispatches_command_variant() {
    let action = TaskAction::Command("echo dispatched".to_owned());
    let result = execute_action(&action, "test-nous", None, None, None, None)
        .await
        .expect("should succeed");
    assert!(result.success);
    assert!(result.output.expect("output").contains("dispatched"));
}

#[tokio::test]
async fn execute_action_dispatches_builtin_variant() {
    // WHY: SelfPrompt is the canonical "no setup needed" builtin — it
    // returns a canned error message without needing bridge or executor.
    let action = TaskAction::Builtin(BuiltinTask::SelfPrompt);
    let result = execute_action(&action, "test-nous", None, None, None, None)
        .await
        .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("self-prompt must be dispatched"),
        "expected canned message"
    );
}

// --- execute_builtin: bridge-dependent paths ---

#[tokio::test]
async fn prosoche_no_bridge_returns_unconfigured() {
    let result = execute_builtin(
        &BuiltinTask::Prosoche,
        "test-nous",
        None,
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("no bridge configured")
    );
}

#[tokio::test]
async fn prosoche_with_bridge_dispatches() {
    let bridge = TestBridge::ok("ok");
    let result = execute_builtin(
        &BuiltinTask::Prosoche,
        "test-nous",
        Some(&bridge),
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    // WHY: Prosoche always reports success=true after a successful
    // dispatch, regardless of the bridge's inner success flag, because
    // the dispatch itself is what's being tracked here.
    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("dispatched"));
    assert_eq!(bridge.call_count(), 1);
    let calls = bridge.calls.lock().expect("not poisoned");
    assert_eq!(calls[0].0, "test-nous");
    assert_eq!(calls[0].1, "daemon:prosoche");
}

#[tokio::test]
async fn prosoche_bridge_error_returns_failure() {
    let bridge = TestBridge::err();
    let result = execute_builtin(
        &BuiltinTask::Prosoche,
        "test-nous",
        Some(&bridge),
        None,
        None,
        None,
    )
    .await
    .expect("inner error should be wrapped, not propagated");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("dispatch failed")
    );
}

#[tokio::test]
async fn self_audit_no_bridge_returns_unconfigured() {
    let result = execute_builtin(
        &BuiltinTask::SelfAudit,
        "test-nous",
        None,
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("no bridge configured")
    );
}

#[tokio::test]
async fn self_audit_with_bridge_dispatches() {
    let bridge = TestBridge::ok("audit-ok");
    let result = execute_builtin(
        &BuiltinTask::SelfAudit,
        "test-nous",
        Some(&bridge),
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("dispatched"));
    let calls = bridge.calls.lock().expect("not poisoned");
    assert_eq!(calls[0].1, "daemon:self-audit");
}

#[tokio::test]
async fn probe_audit_no_bridge_returns_unconfigured() {
    let result = execute_builtin(
        &BuiltinTask::ProbeAudit,
        "test-nous",
        None,
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("no bridge configured")
    );
}

#[tokio::test]
async fn self_prompt_returns_runner_only_message() {
    // WHY: SelfPrompt is dispatched inline by the runner from prosoche
    // output. Reaching this arm directly is a misconfiguration; the
    // canned message lets the operator catch it.
    let result = execute_builtin(
        &BuiltinTask::SelfPrompt,
        "test-nous",
        None,
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("self-prompt must be dispatched")
    );
}

// --- execute_builtin: executor-dependent paths ---

#[tokio::test]
async fn retention_with_executor_returns_summary() {
    let executor: Arc<dyn crate::maintenance::RetentionExecutor> = Arc::new(MockRetention {
        summary: RetentionSummary {
            sessions_cleaned: 3,
            messages_cleaned: 12,
            bytes_freed: 4096,
        },
    });
    let result = execute_builtin(
        &BuiltinTask::RetentionExecution,
        "test-nous",
        None,
        None,
        Some(executor),
        None,
    )
    .await
    .expect("should succeed");
    assert!(result.success);
    let output = result.output.expect("output");
    assert!(output.contains("3 sessions"));
    assert!(output.contains("12 messages"));
    assert!(output.contains("4096 bytes"));
}

#[tokio::test]
async fn knowledge_task_no_executor_returns_not_implemented() {
    let result = execute_builtin(
        &BuiltinTask::DecayRefresh,
        "test-nous",
        None,
        None,
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(!result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("NOT_IMPLEMENTED")
    );
}

#[tokio::test]
async fn knowledge_task_with_executor_returns_report() {
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(MockKnowledge);
    let result = execute_builtin(
        &BuiltinTask::DecayRefresh,
        "test-nous",
        None,
        None,
        None,
        Some(executor),
    )
    .await
    .expect("should succeed");
    assert!(result.success);
    let output = result.output.expect("output");
    assert!(
        output.contains("7 processed"),
        "expected '7 processed' in output, got: {output}"
    );
    assert!(output.contains("2 modified"));
}

// --- prometheus counter helpers ---

#[test]
fn read_prometheus_counter_unknown_metric_returns_zero() {
    // WHY: read_prometheus_counter must return 0 (not panic, not error)
    // for an unknown metric so callers can build snapshots without
    // pre-checking metric registration.
    let value = read_prometheus_counter("test_unknown_metric_xyz_does_not_exist");
    assert_eq!(value, 0);
}

#[test]
fn read_prometheus_counter_with_label_unknown_returns_zero() {
    let value = read_prometheus_counter_with_label(
        "test_unknown_metric_xyz_does_not_exist",
        "status",
        "ok",
    );
    assert_eq!(value, 0);
}
