#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices valid after asserting subsystem presence"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use koina::disk_space::DiskSpaceMonitor;
use koina::http::API_V1;
use pylon::router::build_router;

mod common;
use common::{TestEnv, bearer, issue_test_token, permissive_security};

/// Find the `disk_space` subsystem record in a `/system/status` response body.
fn disk_space_subsystem(body: &serde_json::Value) -> &serde_json::Value {
    body["subsystems"]
        .as_array()
        .expect("subsystems array")
        .iter()
        .find(|s| s["id"] == "disk_space")
        .expect("disk_space subsystem present")
}

#[tokio::test]
async fn disk_space_subsystem_reports_unknown_when_monitoring_disabled() {
    // WHY(#5128): no `.disk_monitor(..)` on the builder -- mirrors
    // `maintenance.diskSpace.enabled = false`.
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let token = issue_test_token(&env.state);

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("{API_V1}/system/status"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build status request"),
        )
        .await
        .expect("status response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = common::read_body_json(response).await;
    let disk_space = disk_space_subsystem(&body);
    assert_eq!(disk_space["status"], "unknown");
    assert_eq!(disk_space["details"]["config_active"], false);
}

#[tokio::test]
async fn disk_space_subsystem_reports_healthy_before_first_refresh() {
    // WHY: a monitor's cached value starts at u64::MAX (assume space
    // available) until the background poller's first successful refresh --
    // see `DiskSpaceMonitor::new`.
    let monitor = DiskSpaceMonitor::new(1024 * 1024 * 1024, 100 * 1024 * 1024);
    let env = TestEnv::builder()
        .with_actor(true)
        .disk_monitor(monitor)
        .build()
        .await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let token = issue_test_token(&env.state);

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("{API_V1}/system/status"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build status request"),
        )
        .await
        .expect("status response");
    assert_eq!(response.status(), StatusCode::OK);

    let body = common::read_body_json(response).await;
    let disk_space = disk_space_subsystem(&body);
    assert_eq!(disk_space["status"], "healthy");
    assert_eq!(disk_space["details"]["config_active"], true);
    // WHY: no refresh occurred, so no refresh timestamp should be fabricated.
    assert!(disk_space.get("last_success").is_none());
}

#[tokio::test]
async fn disk_space_subsystem_reports_failed_at_critical_and_elevates_the_endpoint() {
    // WHY: thresholds set to u64::MAX make ANY real available-bytes reading
    // classify as Critical -- deterministic regardless of the CI runner's
    // actual free disk space (no real filesystem could ever have u64::MAX
    // bytes available).
    let monitor = DiskSpaceMonitor::new(u64::MAX, u64::MAX);
    monitor
        .refresh(std::path::Path::new("/"))
        .expect("refresh against root filesystem");
    let env = TestEnv::builder()
        .with_actor(true)
        .disk_monitor(monitor)
        .build()
        .await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let token = issue_test_token(&env.state);

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("{API_V1}/system/status"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build status request"),
        )
        .await
        .expect("status response");
    // WHY(#5313): a failed subsystem elevates the whole endpoint's aggregate
    // status and HTTP code -- this is the same contract every other
    // subsystem already honors, not disk-space-specific behavior.
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = common::read_body_json(response).await;
    assert_eq!(body["status"], "failed");
    let disk_space = disk_space_subsystem(&body);
    assert_eq!(disk_space["status"], "failed");
    assert!(
        disk_space["failure_reason"]
            .as_str()
            .expect("failure_reason is a string")
            .contains("critical")
    );
    assert!(disk_space.get("last_failure").is_some());
}
