//! Integration tests for meta-insights endpoints.

use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::helpers::*;

/// Create a [`SessionStore`] whose `list_sessions` call fails because a session
/// row cannot be deserialized. This lets us assert that `get_quality_metrics`
/// propagates storage errors instead of returning an empty `200 OK`.
fn broken_session_store() -> (mneme::store::SessionStore, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store_path = tmp.path().join("sessions");

    // WHY: write a JSON object with `session_type` so the legacy backfill scan
    // skips it, but omit the required `Session` fields so `list_sessions`
    // returns a `StoredJsonSnafu` error when it deserializes the row.
    {
        let db = fjall::SingleWriterTxDatabase::builder(&store_path)
            .open()
            .expect("open fjall db");
        let ks = db
            .keyspace("sessions", fjall::KeyspaceCreateOptions::default)
            .expect("create sessions keyspace");
        ks.insert("bad-session", br#"{"session_type":"primary"}"#)
            .expect("insert corrupt session row");
    }

    let store = mneme::store::SessionStore::open(&store_path)
        .expect("reopen store with corrupt row");
    (store, tmp)
}

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
async fn list_agent_perf_marks_unbacked_tool_metrics_unavailable() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/agents"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agent = body["agents"]
        .as_array()
        .expect("agents array")
        .first()
        .expect("at least one agent");
    let unavailable = agent["data_unavailable"]
        .as_array()
        .expect("data_unavailable array");
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "tool_calls_per_session"),
        "tool_calls_per_session should be marked unavailable"
    );
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "tool_success_rate"),
        "tool_success_rate should be marked unavailable"
    );
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "errors_per_session"),
        "errors_per_session should be marked unavailable"
    );
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
async fn get_agent_perf_one_marks_unbacked_tool_metrics_unavailable() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/agents/syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let unavailable = body["data_unavailable"]
        .as_array()
        .expect("data_unavailable array");
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "tool_calls_per_session"),
        "tool_calls_per_session should be marked unavailable"
    );
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "tool_success_rate"),
        "tool_success_rate should be marked unavailable"
    );
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "errors_per_session"),
        "errors_per_session should be marked unavailable"
    );
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
    let unavailable = body["data_unavailable"]
        .as_array()
        .expect("data_unavailable array");
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "thinking_time_ratio"),
        "thinking_time_ratio should be marked unavailable"
    );
}

#[tokio::test]
async fn get_quality_metrics_returns_500_on_storage_failure() {
    let (broken_store, _tmp) = broken_session_store();
    let broken_store = Arc::new(Mutex::new(broken_store));

    let (state, _dir) = test_state().await;
    let mut state = Arc::into_inner(state)
        .expect("test state should have a single Arc reference");
    state.session_store = broken_store;
    let state = Arc::new(state);

    let router = build_router(Arc::clone(&state), &test_security_config());
    let resp = router
        .oneshot(authed_get("/api/v1/metrics/quality"))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "storage failure must not be silently swallowed as an empty 200 OK"
    );
}

#[tokio::test]
async fn get_cost_metrics_marks_unbacked_cost_unavailable() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/metrics/costs"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let unavailable = body["data_unavailable"]
        .as_array()
        .expect("data_unavailable array");
    assert!(
        unavailable.iter().any(|u| u["metric"] == "cost"),
        "cost should be marked unavailable"
    );
}

#[tokio::test]
async fn get_journal_returns_empty_when_no_store() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/journal")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let events = body["events"].as_array().expect("events array");
    assert!(events.is_empty());
    assert!(body["data_unavailable"].as_array().is_some());
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
    let events = body["events"].as_array().expect("events array");
    assert!(events.is_empty());
    assert!(body["data_unavailable"].as_array().is_some());
}
