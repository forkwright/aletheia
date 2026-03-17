#[test]
fn sse_event_type_text_delta() {
    let event = crate::stream::SseEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn sse_event_type_thinking_delta() {
    let event = crate::stream::SseEvent::ThinkingDelta {
        thinking: "hmm".to_owned(),
    };
    assert_eq!(event.event_type(), "thinking_delta");
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
        },
    };
    assert_eq!(event.event_type(), "message_complete");
}

#[test]
fn sse_event_type_error() {
    let event = crate::stream::SseEvent::Error {
        code: "test".to_owned(),
        message: "err".to_owned(),
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
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_complete");
    assert_eq!(json["stop_reason"], "end_turn");
    assert_eq!(json["usage"]["input_tokens"], 100);
    assert_eq!(json["usage"]["output_tokens"], 50);
}

#[test]
fn sse_event_error_serialization() {
    let event = crate::stream::SseEvent::Error {
        code: "turn_failed".to_owned(),
        message: "provider error".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["code"], "turn_failed");
    assert_eq!(json["message"], "provider error");
}

#[test]
fn tui_event_turn_start_type() {
    let event = crate::stream::WebchatEvent::TurnStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
    };
    assert_eq!(event.event_type(), "turn_start");
}

#[test]
fn tui_event_text_delta_type() {
    let event = crate::stream::WebchatEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn tui_event_thinking_delta_type() {
    let event = crate::stream::WebchatEvent::ThinkingDelta {
        text: "hmm".to_owned(),
    };
    assert_eq!(event.event_type(), "thinking_delta");
}

#[test]
fn tui_event_tool_start_type() {
    let event = crate::stream::WebchatEvent::ToolStart {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.event_type(), "tool_start");
}

#[test]
fn tui_event_tool_result_type() {
    let event = crate::stream::WebchatEvent::ToolResult {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        result: "found".to_owned(),
        is_error: false,
        duration_ms: 42,
    };
    assert_eq!(event.event_type(), "tool_result");
}

#[test]
fn tui_event_turn_complete_type() {
    let event = crate::stream::WebchatEvent::TurnComplete {
        outcome: crate::stream::TurnOutcome {
            text: "done".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("mock".to_owned()),
            tool_calls: 0,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
    };
    assert_eq!(event.event_type(), "turn_complete");
}

#[test]
fn tui_event_error_type() {
    let event = crate::stream::WebchatEvent::Error {
        message: "fail".to_owned(),
    };
    assert_eq!(event.event_type(), "error");
}

#[test]
fn tui_event_turn_start_serialization() {
    let event = crate::stream::WebchatEvent::TurnStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "turn_start");
    assert_eq!(json["sessionId"], "s1");
    assert_eq!(json["nousId"], "syn");
    assert_eq!(json["turnId"], "t1");
}

#[test]
fn tui_event_turn_complete_serialization() {
    let event = crate::stream::WebchatEvent::TurnComplete {
        outcome: crate::stream::TurnOutcome {
            text: "response".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("claude".to_owned()),
            tool_calls: 2,
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 20,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "turn_complete");
    let outcome = &json["outcome"];
    assert_eq!(outcome["text"], "response");
    assert_eq!(outcome["nousId"], "syn");
    assert_eq!(outcome["toolCalls"], 2);
    assert_eq!(outcome["cacheReadTokens"], 10);
    assert_eq!(outcome["cacheWriteTokens"], 20);
}
