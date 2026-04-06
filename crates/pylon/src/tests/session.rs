#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;

use axum::http::StatusCode;
use tower::ServiceExt;
use tracing::Instrument;

use super::helpers::*;

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
async fn get_deleted_session_returns_404() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

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
    assert_eq!(body["error"]["code"], "session_not_found");
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
    assert!(body["sessions"].is_array());
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
    let sessions = body["sessions"].as_array().unwrap();
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
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());

    let resp2 = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nous_id=nonexistent"))
        .await
        .unwrap();

    let body2 = body_json(resp2).await;
    let sessions2 = body2["sessions"].as_array().unwrap();
    assert!(sessions2.is_empty());
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
        body["sessions"].as_array().unwrap().len(),
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
                    aletheia_mneme::types::Role::User,
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
                    aletheia_mneme::types::Role::User,
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
