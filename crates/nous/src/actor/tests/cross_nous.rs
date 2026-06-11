#![expect(clippy::expect_used, reason = "test assertions")]

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
