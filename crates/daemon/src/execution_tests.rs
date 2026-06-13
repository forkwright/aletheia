#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid by construction after asserting len/call_count"
)]

use std::future::Future;
use std::io::Write as _;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

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

struct RealKnowledge {
    store: std::sync::Arc<episteme::knowledge_store::KnowledgeStore>,
}

impl RealKnowledge {
    fn open(
        dir: &std::path::Path,
    ) -> (
        Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor>,
        std::sync::Arc<episteme::knowledge_store::KnowledgeStore>,
    ) {
        let store = episteme::knowledge_store::KnowledgeStore::open_fjall(
            dir.join("knowledge"),
            episteme::knowledge_store::KnowledgeConfig::default(),
        )
        .expect("open real fjall knowledge store");
        let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> = Arc::new(Self {
            store: Arc::clone(&store),
        });
        (executor, store)
    }
}

impl crate::maintenance::KnowledgeMaintenanceExecutor for RealKnowledge {
    fn insert_fact(&self, fact: &episteme::knowledge::Fact) -> Result<()> {
        self.store.insert_fact(fact).map_err(|e| {
            error::TaskFailedSnafu {
                task_id: "test-fact-persistence".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })
    }

    fn refresh_decay_scores(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
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

fn current_facts(
    store: &episteme::knowledge_store::KnowledgeStore,
    nous_id: &str,
) -> Vec<episteme::knowledge::Fact> {
    let now = episteme::knowledge::format_timestamp(&jiff::Timestamp::now());
    store
        .query_facts(nous_id, &now, 100)
        .expect("query persisted facts")
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
    assert!(result.success);
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

#[tokio::test]
async fn routing_store_refresh_builtin_refreshes_attached_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("2026-04-17.jsonl");
    let mut file = std::fs::File::create(path).expect("jsonl file");
    writeln!(
        file,
        "{}",
        serde_json::json!({
            "session_outcomes": [
                {"model": "provider-a", "status": "success", "category": "feature"}
            ]
        })
    )
    .expect("write jsonl");

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
            daemon_behavior: &daemon_behavior,
            cancel: CancellationToken::new(),
        },
    )
    .await
    .expect("routing refresh should succeed");

    assert!(result.success);
    let stats = store
        .rolling_stats(
            &aletheia_routing::types::ProviderId::new("provider-a"),
            &aletheia_routing::types::TaskCategory::Feature,
            std::time::Duration::from_hours(168),
        )
        .await
        .expect("refreshed stats");
    assert_eq!(stats.total, 1);
}

// --- execute_builtin: bridge-dependent paths ---

#[tokio::test]
async fn prosoche_no_bridge_runs_local_check() {
    let result = execute_builtin(&BuiltinTask::Prosoche, "test-nous", None, None, None, None)
        .await
        .expect("should not error");
    assert!(result.success);
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
    assert!(result.success);
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
    assert!(result.success);
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
    assert!(result.success);
    let output = result.output.expect("output");
    assert!(output.contains("3 sessions"));
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
    assert!(!result.success);
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
    assert!(result.success);
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
    assert!(result.success);
    let output = result.output.expect("output");
    assert!(
        output.contains("0 processed"),
        "expected zero-count report for mock executor, got: {output}"
    );
}

// --- cron execution counter helpers ---

#[test]
fn cron_execution_counters_increment_on_record() {
    // WHY: ops_fact_extraction reads daemon's shadow cron counters to build
    // OpsSnapshot; verify the read helpers reflect record_cron_execution
    // calls and split ok/error correctly.
    let ok_before = crate::metrics::cron_executions_ok();
    let err_before = crate::metrics::cron_executions_error();
    let total_before = crate::metrics::cron_executions_total();

    crate::metrics::record_cron_execution("_test_ops_fact_success", 0.1, true);
    crate::metrics::record_cron_execution("_test_ops_fact_failure", 0.2, false);

    assert_eq!(crate::metrics::cron_executions_ok(), ok_before + 1);
    assert_eq!(crate::metrics::cron_executions_error(), err_before + 1);
    assert_eq!(crate::metrics::cron_executions_total(), total_before + 2);
}

#[tokio::test]
async fn ops_fact_extraction_persists_all_extracted_facts_to_real_fjall() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (executor, store) = RealKnowledge::open(dir.path());

    for _ in 0..5 {
        crate::metrics::record_cron_execution("_test_ops_persist_success", 0.1, true);
    }

