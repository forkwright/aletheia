use super::helpers::*;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::router::build_router;
use crate::security::SecurityConfig;

#[test]
fn api_error_session_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::SessionNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_nous_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::NousNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_bad_request_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::BadRequest {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn api_error_internal_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Internal {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn api_error_unauthorized_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Unauthorized {
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn api_error_rate_limited_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_secs: 1,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn api_error_forbidden_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Forbidden {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn api_error_service_unavailable_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ServiceUnavailable {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn api_error_validation_failed_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field required".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn api_error_rate_limited_includes_details() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_secs: 5,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    assert_eq!(body["error"]["details"]["retry_after_secs"], 5);
}

#[test]
fn api_error_validation_failed_includes_errors() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field1 required".to_owned(), "field2 invalid".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
}

#[test]
fn security_config_default_values() {
    let config = SecurityConfig::default();
    assert!(config.allowed_origins.is_empty());
    assert_eq!(config.cors_max_age_secs, 3600);
    assert_eq!(config.body_limit_bytes, 1_048_576);
    assert!(config.csrf_enabled);
    assert_eq!(config.csrf_header_name, "x-requested-with");
    // WHY: The default CSRF token is now a CSPRNG-generated 32-char hex string
    // rather than the static "aletheia" value, which was guessable.
    assert_eq!(config.csrf_header_value.len(), 32);
    assert!(
        config
            .csrf_header_value
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    );
    assert_ne!(config.csrf_header_value, "aletheia");
    assert!(!config.tls_enabled);
    assert!(config.tls_cert_path.is_none());
    assert!(config.tls_key_path.is_none());
}

#[test]
fn security_config_from_gateway() {
    use aletheia_taxis::config::GatewayConfig;

    let gw = GatewayConfig::default();
    let config = SecurityConfig::from_gateway(&gw);
    assert!(!config.tls_enabled);
    assert!(config.csrf_enabled);
    assert_eq!(config.cors_max_age_secs, 3600);
}

#[test]
fn deep_merge_overwrites_scalar() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "old"});
    let patch = serde_json::json!({"key": "new"});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"], "new");
}

#[test]
fn deep_merge_adds_missing_keys() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"existing": 1});
    let patch = serde_json::json!({"new_key": 2});
    deep_merge(&mut base, patch);
    assert_eq!(base["existing"], 1);
    assert_eq!(base["new_key"], 2);
}

#[test]
fn deep_merge_recurses_objects() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"nested": {"a": 1, "b": 2}});
    let patch = serde_json::json!({"nested": {"b": 3, "c": 4}});
    deep_merge(&mut base, patch);
    assert_eq!(base["nested"]["a"], 1);
    assert_eq!(base["nested"]["b"], 3);
    assert_eq!(base["nested"]["c"], 4);
}

#[test]
fn deep_merge_replaces_non_object_with_object() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "string"});
    let patch = serde_json::json!({"key": {"nested": true}});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"]["nested"], true);
}

#[tokio::test]
async fn error_response_has_consistent_structure() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());
    assert!(
        body["error"]["request_id"].is_string(),
        "error response must include request_id"
    );
}

#[tokio::test]
async fn malformed_create_body_returns_400() {
    let (app, _dir) = app().await;
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"invalid": true}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn malformed_send_body_returns_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"wrong_field": "abc"}"#))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn session_response_has_all_expected_fields() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;

    assert!(session["id"].is_string());
    assert!(session["nous_id"].is_string());
    assert!(session["session_key"].is_string());
    assert!(session["status"].is_string());
    assert!(session["message_count"].is_number());
    assert!(session["token_count_estimate"].is_number());
    assert!(session["created_at"].is_string());
    assert!(session["updated_at"].is_string());
}

#[tokio::test]
async fn history_messages_have_expected_fields() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
        store
            .append_message(
                id,
                aletheia_mneme::types::Role::User,
                "test message",
                None,
                None,
                10,
            )
            .unwrap();
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let msg = &body["messages"][0];
    assert!(msg["id"].is_number());
    assert!(msg["seq"].is_number());
    assert!(msg["role"].is_string());
    assert!(msg["content"].is_string());
    assert!(msg["created_at"].is_string());
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
}

// ---------------------------------------------------------------------------
// Error response tests: isolated handler error paths with envelope validation
// ---------------------------------------------------------------------------

