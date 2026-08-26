use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use koina::http::BEARER_PREFIX;
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn update_section_typed_happy_path() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/embedding",
        Some(serde_json::json!({
            "provider": "candle",
            "dimension": 512
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "embedding");
    assert!(body["config"].is_object());
}

#[tokio::test]
async fn update_section_bindings_happy_path() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/bindings",
        Some(serde_json::json!([
            { "channel": "signal", "source": "+1234567890", "nousId": "syn" }
        ])),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "bindings");
    assert!(body["config"].is_array());
}

#[tokio::test]
async fn update_section_binding_revoke_is_persisted_but_not_reported_live() {
    let (state, dir) = test_state().await;
    {
        let mut config = state.config.write().await;
        config.bindings.push(taxis::config::ChannelBinding {
            channel: "signal".to_owned(),
            source: "+15550100".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "{source}".to_owned(),
            account: None,
            source_kind: None,
            participants: Vec::new(),
            command_tier: taxis::config::CommandTier::Public,
        });
        taxis::loader::write_config(&state.oikos, &config).unwrap();
    }
    let app = build_router(Arc::clone(&state), &test_security_config());

    let response = app
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/bindings",
            Some(serde_json::json!([])),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        body["restart_required"],
        serde_json::json!(["bindings"]),
        "the revoked route must be reported as staged/restart-required"
    );
    assert_eq!(
        body["config"][0]["source"], "+15550100",
        "the response must report the still-effective live router snapshot"
    );

    let live = state.config.read().await;
    assert_eq!(live.bindings.len(), 1);
    assert_eq!(live.bindings[0].source, "+15550100");
    drop(live);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let staged = taxis::loader::load_config(&oikos).unwrap();
    assert!(
        staged.bindings.is_empty(),
        "the revoke must remain persisted for the next process start"
    );
}

#[tokio::test]
async fn update_section_feature_flags_happy_path() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/feature_flags",
        Some(serde_json::json!([
            {
                "key": "new_ui",
                "description": "Enable the new desktop UI",
                "enabled": true
            }
        ])),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "feature_flags");
    assert!(body["config"].is_array());
    assert_eq!(body["config"][0]["key"], "new_ui");
    assert_eq!(body["config"][0]["enabled"], true);
}

#[tokio::test]
async fn update_section_packs_happy_path() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/packs",
        Some(serde_json::json!(["/opt/packs"])),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "packs");
    assert!(body["config"].is_array());
}

#[tokio::test]
async fn get_config_includes_feature_flags() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/config")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["feature_flags"].is_array());
}

