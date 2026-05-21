//! Parse Codex CLI plain-text output.
//!
//! `codex exec` emits plain text on stdout for headless invocations. The
//! adapter buffers that output and maps it into Hermeneus' Anthropic-native
//! response shape.

use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

/// Normalize buffered Codex stdout into assistant text.
pub(crate) fn parse_output(stdout: &str) -> Result<String> {
    let text = stdout.trim_end_matches(['\r', '\n']).to_owned();
    if text.trim().is_empty() {
        return Err(error::ApiRequestSnafu {
            message: "Codex subprocess produced no text output".to_owned(),
        }
        .build());
    }
    Ok(text)
}

/// Convert Codex plain text into a [`CompletionResponse`].
pub(crate) fn text_to_response(text: &str, model: &str) -> CompletionResponse {
    CompletionResponse {
        id: format!("codex_{}", koina::uuid::uuid_v4()),
        model: model.to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage::default(),
        cost_usd: None,
        duration_ms: None,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn parse_output_trims_trailing_newlines() {
        let text = parse_output("hello world\n\n").unwrap();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn parse_output_preserves_internal_whitespace() {
        let text = parse_output("line one\n\nline two\n").unwrap();
        assert_eq!(text, "line one\n\nline two");
    }

    #[test]
    fn parse_output_rejects_blank_output() {
        let err = parse_output("  \n\t").unwrap_err();
        assert!(err.to_string().contains("no text output"));
    }

    #[test]
    fn text_to_response_wraps_text_block() {
        let response = text_to_response("done", "gpt-5-codex");
        assert!(response.id.starts_with("codex_"));
        assert_eq!(response.model, "gpt-5-codex");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.content.len(), 1);
    }
}
