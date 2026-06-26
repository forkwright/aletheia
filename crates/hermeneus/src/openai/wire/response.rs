//! Parse OpenAI Chat Completions responses into [`CompletionResponse`].
//!
//! Maps `finish_reason` → [`StopReason`] and `tool_calls` → structured
//! [`ContentBlock::ToolUse`]. The OpenAI response shape is stable across
//! OpenAI proper, llama.cpp, ollama, and vllm.

use serde::Deserialize;
use tracing::warn;

use snafu::ResultExt as _;

use crate::error::{ApiRequestSnafu, Error, MalformedToolArgumentsSnafu};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoice {
    pub message: ChatChoiceMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoiceMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ChatChoiceToolCall>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoiceToolCall {
    pub id: String,
    #[expect(
        dead_code,
        reason = "OpenAI always sets type=function for now; kept for forward compatibility"
    )]
    #[serde(default, rename = "type")]
    pub call_type: Option<String>,
    pub function: ChatChoiceFunctionCall,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoiceFunctionCall {
    pub name: String,
    /// JSON-encoded string of arguments — decoded into a
    /// [`serde_json::Value`] when building the [`ContentBlock::ToolUse`].
    #[serde(default)]
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<TokenDetails>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TokenDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesResponse {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub incomplete_details: Option<ResponsesIncompleteDetails>,
    #[serde(default)]
    pub output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
    #[serde(default)]
    pub output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesIncompleteDetails {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ResponsesOutputItem {
    #[serde(rename = "message")]
    Message {
        content: Vec<ResponsesOutputContent>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        call_id: String,
        name: String,
        #[serde(default)]
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ResponsesOutputContent {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub input_tokens_details: Option<TokenDetails>,
}

impl ChatCompletionResponse {
    /// Convert a successful OpenAI response into the Anthropic-shaped
    /// [`CompletionResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error string when the response contains no choices (the
    /// OpenAI API guarantees at least one but we defend against a degenerate
    /// server).
    pub(crate) fn into_response(self) -> Result<CompletionResponse, Error> {
        let Self {
            id,
            model,
            choices,
            usage,
        } = self;

        let mut choices_iter = choices.into_iter();
        let choice = choices_iter.next().ok_or_else(|| {
            ApiRequestSnafu {
                message: "OpenAI response had no choices".to_owned(),
            }
            .build()
        })?;

        let finish_reason = choice.finish_reason.as_deref().unwrap_or("stop");
        let stop_reason = map_finish_reason(finish_reason);

        let mut content: Vec<ContentBlock> = Vec::new();

        if let Some(text) = choice.message.content
            && !text.is_empty()
        {
            content.push(ContentBlock::Text {
                text,
                citations: None,
            });
        }

        for call in choice.message.tool_calls {
            // WHY: arguments is a JSON-encoded string on the wire. An empty
            // string means the tool takes no input — map to an empty object.
            let input = parse_arguments(&call.function.arguments, &call.function.name)?;
            content.push(ContentBlock::ToolUse {
                id: call.id,
                name: call.function.name,
                input,
            });
        }

        let usage = usage
            .map(|u| {
                let cache_read_tokens = u
                    .prompt_tokens_details
                    .as_ref()
                    .map_or(0, |details| details.cached_tokens);
                Usage {
                    input_tokens: u.prompt_tokens.saturating_sub(cache_read_tokens),
                    output_tokens: u.completion_tokens,
                    cache_read_tokens,
                    cache_write_tokens: 0,
                }
            })
            .unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<Usage> chain, not Result — None maps to Usage::default()

        Ok(CompletionResponse {
            id,
            model,
            stop_reason,
            content,
            usage,
            cost_usd: None,
            duration_ms: None,
        })
    }
}

impl ResponsesResponse {
    /// Convert a successful OpenAI Responses body into a hermeneus
    /// [`CompletionResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error string when the response contains neither output
    /// items nor an `output_text` convenience field.
    pub(crate) fn into_response(self) -> Result<CompletionResponse, Error> {
        let Self {
            id,
            model,
            status,
            incomplete_details,
            output,
            usage,
            output_text,
        } = self;

        let mut content = Vec::new();
        let mut saw_function_call = false;

        for item in output {
            match item {
                ResponsesOutputItem::Message { content: parts } => {
                    for part in parts {
                        if let ResponsesOutputContent::OutputText { text } = part
                            && !text.is_empty()
                        {
                            content.push(ContentBlock::Text {
                                text,
                                citations: None,
                            });
                        }
                    }
                }
                ResponsesOutputItem::FunctionCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    saw_function_call = true;
                    content.push(ContentBlock::ToolUse {
                        id: call_id,
                        name: name.clone(),
                        input: parse_arguments(&arguments, &name)?,
                    });
                }
                ResponsesOutputItem::Other => {}
            }
        }

        if content.is_empty()
            && let Some(text) = output_text
            && !text.is_empty()
        {
            content.push(ContentBlock::Text {
                text,
                citations: None,
            });
        }

        if content.is_empty() {
            return Err(ApiRequestSnafu {
                message: "OpenAI Responses response had no output".to_owned(),
            }
            .build());
        }

        let stop_reason = if saw_function_call {
            StopReason::ToolUse
        } else if status.as_deref() == Some("incomplete") {
            // WHY: The Responses API reports why a turn was incomplete. Map the
            // known provider reasons to our stop reasons and preserve anything
            // else as [`StopReason::Unknown`] so callers can see degraded output.
            match incomplete_details
                .as_ref()
                .and_then(|details| details.reason.as_deref())
            {
                Some("max_output_tokens") => StopReason::MaxTokens,
                Some("content_filter") => StopReason::ContentFiltered,
                _ => StopReason::Unknown,
            }
        } else {
            StopReason::EndTurn
        };

        let usage = usage
            .map(|u| {
                let cache_read_tokens = u
                    .input_tokens_details
                    .as_ref()
                    .map_or(0, |details| details.cached_tokens);
                Usage {
                    input_tokens: u.input_tokens.saturating_sub(cache_read_tokens),
                    output_tokens: u.output_tokens,
                    cache_read_tokens,
                    cache_write_tokens: 0,
                }
            })
            .unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<Usage> chain, not Result — None maps to Usage::default()

        Ok(CompletionResponse {
            id,
            model,
            stop_reason,
            content,
            usage,
            cost_usd: None,
            duration_ms: None,
        })
    }
}

/// Parse a provider's JSON-encoded tool arguments.
///
/// # Errors
///
/// Returns [`Error::MalformedToolArguments`] when the argument string is not
/// valid JSON. This prevents malformed provider output from reaching Nous tool
/// dispatch as a normal tool input.
pub(crate) fn parse_arguments(
    arguments: &str,
    tool_name: &str,
) -> Result<serde_json::Value, Error> {
    if arguments.is_empty() {
        Ok(serde_json::json!({}))
    } else {
        serde_json::from_str::<serde_json::Value>(arguments)
            .inspect_err(|e| {
                warn!(
                    error = %e,
                    tool = %tool_name,
                    raw_arguments = %arguments,
                    "OpenAI tool arguments JSON parse failed; rejecting provider tool call"
                );
            })
            .context(MalformedToolArgumentsSnafu {
                tool: tool_name.to_owned(),
            })
    }
}

/// Map OpenAI's `finish_reason` to our [`StopReason`].
///
/// OpenAI values: `stop`, `length`, `tool_calls`, `function_call` (legacy),
/// `content_filter`.
fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "length" => StopReason::MaxTokens,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "content_filter" => StopReason::ContentFiltered,
        "stop" | "" => StopReason::EndTurn,
        other => {
            // WHY: Unknown finish reasons are provider drift, not success.
            // Preserve the signal as [`StopReason::Unknown`] instead of
            // collapsing it into a clean end_turn.
            tracing::debug!(
                reason = other,
                "unknown OpenAI finish_reason; preserving as unknown"
            );
            StopReason::Unknown
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_response_round_trips() {
        let body = r#"{
            "id": "chatcmpl-123",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "hello" },
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.id, "chatcmpl-123");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 5);
        assert_eq!(resp.usage.output_tokens, 3);
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_response_becomes_tool_use_block() {
        let body = r#"{
            "id": "chatcmpl-tool",
            "model": "qwen",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_42",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Austin\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_42");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "Austin");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn content_filter_finish_reason_maps_to_content_filtered() {
        let body = r#"{
            "id": "chatcmpl-filter",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "" },
                "finish_reason": "content_filter",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.stop_reason, StopReason::ContentFiltered);
    }

    #[test]
    fn unknown_finish_reason_maps_to_unknown() {
        let body = r#"{
            "id": "chatcmpl-unknown",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "" },
                "finish_reason": "some_future_reason",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.stop_reason, StopReason::Unknown);
    }

    #[test]
    fn length_finish_reason_maps_to_max_tokens() {
        let body = r#"{
            "id": "chatcmpl-long",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "..." },
                "finish_reason": "length",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn response_with_no_usage_defaults_to_zero() {
        let body = r#"{
            "id": "chatcmpl-no-usage",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }

    #[test]
    fn chat_usage_extracts_cached_prompt_tokens() {
        let body = r#"{
            "id": "chatcmpl-cache",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "cached" },
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 2,
                "total_tokens": 11,
                "prompt_tokens_details": { "cached_tokens": 7 }
            }
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.usage.input_tokens, 2);
        assert_eq!(resp.usage.output_tokens, 2);
        assert_eq!(resp.usage.cache_read_tokens, 7);
        assert_eq!(resp.usage.cache_write_tokens, 0);
    }

    #[test]
    fn responses_usage_extracts_cached_input_tokens() {
        let body = r#"{
            "id": "resp-cache",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "cached" }]
            }],
            "usage": {
                "input_tokens": 12,
                "output_tokens": 3,
                "total_tokens": 15,
                "input_tokens_details": { "cached_tokens": 8 }
            }
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.usage.input_tokens, 4);
        assert_eq!(resp.usage.output_tokens, 3);
        assert_eq!(resp.usage.cache_read_tokens, 8);
        assert_eq!(resp.usage.cache_write_tokens, 0);
    }

    #[test]
    fn empty_arguments_string_becomes_empty_object() {
        let body = r#"{
            "id": "x",
            "model": "m",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "c",
                        "type": "function",
                        "function": { "name": "f", "arguments": "" }
                    }]
                },
                "finish_reason": "tool_calls",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        match &resp.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert!(input.as_object().is_some_and(serde_json::Map::is_empty));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn malformed_arguments_reject_parse_error() {
        let body = r#"{
            "id": "x",
            "model": "m",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "c",
                        "type": "function",
                        "function": { "name": "f", "arguments": "{not json" }
                    }]
                },
                "finish_reason": "tool_calls",
                "index": 0
            }]
        }"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(body).unwrap();
        let err = parsed.into_response().expect_err(
            "malformed Chat tool arguments should be rejected as a provider contract failure",
        );
        assert!(
            matches!(err, Error::MalformedToolArguments { .. }),
            "expected MalformedToolArguments, got {err:?}"
        );
    }

    #[test]
    fn responses_malformed_arguments_reject_parse_error() {
        let body = r#"{
            "id": "resp-malformed",
            "model": "m",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "c",
                "name": "f",
                "arguments": "{not json"
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let err = parsed.into_response().expect_err(
            "malformed Responses function arguments should be rejected as a provider contract failure",
        );
        assert!(
            matches!(err, Error::MalformedToolArguments { .. }),
            "expected MalformedToolArguments, got {err:?}"
        );
    }
}
