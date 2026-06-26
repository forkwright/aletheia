#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tokio::sync::mpsc;

use super::*;
use super::{
    claims, collect_sse_response, parse_sse_data_events, reconnect_path, reconnect_path_for,
    reconnect_running_test_state, reconnect_running_test_state_for, reconnect_test_state,
    response_body,
};

#[tokio::test]
async fn reconnect_turn_rejects_cross_nous_scoped_caller() {
    let (state, _tmp) = reconnect_test_state().await;

    let blocked = reconnect_turn(
        axum::extract::State(state.clone()),
        claims(Role::Agent, Some("nous-b")),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    let Err(err) = blocked else {
        panic!("cross-nous agent reconnect must be rejected");
    };
    assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);

    let blocked = reconnect_turn(
        axum::extract::State(state.clone()),
        claims(Role::Operator, Some("nous-b")),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    let Err(err) = blocked else {
        panic!("cross-nous reconnect must be rejected");
    };
    assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);

    let allowed = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await;
    assert!(allowed.is_ok(), "operator reconnect should succeed");
}

#[tokio::test]
async fn reconnect_turn_receives_message_complete_after_live_wait() {
    let (state, _tmp, handle) = reconnect_running_test_state().await;

    let reconnect = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await
    .expect("reconnect");
    let body_task = tokio::spawn(collect_sse_response(reconnect.into_response()));

    tokio::time::sleep(Duration::from_millis(25)).await;
    handle
        .record(
            "message_complete",
            r#"{"type":"message_complete","stop_reason":"end_turn"}"#,
        )
        .await;
    handle.mark_completed().await;

    let body = body_task.await.expect("body task");
    assert!(body.contains("event: text_delta"), "body: {body}");
    assert!(body.contains("event: message_complete"), "body: {body}");
    assert!(body.contains("end_turn"), "body: {body}");
}

#[tokio::test]
async fn reconnect_turn_receives_error_after_live_wait() {
    let (state, _tmp, handle) = reconnect_running_test_state().await;

    let reconnect = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await
    .expect("reconnect");
    let body_task = tokio::spawn(collect_sse_response(reconnect.into_response()));

    tokio::time::sleep(Duration::from_millis(25)).await;
    handle
        .record(
            "error",
            r#"{"type":"error","code":"turn_failed","message":"failed"}"#,
        )
        .await;
    handle.mark_failed().await;

    let body = body_task.await.expect("body task");
    assert!(body.contains("event: text_delta"), "body: {body}");
    assert!(body.contains("event: error"), "body: {body}");
    assert!(body.contains("turn_failed"), "body: {body}");
}

// ── reconnect lifecycle event ──

#[tokio::test]
async fn reconnect_completed_buffer_reports_completed_replay_only() {
    let (state, _tmp) = reconnect_test_state().await;
    let sse = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path(),
    )
    .await
    .expect("reconnect should succeed");
    let body = response_body(sse).await;
    let events = parse_sse_data_events(&body);
    assert!(
        !events.is_empty(),
        "reconnect must emit at least the lifecycle event"
    );
    let lifecycle = events.first().expect("lifecycle event");
    assert_eq!(
        lifecycle.get("type").and_then(serde_json::Value::as_str),
        Some("turn_reconnect_state")
    );
    assert_eq!(
        lifecycle.get("state").and_then(serde_json::Value::as_str),
        Some("completed")
    );
    assert_eq!(
        lifecycle.get("live").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(
        events
            .iter()
            .any(|e| { e.get("type").and_then(serde_json::Value::as_str) == Some("text_delta") }),
        "completed reconnect must still replay buffered events"
    );
}

