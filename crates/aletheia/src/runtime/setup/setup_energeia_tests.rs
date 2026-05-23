use std::collections::HashSet;
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

#[tokio::test]
async fn energeia_feature_registers_service_backed_runtime_tools() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let oikos = Oikos::from_root(tmp.path());
    let config = AletheiaConfig::default();
    let registry = build_tool_registry(&config, &oikos, &CancellationToken::new())
        .expect("tool registry with energeia services");
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
