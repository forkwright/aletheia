//! Parse Codex CLI `--json` output.
//!
//! `codex exec --json` emits newline-delimited JSON events on stdout.
//! The adapter collects assistant text from `item.completed` events and
//! final usage from the last `turn.completed` event.

use serde_json::Value;
use tracing::warn;

use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

/// Parsed Codex subprocess output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexParsedOutput {
    pub text: String,
    pub usage: Usage,
}

impl CodexParsedOutput {
    pub(crate) fn new(text: String, usage: Usage) -> Self {
        Self { text, usage }
    }
}

/// Parse Codex JSONL output into assistant text and usage.
pub(crate) fn parse_output(stdout: &str) -> Result<CodexParsedOutput> {
    let mut text = String::new();
    let mut usage = Usage::default();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(e) => {
                warn!(
                    error = %e,
                    line = %trimmed,
                    "failed to parse Codex stream-json event"
                );
                continue;
            }
        };

        match value.get("type").and_then(Value::as_str) {
            Some("item.completed") => {
                if value
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("agent_message")
                    && let Some(part) = value
                        .get("item")
                        .and_then(|item| item.get("text"))
                        .and_then(Value::as_str)
                    && !part.is_empty()
                {
                    text.push_str(part);
                }
            }
            Some("turn.completed") => {
                if let Some(parsed) = parse_usage(value.get("usage")) {
                    usage = parsed;
                }
            }
            _ => {} // unrecognized event type, skip
        }
    }

    if text.is_empty() {
        return Err(error::SubprocessFailureSnafu {
            provider: "codex".to_owned(),
            kind: error::SubprocessFailureKind::NoOutput,
            message: "subprocess produced no text output".to_owned(),
        }
        .build());
    }

    Ok(CodexParsedOutput::new(text, usage))
}

fn parse_usage(value: Option<&Value>) -> Option<Usage> {
    let value = value?;
    Some(Usage {
        input_tokens: value
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        output_tokens: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_read_tokens: value
            .get("cached_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_write_tokens: 0,
    })
}

/// Convert parsed Codex text and usage into a `CompletionResponse`.
pub(crate) fn text_to_response(text: &str, usage: Usage, model: &str) -> CompletionResponse {
    CompletionResponse {
        id: format!("codex_{}", koina::uuid::uuid_v4()),
        model: model.to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage,
        cost_usd: None,
        duration_ms: None,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_output_parses_jsonl_text_and_usage() {
        let output = parse_output(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"hello "}}
{"type":"item.completed","item":{"type":"agent_message","text":"world"}}
{"type":"turn.completed","usage":{"input_tokens":11,"cached_input_tokens":3,"output_tokens":7}}"#,
        )
        .unwrap();

        assert_eq!(output.text, "hello world");
        assert_eq!(output.usage.input_tokens, 11);
        assert_eq!(output.usage.output_tokens, 7);
        assert_eq!(output.usage.cache_read_tokens, 3);
        assert_eq!(output.usage.cache_write_tokens, 0);
    }

    #[test]
    fn parse_output_skips_malformed_lines_and_keeps_later_events() {
        let output = parse_output(
            r#"not json
{"type":"item.completed","item":{"type":"agent_message","text":"ok"}}
{"type":"turn.completed","usage":{"input_tokens":2,"cached_input_tokens":1,"output_tokens":4}}"#,
        )
        .unwrap();

        assert_eq!(output.text, "ok");
        assert_eq!(output.usage.input_tokens, 2);
        assert_eq!(output.usage.output_tokens, 4);
        assert_eq!(output.usage.cache_read_tokens, 1);
    }

    #[test]
    fn parse_output_rejects_blank_output() {
        let err = parse_output("  \n\t").unwrap_err();
        assert!(err.to_string().contains("no text output"));
    }

    #[test]
    fn text_to_response_wraps_text_block() {
        let response = text_to_response(
            "done",
            Usage {
                input_tokens: 9,
                output_tokens: 4,
                cache_read_tokens: 2,
                cache_write_tokens: 0,
            },
            "gpt-5-codex",
        );
        assert!(response.id.starts_with("codex_"));
        assert_eq!(response.model, "gpt-5-codex");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.content.len(), 1);
        assert_eq!(response.usage.input_tokens, 9);
    }
}