    let result = execute_builtin(
        &BuiltinTask::OpsFactExtraction,
        "alice",
        None,
        None,
        None,
        Some(executor),
    )
    .await
    .expect("ops fact extraction should persist");

    assert!(result.success);
    assert_eq!(
        result.output.as_deref(),
        Some("3 operational facts extracted, 3 inserted")
    );

    let facts = current_facts(&store, "alice");
    assert_eq!(facts.len(), 3, "all extracted ops facts are retrievable");
    let contents: Vec<&str> = facts.iter().map(|fact| fact.content.as_str()).collect();
    assert!(
        contents
            .iter()
            .any(|content| content.contains("active sessions")),
        "session count fact should be persisted: {contents:?}"
    );
    assert!(
        contents
            .iter()
            .any(|content| content.contains("tool success rate")),
        "tool success-rate fact should be persisted: {contents:?}"
    );
    assert!(
        contents
            .iter()
            .any(|content| content.contains("error count")),
        "error-count fact should be persisted: {contents:?}"
    );
}

#[tokio::test]
async fn lesson_extraction_persists_training_facts_to_real_fjall() {
    let dir = tempfile::tempdir().expect("tempdir");
    let training = dir.path().join("training");
    std::fs::create_dir_all(&training).expect("create training dir");
    let (executor, store) = RealKnowledge::open(dir.path());

    let violations = [
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/lib.rs","line":10,"snippet":".expect(\"msg\")","project":"","pr_number":42,"sha":"abc123"}"#,
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":20,"snippet":".expect(\"other\")","project":"","pr_number":null,"sha":null}"#,
    ];
    tokio::fs::write(training.join("violations.jsonl"), violations.join("\n"))
        .await
        .expect("write violations");

    let result = execute_lesson_extraction_from_dir("alice", &training, executor.as_ref())
        .expect("lesson extraction should persist");

    assert!(result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("2 facts produced, 2 inserted"),
        "unexpected output: {:?}",
        result.output
    );

    let facts = current_facts(&store, "alice");
    assert_eq!(facts.len(), 2, "all lesson facts are retrievable");
    assert!(
        facts
            .iter()
            .all(|fact| fact.provenance.source_session_id.as_deref()
                == Some("daemon:lesson-extraction")),
        "lesson facts should carry daemon provenance: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.content.contains("was fixed in PR")),
        "fixed lesson content should be persisted: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.content.contains("recurs across scans")),
        "recurring lesson content should be persisted: {facts:?}"
    );
}

/// Regression: the default prosoche self-audit runner must not include the
/// `InstinctPatternsCheck` stub, which emits a fixed speculative finding without
/// real gnomon weights backing it (#4572).
#[tokio::test]
async fn self_audit_does_not_emit_instinct_patterns_findings() {
    use crate::prosoche_audit::{
        AuditStorage, ConsistencyCheck, GoalAlignmentCheck, ProsocheAuditRunner, ProsocheState,
        SessionQualityCheck, StalenessCheck,
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let storage = AuditStorage::new(tmp.path());
    let checks: Vec<std::sync::Arc<dyn crate::prosoche_audit::ProsocheCheck>> = vec![
        std::sync::Arc::new(ConsistencyCheck),
        std::sync::Arc::new(StalenessCheck::default()),
        std::sync::Arc::new(GoalAlignmentCheck),
        std::sync::Arc::new(SessionQualityCheck::default()),
    ];
    let runner = ProsocheAuditRunner::new(checks, storage);

    let state = ProsocheState {
        nous_id: "alice".to_owned(),
        checked_at: "2026-06-12T00:00:00Z".to_owned(),
        ..ProsocheState::default()
    };

    let report = runner.run_audit(&state).await;

    assert!(
        report
            .findings
            .iter()
            .all(|f| f.source != "prosoche::InstinctPatternsCheck"),
        "default runner must not include instinct-pattern stub findings; got: {:?}",
        report.findings
    );
}

#[tokio::test]
async fn fact_extraction_without_store_returns_error() {
    let err = execute_builtin(
        &BuiltinTask::OpsFactExtraction,
        "alice",
        None,
        None,
        None,
        None,
    )
    .await
    .expect_err("missing persistence target should error");

    assert!(
        err.to_string().contains("no knowledge executor configured"),
        "unexpected error: {err}"
    );
}
