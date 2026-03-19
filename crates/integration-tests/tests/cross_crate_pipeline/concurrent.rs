//! Concurrent session isolation integration tests.
use super::*;

// ===========================================================================
// 7. Concurrent sessions
// ===========================================================================

#[tokio::test]
async fn concurrent_sessions_isolated() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // Create two sessions with different keys
    let session_a = harness.create_session_with_key(&router, "session-a").await;
    let session_b = harness.create_session_with_key(&router, "session-b").await;
    let id_a = session_a["id"].as_str().expect("session a id");
    let id_b = session_b["id"].as_str().expect("session b id");

    // Send different messages to each session concurrently
    let (body_a, body_b) = tokio::join!(
        harness.send_message(&router, id_a, "Message for session A"),
        harness.send_message(&router, id_b, "Message for session B"),
    );

    // Both should complete successfully
    assert!(
        body_a.contains("event: message_complete"),
        "session A should complete"
    );
    assert!(
        body_b.contains("event: message_complete"),
        "session B should complete"
    );

    // Verify histories are independent
    let history_a = harness.get_history(&router, id_a).await;
    let history_b = harness.get_history(&router, id_b).await;

    let msgs_a = history_a["messages"].as_array().expect("messages a");
    let msgs_b = history_b["messages"].as_array().expect("messages b");

    // Each should have exactly their own messages
    assert!(
        msgs_a.len() >= 2,
        "session A should have user + assistant messages"
    );
    assert!(
        msgs_b.len() >= 2,
        "session B should have user + assistant messages"
    );

    assert_eq!(
        msgs_a[0]["content"], "Message for session A",
        "session A user message should be its own"
    );
    assert_eq!(
        msgs_b[0]["content"], "Message for session B",
        "session B user message should be its own"
    );

    // Messages should not leak across sessions
    let a_contents: Vec<&str> = msgs_a
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    let b_contents: Vec<&str> = msgs_b
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();

    assert!(
        !a_contents.iter().any(|c| c.contains("session B")),
        "session A should not contain session B messages"
    );
    assert!(
        !b_contents.iter().any(|c| c.contains("session A")),
        "session B should not contain session A messages"
    );
}

#[tokio::test]
async fn concurrent_sessions_multiple_turns_isolated() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session_a = harness.create_session_with_key(&router, "multi-a").await;
    let session_b = harness.create_session_with_key(&router, "multi-b").await;
    let id_a = session_a["id"].as_str().expect("id a");
    let id_b = session_b["id"].as_str().expect("id b");

    // Interleave turns between sessions
    let _ = harness.send_message(&router, id_a, "A-turn-1").await;
    let _ = harness.send_message(&router, id_b, "B-turn-1").await;
    let _ = harness.send_message(&router, id_a, "A-turn-2").await;
    let _ = harness.send_message(&router, id_b, "B-turn-2").await;

    let history_a = harness.get_history(&router, id_a).await;
    let history_b = harness.get_history(&router, id_b).await;

    let msgs_a = history_a["messages"].as_array().expect("messages a");
    let msgs_b = history_b["messages"].as_array().expect("messages b");

    // 2 user + 2 assistant each = 4 messages minimum
    assert!(
        msgs_a.len() >= 4,
        "session A should have at least 4 messages (2 turns), got {}",
        msgs_a.len()
    );
    assert!(
        msgs_b.len() >= 4,
        "session B should have at least 4 messages (2 turns), got {}",
        msgs_b.len()
    );

    // Verify message ordering
    let user_msgs_a: Vec<&str> = msgs_a
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(user_msgs_a, vec!["A-turn-1", "A-turn-2"]);

    let user_msgs_b: Vec<&str> = msgs_b
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(user_msgs_b, vec!["B-turn-1", "B-turn-2"]);
}
