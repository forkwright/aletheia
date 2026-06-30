#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]

#[test]
fn sse_event_type_text_delta() {
    let event = crate::stream::SseEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn sse_event_type_tool_use() {
    let event = crate::stream::SseEvent::ToolUse {
        id: "t1".to_owned(),
        name: "search".to_owned(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.event_type(), "tool_use");
}

#[test]
fn sse_event_type_tool_result() {
    let event = crate::stream::SseEvent::ToolResult {
        tool_use_id: "t1".to_owned(),
        content: "result".to_owned(),
        is_error: false,
    };
    assert_eq!(event.event_type(), "tool_result");
}

#[test]
fn sse_event_type_message_complete() {
    let event = crate::stream::SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
        provider: None,
        request_id: None,
    };
    assert_eq!(event.event_type(), "message_complete");
}

#[test]
fn sse_event_type_error() {
    let event = crate::stream::SseEvent::Error {
        code: "test".to_owned(),
        message: "err".to_owned(),
        request_id: None,
    };
    assert_eq!(event.event_type(), "error");
}

#[test]
fn sse_event_serialization_roundtrip() {
    let event = crate::stream::SseEvent::TextDelta {
        text: "hello".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text_delta");
    assert_eq!(json["text"], "hello");
}

#[test]
fn sse_event_message_complete_serialization() {
    let event = crate::stream::SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
        },
        provider: Some("local-proxy".to_owned()),
        request_id: Some("req-789".to_owned()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_complete");
    assert_eq!(json["stop_reason"], "end_turn");
    assert_eq!(json["usage"]["input_tokens"], 100);
    assert_eq!(json["usage"]["output_tokens"], 50);
    assert_eq!(json["provider"], "local-proxy");
    assert_eq!(json["request_id"], "req-789");
}

#[test]
fn sse_event_error_serialization() {
    let event = crate::stream::SseEvent::Error {
        code: "turn_failed".to_owned(),
        message: "provider error".to_owned(),
        request_id: Some("req-123".to_owned()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["code"], "turn_failed");
    assert_eq!(json["message"], "provider error");
    assert_eq!(json["request_id"], "req-123");
}

#[test]
fn sse_event_error_omits_request_id_when_none() {
    let event = crate::stream::SseEvent::Error {
        code: "test".to_owned(),
        message: "err".to_owned(),
        request_id: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(
        json.get("request_id").is_none(),
        "request_id should be omitted when None"
    );
}

#[test]
fn tui_event_message_start_type() {
    let event = crate::stream::TurnStreamEvent::MessageStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
        request_id: None,
    };
    assert_eq!(event.event_type(), "message_start");
}

#[test]
fn tui_event_text_delta_type() {
    let event = crate::stream::TurnStreamEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn tui_event_thinking_delta_type() {
    let event = crate::stream::TurnStreamEvent::ThinkingDelta {
        text: "hmm".to_owned(),
    };
    assert_eq!(event.event_type(), "thinking_delta");
}

#[test]
fn tui_event_tool_use_type() {
    let event = crate::stream::TurnStreamEvent::ToolUse {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.event_type(), "tool_use");
}

#[test]
fn tui_event_tool_result_type() {
    let event = crate::stream::TurnStreamEvent::ToolResult {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        result: "found".to_owned(),
        is_error: false,
        duration_ms: 42,
    };
    assert_eq!(event.event_type(), "tool_result");
}

#[test]
fn tui_event_message_complete_type() {
    let event = crate::stream::TurnStreamEvent::MessageComplete {
        outcome: crate::stream::TurnOutcome {
            text: "done".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("mock".to_owned()),
            provider: None,
            tool_calls: 0,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            stop_reason: "end_turn".to_owned(),
            error: None,
        },
    };
    assert_eq!(event.event_type(), "message_complete");
}

#[test]
fn tui_event_error_type() {
    let event = crate::stream::TurnStreamEvent::Error {
        code: "provider_unavailable".to_owned(),
        message: "fail".to_owned(),
        request_id: Some("req-123".to_owned()),
    };
    assert_eq!(event.event_type(), "error");
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["code"], "provider_unavailable");
    assert_eq!(json["message"], "fail");
    assert_eq!(json["request_id"], "req-123");
}

#[test]
fn tui_event_message_start_serialization() {
    let event = crate::stream::TurnStreamEvent::MessageStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
        request_id: Some("req-abc".to_owned()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_start");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["nous_id"], "syn");
    assert_eq!(json["turn_id"], "t1");
    assert_eq!(json["request_id"], "req-abc");
}

#[test]
fn tui_event_message_complete_serialization() {
    let event = crate::stream::TurnStreamEvent::MessageComplete {
        outcome: crate::stream::TurnOutcome {
            text: "response".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("claude".to_owned()),
            provider: Some("anthropic-cloud".to_owned()),
            tool_calls: 2,
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 20,
            stop_reason: "end_turn".to_owned(),
            error: None,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_complete");
    let outcome = &json["outcome"];
    assert_eq!(outcome["text"], "response");
    assert_eq!(outcome["nous_id"], "syn");
    assert_eq!(outcome["provider"], "anthropic-cloud");
    assert_eq!(outcome["tool_calls"], 2);
    assert_eq!(outcome["cache_read_tokens"], 10);
    assert_eq!(outcome["cache_write_tokens"], 20);
    assert_eq!(outcome["stop_reason"], "end_turn");
    assert!(outcome.get("error").is_none());
}

#[test]
fn sse_event_message_start_serialization() {
    let event = crate::stream::SseEvent::MessageStart {
        status: "accepted".to_owned(),
        session_id: Some("s1".to_owned()),
        nous_id: Some("syn".to_owned()),
        turn_id: Some("t1".to_owned()),
        request_id: Some("req-abc".to_owned()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_start");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["session_id"], "s1");
    assert_eq!(json["nous_id"], "syn");
    assert_eq!(json["turn_id"], "t1");
    assert_eq!(json["request_id"], "req-abc");
}

#[test]
fn sse_event_message_start_omits_optional_ids_when_none() {
    // WHY(#5163): The new identifiers are additive; existing consumers that do
    // not emit them must not produce empty/null fields.
    let event = crate::stream::SseEvent::MessageStart {
        status: "accepted".to_owned(),
        session_id: None,
        nous_id: None,
        turn_id: None,
        request_id: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_start");
    assert_eq!(json["status"], "accepted");
    assert!(json.get("session_id").is_none());
    assert!(json.get("nous_id").is_none());
    assert!(json.get("turn_id").is_none());
    assert!(json.get("request_id").is_none());
}

#[test]
fn message_complete_is_terminal_after_error_event() {
    // WHY(#5164): Pylon may emit an `error` event before `message_complete`.
    // The `error` event is informational; `message_complete` remains the
    // authoritative terminal marker of the stream.
    let error = crate::stream::SseEvent::Error {
        code: "turn_failed".to_owned(),
        message: "provider error".to_owned(),
        request_id: Some("req-123".to_owned()),
    };
    let complete = crate::stream::SseEvent::MessageComplete {
        stop_reason: "error".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
        provider: None,
        request_id: Some("req-123".to_owned()),
    };

    assert_eq!(error.event_type(), "error");
    assert_eq!(complete.event_type(), "message_complete");

    let error_json = serde_json::to_value(&error).unwrap();
    let complete_json = serde_json::to_value(&complete).unwrap();
    assert_eq!(error_json["type"], "error");
    assert_eq!(complete_json["type"], "message_complete");
    assert_eq!(complete_json["stop_reason"], "error");
}

#[test]
fn sse_event_message_complete_includes_cache_tokens() {
    let event = crate::stream::SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 1000,
            cache_write_tokens: 200,
        },
        provider: None,
        request_id: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["usage"]["input_tokens"], 100);
    assert_eq!(json["usage"]["output_tokens"], 50);
    assert_eq!(json["usage"]["cache_read_tokens"], 1000);
    assert_eq!(json["usage"]["cache_write_tokens"], 200);
}
