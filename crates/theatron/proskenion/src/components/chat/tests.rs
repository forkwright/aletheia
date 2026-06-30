use skene::api::types::TurnOutcome;
use skene::id::ToolId;

use super::*;

fn make_state() -> ChatState {
    ChatState::default()
}

fn make_manager() -> ChatStateManager {
    ChatStateManager::new()
}

fn make_outcome(text: &str) -> TurnOutcome {
    TurnOutcome {
        text: text.to_string(),
        nous_id: NousId::from("syn"),
        session_id: "s1".into(),
        model: Some("claude-opus-4-6".to_string()),
        tool_calls: 0,
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        stop_reason: None,
        error: None,
    }
}

#[test]
fn turn_start_resets_streaming_state() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let changed = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    assert!(changed);
    assert!(state.streaming.is_streaming);
    assert!(state.streaming.text.is_empty());
    assert_eq!(state.streaming.turn_id.as_deref(), Some("t1"));
}

#[test]
fn text_delta_with_newline_flushes_immediately() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    let changed = mgr.apply(StreamEvent::TextDelta("hello\n".to_string()), &mut state);
    assert!(changed);
    assert_eq!(state.streaming.text, "hello\n");
}

#[test]
fn text_delta_without_newline_buffers() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    // Force the last_flush AND last_delta to be recent so debounce holds.
    let now = Instant::now();
    mgr.last_flush = now;
    mgr.last_delta = now;

    let changed = mgr.apply(StreamEvent::TextDelta("he".to_string()), &mut state);
    // Debounce not elapsed and no newline: should buffer.
    assert!(!changed);
    assert!(state.streaming.text.is_empty());

    // Tick flushes the buffer.
    let flushed = mgr.tick(&mut state);
    assert!(flushed);
    assert_eq!(state.streaming.text, "he");
}

#[test]
fn text_delta_flushes_after_debounce_interval() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    // Set last_flush to 200ms ago AND last_delta also old → slow stream
    // → effective debounce is 50ms, so 200ms > 50ms → flushes.
    mgr.last_flush = Instant::now() - Duration::from_millis(200);
    mgr.last_delta = Instant::now() - Duration::from_millis(600);

    let changed = mgr.apply(StreamEvent::TextDelta("world".to_string()), &mut state);
    assert!(changed);
    assert_eq!(state.streaming.text, "world");
}

#[test]
fn adaptive_debounce_slow_stream_uses_shorter_interval() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    // Simulate a slow stream: last_delta was >500ms ago → debounce is 50ms.
    // last_flush also >50ms ago → should flush immediately on next delta.
    mgr.last_flush = Instant::now() - Duration::from_millis(70);
    mgr.last_delta = Instant::now() - Duration::from_millis(600);

    let changed = mgr.apply(StreamEvent::TextDelta("slow token".to_string()), &mut state);
    // 70ms elapsed > 50ms slow debounce → should flush.
    assert!(changed);
    assert_eq!(state.streaming.text, "slow token");
}

#[test]
fn adaptive_debounce_fast_stream_uses_longer_interval() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    // Simulate a fast stream: last_delta was <500ms ago → debounce is 100ms.
    // last_flush was only 70ms ago → should NOT flush (70ms < 100ms).
    let now = Instant::now();
    mgr.last_flush = now - Duration::from_millis(70);
    mgr.last_delta = now - Duration::from_millis(50);

    let changed = mgr.apply(StreamEvent::TextDelta("fast token".to_string()), &mut state);
    // 70ms elapsed < 100ms fast debounce → should buffer.
    assert!(!changed);
    assert!(state.streaming.text.is_empty());
}

#[test]
fn tool_start_flushes_pending_text() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    // Buffer some text (force recent flush AND recent delta → fast debounce,
    // 0ms elapsed < 100ms → should buffer).
    let now = Instant::now();
    mgr.last_flush = now;
    mgr.last_delta = now;
    let _ = mgr.apply(StreamEvent::TextDelta("partial".to_string()), &mut state);
    assert!(state.streaming.text.is_empty());

    // Tool start forces flush.
    let _ = mgr.apply(
        StreamEvent::ToolStart {
            tool_name: "read_file".to_string(),
            tool_id: ToolId::from("t1"),
            input: None,
        },
        &mut state,
    );
    assert_eq!(state.streaming.text, "partial");
    assert_eq!(state.streaming.tool_calls.len(), 1);
    assert_eq!(state.streaming.tool_calls[0].tool_name, "read_file");
}

