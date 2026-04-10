//! Parse Claude Code's `stream-json` output format.
//!
//! CC emits newline-delimited JSON events on stdout:
//! - `{"type":"assistant","message":{"type":"text","text":"..."}}`
//! - `{"type":"result","subtype":"success","result":"...","usage":{...},...}`
//!
//! The `result` event carries the final text, usage, cost, and session ID.

use serde::Deserialize;

use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

/// Top-level event envelope from CC's stream-json output.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum CcEvent {
    /// Incremental assistant message (streaming text).
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
    },

    /// Final result event with usage and cost.
    #[serde(rename = "result")]
    Result {
        #[cfg_attr(
            not(test),
            expect(dead_code, reason = "deserialized for completeness; not consumed by provider")
        )]
        subtype: String,
        result: String,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        usage: Option<CcUsage>,
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        session_id: Option<String>,
    },

    /// System event (connection info, etc.). Ignored.
    #[serde(rename = "system")]
    System {
        #[serde(flatten)]
        _extra: serde_json::Value,
    },
}

/// Assistant message body.
#[derive(Debug, Deserialize)]
pub(crate) struct AssistantMessage {
    /// Message type (typically "text").
    #[serde(rename = "type")]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "deserialized for completeness; only `text` is consumed")
    )]
    pub message_type: String,
    /// The text content.
    #[serde(default)]
    pub text: String,
}

/// Usage stats from the CC result event.
#[derive(Debug, Default, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "field names mirror CC's JSON wire format for 1:1 deserialization"
)]
pub(crate) struct CcUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

/// Parse a single line of CC stream-json output into a `CcEvent`.
///
/// Returns `None` for blank lines or lines that fail to parse (logged as warnings).
pub(crate) fn parse_event(line: &str) -> Option<CcEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<CcEvent>(trimmed) {
        Ok(event) => Some(event),
        Err(e) => {
            tracing::warn!(
                error = %e,
                line = %trimmed,
                "failed to parse CC stream-json event"
            );
            None
        }
    }
}

/// Convert a CC result event into a `CompletionResponse`.
pub(crate) fn result_to_response(
    result_text: &str,
    is_error: bool,
    usage: Option<&CcUsage>,
    model: &str,
    session_id: Option<&str>,
) -> Result<CompletionResponse> {
    if is_error {
        return Err(error::ApiRequestSnafu {
            message: format!("CC returned error: {result_text}"),
        }
        .build());
    }

    let content = vec![ContentBlock::Text {
        text: result_text.to_owned(),
        citations: None,
    }];

    let usage = usage.map_or(Usage::default(), |u| Usage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_input_tokens,
        cache_write_tokens: u.cache_creation_input_tokens,
    });

    // WHY: session_id from CC serves as a reasonable response ID; fall back to
    // a generated one when not present.
    let id = match session_id {
        Some(sid) => sid.to_owned(),
        None => format!("cc_{}", koina::uuid::uuid_v4()),
    };

    Ok(CompletionResponse {
        id,
        model: model.to_owned(),
        stop_reason: StopReason::EndTurn,
        content,
        usage,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_assistant_event() {
        let line = r#"{"type":"assistant","message":{"type":"text","text":"Hello world"}}"#;
        let event = parse_event(line).unwrap();
        match event {
            CcEvent::Assistant { message } => {
                assert_eq!(message.message_type, "text");
                assert_eq!(message.text, "Hello world");
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn parse_result_event() {
        let line = r#"{"type":"result","subtype":"success","result":"Hello","session_id":"sess_123","cost_usd":0.001,"duration_ms":1234,"is_error":false,"total_cost_usd":0.001,"usage":{"input_tokens":10,"output_tokens":5}}"#;
        let event = parse_event(line).unwrap();
        match event {
            CcEvent::Result {
                subtype,
                result,
                is_error,
                usage,
                session_id,
                ..
            } => {
                assert_eq!(subtype, "success");
                assert_eq!(result, "Hello");
                assert!(!is_error);
                assert_eq!(session_id.as_deref(), Some("sess_123"));
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 5);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn parse_system_event() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc"}"#;
        let event = parse_event(line).unwrap();
        assert!(matches!(event, CcEvent::System { .. }));
    }

    #[test]
    fn parse_blank_line_returns_none() {
        assert!(parse_event("").is_none());
        assert!(parse_event("   ").is_none());
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_event("not json").is_none());
    }

    #[test]
    fn result_to_response_success() {
        let usage = CcUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
        };
        let resp =
            result_to_response("Hello", false, Some(&usage), "claude-sonnet-4-20250514", Some("sess_1"))
                .unwrap();
        assert_eq!(resp.id, "sess_1");
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
        assert_eq!(resp.usage.cache_read_tokens, 10);
        assert_eq!(resp.usage.cache_write_tokens, 5);
        assert_eq!(resp.content.len(), 1);
    }

    #[test]
    fn result_to_response_error() {
        let err = result_to_response("bad request", true, None, "model", None);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("bad request"));
    }

    #[test]
    fn result_to_response_no_session_id() {
        let resp = result_to_response("ok", false, None, "model", None).unwrap();
        assert!(resp.id.starts_with("cc_"));
    }
}
