#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use axum::http::StatusCode;
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

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

// Regression coverage for #4582: config mutations must be atomic with writes.

/// Return a test state whose config directory is read-only. Permissions are
/// restored by [`restore_config_dir_permissions`] so the temp directory can be
/// cleaned up.
async fn state_with_unwritable_config() -> (Arc<AppState>, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let config_dir = dir.path().join("config");
    #[expect(
        clippy::disallowed_methods,
        reason = "test helper sets permissions to simulate a failed config write"
    )]
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o555))
        .expect("set config dir read-only");
    (state, dir)
}

fn restore_config_dir_permissions(dir: &tempfile::TempDir) {
    let config_dir = dir.path().join("config");
    #[expect(
        clippy::disallowed_methods,
        reason = "test helper restores permissions before tempdir cleanup"
    )]
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o755))
        .expect("restore config dir permissions");
}

#[tokio::test]
async fn patch_nous_enabled_leaves_config_unchanged_on_write_failure() {
    let (state, dir) = state_with_unwritable_config().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let before = {
        let config = state.config.read().await;
        config.agents.list.clone()
    };

    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn",
        Some(serde_json::json!({ "enabled": false })),
    );
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "internal_error");

    let after = {
        let config = state.config.read().await;
        config.agents.list.clone()
    };
    assert_eq!(
        after, before,
        "live agent config must not change when persistence fails"
    );

    restore_config_dir_permissions(&dir);
}

#[tokio::test]
async fn patch_nous_tools_leaves_config_unchanged_on_write_failure() {
    let (state, dir) = state_with_unwritable_config().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let before = {
        let config = state.config.read().await;
        config.agents.list.clone()
    };

    let req = authed_request(
        "PATCH",
        "/api/v1/nous/syn/tools",
        Some(serde_json::json!({ "tool": "read_file", "enabled": false })),
    );
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "internal_error");

    let after = {
        let config = state.config.read().await;
        config.agents.list.clone()
    };
    assert_eq!(
        after, before,
        "live agent config must not change when persistence fails"
    );

    restore_config_dir_permissions(&dir);
}