#[tokio::test]
async fn reconnect_failed_buffer_reports_failed_replay_only() {
    let (state, _tmp) = reconnect_test_state().await;
    let buffer = state
        .turn_buffer_registry
        .get_or_create("ses-a", "turn-failed")
        .await;
    let handle = TurnBufferHandle::new(buffer);
    handle
        .record(
            "error",
            r#"{"type":"error","code":"turn_failed","message":"synthetic failure"}"#,
        )
        .await;
    handle.mark_failed().await;

    let sse = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path_for("turn-failed"),
    )
    .await
    .expect("reconnect should succeed");
    let body = response_body(sse).await;
    let events = parse_sse_data_events(&body);
    assert!(
        !events.is_empty(),
        "reconnect must emit at least the lifecycle event"
    );
    let lifecycle = events.first().expect("lifecycle event");
    assert_eq!(
        lifecycle.get("type").and_then(serde_json::Value::as_str),
        Some("turn_reconnect_state")
    );
    assert_eq!(
        lifecycle.get("state").and_then(serde_json::Value::as_str),
        Some("failed")
    );
    assert_eq!(
        lifecycle.get("live").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(
        events
            .iter()
            .any(|e| e.get("type").and_then(serde_json::Value::as_str) == Some("error")),
        "failed reconnect must replay buffered error events"
    );
}

#[tokio::test]
async fn reconnect_running_buffer_reports_running_live_and_streams_later_event() {
    let (state, handle, _tmp) = reconnect_running_test_state_for("turn-running").await;
    let state_for_reconnect = state;
    let handle_for_producer = handle.clone();

    let reconnect = tokio::spawn(async move {
        let sse = reconnect_turn(
            axum::extract::State(state_for_reconnect),
            claims(Role::Operator, None),
            HeaderMap::new(),
            reconnect_path_for("turn-running"),
        )
        .await
        .expect("reconnect should succeed");
        response_body(sse).await
    });

    // WHY(#5165): let the reconnect stream enter the live-wait loop before
    // publishing the next synthetic event.
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle_for_producer
        .record("text_delta", r#"{"type":"text_delta","text":"second"}"#)
        .await;
    handle_for_producer.mark_completed().await;

    let body = tokio::time::timeout(Duration::from_secs(2), reconnect)
        .await
        .expect("reconnect should finish before timeout")
        .expect("reconnect task should not panic");
    let events = parse_sse_data_events(&body);
    assert!(
        !events.is_empty(),
        "reconnect must emit at least the lifecycle event"
    );
    let lifecycle = events.first().expect("lifecycle event");
    assert_eq!(
        lifecycle.get("type").and_then(serde_json::Value::as_str),
        Some("turn_reconnect_state")
    );
    assert_eq!(
        lifecycle.get("state").and_then(serde_json::Value::as_str),
        Some("running")
    );
    assert_eq!(
        lifecycle.get("live").and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let deltas: Vec<_> = events
        .iter()
        .filter(|e| e.get("type").and_then(serde_json::Value::as_str) == Some("text_delta"))
        .collect();
    assert_eq!(
        deltas.len(),
        2,
        "running reconnect must replay the buffered delta and then stream the later delta"
    );
    let first = deltas.first().expect("first text delta");
    let second = deltas.get(1).expect("second text delta");
    assert_eq!(
        first.get("text").and_then(serde_json::Value::as_str),
        Some("first")
    );
    assert_eq!(
        second.get("text").and_then(serde_json::Value::as_str),
        Some("second")
    );
}

// ── IdempotencyGuard ──

#[test]
fn idempotency_guard_releases_in_flight_on_drop() {
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "drop-key".to_owned();
    let session_id = "session-a".to_owned();
    let body_fingerprint = send_message_body_fingerprint("Hello!");

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
            LookupResult::Miss
        ),
        "precondition: key must be inserted"
    );

    {
        let guard = IdempotencyGuard::new(
            Arc::clone(&cache),
            principal.clone(),
            key.clone(),
            session_id.clone(),
            body_fingerprint.clone(),
        );
        assert!(
            matches!(
                cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
                LookupResult::Conflict { .. }
            ),
            "key must still be in flight while the guard lives"
        );
        drop(guard);
    }

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
            LookupResult::Miss
        ),
        "dropping the guard must release the in-flight key"
    );
}

