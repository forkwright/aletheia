//! Integration test for all 9 energeia tools.
//!
//! Creates an in-memory `EnergeiaStore` + `MockEngine`, registers all 9 tools with
//! real `EnergeiaServices`, calls each with valid input, and verifies that each
//! returns a non-error `ToolResult`.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "integration test assertions"
)]

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use energeia::cron::CronLockStore;
use energeia::engine::{SessionEvent, SessionResult};
use energeia::http::mock::{MockEngine, MockOutcome};
use energeia::orchestrator::{Orchestrator, OrchestratorConfig};
use energeia::qa::{PromptSpec as QaPromptSpec, QaGate};
use energeia::store::EnergeiaStore;
use energeia::types::{MechanicalIssue, QaResult, QaVerdict};
use koina::id::{NousId, SessionId, ToolName};
use tempfile::TempDir;

use organon::builtins::energeia::{EnergeiaServices, register};
use organon::registry::ToolRegistry;
use organon::types::{ToolContext, ToolDef, ToolInput, ToolResult};

// ── Mock QA gate ─────────────────────────────────────────────────────────────

struct AlwaysPassQaGate;

impl QaGate for AlwaysPassQaGate {
    fn evaluate<'a>(
        &'a self,
        prompt: &'a QaPromptSpec,
        pr_number: u64,
        _diff: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = energeia::error::Result<QaResult>> + Send + 'a>>
    {
        Box::pin(async move {
            Ok(QaResult::new(
                prompt.prompt_number,
                pr_number,
                QaVerdict::Pass,
                vec![],
                vec![],
                vec![],
                0.0,
                jiff::Timestamp::now(),
                false,
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
    let cron_db_path = tmp.path().join("cron");
    let cron_db = koina::fjall::FjallDb::open(&cron_db_path, &["cron_locks"]).unwrap();
    let cron_lock_store = Arc::new(CronLockStore::open(Arc::new(cron_db.db)).unwrap());
    cron_lock_store
        .record_fire_started("nightly", jiff::Timestamp::now())
        .unwrap();

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

    let services = Arc::new(
        EnergeiaServices::new(orchestrator, store)
            .with_cron_lock_store(cron_lock_store, vec!["nightly".to_owned()]),
    );

    (tmp, services)
}

fn make_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test").unwrap(),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: std::path::PathBuf::from("/tmp"),
        allowed_roots: vec![std::path::PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
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

fn definition_for(name: &str) -> ToolDef {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");
    registry
        .definitions()
        .into_iter()
        .find(|def| def.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name} definition registered"))
        .clone()
}

fn result_json(result: &ToolResult) -> serde_json::Value {
    serde_json::from_str(&result.content.text_summary()).expect("tool result is JSON text")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn all_nine_tools_return_non_error() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();

    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(services.as_ref()))
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

    // ── 3. dokimasia — run_qa with caller-provided diff ─────────────────────
    let input = make_input(
        "dokimasia",
        serde_json::json!({
            "prompt_number": 1,
            "pr_number": 42,
            "project": "acme/test",
            "diff": "diff --git a/src/lib.rs b/src/lib.rs\n+fn added() {}\n"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "dokimasia");

    // Save the QA result JSON so diorthosis can consume it.
    let qa_result_json = match &result.content {
        organon::types::ToolResultContent::Text(t) => t.clone(),
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

    let input = make_input(
        "metron",
        serde_json::json!({
            "report_type": "status"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "metron (status)");
    let json = result_json(&result);
    assert_eq!(
        json.pointer("/stale_running_dispatches"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        json.pointer("/cron/stale_fire_count"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        json.pointer("/cron/task_fires/0/task_name"),
        Some(&serde_json::json!("nightly"))
    );
    assert!(
        json.pointer("/cron/task_fires/0/last_fire_record/succeeded")
            .is_some(),
        "last fire record must expose success/failure state"
    );

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
            "max_parallel": 2,
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

#[tokio::test]
async fn dokimasia_empty_diff_returns_no_work() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();
    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(services.as_ref())).expect("register");

    let input = make_input(
        "dokimasia",
        serde_json::json!({
            "prompt_number": 1,
            "pr_number": 42,
            "project": "acme/test",
            "diff": ""
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "dokimasia no-work");
    let text = result.content.text_summary();
    assert!(
        text.contains("\"status\": \"no_work\""),
        "empty diff must not produce vacuous Pass: {text}"
    );
}

#[test]
fn dromeus_schema_exposes_parallel_and_turn_limits() {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");
    let definitions = registry.definitions();
    let dromeus = definitions
        .iter()
        .find(|def| def.name.as_str() == "dromeus")
        .expect("dromeus definition registered");

    assert!(
        dromeus.input_schema.properties.contains_key("max_parallel"),
        "dromeus should expose concurrency separately"
    );
    assert!(
        dromeus.input_schema.properties.contains_key("max_turns"),
        "dromeus should expose per-session turn budget separately"
    );
    assert!(
        dromeus.input_schema.properties.contains_key("budget_usd"),
        "dromeus should expose dispatch cost budget"
    );
}

#[tokio::test]
async fn dromeus_dry_run_threads_budget_usd() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();
    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(services.as_ref())).expect("register");

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
            "budget_usd": 1.25,
            "dry_run": true
        }),
    );

    let result = registry.execute(&input, &ctx).await.unwrap();

    assert_non_error(&result, "dromeus budget dry_run");
    let json = result_json(&result);
    assert_eq!(json["budget_usd"], serde_json::json!(1.25));
}

#[tokio::test]
async fn dromeus_rejects_non_positive_budget_usd() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();
    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(services.as_ref())).expect("register");

    let input = make_input(
        "dromeus",
        serde_json::json!({
            "spec": "[]",
            "project": "acme/test",
            "budget_usd": 0
        }),
    );

    let result = registry.execute(&input, &ctx).await.unwrap();

    assert!(result.is_error, "zero budget must be rejected");
    assert!(
        result
            .content
            .text_summary()
            .contains("budget_usd' must be greater than 0"),
        "unexpected error: {:?}",
        result.content
    );
}

#[test]
fn dokimasia_schema_does_not_require_reserved_project() {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");
    let definitions = registry.definitions();
    let dokimasia = definitions
        .iter()
        .find(|def| def.name.as_str() == "dokimasia")
        .expect("dokimasia definition registered");

    assert!(
        dokimasia.input_schema.properties.contains_key("project"),
        "dokimasia should still document the reserved project field"
    );
    assert!(
        !dokimasia
            .input_schema
            .required
            .iter()
            .any(|field| field == "project"),
        "dokimasia should not require a reserved field it cannot persist"
    );
}

#[test]
fn dokimasia_description_advertises_mechanical_only_qa() {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");
    let definitions = registry.definitions();
    let dokimasia = definitions
        .iter()
        .find(|def| def.name.as_str() == "dokimasia")
        .expect("dokimasia definition registered");

    assert!(
        dokimasia.description.contains("mechanical QA checks"),
        "dokimasia should not advertise semantic QA without prompt and LLM wiring"
    );
    assert!(
        dokimasia
            .description
            .contains("caller-provided pull-request diff"),
        "dokimasia should document that callers must supply the diff"
    );
    assert!(
        dokimasia
            .description
            .contains("Semantic acceptance-criteria evaluation requires"),
        "dokimasia should document the semantic QA limitation"
    );
    assert!(
        dokimasia.description.contains("empty diffs return no-work"),
        "dokimasia should document its empty-diff behavior"
    );
}

#[test]
fn placeholder_tool_descriptions_match_current_side_effect_limits() {
    let prographe = definition_for("prographe");
    assert!(
        prographe.description.contains("prompt spec template"),
        "prographe should advertise template rendering, got: {}",
        prographe.description
    );
    assert!(
        prographe
            .description
            .contains("does not allocate queue numbers")
            && prographe.description.contains("write files"),
        "prographe must not claim queue allocation or file writes"
    );
    assert!(
        prographe
            .input_schema
            .properties
            .get("project")
            .expect("project property documented")
            .description
            .contains("no project files are read or written"),
        "prographe project field should document that it is not a file root"
    );

    let schedion = definition_for("schedion");
    assert!(
        schedion.description.contains("empty prompt dependency DAG"),
        "schedion should document its empty-DAG behavior"
    );
    assert!(
        schedion.description.contains("does not load prompt specs"),
        "schedion must not advertise file-backed DAG loading"
    );

    let epitropos = definition_for("epitropos");
    assert!(
        epitropos
            .description
            .contains("placeholder CI steward classification pass"),
        "epitropos should advertise placeholder single-pass classification"
    );
    assert!(
        epitropos.description.contains("does not poll")
            && epitropos.description.contains("merge PRs")
            && epitropos.description.contains("queue repair work"),
        "epitropos must not advertise polling, merge, or repair side effects"
    );
    assert!(
        epitropos
            .input_schema
            .properties
            .get("once")
            .expect("once property documented")
            .description
            .contains("always runs one classification pass"),
        "epitropos once field should match run_once semantics"
    );
}

#[tokio::test]
async fn placeholder_tool_outputs_report_current_side_effect_limits() {
    let ctx = make_ctx();
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");

    let input = make_input(
        "prographe",
        serde_json::json!({
            "project": "acme/test",
            "description": "Add health endpoint"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "prographe");
    let output = result_json(&result);
    assert_eq!(output["prompt_number_assigned"], false);
    assert_eq!(output["files_written"], serde_json::json!([]));
    assert!(
        output["spec"]
            .as_str()
            .expect("spec is text")
            .contains("number: 0"),
        "prographe should return an unallocated template spec"
    );

    let input = make_input("schedion", serde_json::json!({"project": "acme/test"}));
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "schedion");
    let output = result_json(&result);
    assert_eq!(output["node_count"], 0);
    assert_eq!(output["loaded_prompt_specs"], false);
    assert!(
        output["note"]
            .as_str()
            .expect("note is text")
            .contains("No prompt spec files found"),
        "schedion should explain the empty DAG"
    );

    let input = make_input(
        "epitropos",
        serde_json::json!({
            "project": "acme/test",
            "once": false,
            "dry_run": false
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "epitropos");
    let output = result_json(&result);
    assert_eq!(output["mode"], "single_placeholder_pass");
    assert_eq!(output["polling_loop_started"], false);
    assert_eq!(output["merge_side_effects_enabled"], false);
    assert_eq!(output["repair_queue_side_effects_enabled"], false);
    assert_eq!(output["classified_count"], 0);
}

#[test]
fn parateresis_schema_advertises_store_query_only() {
    let mut registry = ToolRegistry::new();
    register(&mut registry, None).expect("register");
    let definitions = registry.definitions();
    let parateresis = definitions
        .iter()
        .find(|def| def.name.as_str() == "parateresis")
        .expect("parateresis definition registered");

    assert!(
        parateresis
            .description
            .contains("return stored observations"),
        "parateresis should describe local observation-store behavior"
    );
    assert!(
        !parateresis.description.contains("pull requests")
            && !parateresis.description.contains("tracking issues"),
        "parateresis must not advertise external PR or issue creation work"
    );
    let days_description = &parateresis
        .input_schema
        .properties
        .get("days")
        .expect("days property documented")
        .description;
    assert!(
        days_description.contains("stored observations"),
        "days should describe the store-query window, got: {days_description}"
    );
}

#[tokio::test]
async fn dokimasia_runs_without_project() {
    let (_tmp, services) = setup();
    let ctx = make_ctx();
    let mut registry = ToolRegistry::new();
    register(&mut registry, Some(services.as_ref())).expect("register");

    let input = make_input(
        "dokimasia",
        serde_json::json!({
            "prompt_number": 1,
            "pr_number": 42,
            "diff": "diff --git a/src/lib.rs b/src/lib.rs\n+fn added() {}\n"
        }),
    );
    let result = registry.execute(&input, &ctx).await.unwrap();
    assert_non_error(&result, "dokimasia without project");
}
