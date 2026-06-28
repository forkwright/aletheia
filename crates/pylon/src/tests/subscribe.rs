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

    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "fact.created",
            serde_json::json!({"fact_id": "f-1", "nous_id": "syn"}),
        ))
        .await;

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: fact.created"),
        "expected fact.created event, got: {text}"
    );
    assert!(text.contains("f-1"), "expected fact_id in payload");
}

#[tokio::test]
async fn subscribe_includes_event_id_and_timestamp() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=fact.created");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "fact.created",
            serde_json::json!({"fact_id": "f-1", "nous_id": "syn"}),
        ))
        .await;

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("id: 1"), "expected SSE id field, got: {text}");
    assert!(
        text.contains("\"at\":"),
        "expected event timestamp in payload, got: {text}"
    );
    assert!(
        text.contains("\"id\":1"),
        "expected event id in payload, got: {text}"
    );
}

#[tokio::test]
async fn subscribe_filters_unwanted_topics() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events/subscribe?topics=turn.complete");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "fact.created",
            serde_json::json!({"fact_id": "f-2", "nous_id": "syn"}),
        ))
        .await;

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
    let topics = body["topics"]
        .as_array()
        .expect("discovery should return topic descriptors");
    let returned: std::collections::HashSet<_> = topics
        .iter()
        .map(|v| {
            v["name"]
                .as_str()
                .expect("topic descriptor should have a name")
        })
        .collect();
    let expected: std::collections::HashSet<_> = DISCOVERABLE_TOPICS.iter().copied().collect();

    assert_eq!(returned, expected);
    let credential_topic = topics
        .iter()
        .find(|topic| topic["name"] == "credential.audit")
        .expect("credential audit topic should be discoverable");
    assert_eq!(credential_topic["visibility"], "operator");
    let required = credential_topic["payload_contract"]["required"]
        .as_array()
        .expect("credential topic should advertise required fields");
    assert!(
        required.iter().any(|field| field == "request_id"),
        "credential audit contract must include request_id"
    );
    assert!(
        required.iter().any(|field| field == "runtime_effect"),
        "credential audit contract must include runtime_effect"
    );

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
        state
            .event_bus
            .publish(DomainEvent::new(
                state.event_bus.next_id(),
                "fact.created",
                serde_json::json!({"fact_id": format!("f-{i}")}),
            ))
            .await;
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

#[tokio::test]
async fn subscribe_replays_from_last_event_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // Publish two events before subscribing.
    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "fact.created",
            serde_json::json!({"fact_id": "f-1", "nous_id": "syn"}),
        ))
        .await;
    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "fact.created",
            serde_json::json!({"fact_id": "f-2", "nous_id": "syn"}),
        ))
        .await;

    // Reconnect after event id 1: only event 2 should be replayed.
    let req = authed_get("/api/v1/events/subscribe?topics=fact.created&last_event_id=1");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("\"fact_id\":\"f-2\""),
        "expected replay of event 2, got: {text}"
    );
    assert!(
        !text.contains("\"fact_id\":\"f-1\""),
        "event 1 should not be replayed, got: {text}"
    );
}

#[tokio::test]
async fn subscribe_reports_gap_for_too_old_last_event_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // WHY: The test EventBus has capacity 256. Publish 258 events so the
    // journal evicts id 1 and 2, making last_event_id=1 unrecoverable.
    for i in 0..258 {
        state
            .event_bus
            .publish(DomainEvent::new(
                state.event_bus.next_id(),
                "fact.created",
                serde_json::json!({"fact_id": format!("f-{i}")}),
            ))
            .await;
    }

    let req = authed_get("/api/v1/events/subscribe?topics=fact.created&last_event_id=1");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: stream_gap"),
        "expected stream_gap control event, got: {text}"
    );
    assert!(
        text.contains("first_missed_id"),
        "expected gap metadata, got: {text}"
    );
}

#[tokio::test]
async fn subscribe_scoped_gap_withholds_missed_id_range() {
    // SECURITY(#5341, #4994, #4617): a scoped token that reconnects past the
    // journal must learn it missed events (the typed `stream_gap` control event)
    // but must NOT receive the raw `first_missed_id`/`last_missed_id` range —
    // that range spans cross-agent events whose existence and volume the scoped
    // token may never observe. The gap object is emitted empty (`{}`).
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // WHY: capacity 256; publish 258 scoped events so id 1 is evicted and a
    // reconnect at last_event_id=1 is unrecoverable. Each carries the scope's
    // nous_id so the scoped token is permitted to subscribe at all.
    for i in 0..258 {
        state
            .event_bus
            .publish(DomainEvent::new(
                state.event_bus.next_id(),
                "fact.created",
                serde_json::json!({"fact_id": format!("f-{i}"), "nous_id": "syn"}),
            ))
            .await;
    }

    let req = authed_get_scoped_as(
        "/api/v1/events/subscribe?topics=fact.created&last_event_id=1",
        symbolon::types::Role::Operator,
        "syn",
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = collect_sse_chunks(resp.into_body(), Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: stream_gap"),
        "scoped reconnect must still signal the gap, got: {text}"
    );
    assert!(
        !text.contains("first_missed_id"),
        "scoped gap must NOT leak the missed-id range, got: {text}"
    );
    assert!(
        !text.contains("last_missed_id"),
        "scoped gap must NOT leak the missed-id range, got: {text}"
    );
}
