use std::sync::Arc;

use axum::http::StatusCode;
use tower::ServiceExt;

use super::helpers::*;

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

// ── Session error envelopes ─────────────────────────────────────────────────

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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty nous_id should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty session_key should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unknown nous_id should return 404"
    );
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "renaming nonexistent session should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn rename_session_empty_name_returns_400_with_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have string id");

    let req = authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty name should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("name"),
        "message should mention name"
    );
}

#[tokio::test]
async fn unarchive_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent-id/unarchive", None);
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unarchiving nonexistent session should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn archive_post_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent-id/archive", None);
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "archiving nonexistent session should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

#[tokio::test]
async fn get_archived_session_returns_404_with_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have string id");

    // Archive the session.
    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("delete request should complete");
    assert_eq!(
        del.status(),
        StatusCode::NO_CONTENT,
        "delete should return 204 No Content"
    );

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "archived session should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
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
    let resp1 = router
        .clone()
        .oneshot(req1)
        .await
        .expect("first create request should complete");
    assert_eq!(
        resp1.status(),
        StatusCode::CREATED,
        "first session creation should return 201"
    );

    let req2 = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "envelope-dup-key"
        })),
    );
    let resp2 = router
        .clone()
        .oneshot(req2)
        .await
        .expect("second create request should complete");
    assert_eq!(
        resp2.status(),
        StatusCode::CONFLICT,
        "duplicate session_key should return 409"
    );
    let body = body_json(resp2).await;
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("already exists"),
        "conflict message should indicate existing session"
    );
}

#[tokio::test]
async fn archived_session_message_conflict_has_envelope() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have string id");

    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("delete request should complete");
    assert_eq!(
        del.status(),
        StatusCode::NO_CONTENT,
        "delete should return 204 No Content"
    );

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("message request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "sending message to archived session should return 409"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("not active"),
        "message should explain session is not active"
    );
}

#[tokio::test]
async fn history_nonexistent_session_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent-id/history"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "history for nonexistent session should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "session_not_found");
}

// ── Knowledge/facts error envelopes ─────────────────────────────────────────

#[tokio::test]
async fn list_facts_invalid_sort_field_returns_400_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts?sort=invalid_field"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "invalid sort field should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
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
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "invalid order value should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "confidence above 1.0 should return 400"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "negative confidence should return 400"
    );
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_IMPLEMENTED,
        "valid confidence update should return 501 until implemented"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_implemented");
}

#[tokio::test]
async fn get_nonexistent_fact_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts/nonexistent-fact-id"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "nonexistent fact should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_found");
}

// ── Message/stream error envelopes ──────────────────────────────────────────

#[tokio::test]
async fn send_message_missing_content_field_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have string id");

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({})),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("request should complete");
    // Axum's Json extractor returns 422 for missing required fields.
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "missing content field should return 422"
    );
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
    let resp = app.oneshot(req).await.expect("request should complete");
    // Axum's Json extractor returns 422 for missing required fields.
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "missing agent_id field should return 422"
    );
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "streaming with unknown agent should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn send_message_no_provider_returns_500_with_envelope() {
    let (state, _dir) = test_state_with_provider(false).await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have string id");

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "test" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "missing provider should return 500"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "internal_error");
    // 500 responses must use a generic message, not leak internal details.
    assert_eq!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string"),
        "An internal error occurred",
        "500 error message should be generic and not leak internal details"
    );
}

// ── Config error envelopes ──────────────────────────────────────────────────

#[tokio::test]
async fn config_get_unknown_section_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/nonexistent_section"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unknown config section should return 404"
    );
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
    let resp = app.oneshot(req).await.expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "updating unknown config section should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "not_found");
}

// ── Nous error envelopes ────────────────────────────────────────────────────

#[tokio::test]
async fn nous_tools_unknown_agent_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent-nous/tools"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "tools request for unknown nous should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

#[tokio::test]
async fn nous_status_unknown_agent_returns_404_with_envelope() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent-nous"))
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "status request for unknown nous should return 404"
    );
    let body = body_json(resp).await;
    assert_error_envelope(&body, "nous_not_found");
}

// ── Cross-cutting ───────────────────────────────────────────────────────────

#[tokio::test]
async fn all_error_codes_include_request_id_in_envelope() {
    let (router, _dir) = app().await;

    // 400: empty nous_id
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({ "nous_id": "", "session_key": "k" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty nous_id should return 400"
    );
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
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "nonexistent session should return 404"
    );
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
        .expect("request should complete");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "nonexistent nous should return 404"
    );
    let body = body_json(resp).await;
    assert!(
        body["error"]["request_id"].is_string(),
        "404 nous error must include request_id"
    );
}
