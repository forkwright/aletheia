//! Integration tests for meta-insights endpoints.

use axum::http::StatusCode;
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn list_agent_perf_returns_ok() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/agents"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["agents"].as_array().expect("agents array");
    assert!(!agents.is_empty());
    assert!(body["anomalies"].is_array());
}

#[tokio::test]
async fn get_agent_perf_one_returns_ok() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/agents/syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["agent_id"], "syn");
}

#[tokio::test]
async fn get_agent_perf_one_unknown_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/agents/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_quality_metrics_returns_ok() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/quality"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["series"].is_object());
}

#[tokio::test]
async fn get_quality_metrics_returns_500_when_list_sessions_fails() {
    let (state, _dir) = test_state().await;
    {
        let store = state.session_store.lock().await;
        store
            .inject_test_session_bytes("corrupt:test-session", b"not valid json")
            .expect("inject malformed session bytes");
    }

    let app = build_router(state, &test_security_config());
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/quality"))
        .await
        .unwrap();

    assert!(!resp.status().is_success());
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn get_journal_returns_empty_when_no_store() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/journal")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let events = body.as_array().expect("journal array");
    assert!(events.is_empty());
}

#[tokio::test]
async fn get_journal_with_query_params_returns_empty() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get(
            "/api/v1/journal?source=pylon&level=error&limit=10",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let events = body.as_array().expect("journal array");
    assert!(events.is_empty());
}
