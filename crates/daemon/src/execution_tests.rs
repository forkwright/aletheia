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

/// Mock knowledge executor that returns partial-error reports for specific
/// operations, so we can regression-test error propagation.
struct PartialErrorKnowledge {
    decay_report: MaintenanceReport,
    graph_report: MaintenanceReport,
}

impl crate::maintenance::KnowledgeMaintenanceExecutor for PartialErrorKnowledge {
    fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> Result<()> {
        Ok(())
    }

    fn refresh_decay_scores(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(self.decay_report.clone())
    }

    fn deduplicate_entities(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn recompute_graph_scores(&self, _nous_id: &str) -> Result<MaintenanceReport> {
        Ok(self.graph_report.clone())
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
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
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
    assert!(result.success);
    assert!(
        result
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("checked_at")
    );
}

#[cfg(feature = "knowledge-store")]
fn make_runtime_prosoche_fact() -> episteme::knowledge::Fact {
    let now = jiff::Timestamp::now();
    episteme::knowledge::Fact {
        id: episteme::id::FactId::new("fact-runtime-prosoche-001").expect("valid id"),
        nous_id: "test-nous".to_owned(),
        fact_type: "observation".to_owned(),
        content: "test content".to_owned(),
        scope: None,
        project_id: None,
        temporal: episteme::knowledge::FactTemporal {
            valid_from: now,
            valid_to: episteme::knowledge::far_future(),
            recorded_at: now,
        },
        provenance: episteme::knowledge::FactProvenance {
            confidence: 0.9,
            tier: episteme::knowledge::EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: episteme::knowledge::FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: episteme::knowledge::FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: episteme::knowledge::FactSensitivity::Public,
        visibility: episteme::knowledge::Visibility::Private,
    }
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn prosoche_no_bridge_uses_context_knowledge_store() {
    let store = episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");
    let fact = make_runtime_prosoche_fact();
    store.insert_fact(&fact).expect("insert fact");

    let daemon_behavior = taxis::config::DaemonBehaviorConfig::default();
    let result = execute_builtin_with_behavior(
        &BuiltinTask::Prosoche,
        ExecutionContext {
            nous_id: "test-nous",
            bridge: None,
            maintenance: None,
            retention_executor: None,
            knowledge_executor: None,
            knowledge_store: Some(Arc::clone(&store)),
            daemon_behavior: &daemon_behavior,
            cancel: CancellationToken::new(),
        },
    )
    .await
    .expect("prosoche should run");

    assert!(result.success);
    let output = result.output.expect("prosoche output");
    let parsed: crate::prosoche::ProsocheResult =
        serde_json::from_str(&output).expect("prosoche JSON output");

    assert!(
        parsed.items.iter().any(|item| {
            matches!(
                item.category,
                crate::prosoche::AttentionCategory::MemoryAnomaly
            ) && item.summary.contains("Orphaned fact")
        }),
        "runtime Prosoche output should include store-backed memory anomaly: {parsed:?}"
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

    assert!(!result.success);
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

    assert!(!result.success, "missing template must be unsuccessful");
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
    assert!(result.success);
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

#[tokio::test]
async fn decay_refresh_partial_errors_return_failure_with_counts_and_detail() {
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(PartialErrorKnowledge {
            decay_report: MaintenanceReport {
                items_processed: 5,
                items_modified: 1,
                errors: 2,
                duration_ms: 42,
                detail: Some("decay refresh: 2 partial failures".to_owned()),
            },
            graph_report: MaintenanceReport::default(),
        });

    let result = execute_builtin(
        &BuiltinTask::DecayRefresh,
        "test-nous",
        None,
        None,
        None,
        Some(executor),
    )
    .await
    .expect("should return a result");

    assert!(
        !result.success,
        "errors > 0 must be reported as task failure"
    );
    let output = result.output.expect("output should be present");
    assert!(
        output.contains("5 processed"),
        "output should include processed count: {output}"
    );
    assert!(
        output.contains("1 modified"),
        "output should include modified count: {output}"
    );
    assert!(
        output.contains("2 errors"),
        "output should include error count: {output}"
    );
    assert!(
        output.contains("decay refresh: 2 partial failures"),
        "output should preserve detail: {output}"
    );
}

#[tokio::test]
async fn graph_recompute_partial_errors_return_failure_with_counts_and_detail() {
    let executor: Arc<dyn crate::maintenance::KnowledgeMaintenanceExecutor> =
        Arc::new(PartialErrorKnowledge {
            decay_report: MaintenanceReport::default(),
            graph_report: MaintenanceReport {
                items_processed: 12,
                items_modified: 4,
                errors: 3,
                duration_ms: 100,
                detail: Some("graph recompute: centrality pass degraded".to_owned()),
            },
        });

    let result = execute_builtin(
        &BuiltinTask::GraphRecompute,
        "test-nous",
        None,
        None,
        None,
        Some(executor),
    )
    .await
    .expect("should return a result");

    assert!(
        !result.success,
        "errors > 0 must be reported as task failure"
    );
    let output = result.output.expect("output should be present");
    assert!(
        output.contains("12 processed"),
        "output should include processed count: {output}"
    );
    assert!(
        output.contains("4 modified"),
        "output should include modified count: {output}"
    );
    assert!(
        output.contains("3 errors"),
        "output should include error count: {output}"
    );
    assert!(
        output.contains("graph recompute: centrality pass degraded"),
        "output should preserve detail: {output}"
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

/// Regression: the default prosoche self-audit runner must include the
/// implemented instinct-pattern check without emitting fixed stub findings.
#[tokio::test]
async fn self_audit_default_instinct_check_is_not_stub() {
    use crate::prosoche_audit::{
        BehaviorPatternSnapshot, ProsocheAuditRunner, ProsocheState, SessionSnapshot,
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let runner = ProsocheAuditRunner::default_checks(tmp.path());

    let mut state = ProsocheState {
        nous_id: "alice".to_owned(),
        checked_at: "2026-06-12T00:00:00Z".to_owned(),
        ..ProsocheState::default()
    };
    state.sessions.push(SessionSnapshot {
        session_id: "session-instinct".to_owned(),
        turn_count: 8,
        error_count: 4,
        completed: false,
        turn_text: "synthetic runtime session".to_owned(),
    });
    state.behavior_patterns.push(BehaviorPatternSnapshot {
        session_id: "session-instinct".to_owned(),
        tool_call_count: 6,
        tool_error_count: 4,
        repeated_action_count: 2,
        no_progress_turns: 2,
        avoidance_markers: 0,
        confidence_claims: 0,
    });

    let report = runner.run_audit(&state).await;

    assert!(
        report
            .findings
            .iter()
            .any(|f| f.source == "prosoche::InstinctPatternsCheck"),
        "default runner must include real instinct-pattern findings; got: {:?}",
        report.findings
    );
    assert!(
        report.findings.iter().all(|finding| finding
            .stats
            .support
            .as_ref()
            .is_none_or(|support| !support.is_stub)),
        "default runner must not emit stub findings; got: {:?}",
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