#[test]
fn idempotency_guard_preserves_completed_entry() {
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "complete-key".to_owned();
    let session_id = "session-a".to_owned();
    let body_fingerprint = send_message_body_fingerprint("Hello!");

    assert!(matches!(
        cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
        LookupResult::Miss
    ));
    cache.complete(
        &principal,
        &key,
        &session_id,
        &body_fingerprint,
        "turn-complete-key",
        axum::http::StatusCode::OK,
        r#"{"ok":true}"#.to_owned(),
    );

    {
        let guard = IdempotencyGuard::new(
            Arc::clone(&cache),
            principal.clone(),
            key.clone(),
            session_id.clone(),
            body_fingerprint.clone(),
        );
        guard.mark_completed();
    }

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
            LookupResult::Hit { .. }
        ),
        "mark_completed must prevent the guard from deleting a finished entry"
    );
}

#[test]
fn idempotency_guard_shared_completion_flag() {
    // WHY(#5453): The stream-side and task-side guards share one completion
    // flag so marking the turn completed in the task prevents the stream-side
    // guard from cleaning up after a normal response drop.
    let cache = Arc::new(crate::idempotency::IdempotencyCache::new());
    let principal = "alice".to_owned();
    let key = "shared-key".to_owned();
    let session_id = "session-a".to_owned();
    let body_fingerprint = send_message_body_fingerprint("Hello!");

    assert!(matches!(
        cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
        LookupResult::Miss
    ));

    let (task_guard, stream_guard) = IdempotencyGuard::new_pair(
        Arc::clone(&cache),
        principal.clone(),
        key.clone(),
        session_id.clone(),
        body_fingerprint.clone(),
    );

    task_guard.mark_completed();
    drop(stream_guard);

    assert!(
        matches!(
            cache.check_or_insert(&principal, &key, &session_id, &body_fingerprint),
            LookupResult::Conflict { .. }
        ),
        "shared completion flag must keep the in-flight entry intact"
    );
}