#[test]
fn tool_result_updates_existing_tool_call() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::ToolStart {
            tool_name: "exec".to_string(),
            tool_id: ToolId::from("tool-1"),
            input: None,
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::ToolResult {
            tool_name: "exec".to_string(),
            tool_id: ToolId::from("tool-1"),
            is_error: false,
            duration_ms: 250,
            result: None,
        },
        &mut state,
    );

    assert!(state.streaming.tool_calls[0].completed);
    assert_eq!(state.streaming.tool_calls[0].duration_ms, Some(250));
}

#[test]
fn turn_complete_commits_message_to_history() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(StreamEvent::TextDelta("hello\n".to_string()), &mut state);

    let _ = mgr.apply(
        StreamEvent::TurnComplete {
            outcome: make_outcome("hello"),
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].role, MessageRole::Assistant);
    assert_eq!(state.messages[0].content, "hello");
    assert!(!state.streaming.is_streaming);
}

#[test]
fn turn_complete_commits_outcome_text_without_deltas() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::TurnComplete {
            outcome: make_outcome("authoritative final text"),
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].content, "authoritative final text");
    assert_eq!(state.messages[0].input_tokens, 100);
    assert_eq!(state.messages[0].output_tokens, 50);
}

#[test]
fn turn_complete_replaces_incomplete_delta_buffer() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(StreamEvent::TextDelta("partial".to_string()), &mut state);
    let _ = mgr.apply(
        StreamEvent::TurnComplete {
            outcome: make_outcome("partial plus final token"),
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].content, "partial plus final token");
}

#[test]
fn turn_abort_preserves_partial_text() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(StreamEvent::TextDelta("partial\n".to_string()), &mut state);

    let _ = mgr.apply(
        StreamEvent::TurnAbort {
            reason: "cancelled".to_string(),
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 1);
    assert_eq!(
        state.messages[0].content,
        "partial\n\n[turn aborted: cancelled]"
    );
    assert!(!state.streaming.is_streaming);
    assert_eq!(
        state.streaming.error.as_deref(),
        Some("stream aborted: cancelled")
    );
}

#[test]
fn turn_abort_with_no_text_commits_terminal_record() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::TurnAbort {
            reason: "cancelled".to_string(),
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].content, "[turn aborted: cancelled]");
    assert_eq!(
        state.streaming.error.as_deref(),
        Some("stream aborted: cancelled")
    );
}

#[test]
fn turn_complete_error_with_no_text_commits_terminal_record() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    let mut outcome = make_outcome("");
    outcome.stop_reason = Some("error".to_string());
    outcome.error = Some("provider unavailable".to_string());
    let _ = mgr.apply(StreamEvent::TurnComplete { outcome }, &mut state);

    assert_eq!(state.messages.len(), 1);
    assert_eq!(
        state.messages[0].content,
        "[turn failed: error: provider unavailable]"
    );
    assert_eq!(state.messages[0].model.as_deref(), Some("claude-opus-4-6"));
    assert_eq!(state.messages[0].input_tokens, 100);
    assert_eq!(state.messages[0].output_tokens, 50);
}

#[test]
fn error_sets_error_and_stops_streaming() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::Error("connection lost".to_string()),
        &mut state,
    );

    assert_eq!(state.streaming.error.as_deref(), Some("connection lost"));
    assert!(!state.streaming.is_streaming);
}

#[test]
fn apply_connection_event_connected() {
    let mut state = make_state();
    apply_connection_event(&mut state, true);
    assert_eq!(state.connection, ConnectionState::Connected);
}

#[test]
fn apply_connection_event_disconnect_increments_attempt() {
    let mut state = make_state();
    apply_connection_event(&mut state, false);
    assert_eq!(
        state.connection,
        ConnectionState::Reconnecting { attempt: 1 }
    );
    apply_connection_event(&mut state, false);
    assert_eq!(
        state.connection,
        ConnectionState::Reconnecting { attempt: 2 }
    );
}