#[tokio::test]
async fn update_section_preserves_cold_gateway_value_in_live_response() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/gateway",
        Some(serde_json::json!({
            "port": 3999
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "gateway");
    assert_ne!(
        body["config"]["port"], 3999,
        "cold gateway port must not be published as live"
    );
    assert!(
        body["restart_required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("gateway.port")),
        "response should report staged restart-required gateway.port"
    );
}

#[tokio::test]
async fn update_section_preserves_cold_gateway_auth_none_role_in_live_response() {
    // WHY(#5324): `gateway.auth.noneRole` is stored in `AppState.none_role` at
    // startup and never refreshed on reload; before the registry declared it
    // a restart-required prefix, this same PUT reported no restart needed
    // even though the live process kept using the old role indefinitely.
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/gateway",
        Some(serde_json::json!({
            "auth": { "noneRole": "admin" }
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["section"], "gateway");
    assert_ne!(
        body["config"]["auth"]["noneRole"], "admin",
        "cold gateway.auth.noneRole must not be published as live"
    );
    assert!(
        body["restart_required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("gateway.auth.noneRole")),
        "response should report staged restart-required gateway.auth.noneRole"
    );
}

#[tokio::test]
async fn sequential_puts_preserve_earlier_staged_cold_values_on_disk() {
    let (app, dir) = app().await;
    let first = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/gateway",
            Some(serde_json::json!({ "port": 3999 })),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/embedding",
            Some(serde_json::json!({
                "provider": "candle",
                "dimension": 512
            })),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let staged = taxis::loader::load_config(&oikos).unwrap();
    assert_eq!(
        staged.gateway.port, 3999,
        "the second PUT must merge from staged disk, not cold-filtered live state"
    );
    assert_eq!(staged.embedding.dimension, 512);
}

#[tokio::test]
async fn concurrent_put_and_reload_leave_the_put_visible_and_persisted() {
    let (app, dir) = app().await;
    let put_app = app.clone();
    let reload_app = app.clone();

    let put = tokio::spawn(async move {
        put_app
            .oneshot(authed_request(
                "PUT",
                "/api/v1/config/embedding",
                Some(serde_json::json!({
                    "provider": "candle",
                    "dimension": 512
                })),
            ))
            .await
            .unwrap()
    });
    let reload = tokio::spawn(async move {
        reload_app
            .oneshot(authed_request("POST", "/api/v1/config/reload", None))
            .await
            .unwrap()
    });

    let (put, reload) = tokio::join!(put, reload);
    assert_eq!(put.unwrap().status(), StatusCode::OK);
    assert_eq!(reload.unwrap().status(), StatusCode::OK);

    let get = app
        .oneshot(authed_get("/api/v1/config/embedding"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let live = body_json(get).await;
    assert_eq!(live["dimension"], 512);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let staged = taxis::loader::load_config(&oikos).unwrap();
    assert_eq!(staged.embedding.dimension, 512);
}

#[tokio::test]
async fn reload_api_stages_command_revoke_without_reporting_it_hot_applied() {
    let (state, dir) = test_state().await;
    {
        let mut live = state.config.write().await;
        live.bindings.push(taxis::config::ChannelBinding {
            channel: "signal".to_owned(),
            source: "+15550100".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "{source}".to_owned(),
            account: Some("primary".to_owned()),
            source_kind: Some(taxis::config::ChannelSourceKind::Direct),
            participants: Vec::new(),
            command_tier: taxis::config::CommandTier::Operator,
        });
        taxis::loader::write_config(&state.oikos, &live).unwrap();
    }

    let mut staged = state.config.read().await.clone();
    staged.bindings.clear();
    staged.messaging.commands.public_commands = vec!["ping".to_owned()];
    taxis::loader::write_config(&state.oikos, &staged).unwrap();

    let app = build_router(Arc::clone(&state), &test_security_config());
    let response = app
        .oneshot(authed_request("POST", "/api/v1/config/reload", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["hot_reloaded"], 0);
    let restart_required = body["restart_required"].as_array().unwrap();
    assert_eq!(restart_required.len(), 2);
    assert!(
        restart_required
            .iter()
            .any(|path| path.as_str() == Some("bindings"))
    );
    assert!(
        restart_required
            .iter()
            .any(|path| path.as_str() == Some("messaging.commands.publicCommands"))
    );

    let live = state.config.read().await;
    assert_eq!(
        live.bindings.len(),
        1,
        "the startup router keeps its old grant"
    );
    assert_eq!(
        live.messaging.commands.public_commands,
        vec!["help".to_owned(), "ping".to_owned()]
    );
    drop(live);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let persisted = taxis::loader::load_config(&oikos).unwrap();
    assert!(persisted.bindings.is_empty());
    assert_eq!(persisted.messaging.commands.public_commands, vec!["ping"]);
}

#[tokio::test]
async fn get_then_put_gateway_preserves_absent_sensitive_options() {
    let (app, dir) = app().await;
    let get = app
        .clone()
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let gateway = body_json(get).await;
    assert_eq!(gateway["auth"]["signingKey"], "***");
    assert_eq!(gateway["tls"]["certPath"], "***");
    assert_eq!(gateway["tls"]["keyPath"], "***");

    let put = app
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/gateway",
            Some(gateway),
        ))
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let staged = taxis::loader::load_config(&oikos).unwrap();
    assert!(staged.gateway.auth.signing_key.is_none());
    assert!(staged.gateway.tls.cert_path.is_none());
    assert!(staged.gateway.tls.key_path.is_none());
}

#[tokio::test]
async fn explicit_null_stages_secret_clear_without_changing_live_key() {
    use koina::secret::SecretString;

    let (state, dir) = test_state().await;
    {
        let mut config = state.config.write().await;
        config.gateway.auth.signing_key = Some(SecretString::from("old-signing-secret"));
        taxis::loader::write_config(&state.oikos, &config).unwrap();
    }
    let app = build_router(Arc::clone(&state), &test_security_config());

    let put = app
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/gateway",
            Some(serde_json::json!({
                "auth": { "signingKey": null }
            })),
        ))
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);

    let live = state.config.read().await;
    assert_eq!(
        live.gateway
            .auth
            .signing_key
            .as_ref()
            .map(SecretString::expose_secret),
        Some("old-signing-secret"),
        "cold key clear must not become effective before restart"
    );
    drop(live);

    let oikos = taxis::oikos::Oikos::from_root(dir.path());
    let staged = taxis::loader::load_config(&oikos).unwrap();
    assert!(
        staged.gateway.auth.signing_key.is_none(),
        "explicit null must remain a deliberate staged clear"
    );
}

#[tokio::test]
async fn marker_for_new_matrix_account_is_rejected_without_mutation() {
    let (app, dir) = app().await;
    let config_path = dir.path().join("config/aletheia.toml");
    let before = std::fs::read_to_string(&config_path).unwrap();

    let put = app
        .oneshot(authed_request(
            "PUT",
            "/api/v1/config/channels",
            Some(serde_json::json!({
                "matrix": {
                    "enabled": true,
                    "accounts": {
                        "primary": {
                            "homeserver": "https://matrix.example.org",
                            "accessTokenEnv": "***"
                        }
                    }
                }
            })),
        ))
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let after = std::fs::read_to_string(config_path).unwrap();
    assert_eq!(
        after, before,
        "rejected marker must not rewrite staged disk"
    );
}

#[tokio::test]
async fn update_section_rejects_semantic_invalidity_after_merge() {
    // WHY(#4583): A partial update that lowers contextTokens below the existing
    // bootstrapMaxTokens must be rejected before persisting, because the merged
    // config would fail the same validation that reload enforces.
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/agents",
        Some(serde_json::json!({
            "defaults": { "contextTokens": 30_000 }
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors.iter().any(|e| e["message"]
            .as_str()
            .unwrap_or("")
            .contains("bootstrapMaxTokens")),
        "expected bootstrapMaxTokens/contextTokens invariant error, got: {errors:?}"
    );
}

#[tokio::test]
async fn update_section_malformed_body_returns_422() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/embedding",
        Some(serde_json::json!({
            "dimension": "not-a-number"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(!errors.is_empty());
    let msg = errors[0]["message"].as_str().unwrap();
    assert!(
        msg.contains("invalid type") || msg.contains("expected"),
        "serde error detail should be present, got: {msg}"
    );
}

#[tokio::test]
async fn update_section_unknown_section_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/secrets",
        Some(serde_json::json!({ "foo": "bar" })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_spec_contains_config_section_schemas() {
    let (app, _dir) = app().await;
    let token = default_token();
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let schemas = body["components"]["schemas"].as_object().unwrap();
    assert!(
        schemas.contains_key("ConfigSectionPayload"),
        "OpenAPI spec must include ConfigSectionPayload schema"
    );
    assert!(
        schemas.contains_key("AgentsConfig"),
        "OpenAPI spec must include AgentsConfig schema"
    );
    assert!(
        schemas.contains_key("GatewayConfig"),
        "OpenAPI spec must include GatewayConfig schema"
    );
    assert!(
        schemas.contains_key("EmbeddingSettings"),
        "OpenAPI spec must include EmbeddingSettings schema"
    );
    assert!(
        schemas.contains_key("FeatureFlagConfig"),
        "OpenAPI spec must include FeatureFlagConfig schema"
    );
    let binding_properties = schemas["ChannelBinding"]["properties"]
        .as_object()
        .expect("ChannelBinding properties");
    assert!(
        binding_properties.contains_key("sourceKind"),
        "binding schema must expose its direct/group selector"
    );
    assert!(
        binding_properties.contains_key("commandTier"),
        "binding schema must expose command authority"
    );
    assert_eq!(
        schemas["ChannelSourceKind"]["enum"],
        serde_json::json!(["direct", "group"])
    );
    assert_eq!(
        schemas["CommandTier"]["enum"],
        serde_json::json!(["public", "operator"])
    );
}

#[tokio::test]
async fn get_config_rejects_readonly() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_as("/api/v1/config", Role::Readonly))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_config_rejects_agent_scope() {
    let (app, _dir) = app().await;
    let token = token_scoped_to(Role::Agent, "syn");
    let req = Request::get("/api/v1/config")
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_section_rejects_readonly() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_as("/api/v1/config/gateway", Role::Readonly))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_section_rejects_agent_scope() {
    let (app, _dir) = app().await;
    let token = token_scoped_to(Role::Agent, "syn");
    let req = Request::get("/api/v1/config/gateway")
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_section_allows_operator_and_returns_redacted_data() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.is_object());
    // WHY: gateway.port is a known non-secret value in the default test config;
    // presence proves the section was returned, while secrets remain redacted.
    assert!(body.get("port").is_some());
}