#[test]
fn turn_complete_event_payload_includes_cache_tokens() {
    let result = nous::pipeline::TurnResult {
        content: "cache-enabled response".to_owned(),
        tool_calls: Vec::new(),
        usage: nous::pipeline::TurnUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 1000,
            cache_write_tokens: 200,
            ..nous::pipeline::TurnUsage::default()
        },
        signals: Vec::new(),
        stop_reason: "end_turn".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        tool_surface_hashes: Vec::new(),
    };

    let payload = turn_complete_event_payload("ses-1", "nous-1", "turn-1", &result);

    assert_eq!(
        payload
            .get("cache_read_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(1000)
    );
    assert_eq!(
        payload
            .get("cache_write_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
}

#[tokio::test]
async fn emit_turn_result_events_buffered_includes_cache_tokens() {
    let (_state, _tmp, handle) = reconnect_running_test_state().await;
    let (tx, mut rx) = mpsc::channel::<(u64, SseEvent)>(8);

    let result = nous::pipeline::TurnResult {
        content: "response with cache".to_owned(),
        tool_calls: Vec::new(),
        usage: nous::pipeline::TurnUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 1000,
            cache_write_tokens: 200,
            ..nous::pipeline::TurnUsage::default()
        },
        signals: Vec::new(),
        stop_reason: "end_turn".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        tool_surface_hashes: Vec::new(),
    };

    emit_turn_result_events_buffered(&tx, &handle, &result, Some("req-cache")).await;
    drop(tx);

    let mut complete: Option<serde_json::Value> = None;
    while let Some((_seq, event)) = rx.recv().await {
        if let SseEvent::MessageComplete { usage, .. } = event {
            let json = serde_json::to_value(usage).expect("usage serializes");
            complete = Some(json);
            break;
        }
    }

    let usage = complete.expect("message_complete event must be emitted");
    assert_eq!(
        usage
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(100)
    );
    assert_eq!(
        usage
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(50)
    );
    assert_eq!(
        usage
            .get("cache_read_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(1000)
    );
    assert_eq!(
        usage
            .get("cache_write_tokens")
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
}

// ── Turn abort / reconnect-after-disconnect (#4794) ──

#[tokio::test]
async fn reconnect_after_disconnect_replays_turn_abort_and_reports_aborted() {
    let (state, handle, _tmp) = reconnect_running_test_state_for("turn-abort").await;
    let (tx, _rx) = mpsc::channel::<(u64, SseEvent)>(4);

    // WHY: Exercise the real disconnect path: record a terminal turn_abort event
    // and mark the buffer aborted.
    crate::handlers::sessions::streaming::emit_turn_abort_sse(
        &tx,
        &handle,
        crate::turn_buffer::TURN_ABORT_REASON_CLIENT_DISCONNECT,
        Some("req-disconnect"),
    )
    .await;

    let sse = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path_for("turn-abort"),
    )
    .await
    .expect("reconnect should succeed");
    let body = response_body(sse).await;
    let events = parse_sse_data_events(&body);

    let lifecycle = events.first().expect("lifecycle event");
    assert_eq!(
        lifecycle.get("type").and_then(serde_json::Value::as_str),
        Some("turn_reconnect_state")
    );
    assert_eq!(
        lifecycle.get("state").and_then(serde_json::Value::as_str),
        Some("aborted")
    );
    assert_eq!(
        lifecycle.get("live").and_then(serde_json::Value::as_bool),
        Some(false)
    );

    assert!(
        events.iter().any(|e| {
            e.get("type").and_then(serde_json::Value::as_str) == Some("turn_abort")
                && e.get("reason").and_then(serde_json::Value::as_str)
                    == Some(crate::turn_buffer::TURN_ABORT_REASON_CLIENT_DISCONNECT)
        }),
        "reconnect must replay the buffered turn_abort event: {body}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.get("type").and_then(serde_json::Value::as_str) == Some("text_delta")),
        "reconnect must still replay pre-abort buffered events: {body}"
    );
}

#[tokio::test]
async fn reconnect_after_server_shutdown_replays_turn_abort() {
    let (state, handle, _tmp) = reconnect_running_test_state_for("turn-shutdown").await;
    let (tx, _rx) = mpsc::channel::<(u64, SseEvent)>(4);

    crate::handlers::sessions::streaming::emit_turn_abort_sse(
        &tx,
        &handle,
        crate::turn_buffer::TURN_ABORT_REASON_SERVER_SHUTDOWN,
        Some("req-shutdown"),
    )
    .await;

    let sse = reconnect_turn(
        axum::extract::State(state),
        claims(Role::Operator, None),
        HeaderMap::new(),
        reconnect_path_for("turn-shutdown"),
    )
    .await
    .expect("reconnect should succeed");
    let body = response_body(sse).await;
    let events = parse_sse_data_events(&body);

    assert!(
        events.iter().any(|e| {
            e.get("type").and_then(serde_json::Value::as_str) == Some("turn_abort")
                && e.get("reason").and_then(serde_json::Value::as_str)
                    == Some(crate::turn_buffer::TURN_ABORT_REASON_SERVER_SHUTDOWN)
        }),
        "reconnect must replay server-shutdown turn_abort event: {body}"
    );
}

#[tokio::test]
async fn reconnect_orphaned_running_buffer_times_out_with_turn_abort() {
    tokio::time::pause();
    let (state, _handle, _tmp) = reconnect_running_test_state_for("turn-orphan").await;

    let reconnect = tokio::spawn(async move {
        let sse = reconnect_turn(
            axum::extract::State(state),
            claims(Role::Operator, None),
            HeaderMap::new(),
            reconnect_path_for("turn-orphan"),
        )
        .await
        .expect("reconnect should succeed");
        response_body(sse).await
    });

    // WHY: let the reconnect task enter the live-wait loop before advancing the clock.
    tokio::time::sleep(Duration::from_millis(10)).await;
    tokio::time::advance(Duration::from_mins(6)).await;

    let body = tokio::time::timeout(Duration::from_secs(2), reconnect)
        .await
        .expect("reconnect should finish after max-live timeout")
        .expect("reconnect task should not panic");
    let events = parse_sse_data_events(&body);

    let lifecycle = events.first().expect("lifecycle event");
    assert_eq!(
        lifecycle.get("state").and_then(serde_json::Value::as_str),
        Some("running")
    );
    assert!(
        events.iter().any(|e| {
            e.get("type").and_then(serde_json::Value::as_str) == Some("turn_abort")
                && e.get("reason").and_then(serde_json::Value::as_str)
                    == Some(crate::turn_buffer::TURN_ABORT_REASON_TIMEOUT)
        }),
        "orphaned running reconnect must close with a turn_abort timeout event: {body}"
    );
}