#[test]
fn apply_connection_event_reconnect_resets_on_connect() {
    let mut state = make_state();
    apply_connection_event(&mut state, false);
    apply_connection_event(&mut state, false);
    apply_connection_event(&mut state, true);
    assert_eq!(state.connection, ConnectionState::Connected);
}

#[test]
fn thinking_delta_flushes_on_newline() {
    let mut state = make_state();
    let mut mgr = make_manager();

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );

    let changed = mgr.apply(
        StreamEvent::ThinkingDelta("step 1\n".to_string()),
        &mut state,
    );
    assert!(changed);
    assert_eq!(state.streaming.thinking, "step 1\n");
}

#[test]
fn tick_returns_false_when_nothing_buffered() {
    let mut state = make_state();
    let mut mgr = make_manager();
    assert!(!mgr.tick(&mut state));
}

#[test]
fn chat_state_default() {
    let state = ChatState::default();
    assert!(state.messages.is_empty());
    assert!(!state.streaming.is_streaming);
    assert_eq!(state.connection, ConnectionState::Disconnected);
    assert!(state.agent_id.is_none());
}

#[test]
fn last_is_user_message_matches_failed_turn_tail() {
    let mut state = make_state();
    state.messages.push(ChatMessage {
        role: MessageRole::User,
        content: "send this".to_string(),
        model: None,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
    });

    assert!(state.last_is_user_message("send this"));
    assert!(!state.last_is_user_message("different text"));
}

#[test]
fn last_is_user_message_false_for_empty_or_assistant_tail() {
    let mut state = make_state();
    assert!(!state.last_is_user_message("anything"));

    state.messages.push(ChatMessage {
        role: MessageRole::Assistant,
        content: "reply".to_string(),
        model: None,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
    });
    assert!(!state.last_is_user_message("reply"));
}

#[test]
fn full_turn_lifecycle() {
    let mut state = make_state();
    let mut mgr = make_manager();

    state.messages.push(ChatMessage {
        role: MessageRole::User,
        content: "Hello".to_string(),
        model: None,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
    });

    let _ = mgr.apply(
        StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "syn".into(),
            turn_id: "t1".into(),
        },
        &mut state,
    );
    assert!(state.streaming.is_streaming);

    let _ = mgr.apply(StreamEvent::TextDelta("Hi ".to_string()), &mut state);
    let _ = mgr.apply(StreamEvent::TextDelta("there!\n".to_string()), &mut state);
    assert_eq!(state.streaming.text, "Hi there!\n");

    let _ = mgr.apply(
        StreamEvent::ToolStart {
            tool_name: "search".to_string(),
            tool_id: ToolId::from("t-1"),
            input: None,
        },
        &mut state,
    );
    let _ = mgr.apply(
        StreamEvent::ToolResult {
            tool_name: "search".to_string(),
            tool_id: ToolId::from("t-1"),
            is_error: false,
            duration_ms: 120,
            result: None,
        },
        &mut state,
    );

    let _ = mgr.apply(
        StreamEvent::TextDelta("Found it.\n".to_string()),
        &mut state,
    );

    let _ = mgr.apply(
        StreamEvent::TurnComplete {
            outcome: TurnOutcome {
                text: "Hi there! Found it.".to_string(),
                nous_id: NousId::from("syn"),
                session_id: "s1".into(),
                model: Some("claude-opus-4-6".to_string()),
                tool_calls: 1,
                input_tokens: 200,
                output_tokens: 80,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                stop_reason: None,
                error: None,
            },
        },
        &mut state,
    );

    assert_eq!(state.messages.len(), 2);
    assert_eq!(state.messages[0].role, MessageRole::User);
    assert_eq!(state.messages[1].role, MessageRole::Assistant);
    assert_eq!(state.messages[1].content, "Hi there! Found it.");
    assert_eq!(state.messages[1].tool_calls, 1);
    assert!(!state.streaming.is_streaming);
}
