use crate::api::types::SseEvent;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

/// Raw events from the three multiplexed sources.
/// Mapped to `Msg` by `App::map_event`.
#[non_exhaustive]
#[derive(Debug)]
pub enum Event {
    /// Terminal keypress, mouse, resize
    Terminal(crossterm::event::Event),
    /// Global SSE event stream
    Sse(SseEvent),
    /// Per-session streaming response
    Stream(StreamEvent),
    /// 30fps UI tick
    Tick,
}

/// Parsed events from a POST /api/sessions/stream response
#[non_exhaustive]
#[derive(Debug)]
pub enum StreamEvent {
    TurnStart {
        session_id: SessionId,
        nous_id: NousId,
        turn_id: TurnId,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        tool_name: String,
        tool_id: ToolId,
        input: Option<serde_json::Value>,
    },
    ToolResult {
        tool_name: String,
        tool_id: ToolId,
        is_error: bool,
        duration_ms: u64,
        result: Option<String>,
    },
    ToolApprovalRequired {
        turn_id: TurnId,
        tool_name: String,
        tool_id: ToolId,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    ToolApprovalResolved {
        tool_id: ToolId,
        decision: String,
    },
    PlanProposed {
        plan: crate::api::types::Plan,
    },
    PlanStepStart {
        plan_id: PlanId,
        step_id: u32,
    },
    PlanStepComplete {
        plan_id: PlanId,
        step_id: u32,
        status: String,
    },
    PlanComplete {
        plan_id: PlanId,
        status: String,
    },
    TurnComplete {
        outcome: crate::api::types::TurnOutcome,
    },
    TurnAbort {
        reason: String,
    },
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_tick_is_constructable() {
        let _ = Event::Tick;
    }

    #[test]
    fn stream_event_text_delta_holds_string() {
        let event = StreamEvent::TextDelta("hello".to_string());
        if let StreamEvent::TextDelta(text) = event {
            assert_eq!(text, "hello");
        } else {
            panic!("expected TextDelta");
        }
    }

    #[test]
    fn stream_event_error_holds_message() {
        let event = StreamEvent::Error("connection lost".to_string());
        if let StreamEvent::Error(msg) = event {
            assert_eq!(msg, "connection lost");
        } else {
            panic!("expected Error");
        }
    }

    #[test]
    fn stream_event_turn_start_fields() {
        let event = StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "n1".into(),
            turn_id: "t1".into(),
        };
        if let StreamEvent::TurnStart {
            session_id,
            nous_id,
            turn_id,
        } = event
        {
            assert!(session_id == *"s1");
            assert!(nous_id == *"n1");
            assert!(turn_id == *"t1");
        }
    }

    #[test]
    fn event_debug_impl_works() {
        let event = Event::Tick;
        let debug = format!("{:?}", event);
        assert!(debug.contains("Tick"));
    }

    #[test]
    fn stream_event_tool_result_fields() {
        let event = StreamEvent::ToolResult {
            tool_name: "read_file".to_string(),
            tool_id: "t1".into(),
            is_error: true,
            duration_ms: 150,
            result: None,
        };
        if let StreamEvent::ToolResult {
            tool_name,
            is_error,
            duration_ms,
            ..
        } = event
        {
            assert_eq!(tool_name, "read_file");
            assert!(is_error);
            assert_eq!(duration_ms, 150);
        }
    }
}
