use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use koina::id::{NousId, SessionId};
use organon::registry::ToolRegistry;
use tokio_util::sync::CancellationToken;

use super::*;

fn tool_context(root: &std::path::Path) -> organon::types::ToolContext {
    organon::types::ToolContext {
        nous_id: NousId::new("test").expect("valid nous id"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: root.to_path_buf(),
        allowed_roots: vec![root.to_path_buf()],
        services: None,
        active_tools: Arc::new(std::sync::RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

async fn assert_tool_not_missing_services(
    registry: &ToolRegistry,
    ctx: &organon::types::ToolContext,
    name: &'static str,
    arguments: serde_json::Value,
) {
    let input = organon::types::ToolInput {
        name: koina::id::ToolName::from_static(name),
        tool_use_id: format!("toolu_{name}"),
        arguments,
    };
    let result = registry.execute(&input, ctx).await.expect("tool executes");
    let text = result.content.text_summary();
    assert!(
        !text.contains("missing EnergeiaServices"),
        "{name} should use runtime services, got: {text}"
    );
    assert!(!result.is_error, "{name} returned error: {text}");
}

fn seed_dispatch_record(store_path: &Path, record: &energeia::store::records::DispatchRecord) {
    let db = fjall::Database::builder(store_path)
        .open()
        .expect("open store");
    let keyspace = db
        .keyspace("energeia", fjall::KeyspaceCreateOptions::default)
        .expect("open energeia partition");
    let value = rmp_serde::to_vec(record).expect("serialize dispatch record");
    let key = format!("dispatch:{}", record.id.as_str());
    keyspace
        .insert(key.as_bytes(), value)
        .expect("insert dispatch record");
    db.persist(fjall::PersistMode::SyncAll)
        .expect("persist dispatch record");
}

fn read_dispatch_record(
    store_path: &Path,
    id: &energeia::store::records::DispatchId,
) -> energeia::store::records::DispatchRecord {
    let db = fjall::Database::builder(store_path)
        .open()
        .expect("open store");
    let keyspace = db
        .keyspace("energeia", fjall::KeyspaceCreateOptions::default)
        .expect("open energeia partition");
    let key = format!("dispatch:{}", id.as_str());
    let value = keyspace
        .get(key.as_bytes())
        .expect("read dispatch record")
        .expect("dispatch record exists");
    rmp_serde::from_slice(&value).expect("deserialize dispatch record")
}

fn stale_running_dispatch_record() -> energeia::store::records::DispatchRecord {
    let id = energeia::store::records::DispatchId::new(koina::ulid::Ulid::new().to_string());
    let created_at = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(2))
        .expect("valid stale timestamp");
    let spec = energeia::types::DispatchSpec::new("acme".to_owned(), vec![1]);
    energeia::store::records::DispatchRecord {
        id,
        project: "acme".to_owned(),
        spec: serde_json::to_string(&spec).expect("serialize dispatch spec"),
        status: energeia::store::records::DispatchStatus::Running,
        created_at,
        finished_at: None,
        total_cost_usd: 0.0,
        total_sessions: 0,
    }
}

#[test]
fn build_tool_registry_reconciles_stale_running_dispatches_on_startup() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let oikos = Oikos::from_root(tmp.path());
    std::fs::create_dir_all(oikos.data()).expect("create data dir");
    let store_path = oikos.data().join("energeia.fjall");
    let record = stale_running_dispatch_record();
    let dispatch_id = record.id.clone();
    seed_dispatch_record(&store_path, &record);

    let config = AletheiaConfig::default();
    let built = build_tool_registry(&config, &oikos, &CancellationToken::new(), None)
        .expect("tool registry with energeia services");
    assert!(
        built.energeia_services.is_some(),
        "energeia services should be available"
    );
    drop(built);

    let reconciled = read_dispatch_record(&store_path, &dispatch_id);
    assert_eq!(
        reconciled.status,
        energeia::store::records::DispatchStatus::Failed
    );
    assert!(
        reconciled.finished_at.is_some(),
        "startup reconciliation should stamp finished_at"
    );
}

#[tokio::test]
async fn energeia_feature_registers_service_backed_runtime_tools() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let oikos = Oikos::from_root(tmp.path());
    let config = AletheiaConfig::default();
    let built = build_tool_registry(&config, &oikos, &CancellationToken::new(), None)
        .expect("tool registry with energeia services");
    let registry = built.registry;
    let names: HashSet<String> = registry
        .definitions()
        .iter()
        .map(|def| def.name.as_str().to_owned())
        .collect();
    for name in ["dromeus", "parateresis", "mathesis", "metron"] {
        assert!(names.contains(name), "{name} should be registered");
    }

    let ctx = tool_context(tmp.path());
    assert_tool_not_missing_services(
        &registry,
        &ctx,
        "mathesis",
        serde_json::json!({
            "action": "record",
            "source": "dispatch",
            "category": "runtime",
            "project": "forkwright/aletheia",
            "lesson": "runtime registry injected energeia services"
        }),
    )
    .await;
    assert_tool_not_missing_services(
        &registry,
        &ctx,
        "mathesis",
        serde_json::json!({
            "action": "list",
            "project": "forkwright/aletheia"
        }),
    )
    .await;
    assert_tool_not_missing_services(
        &registry,
        &ctx,
        "parateresis",
        serde_json::json!({
            "project": "forkwright/aletheia",
            "days": 7
        }),
    )
    .await;
    assert_tool_not_missing_services(
        &registry,
        &ctx,
        "metron",
        serde_json::json!({
            "report_type": "health",
            "days": 30
        }),
    )
    .await;

    let spec = serde_json::json!([
        {
            "number": 1,
            "description": "verify service-backed dry run",
            "depends_on": [],
            "acceptance_criteria": [],
            "blast_radius": [],
            "body": "No dispatch should be spawned during dry_run"
        }
    ]);
    assert_tool_not_missing_services(
        &registry,
        &ctx,
        "dromeus",
        serde_json::json!({
            "spec": spec.to_string(),
            "project": "forkwright/aletheia",
            "dry_run": true
        }),
    )
    .await;
}
