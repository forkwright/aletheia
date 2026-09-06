//! Integration tests for the daemon-task admin routes (#7206):
//! `GET /api/v1/system/daemon/tasks` and the `enable`/`disable`/`retry`
//! mutations.
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use oikonomos::state::{DisableCause, TaskState, TaskStateStore};

use super::helpers::*;

/// Build a test app with one attached daemon-task store, pre-seeded.
async fn app_with_daemon_tasks(
    component: &str,
    seed: &[TaskState],
) -> (axum::Router, tempfile::TempDir, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let store_dir = tempfile::tempdir().expect("store tempdir");
    let store = TaskStateStore::open(store_dir.path()).expect("open task-state store");
    for task in seed {
        store.save(task).expect("seed task state");
    }

    let mut inner = (*state).clone();
    inner.daemon_task_states = Arc::new(vec![(component.to_owned(), store)]);
    let state = Arc::new(inner);

    (build_router(state, &test_security_config()), dir, store_dir)
}

fn disabled_task(task_id: &str, cause: DisableCause) -> TaskState {
    TaskState {
        task_id: task_id.to_owned(),
        enabled: Some(false),
        disable_cause: Some(cause),
        consecutive_failures: 3,
        last_error: Some("connection refused".to_owned()),
        ..TaskState::default()
    }
}

#[tokio::test]
async fn list_tasks_requires_auth() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks("system", &[]).await;
    let resp = app
        .oneshot(
            Request::get("/api/v1/system/daemon/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_tasks_requires_operator_role() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks("system", &[]).await;
    let resp = app
        .oneshot(authed_get_as(
            "/api/v1/system/daemon/tasks",
            symbolon::types::Role::Readonly,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_tasks_reports_state_and_cause() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks(
        "system",
        &[
            disabled_task("routing-store-refresh", DisableCause::AutoFailure),
            TaskState {
                task_id: "trace-rotation".to_owned(),
                enabled: Some(true),
                ..TaskState::default()
            },
        ],
    )
    .await;

    let resp = app
        .oneshot(authed_get("/api/v1/system/daemon/tasks"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let tasks = body["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 2);

    let refresh = tasks
        .iter()
        .find(|t| t["task_id"] == "routing-store-refresh")
        .expect("routing-store-refresh present");
    assert_eq!(refresh["enabled"], false);
    assert_eq!(refresh["cause"], "auto_failure");
    assert_eq!(refresh["runner"], "system");
    // WHY: known to the maintenance registry, so the human-readable name is
    // resolved even though the task has never run in this test.
    assert_eq!(refresh["name"], "Routing after-action store refresh");

    let rotation = tasks
        .iter()
        .find(|t| t["task_id"] == "trace-rotation")
        .expect("trace-rotation present");
    assert_eq!(rotation["enabled"], true);
    assert!(rotation["cause"].is_null());
}

#[tokio::test]
async fn enable_task_requires_operator_role() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks(
        "system",
        &[disabled_task(
            "routing-store-refresh",
            DisableCause::AutoFailure,
        )],
    )
    .await;
    let resp = app
        .oneshot(authed_request_as(
            "POST",
            "/api/v1/system/daemon/tasks/system/routing-store-refresh/enable",
            None,
            symbolon::types::Role::Readonly,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn enable_task_resets_failure_history() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks(
        "system",
        &[disabled_task(
            "routing-store-refresh",
            DisableCause::AutoFailure,
        )],
    )
    .await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/system/routing-store-refresh/enable",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert!(body["cause"].is_null());
    assert_eq!(body["consecutive_failures"], 0);
    assert!(body["last_error"].is_null());
}

#[tokio::test]
async fn disable_task_persists_operator_cause_and_reason() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks(
        "system",
        &[TaskState {
            task_id: "trace-rotation".to_owned(),
            enabled: Some(true),
            ..TaskState::default()
        }],
    )
    .await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/system/trace-rotation/disable",
            Some(serde_json::json!({ "reason": "noisy during migration" })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["cause"], "operator");
    assert_eq!(body["last_error"], "noisy during migration");
}

#[tokio::test]
async fn retry_task_leaves_failure_count_untouched() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks(
        "system",
        &[disabled_task(
            "routing-store-refresh",
            DisableCause::AutoFailure,
        )],
    )
    .await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/system/routing-store-refresh/retry",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["enabled"], true);
    assert!(body["cause"].is_null());
    // WHY: unlike `enable`, `retry` must not reset the failure count -- a
    // further failure should re-disable after one more strike, not three.
    assert_eq!(body["consecutive_failures"], 3);
    assert_eq!(body["last_error"], "connection refused");
}

#[tokio::test]
async fn unknown_runner_is_not_found() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks("system", &[]).await;
    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/no-such-runner/anything/enable",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_task_on_known_runner_is_not_found() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks("system", &[]).await;
    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/system/not-a-real-task/enable",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// A registry-known task with no persisted history yet (never run, never
/// failed) can still be administered -- disabling it before its first run
/// must not require waiting for it to fail three times first.
#[tokio::test]
async fn disable_task_with_no_persisted_history_creates_a_record() {
    let (app, _dir, _store_dir) = app_with_daemon_tasks("system", &[]).await;
    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/daemon/tasks/system/trace-rotation/disable",
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["cause"], "operator");
}
