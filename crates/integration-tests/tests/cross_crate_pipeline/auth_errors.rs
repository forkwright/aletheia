//! Authentication and error propagation integration tests.
use super::*;

// ===========================================================================
// 5. Auth flow
// ===========================================================================

#[tokio::test]
async fn auth_no_token_returns_401() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "test-nous",
                "session_key": "unauthed"
            }))
            .expect("serialize"),
        ))
        .expect("request");

    let resp = router.oneshot(req).await.expect("no-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_invalid_token_returns_401() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/sessions")
        .header("authorization", "Bearer not-a-valid-jwt-token")
        .body(Body::empty())
        .expect("request");

    let resp = router.oneshot(req).await.expect("bad-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_expired_token_returns_401() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // Craft a token with exp far in the past
    let claims = aletheia_symbolon::types::Claims {
        sub: "test-user".to_owned(),
        role: Role::Operator,
        nous_id: None,
        iss: "aletheia-test".to_owned(),
        iat: 1_000_000,
        exp: 1_000_001, // 1970-01-12: well past any leeway
        jti: "expired-test".to_owned(),
        kind: aletheia_symbolon::types::TokenKind::Access,
    };
    let token = harness
        .jwt_manager
        .encode_claims(&claims)
        .expect("encode expired token");

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/sessions")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");

    let resp = router.oneshot(req).await.expect("expired-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_valid_token_returns_200() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions"))
        .await
        .expect("authed request");
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 6. Error propagation
// ===========================================================================

#[tokio::test]
async fn error_invalid_session_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // GET unknown session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent-id"))
        .await
        .expect("get unknown session");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // POST message to unknown session
    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/nonexistent-id/messages",
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send to unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // GET history of unknown session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent-id/history"))
        .await
        .expect("history of unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn error_empty_message_returns_400() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "" })),
    );
    let resp = router.clone().oneshot(req).await.expect("empty message");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn error_empty_rename_returns_400() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "" })),
    );
    let resp = router.clone().oneshot(req).await.expect("empty rename");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn error_provider_failure_returns_sse_error_event() {
    let harness = TestHarness::build_with_provider(Box::new(
        MockProvider::error("simulated provider failure")
            .models(&["mock-model"])
            .named("mock-error"),
    ))
    .await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    // The SSE stream should still start (HTTP 200), but contain an error event
    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "trigger error" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "SSE stream should start with 200 even on provider error"
    );

    let body = body_string(resp).await;
    let events = parse_sse_events(&body);
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();

    // Should have an error event
    assert!(
        event_types.contains(&"error"),
        "should contain error SSE event when provider fails, got: {event_types:?}"
    );
}

#[tokio::test]
async fn error_nonexistent_nous_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "nonexistent-agent",
            "session_key": "test"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("create session for unknown nous");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
