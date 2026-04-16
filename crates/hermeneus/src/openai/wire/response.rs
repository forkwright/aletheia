//! Parse OpenAI Chat Completions responses into [`CompletionResponse`].
//!
//! Maps `finish_reason` → [`StopReason`] and `tool_calls` → structured
//! [`ContentBlock::ToolUse`]. The OpenAI response shape is stable across
//! OpenAI proper, llama.cpp, ollama, and vllm.

use serde::Deserialize;

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
    // OpenAI does not expose cache-tier tokens; leave zero.
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
    pub(crate) fn into_response(self) -> Result<CompletionResponse, String> {
        let Self {
            id,
            model,
            choices,
            usage,
        } = self;

        let mut choices_iter = choices.into_iter();
        let choice = choices_iter
            .next()
            .ok_or_else(|| "OpenAI response had no choices".to_owned())?;

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
            let input: serde_json::Value = if call.function.arguments.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::String(call.function.arguments.clone()))
            };
            content.push(ContentBlock::ToolUse {
                id: call.id,
                name: call.function.name,
                input,
            });
        }

        let usage = usage
            .map(|u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            })
            .unwrap_or_default();

        Ok(CompletionResponse {
            id,
            model,
            stop_reason,
            content,
            usage,
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
        "stop" | "content_filter" | "" => StopReason::EndTurn,
        other => {
            tracing::debug!(
                reason = other,
                "unknown OpenAI finish_reason; treating as end_turn"
            );
            StopReason::EndTurn
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test: indices asserted valid by construction")]
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
}
