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
async fn detailed_health_flags_empty_runtime_harness() {
    let (app, _dir) = app_no_providers().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "unhealthy");
    let checks = body["checks"].as_array().expect("checks is array");
    let runtime = checks
        .iter()
        .find(|check| check["name"] == "runtime_assembly")
        .expect("runtime assembly check present");
    assert_eq!(runtime["status"], "fail");
    assert!(
        runtime["message"]
            .as_str()
            .expect("runtime message")
            .contains("aletheia serve")
    );
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
    assert!(
        names.contains(&"runtime_assembly"),
        "missing runtime_assembly check"
    );
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
    assert_eq!(runtime_check["status"], "pass");

    let details = runtime_check["details"]
        .as_object()
        .expect("details object");
    let supported = details["supported_providers"]
        .as_array()
        .expect("supported_providers array");
    assert!(supported.iter().any(|p| p == "anthropic"));

    let last_effect = details["last_effect"]
        .as_object()
        .expect("last_effect object");
    assert_eq!(last_effect["provider"], "anthropic");
    assert_eq!(last_effect["effect"], "restart_required");
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
    let version = body["openapi"].as_str().unwrap();
    assert!(
        version.starts_with("3."),
        "expected OpenAPI 3.x, got {version}"
    );
}

#[tokio::test]
async fn openapi_spec_has_all_paths() {
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

/// SECURITY(#5174): the `OpenAPI` spec exposes full route shape and schema
/// detail (the same class of operational-topology leak `/metrics` guards
/// against). Unlike `/api/health` (minimal liveness, deliberately public),
/// this surface now requires the same bearer auth as the API it describes.
#[tokio::test]
async fn openapi_docs_requires_auth_in_token_mode() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// SECURITY(#5174): `auth.mode = "none"` grants every request a synthetic
/// role without a token, so the docs route stays reachable there — matching
/// the rest of the API's behavior in no-auth mode, not a separate carve-out.
#[tokio::test]
async fn openapi_docs_reachable_without_token_in_none_mode() {
    let (app, _dir) = app_with_auth_mode("none").await;
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

#[tokio::test]
async fn public_health_contains_no_absolute_paths() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        !body.contains("data_dir"),
        "public health leaked data_dir field: {body}"
    );
    assert!(
        !body.contains('/'),
        "public health must not contain absolute path characters: {body}"
    );
}

#[tokio::test]
async fn deprecated_health_contains_no_absolute_paths() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        !body.contains("data_dir"),
        "deprecated health leaked data_dir field: {body}"
    );
    assert!(
        !body.contains('/'),
        "deprecated health must not contain absolute path characters: {body}"
    );
}

#[tokio::test]
async fn detailed_health_includes_data_dir_for_operator() {
    let (app, dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let data_dir = body["data_dir"]
        .as_str()
        .expect("operator health has data_dir");
    let expected = dir.path().to_string_lossy();
    assert!(
        data_dir.contains(expected.as_ref()),
        "operator data_dir should contain instance root {expected}; got {data_dir}"
    );
}

// ── /api/v1/system/status (#5313) ──

#[tokio::test]
async fn system_status_requires_auth() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/v1/system/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn system_status_requires_operator() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_as(
            "/api/v1/system/status",
            symbolon::types::Role::Readonly,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn system_status_lists_every_subsystem_with_an_owner() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/status"))
        .await
        .unwrap();

    assert_ne!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "baseline test state should not have a genuinely failed subsystem"
    );
    let body = body_json(resp).await;
    assert!(body["status"].is_string());
    assert!(body["generated_at"].is_string());

    let subsystems = body["subsystems"].as_array().expect("subsystems array");
    let expected_ids = [
        "provider_reachability",
        "provider_credentials",
        "embeddings",
        "session_store",
        "nous_runtime",
        "turn_event_persistence",
        "memory_graph",
        "daemon_runtime",
        "tool_execution_history",
        "training_qa_persistence",
        "metrics_exposure",
        "event_bus",
        "config_security_posture",
    ];
    let ids: Vec<&str> = subsystems.iter().filter_map(|s| s["id"].as_str()).collect();
    for expected in expected_ids {
        assert!(ids.contains(&expected), "missing subsystem: {expected}");
    }

    for subsystem in subsystems {
        assert!(
            subsystem["owner"].as_str().is_some_and(|o| !o.is_empty()),
            "every subsystem needs a non-empty owner: {subsystem:?}"
        );
        assert!(subsystem["last_checked"].is_string());
        let status = subsystem["status"].as_str().expect("status is a string");
        assert!(
            ["healthy", "degraded", "failed", "unknown"].contains(&status),
            "unexpected status vocabulary: {status}"
        );
    }
}

#[tokio::test]
async fn system_status_reports_unknown_for_unwired_subsystems_without_failing_aggregate() {
    // WHY(#5313): "unknown" must never be silently omitted, and must never
    // be confused with "healthy" or force the aggregate to "failed" — a
    // subsystem this endpoint cannot see yet is a gap to close, not
    // evidence the system is down.
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/status"))
        .await
        .unwrap();

    assert_ne!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    let subsystems = body["subsystems"].as_array().expect("subsystems array");

    for id in ["daemon_runtime", "training_qa_persistence"] {
        let subsystem = subsystems
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("missing subsystem: {id}"));
        assert_eq!(subsystem["status"], "unknown");
        assert!(
            subsystem["failure_reason"].is_string(),
            "unknown subsystem should explain why: {subsystem:?}"
        );
    }
}

#[tokio::test]
async fn system_status_degrades_when_no_providers_registered() {
    let (app, _dir) = app_no_providers().await;
    let resp = app
        .oneshot(authed_get("/api/v1/system/status"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "degraded");

    let subsystems = body["subsystems"].as_array().expect("subsystems array");
    let provider_reachability = subsystems
        .iter()
        .find(|s| s["id"] == "provider_reachability")
        .expect("provider_reachability present");
    assert_eq!(provider_reachability["status"], "degraded");
    assert!(provider_reachability["degraded_reason"].is_string());
}

#[tokio::test]
async fn system_status_fails_when_a_nous_actor_dies() {
    let (state, _dir) = test_state().await;
    stop_actor_until_channel_closes(&state, "syn").await;
    let app = build_router(Arc::clone(&state), &test_security_config());

    let resp = app
        .oneshot(authed_get("/api/v1/system/status"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "failed");

    let subsystems = body["subsystems"].as_array().expect("subsystems array");
    let nous_runtime = subsystems
        .iter()
        .find(|s| s["id"] == "nous_runtime")
        .expect("nous_runtime present");
    assert_eq!(nous_runtime["status"], "failed");
    assert!(nous_runtime["failure_reason"].is_string());
}

#[tokio::test]
async fn system_status_turn_event_persistence_reports_a_count() {
    let (app, _dir) = app().await;

    let resp = app
        .oneshot(authed_get("/api/v1/system/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let subsystems = body["subsystems"].as_array().expect("subsystems array");
    let turn_events = subsystems
        .iter()
        .find(|s| s["id"] == "turn_event_persistence")
        .expect("turn_event_persistence present");
    assert_eq!(turn_events["status"], "healthy");
    assert_eq!(
        turn_events["details"]["active_turn_buffers"], 0,
        "no turns have streamed in this test state"
    );
}
