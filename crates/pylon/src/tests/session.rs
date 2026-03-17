use std::sync::Arc;

use axum::http::StatusCode;
use tower::ServiceExt;
use tracing::Instrument;

use super::helpers::*;

#[tokio::test]
async fn create_session_returns_201() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;
    assert!(session["id"].is_string(), "session id should be a string");
    assert_eq!(
        session["nous_id"], "syn",
        "nous_id should match the requested agent"
    );
    assert_eq!(
        session["session_key"], "test-session",
        "session_key should match the requested key"
    );
    assert_eq!(
        session["status"], "active",
        "newly created session should have active status"
    );
}

#[tokio::test]
async fn get_session_returns_created_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("GET session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET session should return 200 OK"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["id"], id,
        "returned session id should match requested id"
    );
    assert_eq!(
        body["nous_id"], "syn",
        "returned nous_id should match the created session"
    );
}

#[tokio::test]
async fn get_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .expect("GET unknown session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET on unknown session should return 404"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "session_not_found",
        "error code should indicate session not found"
    );
}

#[tokio::test]
async fn close_session_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    let resp = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("DELETE session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "DELETE session should return 204 No Content"
    );
}

#[tokio::test]
async fn get_deleted_session_returns_404() {
    // DELETE archives the session; subsequent GET must return 404 (#1251).
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("DELETE session request should succeed");
    assert_eq!(
        del.status(),
        StatusCode::NO_CONTENT,
        "DELETE session should return 204 No Content"
    );

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("GET deleted session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET on deleted session should return 404"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "session_not_found",
        "error code should indicate session not found"
    );
}

#[tokio::test]
async fn close_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_delete("/api/v1/sessions/nonexistent"))
        .await
        .expect("DELETE unknown session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "DELETE on unknown session should return 404"
    );
}

#[tokio::test]
async fn concurrent_session_creation() {
    let (state, _dir) = test_state().await;
    let mut handles = Vec::new();

    for i in 0..5 {
        let router = build_router(Arc::clone(&state), &test_security_config());
        handles.push(tokio::spawn(
            async move {
                let req = authed_request(
                    "POST",
                    "/api/v1/sessions",
                    Some(serde_json::json!({
                        "nous_id": "syn",
                        "session_key": format!("concurrent-{i}")
                    })),
                );
                let resp = router
                    .oneshot(req)
                    .await
                    .expect("concurrent session creation request should succeed");
                resp.status()
            }
            .instrument(tracing::info_span!("test_session_create", index = i)),
        ));
    }

    for handle in handles {
        let status = handle
            .await
            .expect("concurrent session task should complete without panic");
        assert_eq!(
            status,
            StatusCode::CREATED,
            "concurrent session creation should return 201 Created"
        );
    }
}

#[tokio::test]
async fn double_close_session_is_idempotent() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let first = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("first close");
    assert_eq!(
        first.status(),
        StatusCode::NO_CONTENT,
        "first DELETE session should return 204 No Content"
    );

    let second = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("second close");
    assert_eq!(
        second.status(),
        StatusCode::NO_CONTENT,
        "second DELETE session should return 204 No Content (idempotent)"
    );

    // After both closes the session is gone from GET (#1251).
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get after double close");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET after double close should return 404"
    );
}

#[tokio::test]
async fn get_session_after_create_reflects_state() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("response");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET session after create should return 200 OK"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["id"], id,
        "returned session id should match the created session id"
    );
    assert_eq!(
        body["status"], "active",
        "session status should be active after creation"
    );
    assert_eq!(
        body["nous_id"], "syn",
        "session nous_id should match the created session"
    );
}

// ── List sessions ───────────────────────────────────────────────────────────

#[tokio::test]
async fn list_sessions_returns_empty_initially() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions"))
        .await
        .expect("GET sessions request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET sessions should return 200 OK"
    );
    let body = body_json(resp).await;
    assert!(
        body["sessions"].is_array(),
        "response should contain a sessions array"
    );
}

