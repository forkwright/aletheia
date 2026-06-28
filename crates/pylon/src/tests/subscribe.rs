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
async fn subscribe_uses_sse_id_and_payload_data() {
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
        text.contains("data: {\"fact_id\":\"f-1\",\"nous_id\":\"syn\"}"),
        "expected topic payload as SSE data, got: {text}"
    );
    assert!(
        !text.contains("\"payload\""),
        "SSE data must not wrap the payload in a DomainEvent envelope, got: {text}"
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
async fn canonical_events_defaults_to_discoverable_topics() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    state
        .event_bus
        .publish(DomainEvent::new(
            state.event_bus.next_id(),
            "background.progress",
            serde_json::json!({
                "nous_id": "syn",
                "task_type": "distillation",
                "stage": "started",
                "message": "distillation started"
            }),
        ))
        .await;

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: background.progress"),
        "default canonical stream must include background progress, got: {text}"
    );
    assert!(
        text.contains("\"task_type\":\"distillation\""),
        "expected background progress payload, got: {text}"
    );
}

#[tokio::test]
async fn canonical_events_reports_session_created_and_archived_from_handlers() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events?topics=session.created,session.archived");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");
    let archive = router
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/v1/sessions/{id}/archive"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(archive.status(), StatusCode::NO_CONTENT);

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: session.created"),
        "expected session.created from create handler, got: {text}"
    );
    assert!(
        text.contains("event: session.archived"),
        "expected session.archived from archive handler, got: {text}"
    );
    assert!(
        text.contains(id),
        "expected session id in lifecycle payload"
    );
}

fn stream_turn_req(
    session_key: &str,
    message: &str,
    client_turn_id: &str,
) -> axum::http::Request<axum::body::Body> {
    authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "syn",
            "message": message,
            "session_key": session_key,
            "client_turn_id": client_turn_id,
        })),
    )
}

#[tokio::test]
async fn canonical_events_reports_turn_complete_from_stream_handler() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events?topics=turn.complete");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let turn = router
        .clone()
        .oneshot(stream_turn_req(
            "global-turn-complete",
            "complete this streamed turn",
            "01ARZ3NDEKTSV4RRFFQ69G5FB0",
        ))
        .await
        .unwrap();
    assert_eq!(turn.status(), StatusCode::OK);
    let _turn_body = body_string(turn).await;

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: turn.complete"),
        "expected turn.complete from streaming handler, got: {text}"
    );
    assert!(
        text.contains("\"turn_id\":\"01ARZ3NDEKTSV4RRFFQ69G5FB0\""),
        "expected client turn id in payload, got: {text}"
    );
}

#[tokio::test]
async fn canonical_events_reports_turn_error_from_stream_handler() {
    let (state, _dir) = test_state_with_error_provider("simulated provider failure").await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_get("/api/v1/events?topics=turn.error");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();

    let turn = router
        .clone()
        .oneshot(stream_turn_req(
            "global-turn-error",
            "trigger provider failure",
            "01ARZ3NDEKTSV4RRFFQ69G5FB1",
        ))
        .await
        .unwrap();
    assert_eq!(turn.status(), StatusCode::OK);
    let _turn_body = body_string(turn).await;

    let bytes = collect_sse_chunks(body, Duration::from_secs(2)).await;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("event: turn.error"),
        "expected turn.error from streaming handler, got: {text}"
    );
    assert!(
        text.contains("\"code\":\"provider_unavailable\""),
        "expected sanitized turn error code, got: {text}"
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

    for topic in [
        "background.progress",
        "session.archived",
        "session.created",
        "turn.complete",
        "turn.error",
    ] {
        assert!(returned.contains(topic), "discovery must include {topic}");
    }
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
