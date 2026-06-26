#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::http::StatusCode;
use koina::id::ToolName;
use organon::error::Result;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::types::{InputSchema, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolResult};
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

struct ProbeExecutor;

impl ToolExecutor for ProbeExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a organon::types::ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::text("ok")) })
    }
}

fn probe_tool_registry() -> ToolRegistry {
    let tool_name = ToolName::new("probe_tool").expect("valid tool name");
    let tool_def = ToolDef {
        name: tool_name,
        description: "Probe tool for pylon nous tests.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: vec![].into_iter().collect(),
            required: Vec::new(),
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![organon::types::ToolTag::Recon],
    };

    let mut registry = ToolRegistry::new();
    registry
        .register(tool_def, Box::new(ProbeExecutor))
        .expect("register tool");
    registry
}

/// WHY: Unix-only helper that makes a directory read-only for the test body
/// and reliably restores write permissions when dropped so the tempdir can
/// be cleaned up.
#[cfg(unix)]
struct ReadOnlyGuard<'a> {
    path: &'a std::path::Path,
    orig_mode: u32,
}

#[cfg(unix)]
impl<'a> ReadOnlyGuard<'a> {
    fn new(path: &'a std::path::Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).unwrap();
        let orig_mode = meta.permissions().mode();
        let mut perms = meta.permissions();
        perms.set_mode(orig_mode & !0o222);
        std::fs::set_permissions(path, perms).unwrap();
        Self { path, orig_mode }
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyGuard<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(self.path).unwrap().permissions();
        perms.set_mode(self.orig_mode);
        let _ = std::fs::set_permissions(self.path, perms);
    }
}

#[tokio::test]
async fn list_nous_returns_agents() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
    assert_eq!(agents[0]["enabled"], true);
    assert!(agents[0]["tools"].is_array());
}

#[tokio::test]
async fn list_nous_hides_private_agents_from_readonly_callers() {
    let (state, _dir) = test_state_with_private_nous().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let resp = router
        .oneshot(authed_get_as("/api/v1/nous", Role::Readonly))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
}

#[tokio::test]
async fn list_nous_includes_private_agents_for_operators() {
    let (state, _dir) = test_state_with_private_nous().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let resp = router.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    let ids: Vec<&str> = agents
        .iter()
        .filter_map(|agent| agent["id"].as_str())
        .collect();
    assert_eq!(agents.len(), 2);
    assert!(ids.contains(&"syn"));
    assert!(ids.contains(&"hidden"));
}

#[tokio::test]
async fn private_nous_status_and_tools_reject_readonly_unscoped_callers() {
    let (state, _dir) = test_state_with_private_nous().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    for path in ["/api/v1/nous/hidden", "/api/v1/nous/hidden/tools"] {
        let resp = router
            .clone()
            .oneshot(authed_get_as(path, Role::Readonly))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{path}");
        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], "forbidden");
    }
}

#[tokio::test]
async fn private_nous_status_and_tools_reject_readonly_scoped_callers() {
    let (state, _dir) = test_state_with_private_nous().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    for path in ["/api/v1/nous/hidden", "/api/v1/nous/hidden/tools"] {
        let resp = router
            .clone()
            .oneshot(authed_get_scoped_as(path, Role::Readonly, "hidden"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{path}");
        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], "forbidden");
    }
}

#[tokio::test]
async fn private_nous_status_and_tools_reject_cross_scope_callers() {
    let (state, _dir) = test_state_with_private_nous().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    for path in ["/api/v1/nous/hidden", "/api/v1/nous/hidden/tools"] {
        let resp = router
            .clone()
            .oneshot(authed_get_scoped_as(path, Role::Operator, "syn"))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{path}");
        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], "forbidden");
    }
}

#[tokio::test]
async fn get_nous_status() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous/syn")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "syn");
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());
}

#[tokio::test]
async fn get_unknown_nous_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

#[tokio::test]
async fn get_nous_tools() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/syn/tools"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["tools"].is_array());
}

#[tokio::test]
async fn patch_nous_enabled_updates_summary() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn",
        Some(serde_json::json!({ "enabled": false })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "syn");
    assert_eq!(body["enabled"], false);
}

#[tokio::test]
async fn patch_nous_tools_unknown_tool_returns_400() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn/tools",
        Some(serde_json::json!({ "tool": "definitely-not-real", "enabled": true })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn nous_list_from_manager() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let resp = router.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
    assert_eq!(agents[0]["model"], "mock-model");
    assert_eq!(agents[0]["status"], "active");
    assert_eq!(agents[0]["enabled"], true);
}

#[tokio::test]
async fn nous_tools_unknown_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent/tools"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

