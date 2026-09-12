//! Integration tests for meta-insights endpoints.

use axum::http::StatusCode;
use mneme::store::test_support::inject_raw_tool_audit_row;
use mneme::store::{SessionStore, test_support::inject_raw_session_row};
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

/// Error path (#7200): `Role::Readonly` is dashboard-only and must not read
/// per-agent performance metrics, scoped or not.
#[tokio::test]
async fn get_agent_perf_one_readonly_role_returns_403() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_as(
            "/api/v1/metrics/agents/syn",
            symbolon::types::Role::Readonly,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

/// Happy path (#7200): an `Agent`-role token scoped to its own nous may
/// still read its own performance metrics -- the new role floor is
/// additive, not a regression for the documented "own sessions" grant.
#[tokio::test]
async fn get_agent_perf_one_agent_role_scoped_to_self_returns_ok() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_scoped_as(
            "/api/v1/metrics/agents/syn",
            symbolon::types::Role::Agent,
            "syn",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Error path (#7200): an `Agent`-role token scoped to a different nous must
/// not read another agent's performance metrics.
#[tokio::test]
async fn get_agent_perf_one_agent_role_scoped_to_other_returns_403() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get_scoped_as(
            "/api/v1/metrics/agents/syn",
            symbolon::types::Role::Agent,
            "other-nous",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
async fn get_quality_metrics_returns_500_when_session_scan_fails() {
    let session_dir = tempfile::TempDir::new().expect("session store tempdir");
    let store_path = session_dir.path().join("sessions");
    inject_raw_session_row(
        &store_path,
        "ses-corrupt-quality",
        br#"{"session_type":"primary"}"#,
    )
    .expect("raw corrupt session row injected");
    // WHY: `open` now refuses a store with no schema manifest at all
    // (that gate is this PR's feature under test elsewhere) — stamp one
    // so the store opens cleanly and the corrupt row is reached by the
    // scan this test is actually exercising.
    SessionStore::stamp_legacy_schema_manifest(&store_path)
        .expect("legacy schema manifest stamped over injected row");
    let corrupt_store = SessionStore::open(&store_path).expect("corrupt session store opens");

    let (state, _dir) = test_state().await;
    {
        let mut store = state.session_store.lock().await;
        *store = corrupt_store;
    }
    let app = build_router(state, &test_security_config());

    let resp = app
        .oneshot(authed_get("/api/v1/metrics/quality"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn get_tool_stats_survives_a_corrupt_tool_audit_row() {
    // WHY(#7217): unlike `/api/v1/metrics/quality` above (a genuine
    // session-scan failure, which correctly stays a 500), a single
    // malformed `tool_audit` row is not a storage failure -- WHY(#5760
    // precedent) in `get_tool_stats` reserves the 500 for the read itself
    // failing outright, and a corrupt row must surface through
    // `data_unavailable` instead.
    let session_dir = tempfile::TempDir::new().expect("session store tempdir");
    let store_path = session_dir.path().join("sessions");
    inject_raw_tool_audit_row(
        &store_path,
        "00000000000000000001",
        br#"{"id":1,"session_id":"ses-x","nous_id":"alice","turn_seq":1,
             "tool_call_id":"tc-corrupt","tool_name":null,"duration_ms":1,
             "is_error":false,"outcome":"error","result":null,"approval":null,
             "receipt":"","created_at":"2026-09-06T00:00:00.000Z"}"#,
    )
    .expect("raw corrupt tool_audit row injected");
    SessionStore::stamp_legacy_schema_manifest(&store_path)
        .expect("legacy schema manifest stamped over injected row");
    let corrupt_store = SessionStore::open(&store_path).expect("corrupt session store opens");

    let (state, _dir) = test_state().await;
    {
        let mut store = state.session_store.lock().await;
        *store = corrupt_store;
    }
    let app = build_router(state, &test_security_config());

    let resp = app.oneshot(authed_get("/api/tool-stats")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let unavailable = body["data_unavailable"]
        .as_array()
        .expect("data_unavailable array");
    assert!(
        unavailable
            .iter()
            .any(|u| u["metric"] == "tool_audit_corrupt"),
        "corrupt row must be disclosed; body={body}"
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
