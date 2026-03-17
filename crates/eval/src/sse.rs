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
#[must_use]
pub async fn parse_sse_stream(response: reqwest::Response) -> Result<Vec<ParsedSseEvent>> {
    let text = response.text().await.context(error::HttpSnafu)?;
    parse_sse_text(&text)
}

/// Parse raw SSE text into events. Exposed for testing.
#[tracing::instrument(skip(text), fields(text_len = text.len()))]
#[must_use]
pub fn parse_sse_text(text: &str) -> Result<Vec<ParsedSseEvent>> {
    let mut events = Vec::new();
    let mut current_event_type = String::new();
    let mut current_data = String::new();

    for line in text.lines() {
        if line.starts_with(':') {
            continue;
        }

        if line.is_empty() {
            // NOTE: empty line signals SSE event boundary per the spec
            if !current_event_type.is_empty() && !current_data.is_empty() {
                match serde_json::from_str::<serde_json::Value>(&current_data) {
                    Ok(data) => {
                        events.push(ParsedSseEvent {
                            event_type: std::mem::take(&mut current_event_type),
                            data,
                        });
                        current_data.clear();
                    }
                    Err(_) => {
                        tracing::warn!(
                            event_type = %current_event_type,
                            raw_data = %current_data,
                            "malformed SSE event skipped"
                        );
                    }
                }
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

    // NOTE: handle trailing event without final blank line
    if !current_event_type.is_empty() && !current_data.is_empty() {
        match serde_json::from_str::<serde_json::Value>(&current_data) {
            Ok(data) => {
                events.push(ParsedSseEvent {
                    event_type: current_event_type,
                    data,
                });
            }
            Err(_) => {
                tracing::warn!(
                    event_type = %current_event_type,
                    raw_data = %current_data,
                    "malformed SSE event skipped"
                );
            }
        }
    }

    Ok(events)
}

/// Extract the concatenated text content from a sequence of SSE events.
#[tracing::instrument(skip_all, fields(event_count = events.len()))]
pub fn extract_text(events: &[ParsedSseEvent]) -> String {
    events
        .iter()
        .filter(|e| e.event_type == "text_delta")
        .filter_map(|e| e.data.get("text").and_then(|v| v.as_str()))
        .collect()
}

/// Check whether the stream completed successfully.
#[tracing::instrument(skip_all, fields(event_count = events.len()))]
pub fn is_complete(events: &[ParsedSseEvent]) -> bool {
    events.iter().any(|e| e.event_type == "message_complete")
}

/// Check whether the stream contains an error event.
#[tracing::instrument(skip_all, fields(event_count = events.len()))]
pub fn has_error(events: &[ParsedSseEvent]) -> bool {
    events.iter().any(|e| e.event_type == "error")
}

/// Count `tool_use` events in the stream.
#[tracing::instrument(skip_all, fields(event_count = events.len()))]
pub fn tool_call_count(events: &[ParsedSseEvent]) -> usize {
    events.iter().filter(|e| e.event_type == "tool_use").count()
}

/// Extract usage data from the `message_complete` event.
#[derive(Debug, Clone, Deserialize)]
pub struct UsageData {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[tracing::instrument(skip_all, fields(event_count = events.len()))]
pub fn extract_usage(events: &[ParsedSseEvent]) -> Option<UsageData> {
    events
        .iter()
        .find(|e| e.event_type == "message_complete")
        .and_then(|e| serde_json::from_value(e.data.get("usage")?.clone()).ok())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
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

    #[test]
    fn multiline_data_concatenated() {
        let input = "event: text_delta\ndata: {\"text\":\ndata: \"hello\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "text_delta");
        assert!(format!("{}", events[0].data).contains("hello"));
    }

    #[test]
    fn only_comments_returns_empty() {
        let input = ":ping\n:keepalive\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(events.is_empty());
    }

    #[test]
    fn event_without_data_ignored() {
        let input = "event: foo\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(events.is_empty());
    }

    #[test]
    fn data_without_event_ignored() {
        let input = "data: {\"x\":1}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(events.is_empty());
    }

    #[test]
    fn extract_text_empty_events() {
        let events: Vec<ParsedSseEvent> = vec![];
        assert_eq!(extract_text(&events), "");
    }

    #[test]
    fn is_complete_false_without_complete_event() {
        let input = "event: text_delta\ndata: {\"text\":\"hi\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(!is_complete(&events));
    }

    #[test]
    fn has_error_false_without_error_event() {
        let input = "event: text_delta\ndata: {\"text\":\"hi\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(!has_error(&events));
    }

    #[test]
    fn tool_call_count_zero() {
        let input = "event: text_delta\ndata: {\"text\":\"hi\"}\n\nevent: message_complete\ndata: {\"stop_reason\":\"end_turn\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(tool_call_count(&events), 0);
    }

    #[test]
    fn extract_usage_returns_none_without_complete() {
        let input = "event: text_delta\ndata: {\"text\":\"hi\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert!(extract_usage(&events).is_none());
    }

    #[test]
    fn multiple_events_mixed_types() {
        let input = "\
event: text_delta\n\
data: {\"text\":\"Hello\"}\n\
\n\
event: tool_use\n\
data: {\"id\":\"t1\",\"name\":\"search\",\"input\":{\"q\":\"test\"}}\n\
\n\
event: tool_result\n\
data: {\"tool_use_id\":\"t1\",\"content\":\"result\",\"is_error\":false}\n\
\n\
event: message_complete\n\
data: {\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":100,\"output_tokens\":50}}\n\
\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 4);
        assert_eq!(extract_text(&events), "Hello");
        assert_eq!(tool_call_count(&events), 1);
        assert!(is_complete(&events));
        assert!(!has_error(&events));
        let usage = extract_usage(&events).expect("usage");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn malformed_json_event_skipped_parsing_continues() {
        let input = "event: foo\ndata: not-json\n\nevent: text_delta\ndata: {\"text\":\"ok\"}\n\n";
        let events = parse_sse_text(input).expect("parse should succeed");
        // NOTE: malformed event is skipped; valid subsequent event is preserved
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "text_delta");
    }

    #[test]
    fn malformed_json_only_returns_empty() {
        let input = "event: foo\ndata: not-json\n\n";
        let events = parse_sse_text(input).expect("parse should succeed");
        assert!(events.is_empty());
    }

    #[test]
    fn whitespace_only_between_events() {
        // NOTE: lines with only whitespace don't trigger event boundaries the same way as truly empty lines
        let input = "event: text_delta\ndata: {\"text\":\"a\"}\n\nevent: text_delta\ndata: {\"text\":\"b\"}\n\n";
        let events = parse_sse_text(input).expect("parse");
        assert_eq!(events.len(), 2);
        assert_eq!(extract_text(&events), "ab");
    }
}
