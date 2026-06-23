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
