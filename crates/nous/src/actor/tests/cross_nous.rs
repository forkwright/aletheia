#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[tokio::test]
async fn cross_nous_message_processes_successfully() {
    let (handle, join, cross_tx, _dir) = spawn_test_actor_with_cross();

    let envelope = CrossNousEnvelope {
        message: CrossNousMessage::new("sender", "test-agent", "Hello cross"),
    };

    cross_tx.send(envelope).await.expect("send cross message");

    let mut session_count = 0;
    for _ in 0..100 {
        let status = handle.status().await.expect("actor should be alive");
        session_count = status.session_count;
        if session_count == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        session_count, 1,
        "cross-nous message should create a session"
    );

    let status = handle.status().await.expect("actor should be idle");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

/// Regression test for #5023: `handle_cross_message` previously called
/// `execute_turn_with_panic_boundary` directly, bypassing `mark_turn_active`
/// and `finalize_turn` entirely. That meant a cross turn never ran
/// `finalize_turn`'s corpus side-effect spawning (extraction, skill
/// analysis, distillation, auto-dream), regardless of configuration —
/// asserted here against extraction, the side effect with no provider or
/// knowledge-store dependency to fixture around.
#[tokio::test]
async fn handle_cross_message_runs_finalize_turn_side_effects() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    let envelope = CrossNousEnvelope {
        message: CrossNousMessage::new("sender", "test-agent", "cross message content"),
    };

    actor
        .handle_cross_message(envelope)
        .await
        .expect("cross message should process");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "finalize_turn's corpus side-effect spawning must run for a cross \
         turn the same way it does for a normal turn"
    );
}

/// Regression test for #5023: a cross turn must be visible to health
/// diagnostics as active work, not read as idle/stuck-and-dead, while it is
/// running (`check_health()` reads `active_turn` for exactly this). A turn
/// that skipped `mark_turn_active` would leave `active_turn` false for the
/// entire duration of a long cross turn. Runs on `pending_providers()`
/// (never completes) inside a spawned task so the assertion can observe
/// shared actor state WHILE the turn is genuinely in flight.
#[tokio::test]
async fn handle_cross_message_marks_turn_active_before_execution() {
    let (mut actor, _tx, _dir) =
        make_test_actor_with_providers(pending_providers(), PipelineConfig::default());
    let active_turn = Arc::clone(&actor.runtime.active_turn);

    let envelope = CrossNousEnvelope {
        message: CrossNousMessage::new("sender", "test-agent", "cross message content"),
    };

    let handle = tokio::spawn(async move {
        let _ = actor.handle_cross_message(envelope).await;
    });

    let mut observed_active = false;
    for _ in 0..200 {
        if active_turn.load(std::sync::atomic::Ordering::Acquire) {
            observed_active = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        observed_active,
        "mark_turn_active must run before turn execution begins, so \
         active_turn is true while a cross turn is genuinely in flight"
    );

    handle.abort();
}

#[tokio::test]
async fn cross_nous_message_survives_pipeline_panic() {
    let (handle, join, cross_tx, _dir) = spawn_panicking_actor_with_cross();

    let envelope = CrossNousEnvelope {
        message: CrossNousMessage::new("sender", "test-agent", "Hello cross"),
    };

    cross_tx.send(envelope).await.expect("send cross message");

    let mut panic_count = 0;
    for _ in 0..100 {
        let status = handle.status().await.expect("actor should be alive");
        panic_count = status.panic_count;
        if panic_count == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        panic_count, 1,
        "cross-nous pipeline panic should be recorded"
    );

    let status = handle
        .status()
        .await
        .expect("actor still alive after panic");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn cross_nous_ask_receives_reply_through_actor() {
    let router = Arc::new(crate::cross::CrossNousRouter::default());
    let (handle, join, _dir) = spawn_test_actor_in_router(Arc::clone(&router)).await;

    let msg = CrossNousMessage::new("sender", "test-agent", "Hello cross")
        .with_reply(Duration::from_secs(5));

    let reply = router.ask(msg).await.expect("ask should succeed");

    assert_eq!(reply.from, "test-agent");
    assert!(
        reply.content.contains("Hello from actor!"),
        "expected actor turn content in reply, got: {}",
        reply.content
    );

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn cross_nous_ask_honors_target_session() {
    let router = Arc::new(crate::cross::CrossNousRouter::default());
    let (handle, join, _dir) = spawn_test_actor_in_router(Arc::clone(&router)).await;

    let first = CrossNousMessage::new("sender", "test-agent", "first")
        .with_target_session("session-a")
        .with_reply(Duration::from_secs(5));
    let second = CrossNousMessage::new("sender", "test-agent", "second")
        .with_target_session("session-b")
        .with_reply(Duration::from_secs(5));

    let reply_a = router.ask(first).await.expect("first ask should succeed");
    let reply_b = router.ask(second).await.expect("second ask should succeed");

    assert_eq!(reply_a.from, "test-agent");
    assert_eq!(reply_b.from, "test-agent");

    let status = handle.status().await.expect("actor should be alive");
    assert_eq!(
        status.session_count, 2,
        "two distinct target sessions should exist"
    );

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn cross_nous_typed_payload_returns_reply() {
    let router = Arc::new(crate::cross::CrossNousRouter::default());
    let (handle, join, _dir) = spawn_test_actor_in_router(Arc::clone(&router)).await;

    let msg = crate::cross::knowledge::verify_message(
        "sender",
        "test-agent",
        "the sky is blue",
        koina::id::NousId::new("sender").expect("valid id"),
        Duration::from_secs(5),
    );

    let reply = router.ask(msg).await.expect("typed ask should succeed");

    assert_eq!(reply.from, "test-agent");
    assert!(
        reply.content.contains("verify acknowledged"),
        "expected typed handler reply, got: {}",
        reply.content
    );

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}
