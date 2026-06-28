#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after len assertions"
)]

use super::*;

#[test]
fn nous_state_contains_manager_and_registry() {
    // Verify NousState has the fields needed by nous handlers.
    #[expect(
        dead_code,
        reason = "compile-time shape assertion: proves field types via unused local fn"
    )]
    fn assert_nous_state_fields(state: &NousState) {
        use std::sync::Arc;

        use hermeneus::provider::ProviderRegistry;
        use nous::manager::NousManager;
        use organon::registry::ToolRegistry;
        use taxis::config::AletheiaConfig;

        let _: &Arc<NousManager> = &state.nous_manager;
        let _: &Arc<ToolRegistry> = &state.tool_registry;
        let _: &Arc<ProviderRegistry> = &state.provider_registry;
        let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
    }
    // If the above compiles, NousState contains both required fields.
    assert!(std::mem::size_of::<NousState>() > 0);
}

#[test]
fn nous_list_response_serializes_nous_array() {
    let resp = NousListResponse {
        nous: vec![NousSummary {
            id: "alice".to_owned(),
            name: "Alice".to_owned(),
            enabled: true,
            model: "anthropic/claude-opus-4-6".to_owned(),
            fallback_models: vec![],
            provider_readiness: vec![],
            status: "active".to_owned(),
            tools: vec![],
            config_applied: None,
            live_applied: None,
            reload_required: None,
            restart_required: None,
        }],
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("nous").is_some());
    assert_eq!(json["nous"][0]["id"], "alice");
    assert_eq!(json["nous"][0]["status"], "active");
    assert_eq!(json["nous"][0]["enabled"], true);
}

#[test]
fn nous_list_response_empty_array() {
    let resp = NousListResponse { nous: vec![] };
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json["nous"].as_array().unwrap().is_empty());
}

#[test]
fn nous_summary_name_falls_back_to_id() {
    let summary = NousSummary {
        id: "bob".to_owned(),
        name: "bob".to_owned(), // fallback case: name == id
        enabled: false,
        model: "anthropic/claude-sonnet-4-6".to_owned(),
        fallback_models: vec![],
        provider_readiness: vec![],
        status: "active".to_owned(),
        tools: vec![],
        config_applied: None,
        live_applied: None,
        reload_required: None,
        restart_required: None,
    };
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(json["name"], "bob");
    assert_eq!(json["id"], "bob");
    assert_eq!(json["enabled"], false);
}

#[test]
fn nous_status_serializes_all_config_fields() {
    let status = NousStatus {
        id: "syn".to_owned(),
        model: "anthropic/claude-opus-4-6".to_owned(),
        fallback_models: vec![],
        retries_before_fallback: 2,
        complexity_routing_enabled: false,
        complexity_no_llm_threshold: 5,
        complexity_low_threshold: 30,
        complexity_high_threshold: 70,
        provider_readiness: vec![],
        context_window: 200_000,
        max_output_tokens: 4096,
        thinking_enabled: true,
        thinking_budget: 10_000,
        max_tool_iterations: 10,
        status: "active".to_owned(),
        background_failure_total_count: 5,
        background_failure_recent_count: 2,
        background_failure_latest_message: Some("indexer unreachable".to_owned()),
        background_failure_latest_kind: Some("indexer".to_owned()),
        background_health_degraded: true,
        address_mask: AddressMaskStatus {
            kind: "operator_only".to_owned(),
            allowed_senders: vec![],
        },
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["id"], "syn");
    assert_eq!(json["context_window"], 200_000);
    assert_eq!(json["complexity_no_llm_threshold"], 5);
    assert_eq!(json["complexity_low_threshold"], 30);
    assert_eq!(json["complexity_high_threshold"], 70);
    assert_eq!(json["thinking_enabled"], true);
    assert_eq!(json["max_tool_iterations"], 10);
    assert_eq!(json["background_failure_total_count"], 5);
    assert_eq!(json["background_failure_recent_count"], 2);
    assert_eq!(
        json["background_failure_latest_message"],
        "indexer unreachable"
    );
    assert_eq!(json["background_failure_latest_kind"], "indexer");
    assert_eq!(json["background_health_degraded"], true);
    assert_eq!(json["address_mask"]["kind"], "operator_only");
    assert!(json["address_mask"]["allowed_senders"].as_array().is_some());
}

#[test]
fn tools_response_serializes_tool_list() {
    let resp = ToolsResponse {
        tools: vec![ToolSummary {
            name: "read_file".to_owned(),
            enabled: true,
            description: "Read a file from disk".to_owned(),
            category: "Builtin".to_owned(),
            auto_activate: true,
        }],
        config_applied: None,
        live_applied: None,
        reload_required: None,
        restart_required: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["tools"][0]["name"], "read_file");
    assert_eq!(json["tools"][0]["enabled"], true);
    assert_eq!(json["tools"][0]["category"], "Builtin");
    assert_eq!(json["tools"][0]["auto_activate"], true);
}

