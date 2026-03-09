//! SSE stream parser for pylon event streams.

use serde::Deserialize;
use snafu::ResultExt;

use crate::error::{self, Result};

/// A single parsed SSE event from the pylon stream.
#[derive(Debug, Clone)]
pub struct ParsedSseEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Consume an entire SSE response body and parse it into discrete events.
#[tracing::instrument(skip(response))]
pub async fn parse_sse_stream(response: reqwest::Response) -> Result<Vec<ParsedSseEvent>> {
    let text = response.text().await.context(error::HttpSnafu)?;
    parse_sse_text(&text)
}

/// Parse raw SSE text into events. Exposed for testing.
#[tracing::instrument(skip(text), fields(text_len = text.len()))]
pub fn parse_sse_text(text: &str) -> Result<Vec<ParsedSseEvent>> {
    let mut events = Vec::new();
    let mut current_event_type = String::new();
    let mut current_data = String::new();

    for line in text.lines() {
        if line.starts_with(':') {
            // Comment / keepalive — skip
            continue;
        }

        if line.is_empty() {
            // Empty line = event boundary
            if !current_event_type.is_empty() && !current_data.is_empty() {
                let data: serde_json::Value =
                    serde_json::from_str(&current_data).context(error::JsonSnafu)?;
                events.push(ParsedSseEvent {
                    event_type: std::mem::take(&mut current_event_type),
                    data,
                });
                current_data.clear();
            }
            current_event_type.clear();
            current_data.clear();
            continue;
        }

        if let Some(value) = line.strip_prefix("event: ") {
            value.clone_into(&mut current_event_type);
        } else if let Some(value) = line.strip_prefix("data: ") {
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(value);
        } else if let Some(value) = line.strip_prefix("event:") {
            value.trim().clone_into(&mut current_event_type);
        } else if let Some(value) = line.strip_prefix("data:") {
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(value.trim());
        }
    }

    // Handle trailing event without final blank line
    if !current_event_type.is_empty() && !current_data.is_empty() {
        let data: serde_json::Value =
            serde_json::from_str(&current_data).context(error::JsonSnafu)?;
        events.push(ParsedSseEvent {
            event_type: current_event_type,
            data,
        });
    }

    Ok(events)
}

/// Extract the concatenated text content from a sequence of SSE events.
pub fn extract_text(events: &[ParsedSseEvent]) -> String {
    events
        .iter()
        .filter(|e| e.event_type == "text_delta")
        .filter_map(|e| e.data.get("text").and_then(|v| v.as_str()))
        .collect()
}

/// Check whether the stream completed successfully.
pub fn is_complete(events: &[ParsedSseEvent]) -> bool {
    events.iter().any(|e| e.event_type == "message_complete")
}

/// Check whether the stream contains an error event.
pub fn has_error(events: &[ParsedSseEvent]) -> bool {
    events.iter().any(|e| e.event_type == "error")
}

/// Count `tool_use` events in the stream.
pub fn tool_call_count(events: &[ParsedSseEvent]) -> usize {
    events.iter().filter(|e| e.event_type == "tool_use").count()
}

/// Extract usage data from the `message_complete` event.
#[derive(Debug, Clone, Deserialize)]
pub struct UsageData {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub fn extract_usage(events: &[ParsedSseEvent]) -> Option<UsageData> {
    events
        .iter()
        .find(|e| e.event_type == "message_complete")
        .and_then(|e| serde_json::from_value(e.data.get("usage")?.clone()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_events() {
        let input = "event: text_delta\ndata: {\"text\":\"Hello\"}\n\nevent: message_complete\ndata: {\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "text_delta");
        assert_eq!(events[0].data["text"], "Hello");
        assert_eq!(events[1].event_type, "message_complete");
        assert_eq!(events[1].data["stop_reason"], "end_turn");
    }

    #[test]
    fn skips_keepalive_comments() {
        let input = ":ping\n\nevent: text_delta\ndata: {\"text\":\"hi\"}\n\n:ping\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "text_delta");
    }

    #[test]
    fn handles_trailing_event_without_blank_line() {
        let input = "event: text_delta\ndata: {\"text\":\"end\"}";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn empty_input_returns_empty() {
        let events = parse_sse_text("").expect("parse");
        assert!(events.is_empty());
    }

    #[test]
    fn extract_text_concatenates() {
        let input = "event: text_delta\ndata: {\"text\":\"Hello \"}\n\nevent: text_delta\ndata: {\"text\":\"world\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(extract_text(&events), "Hello world");
    }

    #[test]
    fn is_complete_detects_message_complete() {
        let input = "event: message_complete\ndata: {\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":0,\"output_tokens\":0}}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(is_complete(&events));
    }

    #[test]
    fn has_error_detects_error_event() {
        let input = "event: error\ndata: {\"code\":\"fail\",\"message\":\"boom\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(has_error(&events));
    }

    #[test]
    fn tool_call_count_works() {
        let input = "event: tool_use\ndata: {\"id\":\"1\",\"name\":\"t\",\"input\":{}}\n\nevent: tool_result\ndata: {\"tool_use_id\":\"1\",\"content\":\"ok\",\"is_error\":false}\n\nevent: tool_use\ndata: {\"id\":\"2\",\"name\":\"t\",\"input\":{}}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(tool_call_count(&events), 2);
    }

    #[test]
    fn extract_usage_works() {
        let input = "event: message_complete\ndata: {\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":42,\"output_tokens\":7}}\n\n";
        let events = parse_sse_text(input).expect("parse");
        let usage = extract_usage(&events).expect("usage");
        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 7);
    }

    #[test]
    fn handles_no_space_after_colon() {
        let input = "event:text_delta\ndata:{\"text\":\"hi\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "text_delta");
    }
}
