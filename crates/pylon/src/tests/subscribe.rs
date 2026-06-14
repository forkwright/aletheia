//! Integration tests for the domain-event subscription endpoint.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use tokio_stream::StreamExt;
use tower::ServiceExt;

use crate::event_bus::{DISCOVERABLE_TOPICS, DomainEvent};

use super::helpers::*;

/// Collect SSE body chunks for up to `duration`, returning the concatenated bytes.
async fn collect_sse_chunks(body: axum::body::Body, duration: Duration) -> Vec<u8> {
    let mut stream = body.into_data_stream();
    let deadline = tokio::time::Instant::now() + duration;
    let mut collected = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => collected.extend_from_slice(&chunk),
            _ => break,
        }
    }
    collected
}

#[tokio::test]
async fn subscribe_returns_matching_events() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=fact.created");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    state.event_bus.publish(DomainEvent::new(
        "fact.created",
        serde_json::json!({"fact_id": "f-1", "nous_id": "syn"}),
    ));

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: fact.created"),
        "expected fact.created event, got: {text}"
    );
    assert!(text.contains("f-1"), "expected fact_id in payload");
}

#[tokio::test]
async fn subscribe_filters_unwanted_topics() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=turn.complete");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    state.event_bus.publish(DomainEvent::new(
        "fact.created",
        serde_json::json!({"fact_id": "f-2", "nous_id": "syn"}),
    ));

    let bytes = collect_sse_chunks(body, Duration::from_secs(1)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains("event: fact.created"),
        "should not receive fact.created when subscribed to turn.complete"
    );
}

#[tokio::test]
async fn subscribe_heartbeat_keeps_connection_alive() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=fact.created");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(": heartbeat"),
        "expected heartbeat comment in SSE stream, got: {text}"
    );
}

#[tokio::test]
async fn subscribe_auth_required() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            axum::http::Request::get("/api/v1/events/subscribe?topics=fact.created")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn discovery_returns_current_pylon_topics() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/discovery");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let topics = body
        .as_array()
        .expect("discovery should return a JSON array");
    let returned: std::collections::HashSet<_> = topics
        .iter()
        .map(|v| v.as_str().expect("topic should be a string"))
        .collect();
    let expected: std::collections::HashSet<_> = DISCOVERABLE_TOPICS.iter().copied().collect();

    assert_eq!(returned, expected);

    // WHY: These topics were previously advertised but have no current pylon
    // publisher, so they must not appear in discovery.
    assert!(
        !returned.contains("session.started"),
        "session.started has no current pylon publisher"
    );
    assert!(
        !returned.contains("session.ended"),
        "session.ended has no current pylon publisher"
    );
}

#[tokio::test]
async fn subscribe_surfaces_lag_as_sse_control_event() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=fact.created");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    // WHY: A subscriber that does not poll while the broadcast ring overflows
    // must be informed that messages were dropped, rather than silently
    // skipping them.
    for i in 0..257 {
        state.event_bus.publish(DomainEvent::new(
            "fact.created",
            serde_json::json!({"fact_id": format!("f-{i}")}),
        ));
    }

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: stream_lagged"),
        "expected stream_lagged control event, got: {text}"
    );
    assert!(
        text.contains("dropped"),
        "expected dropped count in lag payload, got: {text}"
    );
}