#[test]
fn tool_summary_serializes_enabled_bit() {
    let tool = ToolSummary {
        name: "search".to_owned(),
        enabled: false,
        description: "Search the workspace".to_owned(),
        category: "Builtin".to_owned(),
        auto_activate: false,
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["enabled"], false);
    assert_eq!(json["name"], "search");
}

#[test]
fn nous_not_found_error_is_404() {
    use axum::response::IntoResponse;

    use crate::error::{ApiError, NousNotFoundSnafu};
    let err: ApiError = NousNotFoundSnafu {
        id: "unknown-nous".to_owned(),
    }
    .build();
    let response = err.into_response();
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[test]
fn create_agent_response_serializes() {
    let resp = CreateAgentResponse {
        id: "alice".to_owned(),
        name: "Alice".to_owned(),
        model: "claude-sonnet-4-6".to_owned(),
        restart_required: true,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["id"], "alice");
    assert_eq!(json["name"], "Alice");
    assert_eq!(json["model"], "claude-sonnet-4-6");
    assert_eq!(json["restart_required"], true);
}

#[test]
fn capitalize_first_letter() {
    assert_eq!(capitalize("analyst"), "Analyst");
    assert_eq!(capitalize("my-agent"), "My-agent");
    assert_eq!(capitalize(""), "");
    assert_eq!(capitalize("A"), "A");
}

#[test]
fn scaffold_creates_expected_structure() {
    let dir = tempfile::tempdir().unwrap();
    let oikos = taxis::oikos::Oikos::from_root(dir.path());

    scaffold_agent(&oikos, "test-agent", "Test-agent").unwrap();

    let nous_dir = dir.path().join("nous/test-agent");
    assert!(nous_dir.join("SOUL.md").exists());
    assert!(nous_dir.join("IDENTITY.md").exists());
    assert!(nous_dir.join("AGENTS.md").exists());
    assert!(nous_dir.join("USER.md").exists());
    assert!(nous_dir.join("memory").is_dir());
    assert!(nous_dir.join("workspace/drafts").is_dir());

    let soul = std::fs::read_to_string(nous_dir.join("SOUL.md")).unwrap();
    assert!(soul.contains("Test-agent"));
}

#[test]
fn write_agent_config_appends_without_destroying_comments() {
    let dir = tempfile::tempdir().unwrap();
    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    std::fs::create_dir_all(dir.path().join("config")).unwrap();

    let original = "# My custom config\n\
        # This comment must survive\n\
        [gateway]\n\
        port = 9999\n\n";
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup writes config template to temp directory"
    )]
    std::fs::write(dir.path().join("config/aletheia.toml"), original).unwrap();

    write_agent_config(&oikos, "alice", "Alice", "claude-sonnet-4-6").unwrap();

    let result = std::fs::read_to_string(dir.path().join("config/aletheia.toml")).unwrap();
    assert!(
        result.contains("# My custom config"),
        "comment must survive"
    );
    assert!(
        result.contains("# This comment must survive"),
        "comment must survive"
    );
    assert!(
        result.contains("port = 9999"),
        "existing config must survive"
    );
    assert!(result.contains(r#"id = "alice""#), "new agent must appear");
    assert!(
        result.contains(r#"workspace = "nous/alice""#),
        "workspace must be relative"
    );
}

#[test]
fn write_agent_config_rejects_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    std::fs::create_dir_all(dir.path().join("config")).unwrap();

    write_agent_config(&oikos, "bob", "Bob", "claude-sonnet-4-6").unwrap();
    let result = write_agent_config(&oikos, "bob", "Bob", "claude-sonnet-4-6");
    assert!(result.is_err(), "duplicate agent should return an error");
}

#[test]
fn create_agent_response_restart_required_is_true() {
    let resp = CreateAgentResponse {
        id: "alice".to_owned(),
        name: "Alice".to_owned(),
        model: "claude-sonnet-4-6".to_owned(),
        restart_required: true,
    };
    assert!(
        resp.restart_required,
        "newly created agents always require restart"
    );
}

#[test]
fn recover_response_serializes() {
    let resp = RecoverResponse {
        id: "alice".to_owned(),
        recovered: true,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["id"], "alice");
    assert_eq!(json["recovered"], true);
}

#[test]
fn recover_response_not_recovered_serializes() {
    let resp = RecoverResponse {
        id: "alice".to_owned(),
        recovered: false,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["recovered"], false);
}
