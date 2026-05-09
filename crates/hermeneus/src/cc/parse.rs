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
    Assistant { message: AssistantMessage },

    /// Final result event with usage and cost.
    ///
    /// WHY(#3717): `result` is only present on success-shaped events
    /// (`subtype = "success"`). When CC hits `error_max_turns` or another
    /// error subtype, it omits `result` entirely and populates `errors`
    /// (array of messages) + `terminal_reason` instead. Making `result`
    /// optional + surfacing `errors`/`terminal_reason` keeps the parser
    /// from dropping the whole event on error-subtype terminations.
    #[serde(rename = "result")]
    Result {
        #[cfg_attr(
            not(test),
            expect(
                dead_code,
                reason = "deserialized for completeness; not consumed by provider"
            )
        )]
        subtype: String,
        #[serde(default)]
        result: Option<String>,
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
        /// Error messages emitted when CC terminates before producing a
        /// final `result` (e.g. `error_max_turns`).
        #[serde(default)]
        errors: Vec<String>,
        /// Terminal reason string (`"max_turns"`, `"error"`, etc.) for
        /// error-subtype result events.
        #[serde(default)]
        terminal_reason: Option<String>,
    },

    /// System event (connection info, etc.). Ignored.
    #[serde(rename = "system")]
    System {
        #[serde(flatten)]
        _extra: serde_json::Value,
    },

    /// Rate-limit status event emitted by newer CC CLI versions. Ignored —
    /// the rate limit info is informational and doesn't affect the response.
    #[serde(rename = "rate_limit_event")]
    RateLimit {
        #[serde(flatten)]
        _extra: serde_json::Value,
    },

    /// User-turn echo event emitted by newer CC CLI versions. Ignored.
    #[serde(rename = "user")]
    User {
        #[serde(flatten)]
        _extra: serde_json::Value,
    },
}

/// Assistant message body.
///
/// Two on-wire shapes coexist depending on `claude` CLI version:
///
/// 1. **Legacy** (CC ≤ 1.x): the message envelope itself carries `text`:
///    `{"type":"assistant","message":{"type":"text","text":"…"}}`
///
/// 2. **Current** (CC 2.x, `2.1.119` confirmed): the envelope wraps an
///    Anthropic-API-shaped message whose `content` is an array of blocks,
///    of which a `text`-typed block carries the actual text:
///    `{"type":"assistant","message":{"type":"message","role":"assistant",
///      "content":[{"type":"text","text":"…"}], …}}`
///
/// `Self::deserialize` accepts both. The flattened text is exposed on
/// `Self::text` regardless of which shape arrived. Without this, CC 2.x's
/// nested `content[0].text` lands as an empty string, the `on_delta`
/// guard at `process.rs:531` skips emitting `TextDelta`, and TUIs that
/// render incrementally see nothing until the final `result` event.
#[derive(Debug)]
pub(crate) struct AssistantMessage {
    /// Message type kept for forward compat / debugging.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "deserialized for completeness; only `text` is consumed"
        )
    )]
    pub message_type: String,
    /// The text content, flattened from whichever wire shape arrived.
    pub text: String,
}

impl<'de> Deserialize<'de> for AssistantMessage {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WireShape {
            /// CC 2.x — Anthropic-API-shaped envelope with a content-block array.
            New {
                #[serde(rename = "type")]
                message_type: String,
                content: Vec<ContentBlock>,
            },
            /// CC ≤ 1.x — text directly on the envelope.
            Legacy {
                #[serde(rename = "type")]
                message_type: String,
                #[serde(default)]
                text: String,
            },
        }

        #[derive(Deserialize)]
        struct ContentBlock {
            #[serde(rename = "type")]
            block_type: String,
            #[serde(default)]
            text: String,
        }

        match WireShape::deserialize(deserializer)? {
            WireShape::New {
                message_type,
                content,
            } => {
                // Concatenate every text-typed block's text. A normal
                // assistant turn has exactly one `text` block; tool-use
                // turns have a `text` block plus `tool_use` blocks
                // whose text we ignore here (they go through the
                // tool-stream channel, not the text-delta channel).
                let text = content
                    .into_iter()
                    .filter(|b| b.block_type == "text")
                    .map(|b| b.text)
                    .collect::<String>();
                Ok(Self { message_type, text })
            }
            WireShape::Legacy { message_type, text } => Ok(Self { message_type, text }),
        }
    }
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
        cost_usd: None,
        duration_ms: None,
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
                assert_eq!(result.as_deref(), Some("Hello"));
                assert!(!is_error);
                assert_eq!(session_id.as_deref(), Some("sess_123"));
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 5);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    // WHY(#3717): regression — when CC hits `error_max_turns` the `result`
    // field is absent and the event instead carries `errors` + `terminal_reason`.
    // Before this fix, `result: String` made the whole event fail to parse
    // with "missing field `result`", dropping it and producing a
    // pipeline_error upstream.
    #[test]
    fn parse_result_event_error_subtype_omits_result_field() {
        let line = r#"{"type":"result","subtype":"error_max_turns","duration_ms":4065,"duration_api_ms":5062,"is_error":true,"num_turns":2,"stop_reason":"tool_use","session_id":"sess_456","total_cost_usd":0.125,"usage":{"input_tokens":1,"output_tokens":2},"permission_denials":[],"terminal_reason":"max_turns","fast_mode_state":"off","uuid":"u-1","errors":["Reached maximum number of turns (1)"]}"#;
        let event = parse_event(line).unwrap();
        match event {
            CcEvent::Result {
                subtype,
                result,
                is_error,
                errors,
                terminal_reason,
                session_id,
                ..
            } => {
                assert_eq!(subtype, "error_max_turns");
                assert_eq!(result, None, "error subtype omits the `result` field");
                assert!(is_error);
                assert_eq!(
                    errors,
                    vec!["Reached maximum number of turns (1)".to_owned()]
                );
                assert_eq!(terminal_reason.as_deref(), Some("max_turns"));
                assert_eq!(session_id.as_deref(), Some("sess_456"));
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    // WHY(#3717): regression — CC's stream-json emits a top-level
    // `{"type":"user"}` envelope for tool_result messages. The parser must
    // accept this variant and route it to the ignored `User` arm (the
    // tool-result content is handled via the Assistant turn that preceded
    // it; this echo is informational).
    #[test]
    fn parse_user_event_with_tool_result_message() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"File does not exist.","is_error":true,"tool_use_id":"toolu_abc"}]},"parent_tool_use_id":null,"session_id":"s-1","uuid":"u-2","timestamp":"2026-04-19T13:41:32.010Z","tool_use_result":"Error: File does not exist"}"#;
        let event = parse_event(line).unwrap();
        assert!(matches!(event, CcEvent::User { .. }));
    }

    #[test]
    fn parse_system_event() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc"}"#;
        let event = parse_event(line).unwrap();
        assert!(matches!(event, CcEvent::System { .. }));
    }

    #[test]
    fn parse_user_event() {
        let line = r#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
        let event = parse_event(line).unwrap();
        assert!(matches!(event, CcEvent::User { .. }));
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
        let resp = result_to_response(
            "Hello",
            false,
            Some(&usage),
            "claude-sonnet-4-20250514",
            Some("sess_1"),
        )
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
