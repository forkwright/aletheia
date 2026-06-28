#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;

use axum::http::StatusCode;
use tower::ServiceExt;
use tracing::Instrument;

use super::helpers::*;

#[test]
fn skene_session_lifecycle_values_match_backend_session_statuses() {
    let client_values: Vec<&str> = skene::api::types::SessionLifecycle::ALL
        .iter()
        .map(|status| status.as_str())
        .collect();
    let backend_values: Vec<&str> = mneme::types::SessionStatus::ALL
        .iter()
        .map(|status| status.as_str())
        .collect();

    assert_eq!(client_values, backend_values);
    assert!(!client_values.contains(&"idle"));
}

#[tokio::test]
async fn create_session_returns_201() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;
    assert!(session["id"].is_string());
    assert_eq!(session["nous_id"], "syn");
    assert_eq!(session["session_key"], "test-session");
    assert_eq!(session["status"], "active");
}

#[tokio::test]
async fn get_session_returns_created_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], id);
    assert_eq!(body["nous_id"], "syn");
}

#[tokio::test]
async fn get_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn close_session_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn get_archived_session_returns_404() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let del = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);

    // WHY: archived sessions must not be visible via normal GET (#3196).
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn close_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_delete("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
                let resp = router.oneshot(req).await.unwrap();
                resp.status()
            }
            .instrument(tracing::info_span!("test_session_create", index = i)),
        ));
    }

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
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
    assert_eq!(first.status(), StatusCode::NO_CONTENT);

    let second = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("second close");
    assert_eq!(second.status(), StatusCode::NO_CONTENT);

    // WHY: archived sessions must not be visible via normal GET (#3196).
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get after double close");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], id);
    assert_eq!(body["status"], "active");
    assert_eq!(body["nous_id"], "syn");
}

#[tokio::test]
async fn list_sessions_returns_empty_initially() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/sessions")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["items"].is_array());
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
        .unwrap();

    let body = body_json(resp).await;
    let sessions = body["items"].as_array().unwrap();
    assert!(!sessions.is_empty());
    assert_eq!(sessions[0]["nous_id"], "syn");
}

#[tokio::test]
async fn list_sessions_filter_by_nous_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    create_test_session(&router).await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nous_id=syn"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let sessions = body["items"].as_array().unwrap();
    assert!(!sessions.is_empty());

    let resp2 = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nous_id=nonexistent"))
        .await
        .unwrap();

    let body2 = body_json(resp2).await;
    let sessions2 = body2["items"].as_array().unwrap();
    assert!(sessions2.is_empty());
}

#[tokio::test]
async fn list_sessions_search_and_status_filters() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let active_req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "alpha-session"
        })),
    );
    let active_resp = router.clone().oneshot(active_req).await.unwrap();
    assert_eq!(active_resp.status(), StatusCode::CREATED);

    let archived_req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "beta-session"
        })),
    );
    let archived_resp = router.clone().oneshot(archived_req).await.unwrap();
    assert_eq!(archived_resp.status(), StatusCode::CREATED);
    let archived = body_json(archived_resp).await;
    let archived_id = archived["id"].as_str().unwrap().to_owned();

    let close_resp = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{archived_id}")))
        .await
        .unwrap();
    assert_eq!(close_resp.status(), StatusCode::NO_CONTENT);

    let search_resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?search=alpha"))
        .await
        .unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = body_json(search_resp).await;
    let search_items = search_body["items"].as_array().unwrap();
    assert_eq!(search_items.len(), 1);
    assert_eq!(search_items[0]["session_key"], "alpha-session");

    let status_resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?status=archived"))
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body = body_json(status_resp).await;
    let status_items = status_body["items"].as_array().unwrap();
    assert_eq!(status_items.len(), 1);
    assert_eq!(status_items[0]["session_key"], "beta-session");
    assert_eq!(status_items[0]["status"], "archived");

    let combined_resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?search=beta&status=archived"))
        .await
        .unwrap();
    assert_eq!(combined_resp.status(), StatusCode::OK);
    let combined_body = body_json(combined_resp).await;
    let combined_items = combined_body["items"].as_array().unwrap();
    assert_eq!(combined_items.len(), 1);
    assert_eq!(combined_items[0]["session_key"], "beta-session");
}

