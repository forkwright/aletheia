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
    let mut fact =
        eidos::test_fixtures::make_fact("fact-runtime-prosoche-001", "test-nous", "test content");
    fact.fact_type = "observation".to_owned();
    fact.temporal = episteme::knowledge::FactTemporal {
        valid_from: now,
        valid_to: episteme::knowledge::far_future(),
        recorded_at: now,
    };
    fact.provenance.tier = episteme::knowledge::EpistemicTier::Verified;
    fact
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

/// #6419: lesson extraction reads after-action JSONL (the real,
/// instance-populated telemetry energeia writes after every dispatch), not
/// the retired workflow/training/ phronesis schema. This is the regression
/// guard — reverting `execute_lesson_extraction_from_dir` back to reading a
/// path that cannot exist would make this test fail to find any facts.
#[tokio::test]
async fn lesson_extraction_persists_after_action_facts_to_real_fjall() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().join("after-actions");
    std::fs::create_dir_all(&log_dir).expect("create after-action log dir");
    let (executor, store) = RealKnowledge::open(dir.path());

    let records = [
        serde_json::json!({
            "dispatch_id": "d1",
            "session_outcomes": [
                {"status": "failed", "model": "sonnet", "failure_class": "prompt-quality", "category": "refactor"},
                {"status": "success", "model": "sonnet"},
            ],
            "qa_verdict": "partial",
            "prompt_hash": "hash-1",
        }),
        serde_json::json!({
            "dispatch_id": "d2",
            "session_outcomes": [{"status": "success", "model": "sonnet"}],
            "qa_verdict": "pass",
            "prompt_hash": "hash-2",
        }),
    ];
    let lines = records
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(log_dir.join("2026-08-04.jsonl"), lines)
        .await
        .expect("write after-action fixture");

    let result = execute_lesson_extraction_from_dir("alice", &log_dir, executor.as_ref())
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
    assert_eq!(
        facts.len(),
        2,
        "one verdict fact + one failure-class fact are retrievable"
    );
    assert!(
        facts
            .iter()
            .all(|fact| fact.provenance.source_session_id.as_deref()
                == Some("daemon:lesson-extraction")),
        "lesson facts should carry daemon provenance: {facts:?}"
    );
    assert!(
        facts.iter().any(|fact| fact.content.contains("partial")),
        "the non-passing verdict should be persisted: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.content.contains("prompt-quality")),
        "the session failure class should be persisted: {facts:?}"
    );
    // d2 passed cleanly with no session failures — it must not contribute
    // any fact of its own (no manufactured signal from a clean dispatch).
    assert!(
        facts.iter().all(|fact| !fact.content.contains("hash-2")),
        "a clean dispatch must not be persisted as a lesson: {facts:?}"
    );
}

/// A dispatch that passed cleanly with no session failures is not signal —
/// persisting it would be exactly the kind of manufactured-fact noise the
/// old phronesis-era #5384 fix guarded against (don't claim more than the
/// evidence supports). Here that means: no facts at all.
#[tokio::test]
async fn clean_dispatch_produces_no_lesson_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().join("after-actions");
    std::fs::create_dir_all(&log_dir).expect("create after-action log dir");
    let (executor, store) = RealKnowledge::open(dir.path());

    let record = serde_json::json!({
        "dispatch_id": "d3",
        "session_outcomes": [{"status": "success", "model": "sonnet"}],
        "qa_verdict": "pass",
        "prompt_hash": "hash-3",
    });
    tokio::fs::write(log_dir.join("2026-08-04.jsonl"), record.to_string())
        .await
        .expect("write after-action fixture");

    let result = execute_lesson_extraction_from_dir("alice", &log_dir, executor.as_ref())
        .expect("lesson extraction should persist");
    assert!(result.is_success());

    let facts = current_facts(&store, "alice");
    assert!(
        facts.is_empty(),
        "a clean, fully-passing dispatch must not manufacture a lesson fact: {facts:?}"
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
        session_age_days: Some(0.0),
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
