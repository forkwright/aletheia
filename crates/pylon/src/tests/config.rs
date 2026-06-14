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
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
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
