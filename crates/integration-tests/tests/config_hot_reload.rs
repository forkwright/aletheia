//! Config hot-reload integration test: verify `config_tx` broadcasts changes.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::Duration;

use integration_tests::harness::TestHarness;

#[tokio::test]
async fn config_tx_broadcasts_updated_config() {
    let harness = TestHarness::build().await;

    // Subscribe to config changes via the public config_tx field.
    let mut rx = harness.state.config_tx.subscribe();
    let initial = rx.borrow_and_update().clone();

    // Build a mutated config (change a field we can observe).
    let mut new_config = initial.clone();
    new_config.gateway.port = 9999;

    // Broadcast the change.
    harness
        .state
        .config_tx
        .send(new_config.clone())
        .expect("send config");

    // Wait for the subscriber to see the new value.
    let timeout = tokio::time::timeout(Duration::from_secs(5), rx.changed());
    timeout
        .await
        .expect("timed out waiting for config change")
        .expect("config_tx channel closed");

    let updated = rx.borrow_and_update().clone();
    assert_eq!(
        updated.gateway.port, 9999,
        "subscriber should receive updated config port"
    );
}
