#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid by construction after asserting len/call_count"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::bridge::DaemonBridge;
use crate::maintenance::{MaintenanceConfig, MaintenanceReport, RetentionSummary};

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
                outcome: TaskOutcome::Success,
                errors: 0,
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
        self.calls.lock().expect("not poisoned").push((
            nous_id.to_owned(),
            session_key.to_owned(),
            prompt.to_owned(),
        ));
        // WHY: clone the canned outcome out of the mutex so the future
        // doesn't borrow self past the lock guard. Snafu errors aren't
        // Clone, so we synthesize a fresh one for each Err invocation.
        let res: Result<ExecutionResult> = match &*self.result.lock().expect("not poisoned") {
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
    fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> Result<()> {
        Ok(())
    }

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

    fn materialize_derived_facts(&self) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn discover_serendipitous_facts(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
}

// --- execute_command ---

#[tokio::test]
async fn execute_command_success_captures_stdout() {
    let result = execute_command(
        "echo hello",
        CancellationToken::new(),
        Duration::from_mins(1),
    )
    .await
    .expect("should succeed");
    assert!(result.is_success());
    let output = result.output.expect("should have output");
    assert!(
        output.contains("hello"),
        "stdout should contain 'hello', got: {output}"
    );
}

#[tokio::test]
async fn execute_command_failure_returns_error() {
    let err = execute_command("exit 7", CancellationToken::new(), Duration::from_mins(1))
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
async fn execute_command_failure_summarizes_stderr_in_reason() {
    // WHY(#4948): command stderr may contain secrets or private paths. Failures
    // report metadata and digests instead of carrying raw stderr forward.
    let err = execute_command(
        "echo 'something failed synthetic-sensitive-token-4721 /tmp/acme.corp/private' >&2; exit 1",
        CancellationToken::new(),
        Duration::from_mins(1),
    )
    .await
    .expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("process output summary"),
        "expected stderr metadata in reason, got: {msg}"
    );
    assert!(
        !msg.contains("something failed"),
        "stderr text leaked: {msg}"
    );
    assert!(
        !msg.contains("synthetic-sensitive-token-4721"),
        "secret leaked: {msg}"
    );
    assert!(
        !msg.contains("/tmp/acme.corp/private"),
        "private path leaked: {msg}"
    );
}

#[tokio::test]
async fn execute_command_respects_cancellation_token() {
    // WHY: graceful cancellation must terminate a command promptly instead of
    // waiting for the outer 2× in-flight watchdog.
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let start = std::time::Instant::now();

    let handle = tokio::spawn(async move {
        execute_command("sleep 30", cancel_for_task, Duration::from_mins(1)).await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel();

    let err = handle
        .await
        .expect("join should succeed")
        .expect_err("should be cancelled");
    assert!(
        err.to_string().contains("cancelled"),
        "expected cancelled error, got: {err}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "cancelled command should exit quickly, took {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn execute_command_respects_per_task_timeout() {
    // WHY: a per-task timeout independent of the outer watchdog kills hung
    // commands even when no explicit cancellation request is issued.
    let start = std::time::Instant::now();
    let err = execute_command(
        "sleep 30",
        CancellationToken::new(),
        Duration::from_millis(250),
    )
    .await
    .expect_err("should time out");
    assert!(
        err.to_string().contains("timed out"),
        "expected timeout error, got: {err}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "timed-out command should exit quickly, took {:?}",
        start.elapsed()
    );
}

// --- execute_action dispatch ---

#[tokio::test]
async fn execute_action_dispatches_command_variant() {
    let action = TaskAction::Command("echo dispatched".to_owned());
    let result = execute_action(
        &action,
        "test-nous",
        None,
        None,
        None,
        None,
        &taxis::config::DaemonBehaviorConfig::default(),
    )
    .await
    .expect("should succeed");
    assert!(result.is_success());
    assert!(result.output.expect("output").contains("dispatched"));
}

#[tokio::test]
async fn execute_action_dispatches_builtin_variant() {
    // WHY: SelfPrompt is the canonical "no setup needed" builtin — it
    // returns a canned error message without needing bridge or executor.
    let action = TaskAction::Builtin(BuiltinTask::SelfPrompt);
    let result = execute_action(
        &action,
        "test-nous",
        None,
        None,
        None,
        None,
        &taxis::config::DaemonBehaviorConfig::default(),
    )
    .await
    .expect("should not error");
    assert!(!result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("self-prompt must be dispatched"),
        "expected canned message"
    );
}

#[tokio::test]
async fn routing_store_refresh_builtin_refreshes_attached_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("2026-04-17.jsonl");
    let content = format!(
        "{}\n",
        serde_json::json!({
            "session_outcomes": [
                {"model": "provider-a", "status": "success", "category": "feature"}
            ]
        })
    );
    tokio::fs::write(&path, content).await.expect("write jsonl");

    let store = Arc::new(aletheia_routing::AfterActionStore::new(
        tmp.path().to_owned(),
    ));
    let config = MaintenanceConfig {
        after_action_store: Some(Arc::clone(&store)),
        ..MaintenanceConfig::default()
    };
    let daemon_behavior = taxis::config::DaemonBehaviorConfig::default();
    let result = execute_builtin_with_behavior(
        &BuiltinTask::RoutingStoreRefresh,
        ExecutionContext {
            nous_id: "system",
            bridge: None,
            maintenance: Some(&config),
            retention_executor: None,
            knowledge_executor: None,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            daemon_behavior: &daemon_behavior,
            cancel: CancellationToken::new(),
            timeout: Duration::from_mins(5),
        },
    )
    .await
    .expect("routing refresh should succeed");

    assert!(result.is_success());
    let stats = store
        .rolling_stats(
            &aletheia_routing::types::ProviderId::new("provider-a"),
            &aletheia_routing::types::TaskCategory::Feature,
            std::time::Duration::from_hours(168),
        )
        .await
        .expect("rolling stats query")
        .expect("refreshed stats");
    assert_eq!(stats.total, 1);
}

// --- execute_builtin: bridge-dependent paths ---

#[tokio::test]
async fn prosoche_no_bridge_runs_local_check() {
    let result = execute_builtin(&BuiltinTask::Prosoche, "test-nous", None, None, None, None)
        .await
        .expect("should not error");
    assert!(result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("checked_at")
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
    assert!(result.is_success());
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
    assert!(!result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("dispatch failed")
    );
}

#[tokio::test]
async fn self_audit_no_bridge_runs_prosoche_runner() {
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
    .await
    .expect("should not error");
    assert!(result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("prosoche self-audit complete")
    );
    assert!(
        std::fs::read_dir(tmp.path().join("audits"))
            .expect("audit dir")
            .next()
            .is_some(),
        "self-audit should persist a report"
    );
}

#[tokio::test]
async fn self_audit_persist_failure_returns_unsuccessful_result() {
    let file = tempfile::NamedTempFile::new().expect("tempfile");
    let maintenance = crate::maintenance::MaintenanceConfig {
        prosoche_audit_dir: file.path().to_path_buf(),
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
    .await
    .expect("should compute report even when persistence fails");

    assert!(!result.is_success());
    let output = result.output.as_deref().unwrap_or_default();
    assert!(output.contains("report computed but not persisted"));
    assert!(output.contains("persist error"));
}

#[tokio::test]
async fn self_audit_with_bridge_runs_local_runner() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let maintenance = crate::maintenance::MaintenanceConfig {
        prosoche_audit_dir: tmp.path().join("audits"),
        ..crate::maintenance::MaintenanceConfig::default()
    };
    let bridge = TestBridge::ok("audit-ok");
    let result = execute_builtin(
        &BuiltinTask::SelfAudit,
        "test-nous",
        Some(&bridge),
        Some(&maintenance),
        None,
        None,
    )
    .await
    .expect("should not error");
    assert!(result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("prosoche self-audit complete")
    );
    let calls = bridge.calls.lock().expect("not poisoned");
    assert!(calls.is_empty());
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
    assert!(!result.is_success());
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
    assert!(!result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("self-prompt must be dispatched")
    );
}

#[tokio::test]
async fn drift_detection_missing_template_reports_unsuccessful() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing_example = tmp.path().join("definitely-missing-instance.example");
    let maintenance = crate::maintenance::MaintenanceConfig {
        drift_detection: crate::maintenance::DriftDetectionConfig {
            enabled: true,
            instance_root: tmp.path().join("instance"),
            example_root: missing_example.clone(),
            alert_on_missing: true,
            ignore_patterns: Vec::new(),
            optional_patterns: Vec::new(),
        },
        ..crate::maintenance::MaintenanceConfig::default()
    };

    let result = execute_builtin(
        &BuiltinTask::DriftDetection,
        "test-nous",
        None,
        Some(&maintenance),
        None,
        None,
    )
    .await
    .expect("should not error even when template is missing");

    assert!(
        !result.is_success(),
        "missing template must be unsuccessful"
    );
    let output = result.output.as_deref().unwrap_or_default();
    assert!(
        output.contains("template unavailable"),
        "expected unavailable warning, got: {output}"
    );
    assert!(
        output.contains(&missing_example.display().to_string()),
        "expected template path in output, got: {output}"
    );
}

// --- execute_builtin: executor-dependent paths ---

#[tokio::test]
async fn retention_with_executor_returns_summary() {
    let executor: Arc<dyn crate::maintenance::RetentionExecutor> = Arc::new(MockRetention {
        summary: RetentionSummary {
            sessions_cleaned: 3,
            cap_sessions_cleaned: 1,
            messages_cleaned: 12,
            blackboard_entries_cleaned: 2,
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
    assert!(result.is_success());
    let output = result.output.expect("output");
    assert!(output.contains("3 sessions (1 cap)"));
    assert!(output.contains("12 messages"));
    assert!(output.contains("2 blackboard entries"));
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
    assert!(!result.is_success());
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("no knowledge maintenance executor configured")
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
    assert!(result.is_success());
    let output = result.output.expect("output");
    assert!(
        output.contains("7 processed"),
        "expected '7 processed' in output, got: {output}"
    );
    assert!(output.contains("2 modified"));
}

#[tokio::test]
async fn serendipity_discovery_with_executor_returns_report() {
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(MockKnowledge);
    let result = execute_builtin(
        &BuiltinTask::SerendipityDiscovery,
        "test-nous",
        None,
        None,
        None,
        Some(executor),
    )
    .await
    .expect("should succeed");
    assert!(result.is_success());
    let output = result.output.expect("output");
    assert!(
        output.contains("0 processed"),
        "expected zero-count report for mock executor, got: {output}"
    );
}
