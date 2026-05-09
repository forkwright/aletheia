//! SSE backpressure test: slow consumer should not cause OOM.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::Duration;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use integration_tests::harness::{TestHarness, body_json};
use tower::ServiceExt;

/// Parse every `data:` line in an SSE body into JSON values.
fn collect_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

#[tokio::test]
async fn slow_consumer_completes_without_oom() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "test-nous",
            "session_key": "backpressure-test"
        })),
    );
    let resp = router.clone().oneshot(req).await.expect("create session");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let session = body_json(resp).await;
    let id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(resp.status(), StatusCode::OK);

    // Simulate a slow consumer: read one frame per second.
    let mut body = resp.into_body();
    let mut collected = Vec::new();
    while let Some(Ok(frame)) = body.frame().await {
        if let Some(chunk) = frame.data_ref() {
            collected.extend_from_slice(chunk);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    let body_str = String::from_utf8(collected).expect("utf8");
    let events = collect_sse_data_events(&body_str);
    assert!(
        events.iter().any(|e| e["type"] == "text_delta"),
        "slow consumer should still receive text_delta, got: {body_str}"
    );
    assert!(
        events.iter().any(|e| e["type"] == "message_complete"),
        "slow consumer should still receive message_complete, got: {body_str}"
    );
}