/// Verify that an error JSON body has the required `{error: {code, message}}`
/// envelope and optionally matches expected code and status.
fn assert_error_envelope(body: &serde_json::Value, expected_code: &str) {
    assert!(body["error"].is_object(), "response must have error object");
    assert!(
        body["error"]["code"].is_string(),
        "error must have string code"
    );
    assert!(
        body["error"]["message"].is_string(),
        "error must have string message"
    );
    assert_eq!(
        body["error"]["code"], expected_code,
        "error code mismatch: expected '{expected_code}', got '{}'",
        body["error"]["code"]
    );
}

#[tokio::test]
async fn create_session_empty_nous_id_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "",
            "session_key": "test-key"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nous_id"),
        "message should mention nous_id"
    );
}

#[tokio::test]
async fn create_session_empty_session_key_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": ""
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("session_key"),
        "message should mention session_key"
    );
}

#[tokio::test]
async fn create_session_unknown_nous_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "nonexistent-nous",
            "session_key": "test-key"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn rename_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/sessions/nonexistent-id/name",
        Some(serde_json::json!({ "name": "New Name" })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn rename_session_empty_name_returns_400_with_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("name"),
        "message should mention name"
    );
}

#[tokio::test]
async fn unarchive_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent-id/unarchive", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn archive_post_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent-id/archive", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn get_archived_session_returns_404_with_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    // Archive the session.
    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn list_facts_invalid_sort_field_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts?sort=invalid_field"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid sort field"),
        "message should describe invalid sort"
    );
}

#[tokio::test]
async fn list_facts_invalid_order_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get(
            "/api/v1/knowledge/facts?sort=confidence&order=upward",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid order"),
        "message should describe invalid order"
    );
}

#[tokio::test]
async fn update_confidence_above_range_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/fact-01/confidence",
        Some(serde_json::json!({ "confidence": 1.5 })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("confidence"),
        "message should mention confidence"
    );
}

#[tokio::test]
async fn update_confidence_negative_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/fact-01/confidence",
        Some(serde_json::json!({ "confidence": -0.1 })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
}

#[tokio::test]
async fn update_confidence_valid_range_returns_501_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/fact-01/confidence",
        Some(serde_json::json!({ "confidence": 0.5 })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_implemented");
}

#[tokio::test]
async fn get_nonexistent_fact_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts/nonexistent-fact-id"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn send_message_missing_content_field_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({})),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    // Axum's Json extractor returns 422 for missing required fields.
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn stream_turn_missing_agent_id_returns_422() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "message": "hello"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    // Axum's Json extractor returns 422 for missing required fields.
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn stream_turn_unknown_agent_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "agentId": "nonexistent-agent",
            "message": "hello"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn duplicate_session_conflict_has_error_envelope() {
    let (router, _dir) = app().await;

    let req1 = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "envelope-dup-key"
        })),
    );
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::CREATED);

    let req2 = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "envelope-dup-key"
        })),
    );
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
    let body = body_json(resp2).await;
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already exists"),
        "conflict message should indicate existing session"
    );
}

#[tokio::test]
async fn archived_session_message_conflict_has_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not active"),
        "message should explain session is not active"
    );
}

#[tokio::test]
async fn send_message_no_provider_returns_500_with_envelope() {
    let (state, _dir) = test_state_with_provider(false).await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "test" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "internal_error");
    // 500 responses must use a generic message, not leak internal details.
    assert_eq!(
        body["error"]["message"].as_str().unwrap(),
        "An internal error occurred"
    );
}

#[tokio::test]
async fn config_get_unknown_section_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/nonexistent_section"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn config_update_unknown_section_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/nonexistent_section",
        Some(serde_json::json!({ "key": "value" })),
    );
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_found");
}

#[tokio::test]
async fn nous_tools_unknown_agent_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent-nous/tools"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn nous_status_unknown_agent_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent-nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn history_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent-id/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn all_error_codes_include_request_id_in_envelope() {
    let (router, _dir) = app().await;

    // 400: empty nous_id
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({ "nous_id": "", "session_key": "k" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(
        body["error"]["request_id"].is_string(),
        "400 error must include request_id"
    );

    // 404: session not found
    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions/no-such-session"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert!(
        body["error"]["request_id"].is_string(),
        "404 error must include request_id"
    );

    // 404: nous not found
    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous/no-such-nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert!(
        body["error"]["request_id"].is_string(),
        "404 nous error must include request_id"
    );
}
