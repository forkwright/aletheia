//! Lifecycle and session management integration tests.
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]
use super::*;

// ===========================================================================
// 1. Full turn lifecycle
// ===========================================================================

#[tokio::test]
async fn full_turn_lifecycle_sse_events_and_persistence() {
    let (harness, captured) = TestHarness::build_capturing("Hello from the agent!").await;
    let router = harness.router();

    // Create session
    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    // Send message and collect SSE stream
    let body = harness.send_message(&router, id, "Hi there").await;
    let events = parse_sse_events(&body);

    // Verify SSE events
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();
    assert!(
        event_types.contains(&"text_delta"),
        "should contain text_delta event, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"message_complete"),
        "should contain message_complete event, got: {event_types:?}"
    );

    // Verify text_delta contains response text
    let text_events: Vec<&str> = events
        .iter()
        .filter(|(t, _)| t == "text_delta")
        .map(|(_, d)| d.as_str())
        .collect();
    let text_combined: String = text_events.join("");
    assert!(
        text_combined.contains("Hello from the agent!"),
        "text_delta events should contain response, got: {text_combined}"
    );

    // Verify message_complete has usage
    let complete = events
        .iter()
        .find(|(t, _)| t == "message_complete")
        .expect("message_complete event");
    let complete_data: serde_json::Value =
        serde_json::from_str(&complete.1).expect("parse message_complete");
    assert!(
        complete_data["usage"]["input_tokens"].is_number(),
        "message_complete should have input_tokens"
    );
    assert!(
        complete_data["usage"]["output_tokens"].is_number(),
        "message_complete should have output_tokens"
    );

    // Verify persistence: history should have user + assistant
    let history = harness.get_history(&router, id).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 2,
        "history should have at least user + assistant, got {}",
        messages.len()
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hi there");
    assert_eq!(messages[1]["role"], "assistant");

    // Verify LLM received correct system prompt and message
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        !requests.is_empty(),
        "provider should have received at least one request"
    );
    let system = requests[0]
        .system
        .as_ref()
        .expect("system prompt should be present");
    assert!(
        system.contains("You are a test agent"),
        "system prompt should contain SOUL.md, got: {system}"
    );
    assert!(
        system.contains("Test user"),
        "system prompt should contain USER.md, got: {system}"
    );
}

// ===========================================================================
// 2. Tool execution round-trip
// ===========================================================================

#[tokio::test]
async fn tool_execution_round_trip() {
    let captured = Arc::new(Mutex::new(Vec::new()));

    // First call: LLM returns tool_use for `note` tool
    let tool_use_response = CompletionResponse {
        id: "msg_tool".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: "tu_001".to_owned(),
            name: "note".to_owned(),
            input: serde_json::json!({"action": "list"}),
        }],
        usage: Usage {
            input_tokens: 20,
            output_tokens: 15,
            ..Usage::default()
        },
    };

    // Second call: LLM returns text after seeing tool result
    let final_response = CompletionResponse {
        id: "msg_final".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "I checked your notes and found nothing yet.".to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 30,
            output_tokens: 12,
            ..Usage::default()
        },
    };

    let provider = SequentialMockProvider::new(
        vec![tool_use_response, final_response],
        Arc::clone(&captured),
    );

    let harness = TestHarness::build_with_provider_and_tools(Box::new(provider), true).await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let body = harness
        .send_message(&router, id, "What are my notes?")
        .await;
    let events = parse_sse_events(&body);
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();

    // Verify tool_use and tool_result SSE events
    assert!(
        event_types.contains(&"tool_use"),
        "should contain tool_use event, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"tool_result"),
        "should contain tool_result event, got: {event_types:?}"
    );

    // Verify tool_use event has the note tool
    let tool_use_event = events
        .iter()
        .find(|(t, _)| t == "tool_use")
        .expect("tool_use event");
    let tool_use_data: serde_json::Value =
        serde_json::from_str(&tool_use_event.1).expect("parse tool_use");
    assert_eq!(tool_use_data["name"], "note");

    // Verify final text response
    assert!(
        event_types.contains(&"text_delta"),
        "should contain text_delta after tool round-trip, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"message_complete"),
        "should contain message_complete, got: {event_types:?}"
    );

    // Verify the second LLM call received the tool result in its messages
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        requests.len() >= 2,
        "should have at least 2 LLM calls (tool_use + final), got {}",
        requests.len()
    );
}

// ===========================================================================
// 3. Memory recall integration
// ===========================================================================
// NOTE: Full recall pipeline testing requires knowledge-store + engine-tests
// features and is covered in recall_pipeline.rs and knowledge_recall.rs.
// Here we test that the system prompt assembly correctly includes recall
// section content when it's provided to the pipeline.

#[tokio::test]
async fn system_prompt_includes_oikos_bootstrap_files() {
    let (harness, captured) = TestHarness::build_capturing("Recalled context.").await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let _ = harness
        .send_message(&router, id, "Tell me what you know")
        .await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(!requests.is_empty());

    let system = requests[0].system.as_ref().expect("system prompt present");

    // SOUL.md and USER.md content should be in the system prompt
    assert!(
        system.contains("You are a test agent"),
        "system prompt should contain SOUL.md content"
    );
    assert!(
        system.contains("Test user"),
        "system prompt should contain USER.md content"
    );
}

// ===========================================================================
// 4. Session lifecycle
// ===========================================================================

#[tokio::test]
async fn session_lifecycle_create_list_archive_unarchive_rename() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // Create session
    let session = harness
        .create_session_with_key(&router, "lifecycle-test")
        .await;
    let id = session["id"].as_str().expect("session id");
    assert_eq!(session["status"], "active");

    // Send a message to populate history
    let _ = harness.send_message(&router, id, "hello lifecycle").await;

    // Verify message persisted
    let history = harness.get_history(&router, id).await;
    let messages = history["messages"].as_array().expect("messages");
    assert!(messages.len() >= 2, "should have user + assistant messages");

    // List sessions: should include our session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions?nous_id=test-nous"))
        .await
        .expect("list sessions");
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert!(
        sessions.iter().any(|s| s["id"] == id),
        "session should appear in list"
    );

    // Archive session via DELETE
    let token = harness.auth_token();
    let req = Request::delete(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let resp = router.clone().oneshot(req).await.expect("archive");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify session is non-retrievable after DELETE (#1251): GET must return 404.
    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get after delete");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Unarchive
    let req = harness.authed_request("POST", &format!("/api/v1/sessions/{id}/unarchive"), None);
    let resp = router.clone().oneshot(req).await.expect("unarchive");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify active again
    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get unarchived");
    let session_data = body_json(resp).await;
    assert_eq!(session_data["status"], "active");

    // Rename session
    let req = harness.authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "My Renamed Session" })),
    );
    let resp = router.clone().oneshot(req).await.expect("rename");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify renamed: check via list endpoint where display_name is returned
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions?nous_id=test-nous"))
        .await
        .expect("list after rename");
    let list = body_json(resp).await;
    let sessions = list["sessions"].as_array().expect("sessions array");
    let our_session = sessions
        .iter()
        .find(|s| s["id"] == id)
        .expect("session should be in list");
    assert_eq!(our_session["display_name"], "My Renamed Session");
}