#[tokio::test]
async fn list_sessions_includes_created_session() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    create_test_session(&router).await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions"))
        .await
        .expect("GET sessions request should succeed");

    let body = body_json(resp).await;
    let sessions = body["sessions"]
        .as_array()
        .expect("response body should contain a sessions array");
    assert!(
        !sessions.is_empty(),
        "sessions list should contain at least one session after creation"
    );
    assert_eq!(
        sessions[0]["nousId"], "syn",
        "listed session should have the expected nousId"
    );
}

#[tokio::test]
async fn list_sessions_filter_by_nous_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    create_test_session(&router).await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nousId=syn"))
        .await
        .expect("GET sessions filtered by nousId request should succeed");

    let body = body_json(resp).await;
    let sessions = body["sessions"]
        .as_array()
        .expect("response body should contain a sessions array");
    assert!(
        !sessions.is_empty(),
        "filtering by existing nousId should return at least one session"
    );

    let resp2 = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nousId=nonexistent"))
        .await
        .expect("GET sessions filtered by nonexistent nousId request should succeed");

    let body2 = body_json(resp2).await;
    let sessions2 = body2["sessions"]
        .as_array()
        .expect("response body should contain a sessions array");
    assert!(
        sessions2.is_empty(),
        "filtering by nonexistent nousId should return an empty list"
    );
}

#[tokio::test]
async fn list_sessions_limit_param_returns_n_sessions() {
    // GET /api/v1/sessions?limit=N must return exactly N sessions (#1254).
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // Create 5 sessions with distinct keys.
    for i in 0..5_u32 {
        let req = authed_request(
            "POST",
            "/api/v1/sessions",
            Some(serde_json::json!({
                "nous_id": "syn",
                "session_key": format!("limit-test-{i}")
            })),
        );
        let resp = router
            .clone()
            .oneshot(req)
            .await
            .expect("session creation request should succeed");
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "each session creation should return 201 Created"
        );
    }

    let resp = router
        .oneshot(authed_get("/api/v1/sessions?limit=3"))
        .await
        .expect("GET sessions with limit request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET sessions with limit should return 200 OK"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["sessions"]
            .as_array()
            .expect("response body should contain a sessions array")
            .len(),
        3,
        "limit=3 must return exactly 3 sessions"
    );
}

// ── Archive ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn archive_via_post_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    let req = authed_request("POST", &format!("/api/v1/sessions/{id}/archive"), None);
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST archive request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "POST archive should return 204 No Content"
    );

    // Archived sessions are non-retrievable via GET: same semantics as DELETE (#1251).
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("GET archived session request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET on archived session should return 404"
    );
}

#[tokio::test]
async fn archive_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent/archive", None);
    let resp = app
        .oneshot(req)
        .await
        .expect("POST archive unknown session request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "POST archive on unknown session should return 404"
    );
}

#[tokio::test]
async fn create_session_unknown_nous_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "nonexistent-agent",
            "session_key": "test"
        })),
    );

    let resp = app
        .oneshot(req)
        .await
        .expect("POST session with unknown nous request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "creating session with unknown nous should return 404"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "nous_not_found",
        "error code should indicate nous not found"
    );
}

#[tokio::test]
async fn create_duplicate_session_key_returns_409() {
    // POST /api/v1/sessions with an existing session_key must return 409 (#1249).
    let (router, _dir) = app().await;

    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "duplicate-key"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("first session creation request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "first session creation should return 201 Created"
    );

    // Second request with same key must conflict.
    let req2 = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "duplicate-key"
        })),
    );
    let resp2 = router
        .clone()
        .oneshot(req2)
        .await
        .expect("second session creation request should succeed");
    assert_eq!(
        resp2.status(),
        StatusCode::CONFLICT,
        "duplicate session key should return 409 Conflict"
    );
    let body = body_json(resp2).await;
    assert_eq!(
        body["error"]["code"], "conflict",
        "error code should indicate conflict"
    );
}

