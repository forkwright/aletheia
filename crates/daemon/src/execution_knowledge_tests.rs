#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
#[cfg(feature = "knowledge-store")]
use std::time::Duration;

#[cfg(feature = "knowledge-store")]
use tokio_util::sync::CancellationToken;

use crate::maintenance::MaintenanceReport;

use super::*;

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
            timeout: Duration::from_mins(5),
        },
    )
    .await
    .expect("prosoche should run");

    assert!(result.is_success());
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

    assert!(result.is_success());
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

    assert!(result.is_success());
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
        session_age_days: Some(0),
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
