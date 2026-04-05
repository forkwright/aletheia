//! Integration test for all 9 energeia tools.
//!
//! Creates an in-memory EnergeiaStore + MockEngine, registers all 9 tools with
//! real EnergeiaServices, calls each with valid input, and verifies that each
//! returns a non-error ToolResult.

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use aletheia_energeia::engine::{SessionEvent, SessionResult};
use aletheia_energeia::http::mock::{MockEngine, MockOutcome};
use aletheia_energeia::orchestrator::{Orchestrator, OrchestratorConfig};
use aletheia_energeia::qa::{PromptSpec as QaPromptSpec, QaGate};
use aletheia_energeia::store::EnergeiaStore;
use aletheia_energeia::types::{MechanicalIssue, QaResult, QaVerdict};
use aletheia_koina::id::{NousId, SessionId, ToolName};
use tempfile::TempDir;

use aletheia_organon::builtins::energeia::{EnergeiaServices, register};
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolInput, ToolResult};

// ── Mock QA gate ─────────────────────────────────────────────────────────────

struct AlwaysPassQaGate;

impl QaGate for AlwaysPassQaGate {
    fn evaluate<'a>(
        &'a self,
        prompt: &'a QaPromptSpec,
        pr_number: u64,
        _diff: &'a str,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = aletheia_energeia::error::Result<QaResult>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(QaResult::new(
                prompt.prompt_number,
                pr_number,
                QaVerdict::Pass,
                vec![],
                vec![],
                0.0,
                jiff::Timestamp::now(),
            ))
        })
    }

    fn mechanical_check(&self, _diff: &str, _prompt: &QaPromptSpec) -> Vec<MechanicalIssue> {
        vec![]
    }
}

// ── Test fixtures ─────────────────────────────────────────────────────────────

fn setup() -> (TempDir, Arc<EnergeiaServices>) {
    let tmp = TempDir::new().unwrap();
    let db = fjall::Database::builder(tmp.path()).open().unwrap();
    let store = Arc::new(EnergeiaStore::new(&db).unwrap());

    // One success outcome for any dispatch.
    let engine = Arc::new(MockEngine::new(vec![MockOutcome::Success {
        events: vec![SessionEvent::TurnComplete { turn: 5 }],
        result: SessionResult::new(
            "test-sess".to_owned(),
            0.10,
            5,
            500,
            true,
            Some("done".to_owned()),
        ),
    }]));

    let qa: Arc<dyn QaGate> = Arc::new(AlwaysPassQaGate);
    let config = OrchestratorConfig::new().max_concurrent(2);
    let orchestrator =
        Arc::new(Orchestrator::new(engine, qa, config).with_store(Arc::clone(&store)));

    let services = Arc::new(EnergeiaServices {
        orchestrator,
        store,
    });

    (tmp, services)
}

fn make_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test").unwrap(),
        session_id: SessionId::new(),
        workspace: std::path::PathBuf::from("/tmp"),
        allowed_roots: vec![std::path::PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn make_input(name: &'static str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::from_static(name),
        tool_use_id: format!("toolu_{name}"),
        arguments: args,
    }
}

fn assert_non_error(result: &ToolResult, tool: &str) {
    assert!(
        !result.is_error,
        "{tool}: expected non-error result, got error: {:?}",
        result.content
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn all_nine_tools_return_non_error() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();

    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(Arc::clone(&services)))
        .expect("all 9 tools register without collision");

    assert_eq!(
        registry.definitions().len(),
        9,
        "expected exactly 9 tools registered"
    );

    // ── 1. schedion — pure DAG computation, no services needed ───────────────
    let input = make_input("schedion", serde_json::json!({"project": "acme/test"}));
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "schedion");

    // ── 2. prographe — template rendering, no services needed ────────────────
    let input = make_input(
        "prographe",
        serde_json::json!({
            "project": "acme/test",
            "description": "Add health endpoint",
            "criteria": ["GET /health returns 200", "includes version"]
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "prographe");

    // ── 3. dokimasia — run_qa with empty diff, no services needed ────────────
    let input = make_input(
        "dokimasia",
        serde_json::json!({
            "prompt_number": 1,
            "pr_number": 42,
            "project": "acme/test"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "dokimasia");

    // Save the QA result JSON so diorthosis can consume it.
    let qa_result_json = match &result.content {
        aletheia_organon::types::ToolResultContent::Text(t) => t.clone(),
        _ => panic!("dokimasia returned non-text content"),
    };

    // ── 4. diorthosis — generate_corrective from inline QA result ────────────
    // The QA result from dokimasia has verdict=Pass and no failed criteria,
    // so diorthosis should report "no corrective needed" (also a non-error result).
    let input = make_input(
        "diorthosis",
        serde_json::json!({
            "qa_result_id": qa_result_json,
            "original_prompt_number": 1
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "diorthosis");

    // ── 5. epitropos — steward single-pass classification ────────────────────
    let input = make_input(
        "epitropos",
        serde_json::json!({
            "project": "acme/test",
            "once": true,
            "dry_run": true
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "epitropos");

    // ── 6. mathesis record — add_lesson ──────────────────────────────────────
    let input = make_input(
        "mathesis",
        serde_json::json!({
            "action": "record",
            "source": "qa",
            "category": "testing",
            "project": "acme/test",
            "lesson": "Always run clippy before pushing"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "mathesis (record)");

    // ── 7. mathesis list — query_lessons ─────────────────────────────────────
    let input = make_input(
        "mathesis",
        serde_json::json!({
            "action": "list",
            "source": "qa",
            "project": "acme/test"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "mathesis (list)");

    // ── 8. parateresis — observation pipeline ────────────────────────────────
    let input = make_input(
        "parateresis",
        serde_json::json!({
            "project": "acme/test",
            "days": 7
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "parateresis");

    // ── 9. metron health — health_report ─────────────────────────────────────
    let input = make_input(
        "metron",
        serde_json::json!({
            "report_type": "health",
            "days": 30
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "metron (health)");

    // ── 9b. metron cost — cost_report ────────────────────────────────────────
    let input = make_input(
        "metron",
        serde_json::json!({
            "report_type": "cost",
            "days": 7
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "metron (cost)");

    // ── 10. dromeus dry_run — Orchestrator::dry_run ──────────────────────────
    let spec = serde_json::json!([
        {
            "number": 1,
            "description": "implement task 1",
            "depends_on": [],
            "acceptance_criteria": [],
            "blast_radius": [],
            "body": "Do the task"
        }
    ]);
    let input = make_input(
        "dromeus",
        serde_json::json!({
            "spec": spec.to_string(),
            "project": "acme/test",
            "dry_run": true
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "dromeus (dry_run)");
}

#[test]
fn register_without_services_does_not_panic() {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register without services should not error");
    assert_eq!(registry.definitions().len(), 9);
}
