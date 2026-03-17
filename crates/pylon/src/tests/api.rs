use super::helpers::*;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::router::build_router;

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
    assert!(body["version"].is_string());
    assert!(body["uptime_seconds"].is_number());
    assert!(body["checks"].is_array());
}

#[tokio::test]
async fn health_degraded_without_providers() {
    let (app, _dir) = app_no_providers().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["status"], "degraded");
}

#[tokio::test]
async fn health_checks_have_expected_shape() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
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
async fn config_get_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/config").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
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

    // Inject a signing key so there is a secret value to redact.
    {
        let mut config = state.config.write().await;
        config.gateway.auth.signing_key = Some("super-secret-signing-key".to_owned());
    }

    let router = build_router(Arc::clone(&state), &test_security_config());
    let resp = router
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    // The raw secret must not appear anywhere in the response.
    assert!(
        !body.to_string().contains("super-secret-signing-key"),
        "signing key must not appear in API response"
    );
    // The field must be replaced with the redaction placeholder.
    assert_eq!(body["auth"]["signingKey"], "***");
    // Non-secret fields must still be present and correct.
    assert_eq!(body["port"], 18789);
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
    assert!(paths.contains_key("/api/v1/sessions"));
    assert!(paths.contains_key("/api/v1/sessions/{id}"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/messages"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/history"));
    assert!(paths.contains_key("/api/v1/nous"));
    assert!(paths.contains_key("/api/v1/nous/{id}"));
    assert!(paths.contains_key("/api/v1/nous/{id}/tools"));
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
    assert!(
        content_type.contains("text/plain"),
        "expected text/plain content type, got: {content_type}"
    );
}

#[tokio::test]
async fn metrics_no_auth_required() {
    let (app, _dir) = app().await;
    // No authorization header: should still succeed
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_contains_aletheia_prefixed_families() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_string(resp).await;
    assert!(
        body.contains("aletheia_http_requests_total"),
        "should contain HTTP request counter"
    );
    assert!(
        body.contains("aletheia_uptime_seconds"),
        "should contain uptime gauge"
    );
    assert!(
        body.contains("# HELP"),
        "should contain Prometheus HELP comments"
    );
    assert!(
        body.contains("# TYPE"),
        "should contain Prometheus TYPE comments"
    );
}

#[tokio::test]
async fn metrics_counters_increment_after_request() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // Make a health request first to increment the counter
    let _ = router
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Then check /metrics for the counter
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
async fn unknown_route_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nonexistent"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn old_api_nous_path_returns_gone() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::GONE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/nous")
    );
}

#[tokio::test]
async fn fallback_404_returns_json_error() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/totally/unknown/path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/totally/unknown/path")
    );
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn put_on_sessions_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn delete_on_nous_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_on_health_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn request_id_present_in_error_responses() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let request_id = body["error"]["request_id"].as_str().unwrap();
    assert!(!request_id.is_empty());
    assert!(request_id.len() >= 20, "request_id should be a ULID");
}

#[tokio::test]
async fn empty_json_body_send_message_returns_400() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({})),
    );

    let resp = router.clone().oneshot(req).await.expect("response");
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "expected 400 or 422, got {}",
        resp.status()
    );
}