#[tokio::test]
async fn list_sessions_limit_param_returns_n_sessions() {
    // NOTE: GET /api/v1/sessions?limit=N must return exactly N sessions (#1254).
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    for i in 0..5_u32 {
        let req = authed_request(
            "POST",
            "/api/v1/sessions",
            Some(serde_json::json!({
                "nous_id": "syn",
                "session_key": format!("limit-test-{i}")
            })),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    let resp = router
        .oneshot(authed_get("/api/v1/sessions?limit=3"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(
        body["items"].as_array().unwrap().len(),
        3,
        "limit=3 must return exactly 3 sessions"
    );
}

#[tokio::test]
async fn archive_via_post_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request("POST", &format!("/api/v1/sessions/{id}/archive"), None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // WHY: archived sessions must not be visible via normal GET (#3196).
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn archive_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent/archive", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

#[tokio::test]
async fn create_duplicate_session_key_returns_409() {
    // NOTE: POST /api/v1/sessions with an existing session_key must return 409 (#1249).
    let (router, _dir) = app().await;

    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "duplicate-key"
        })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let req2 = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "duplicate-key"
        })),
    );
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
    let body = body_json(resp2).await;
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn send_message_to_archived_session_returns_409() {
    // NOTE: POST /api/v1/sessions/{id}/messages on an archived session must return 409 (#1250).
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
    assert_eq!(body["error"]["code"], "conflict");
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
async fn history_empty_for_new_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn history_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent/history"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_with_limit() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
        for i in 1..=5 {
            store
                .append_message(
                    id,
                    mneme::types::Role::User,
                    &format!("message {i}"),
                    None,
                    None,
                    10,
                )
                .unwrap();
        }
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?limit=3"
        )))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn history_before_filter() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
        for i in 1..=5 {
            store
                .append_message(
                    id,
                    mneme::types::Role::User,
                    &format!("message {i}"),
                    None,
                    None,
                    10,
                )
                .unwrap();
        }
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?before=3"
        )))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["seq"].as_i64().unwrap() < 3));
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
            .append_message(id, mneme::types::Role::User, "test message", None, None, 10)
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
async fn create_session_empty_nous_id_returns_422() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "",
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "nous_id" && e["code"] == "required")
    );
}

#[tokio::test]
async fn create_session_empty_session_key_returns_422() {
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

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "session_key" && e["code"] == "required")
    );
}

#[tokio::test]
async fn create_session_oversized_nous_id_returns_422() {
    let (app, _dir) = app().await;
    let oversized_nous_id = "a".repeat(300);
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": oversized_nous_id,
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "nous_id" && e["code"] == "too_long")
    );
}

#[tokio::test]
async fn create_session_oversized_session_key_returns_422() {
    let (app, _dir) = app().await;
    let oversized_session_key = "b".repeat(300);
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": oversized_session_key
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "session_key" && e["code"] == "too_long")
    );
}

#[tokio::test]
async fn rename_session_empty_name_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "name" && e["code"] == "required")
    );
}

#[tokio::test]
async fn rename_session_oversized_name_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let oversized_name = "c".repeat(300);
    let req = authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": oversized_name })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "name" && e["code"] == "too_long")
    );
}

#[tokio::test]
async fn rename_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/sessions/nonexistent/name",
        Some(serde_json::json!({ "name": "new name" })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn purge_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("DELETE", "/api/v1/sessions/nonexistent/purge", None);
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn unarchive_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent/unarchive", None);
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn unarchive_active_session_succeeds() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request("POST", &format!("/api/v1/sessions/{id}/archive"), None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let req = authed_request("POST", &format!("/api/v1/sessions/{id}/unarchive"), None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "active");
}

// ── scope enforcement on session reads ─────────────────────────────────────

/// Authed request issued with a token scoped to `scope_nous_id`.
fn scoped_get(uri: &str, scope_nous_id: &str) -> axum::http::Request<axum::body::Body> {
    let token = token_scoped_to(symbolon::types::Role::Operator, scope_nous_id);
    axum::http::Request::get(uri)
        .header(
            "authorization",
            format!("{}{token}", koina::http::BEARER_PREFIX),
        )
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn get_session_rejects_token_scoped_to_a_different_agent() {
    // WHY: GET /api/v1/sessions/{id} loaded by id only — without a scope
    // check, a token scoped to `audit-bot` could read any `syn` session's
    // metadata (created_at, message_count, origin display name).
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(scoped_get(&format!("/api/v1/sessions/{id}"), "audit-bot"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn get_session_accepts_token_scoped_to_matching_agent() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(scoped_get(&format!("/api/v1/sessions/{id}"), "syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn history_rejects_token_scoped_to_a_different_agent() {
    // WHY: regression guard — without the scope check, a token scoped to
    // `audit-bot` could read the full message history of any `syn` session,
    // the most sensitive read in the sessions surface.
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(scoped_get(
            &format!("/api/v1/sessions/{id}/history"),
            "audit-bot",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn history_accepts_token_scoped_to_matching_agent() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(scoped_get(&format!("/api/v1/sessions/{id}/history"), "syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_sessions_rejects_query_filter_outside_scope() {
    // WHY: the handler rejects mismatched explicit ?nous_id filters rather
    // than silently rewriting them, so a scoped caller can never observe
    // other agents' session ids.
    let (router, _dir) = app().await;

    let resp = router
        .clone()
        .oneshot(scoped_get("/api/v1/sessions?nous_id=audit-bot", "syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_sessions_implicitly_filters_to_scope_when_no_query() {
    // WHY: a bare GET /api/v1/sessions from a scoped token must implicitly
    // filter to that scope. Without this filter, the list would enumerate
    // every session for every agent.
    let (router, _dir) = app().await;

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(scoped_get("/api/v1/sessions", "syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let items = body["items"]
        .as_array()
        .expect("paginated response has items array");
    assert_eq!(
        items.len(),
        1,
        "scoped list returns only the in-scope session"
    );
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[0]["nous_id"], "syn");
}
