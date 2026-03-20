use theatron_core::api::types::SseEvent;

pub use theatron_core::events::StreamEvent;

/// Raw events from the three multiplexed sources.
/// Mapped to `Msg` by `App::map_event`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Event {
    /// Terminal keypress, mouse, resize
    Terminal(crossterm::event::Event),
    /// Global SSE event stream
    Sse(SseEvent),
    /// Per-session streaming response
    Stream(StreamEvent),
    /// 60fps UI tick
    Tick,
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
