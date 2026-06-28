#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn health_no_auth_required() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
    assert_eq!(
        body.as_object().expect("health response object").len(),
        1,
        "public health must remain minimal liveness only"
    );
}

#[tokio::test]
async fn health_returns_200() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
    assert!(body.get("version").is_none());
    assert!(body.get("uptime_seconds").is_none());
    assert!(body.get("checks").is_none());
    assert!(body.get("data_dir").is_none());
}

#[tokio::test]
async fn public_health_does_not_expose_diagnostics() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(!body.contains("data_dir"), "public health leaked data_dir");
    assert!(
        !body.contains("credential"),
        "public health leaked credential diagnostics"
    );
    assert!(
        !body.contains("sk-ant"),
        "public health leaked credential data"
    );
}

#[tokio::test]
async fn detailed_health_degraded_without_providers() {
    let (app, _dir) = app_no_providers().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["status"], "degraded");
}

#[tokio::test]
async fn detailed_health_requires_auth() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/v1/system/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn detailed_health_requires_operator() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_as(
            "/api/v1/system/health",
            symbolon::types::Role::Readonly,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn detailed_health_checks_have_expected_shape() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    let checks = body["checks"].as_array().expect("checks is array");
    assert!(checks.len() >= 2, "expected at least 2 health checks");

    for check in checks {
        assert!(check["name"].is_string(), "each check has a name");
        assert!(check["status"].is_string(), "each check has a status");
    }

    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(
        names.contains(&"session_store"),
        "missing session_store check"
    );
    assert!(names.contains(&"providers"), "missing providers check");
    assert!(body["data_dir"].is_string(), "operator health has data_dir");
}

#[tokio::test]
async fn detailed_health_exposes_credential_runtime_state() {
    let (app, _dir) = app_with_anthropic_provider().await;

    // Trigger a mutation so the runtime manager records an effect.
    let add = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": "sk-test-health-secret",
                "role": "backup"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);

    let validate = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/anthropic:backup/validate",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(validate.status(), StatusCode::OK);

    let resp = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let checks = body["checks"].as_array().expect("checks is array");
    let runtime_check = checks
        .iter()
        .find(|c| c["name"] == "credential_runtime")
        .expect("credential_runtime check present");
    assert_eq!(runtime_check["status"], "warn");

    let details = runtime_check["details"]
        .as_object()
        .expect("details object");
    let supported = details["supported_providers"]
        .as_array()
        .expect("supported_providers array");
    let anthropic = supported
        .iter()
        .find(|p| p["name"] == "anthropic")
        .expect("anthropic capability present");
    assert_eq!(anthropic["hot_apply_supported"], false);
    assert_eq!(anthropic["restart_required"], true);
    assert_eq!(anthropic["runtime_effect"], "restart_required");
    assert_eq!(anthropic["availability"]["status"], "up");

    let last_effect = details["last_effect"]
        .as_object()
        .expect("last_effect object");
    assert_eq!(last_effect["provider"], "anthropic");
    assert_eq!(last_effect["effect"], "restart_required");
    assert_eq!(details["restart_required"], true);
    assert_eq!(details["degraded"], true);

    let last_mutation = details["last_mutation_result"]
        .as_object()
        .expect("last_mutation_result object");
    assert_eq!(last_mutation["provider"], "anthropic");
    assert_eq!(last_mutation["role"], "backup");
    assert_eq!(last_mutation["action"], "add");
    assert_eq!(last_mutation["result"], "success");
    assert_eq!(last_mutation["runtime_effect"], "restart_required");

    let last_validation = details["last_successful_validation"]
        .as_object()
        .expect("last_successful_validation object");
    assert_eq!(last_validation["provider"], "anthropic");
    assert_eq!(last_validation["role"], "backup");
    assert_eq!(last_validation["action"], "validate");
    assert_eq!(last_validation["credential_status"], "valid");
}

#[tokio::test]
async fn metrics_returns_200_with_prometheus_content_type() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    // WHY: prometheus-client emits OpenMetrics text format natively. Prometheus
    // scrapers accept this format directly, so the content-type advertises
    // OpenMetrics rather than the legacy text/plain.
    assert!(
        content_type.contains("application/openmetrics-text"),
        "expected openmetrics-text content type, got: {content_type}"
    );
}

#[tokio::test]
async fn metrics_no_auth_required() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_contains_aletheia_prefixed_families() {
    let (app, _dir) = app().await;

    // WHY: `aletheia_http_requests` is a labeled counter Family — it emits no
    // `_total` series until a request is recorded, so record one first.
    let recorded = app
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(recorded.status(), StatusCode::OK);

    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_string(resp).await;
    assert!(
        body.contains("aletheia_http_requests_total"),
        "should expose the HTTP request counter family; got: {body}"
    );
    assert!(
        body.contains("aletheia_uptime_seconds"),
        "should expose the uptime gauge; got: {body}"
    );
    assert!(
        body.contains("/api/health"),
        "recorded request path should appear as a counter label; got: {body}"
    );
    assert!(
        body.contains("# HELP") && body.contains("# TYPE"),
        "should contain Prometheus HELP/TYPE metadata; got: {body}"
    );
}

#[tokio::test]
async fn metrics_counters_increment_after_request() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let _ = router
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_string(resp).await;
    assert!(
        body.contains("/api/health"),
        "should contain the health endpoint path in metrics"
    );
}

#[tokio::test]
async fn openapi_spec_returns_valid_json() {
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
    let version = body["openapi"].as_str().unwrap();
    assert!(
        version.starts_with("3."),
        "expected OpenAPI 3.x, got {version}"
    );
}

#[tokio::test]
async fn openapi_spec_has_all_paths() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    let paths = body["paths"].as_object().unwrap();
    assert!(paths.contains_key("/api/health"));
    assert!(paths.contains_key("/api/v1/system/health"));
    assert!(paths.contains_key("/api/v1/sessions"));
    assert!(paths.contains_key("/api/v1/sessions/{id}"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/messages"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/history"));
    assert!(paths.contains_key("/api/v1/nous"));
    assert!(paths.contains_key("/api/v1/nous/{id}"));
    assert!(paths.contains_key("/api/v1/nous/{id}/tools"));
    assert!(paths.contains_key("/api/v1/nous/{id}/recover"));
    assert!(paths.contains_key("/api/v1/events/subscribe"));
    assert!(paths.contains_key("/api/v1/events/discovery"));
    let nous_path = paths["/api/v1/nous"].as_object().unwrap();
    assert!(nous_path.contains_key("post"));
}

#[tokio::test]
async fn openapi_spec_advertises_bearer_auth_in_token_mode() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(
        body["components"]["securitySchemes"]
            .get("bearer_auth")
            .is_some()
    );
}

#[tokio::test]
async fn openapi_spec_omits_bearer_auth_in_none_mode() {
    let (app, _dir) = app_with_auth_mode("none").await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(
        body["components"]["securitySchemes"]
            .get("bearer_auth")
            .is_none()
    );
}

#[tokio::test]
async fn openapi_docs_no_auth_required() {
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
}

#[tokio::test]
async fn openapi_spec_has_schemas() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    let schemas = body["components"]["schemas"].as_object().unwrap();
    assert!(schemas.contains_key("SessionResponse"));
    assert!(schemas.contains_key("ErrorResponse"));
    assert!(schemas.contains_key("HealthResponse"));
    assert!(schemas.contains_key("NousStatus"));
    assert!(schemas.contains_key("AgentDefinition"));
    assert!(schemas.contains_key("CreateAgentResponse"));
    assert!(schemas.contains_key("RecoverResponse"));
}