#[tokio::test]
async fn nous_status_response_has_all_fields() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous/syn")).await.unwrap();

    let body = body_json(resp).await;
    assert!(body["id"].is_string());
    assert!(body["model"].is_string());
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());
    assert!(body["thinking_enabled"].is_boolean());
    assert!(body["thinking_budget"].is_number());
    assert!(body["max_tool_iterations"].is_number());
    assert!(body["status"].is_string());
    assert!(body["background_failure_total_count"].is_number());
    assert!(body["background_failure_recent_count"].is_number());
    assert!(
        body["background_failure_latest_message"].is_null()
            || body["background_failure_latest_message"].is_string()
    );
    assert!(
        body["background_failure_latest_kind"].is_null()
            || body["background_failure_latest_kind"].is_string()
    );
    assert!(body["background_health_degraded"].is_boolean());
    assert_eq!(body["address_mask"]["kind"], "public");
    assert!(body["address_mask"]["allowed_senders"].is_array());
}

#[tokio::test]
async fn config_get_returns_redacted_config() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/config")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.is_object());
}

#[tokio::test]
async fn config_get_section_valid() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_get_section_invalid_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/nonexistent_section"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_update_invalid_section_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/nonexistent_section",
        Some(serde_json::json!({"key": "value"})),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn gateway_config_signing_key_is_redacted() {
    let (state, _dir) = test_state().await;

    {
        let mut config = state.config.write().await;
        config.gateway.auth.signing_key = Some(koina::secret::SecretString::from(
            "super-secret-signing-key",
        ));
    }

    let router = build_router(Arc::clone(&state), &test_security_config());
    let resp = router
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    // WHY: The raw secret must not appear anywhere in the response.
    assert!(
        !body.to_string().contains("super-secret-signing-key"),
        "signing key must not appear in API response"
    );
    assert_eq!(body["auth"]["signingKey"], "***");
    assert_eq!(body["port"], 18789);
}

#[tokio::test]
async fn nous_recover_unknown_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/nous/nonexistent/recover", None);
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

#[tokio::test]
async fn nous_recover_requires_auth() {
    let (app, _dir) = app().await;
    let req = json_request("POST", "/api/v1/nous/syn/recover", None);
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[cfg(unix)]
async fn patch_nous_enabled_rolls_back_on_persist_failure() {
    let (app, dir) = app().await;
    let config_dir = dir.path().join("config");

    // WHY: Simulate a failed config write by making the config directory
    // read-only. The handler must not mutate live state before persistence
    // succeeds (see #4582).
    let _guard = ReadOnlyGuard::new(&config_dir);

    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn",
        Some(serde_json::json!({ "enabled": false })),
    );
    let resp = app.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // WHY: The guard is dropped before `dir`, restoring write permissions.
    // Verify the in-memory config was not mutated by the failed write.
    let resp = app
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().expect("nous array");
    let syn = agents
        .iter()
        .find(|a| a["id"] == "syn")
        .expect("syn in list");
    assert_eq!(syn["enabled"], true);
}

#[tokio::test]
#[cfg(unix)]
async fn patch_nous_tools_rolls_back_on_persist_failure() {
    let (state, dir) = test_state().await;
    let mut state = Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique app state"));
    state.tool_registry = Arc::new(probe_tool_registry());
    let state = Arc::new(state);
    let app = build_router(Arc::clone(&state), &test_security_config());
    let config_dir = dir.path().join("config");

    let baseline = app
        .clone()
        .oneshot(authed_get("/api/v1/nous/syn/tools"))
        .await
        .unwrap();
    assert_eq!(baseline.status(), StatusCode::OK);
    let baseline_body = body_json(baseline).await;
    let tools = baseline_body["tools"].as_array().expect("tools array");
    let probe = tools
        .iter()
        .find(|t| t["name"] == "probe_tool")
        .expect("probe_tool present");
    assert_eq!(probe["enabled"], true);

    // WHY: Simulate a failed config write so the allowlist update must abort
    // without mutating live config (see #4582).
    let _guard = ReadOnlyGuard::new(&config_dir);

    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn/tools",
        Some(serde_json::json!({ "tool": "probe_tool", "enabled": false })),
    );
    let resp = app.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // WHY: Guard drops here, restoring write permissions before `dir`.
    // Verify the live allowlist is unchanged.
    let after = app
        .clone()
        .oneshot(authed_get("/api/v1/nous/syn/tools"))
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::OK);
    let after_body = body_json(after).await;
    let tools = after_body["tools"].as_array().expect("tools array");
    let probe = tools
        .iter()
        .find(|t| t["name"] == "probe_tool")
        .expect("probe_tool still present");
    assert_eq!(probe["enabled"], true);
}