#[tokio::test]
async fn send_message_to_archived_session_returns_409() {
    // POST /api/v1/sessions/{id}/messages on an archived session must return 409 (#1250).
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    // Archive the session first.
    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("DELETE session request should succeed");
    assert_eq!(
        del.status(),
        StatusCode::NO_CONTENT,
        "DELETE session should return 204 No Content"
    );

    // Sending a message to the archived session must be rejected.
    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST message to archived session request should succeed");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "sending message to archived session should return 409 Conflict"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "conflict",
        "error code should indicate conflict"
    );
}

// ── Response shape ──────────────────────────────────────────────────────────

#[tokio::test]
async fn session_response_has_all_expected_fields() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;

    assert!(
        session["id"].is_string(),
        "session response should contain a string id field"
    );
    assert!(
        session["nous_id"].is_string(),
        "session response should contain a string nous_id field"
    );
    assert!(
        session["session_key"].is_string(),
        "session response should contain a string session_key field"
    );
    assert!(
        session["status"].is_string(),
        "session response should contain a string status field"
    );
    assert!(
        session["message_count"].is_number(),
        "session response should contain a numeric message_count field"
    );
    assert!(
        session["token_count_estimate"].is_number(),
        "session response should contain a numeric token_count_estimate field"
    );
    assert!(
        session["created_at"].is_string(),
        "session response should contain a string created_at field"
    );
    assert!(
        session["updated_at"].is_string(),
        "session response should contain a string updated_at field"
    );
}

// ── History ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn history_empty_for_new_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("GET session history request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET session history should return 200 OK"
    );
    let body = body_json(resp).await;
    assert!(
        body["messages"]
            .as_array()
            .expect("response should contain a messages array")
            .is_empty(),
        "history for a new session should be empty"
    );
}

#[tokio::test]
async fn history_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent/history"))
        .await
        .expect("GET history for unknown session request should succeed");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "GET history for unknown session should return 404"
    );
}

#[tokio::test]
async fn history_with_limit() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    {
        let store = state.session_store.lock().await;
        for i in 1..=5 {
            store
                .append_message(
                    id,
                    aletheia_mneme::types::Role::User,
                    &format!("message {i}"),
                    None,
                    None,
                    10,
                )
                .expect("appending message to session store should succeed");
        }
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?limit=3"
        )))
        .await
        .expect("GET session history with limit request should succeed");

    let body = body_json(resp).await;
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("response should contain a messages array")
            .len(),
        3,
        "history with limit=3 should return exactly 3 messages"
    );
}

#[tokio::test]
async fn history_before_filter() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

    {
        let store = state.session_store.lock().await;
        for i in 1..=5 {
            store
                .append_message(
                    id,
                    aletheia_mneme::types::Role::User,
                    &format!("message {i}"),
                    None,
                    None,
                    10,
                )
                .expect("appending message to session store should succeed");
        }
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?before=3"
        )))
        .await
        .expect("GET session history with before filter request should succeed");

    let body = body_json(resp).await;
    let messages = body["messages"]
        .as_array()
        .expect("response should contain a messages array");
    assert!(
        messages.iter().all(|m| m["seq"]
            .as_i64()
            .expect("message seq should be a valid i64")
            < 3),
        "all returned messages should have seq less than the before filter value"
    );
}

#[tokio::test]
async fn history_messages_have_expected_fields() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have a string id");

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
            .expect("appending message to session store should succeed");
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("GET session history request should succeed");

    let body = body_json(resp).await;
    let msg = &body["messages"][0];
    assert!(
        msg["id"].is_number(),
        "message should have a numeric id field"
    );
    assert!(
        msg["seq"].is_number(),
        "message should have a numeric seq field"
    );
    assert!(
        msg["role"].is_string(),
        "message should have a string role field"
    );
    assert!(
        msg["content"].is_string(),
        "message should have a string content field"
    );
    assert!(
        msg["created_at"].is_string(),
        "message should have a string created_at field"
    );
}
