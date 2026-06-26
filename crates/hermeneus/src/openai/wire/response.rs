//! Parse OpenAI Chat Completions responses into [`CompletionResponse`].
//!
//! Maps `finish_reason` → [`StopReason`] and `tool_calls` → structured
//! [`ContentBlock::ToolUse`]. The OpenAI response shape is stable across
//! OpenAI proper, llama.cpp, ollama, and vllm.

use serde::Deserialize;
use serde::de::Error as _;
use tracing::warn;

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

/// One item in the OpenAI Responses API `output` array.
///
/// Deserialized manually so unknown future item types are captured with their
/// raw JSON and provider type instead of being silently dropped.
#[derive(Debug)]
pub(crate) enum ResponsesOutputItem {
    Message {
        content: Vec<ResponsesOutputContent>,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    Reasoning {
        id: String,
        summary: Vec<ReasoningSummary>,
    },
    WebSearchCall {
        id: String,
    },
    FileSearchCall {
        id: String,
    },
    ComputerCall {
        call_id: String,
        action: serde_json::Value,
    },
    ComputerCallOutput {
        call_id: String,
        output: serde_json::Value,
    },
    Unsupported {
        provider_type: String,
        raw: serde_json::Value,
    },
}

impl<'de> Deserialize<'de> for ResponsesOutputItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let provider_type = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned();
        match provider_type.as_str() {
            "message" => {
                let content = value
                    .get("content")
                    .map(|v| Vec::<ResponsesOutputContent>::deserialize(v))
                    .transpose()
                    .map_err(D::Error::custom)?
                    .unwrap_or_default();
                Ok(Self::Message { content })
            }
            "function_call" => {
                let call_id = value
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let name = value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let arguments = value
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Self::FunctionCall {
                    call_id,
                    name,
                    arguments,
                })
            }
            "reasoning" => {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let summary = value
                    .get("summary")
                    .map(|v| Vec::<ReasoningSummary>::deserialize(v))
                    .transpose()
                    .map_err(D::Error::custom)?
                    .unwrap_or_default();
                Ok(Self::Reasoning { id, summary })
            }
            "web_search_call" => {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Self::WebSearchCall { id })
            }
            "file_search_call" => {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Self::FileSearchCall { id })
            }
            "computer_call" => {
                let call_id = value
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let action = value.get("action").cloned().unwrap_or_default();
                Ok(Self::ComputerCall { call_id, action })
            }
            "computer_call_output" => {
                let call_id = value
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let output = value.get("output").cloned().unwrap_or_default();
                Ok(Self::ComputerCallOutput { call_id, output })
            }
            _ => Ok(Self::Unsupported { provider_type, raw: value }),
        }
    }
}

/// One content part inside a Responses API `message` output item.
#[derive(Debug)]
pub(crate) enum ResponsesOutputContent {
    OutputText { text: String },
    Refusal { refusal: String },
    Unsupported {
        provider_type: String,
        raw: serde_json::Value,
    },
}

impl<'de> Deserialize<'de> for ResponsesOutputContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let provider_type = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned();
        match provider_type.as_str() {
            "output_text" => {
                let text = value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Self::OutputText { text })
            }
            "refusal" => {
                let refusal = value
                    .get("refusal")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Self::Refusal { refusal })
            }
            _ => Ok(Self::Unsupported { provider_type, raw: value }),
        }
    }
}

/// A single summary entry inside a Responses API `reasoning` item.
#[derive(Debug, Deserialize)]
pub(crate) struct ReasoningSummary {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
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
            let input = parse_arguments(&call.function.arguments);
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
    pub(crate) fn into_response(self) -> Result<CompletionResponse, String> {
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
                        match part {
                            ResponsesOutputContent::OutputText { text } if !text.is_empty() => {
                                content.push(ContentBlock::Text {
                                    text,
                                    citations: None,
                                });
                            }
                            ResponsesOutputContent::Refusal { refusal } => {
                                content.push(ContentBlock::Unsupported {
                                    kind: "content".to_owned(),
                                    provider_type: Some("refusal".to_owned()),
                                    detail: Some(serde_json::json!({ "refusal": refusal })),
                                });
                            }
                            ResponsesOutputContent::Unsupported {
                                provider_type,
                                raw,
                            } => {
                                content.push(ContentBlock::Unsupported {
                                    kind: "content".to_owned(),
                                    provider_type: Some(provider_type),
                                    detail: Some(raw),
                                });
                            }
                            // WHY: Empty output_text is intentionally elided; it carries
                            // no information and would pollute the content array.
                            ResponsesOutputContent::OutputText { .. } => {}
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
                        name,
                        input: parse_arguments(&arguments),
                    });
                }
                ResponsesOutputItem::Reasoning { id, summary } => {
                    let thinking: String = summary
                        .into_iter()
                        .filter_map(|part| part.text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !thinking.is_empty() {
                        content.push(ContentBlock::Thinking {
                            thinking,
                            signature: None,
                        });
                    } else {
                        content.push(ContentBlock::Unsupported {
                            kind: "output_item".to_owned(),
                            provider_type: Some("reasoning".to_owned()),
                            detail: Some(serde_json::json!({ "id": id })),
                        });
                    }
                }
                ResponsesOutputItem::WebSearchCall { id } => {
                    content.push(ContentBlock::ServerToolUse {
                        id,
                        name: "web_search".to_owned(),
                        input: serde_json::Value::Null,
                    });
                }
                ResponsesOutputItem::FileSearchCall { id } => {
                    content.push(ContentBlock::ServerToolUse {
                        id,
                        name: "file_search".to_owned(),
                        input: serde_json::Value::Null,
                    });
                }
                ResponsesOutputItem::ComputerCall { call_id, action } => {
                    content.push(ContentBlock::ServerToolUse {
                        id: call_id,
                        name: "computer".to_owned(),
                        input: action,
                    });
                }
                ResponsesOutputItem::ComputerCallOutput { call_id, output } => {
                    content.push(ContentBlock::WebSearchToolResult {
                        tool_use_id: call_id,
                        content: output,
                    });
                }
                ResponsesOutputItem::Unsupported {
                    provider_type,
                    raw,
                } => {
                    content.push(ContentBlock::Unsupported {
                        kind: "output_item".to_owned(),
                        provider_type: Some(provider_type),
                        detail: Some(raw),
                    });
                }
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
            return Err("OpenAI Responses response had no output".to_owned());
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

pub(crate) fn parse_arguments(arguments: &str) -> serde_json::Value {
    if arguments.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    error = %e,
                    raw_arguments = %arguments,
                    "OpenAI tool arguments JSON parse failed; returning error object to agent"
                );
                serde_json::json!({
                    "_parse_error": format!("malformed tool input: {e}"),
                    "_raw_input": arguments,
                })
            }
        }
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
    fn malformed_arguments_becomes_parse_error_object() {
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
        let resp = parsed.into_response().unwrap();
        match &resp.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert!(
                    input.is_object(),
                    "malformed arguments must be an object, not a string: {input:?}"
                );
                let Some(obj) = input.as_object() else {
                    panic!("malformed arguments should produce an object: {input:?}");
                };
                assert!(
                    obj.get("_parse_error")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.starts_with("malformed tool input:")),
                    "expected _parse_error field: {input:?}"
                );
                assert!(
                    obj.contains_key("_raw_input"),
                    "expected _raw_input field: {input:?}"
                );
                assert!(
                    obj.get("_raw_input")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s == "{not json"),
                    "_raw_input should preserve raw argument string: {input:?}"
                );
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn responses_malformed_arguments_becomes_parse_error_object() {
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
        let resp = parsed.into_response().unwrap();
        match &resp.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert!(
                    input.is_object(),
                    "malformed arguments must be an object, not a string: {input:?}"
                );
                let Some(obj) = input.as_object() else {
                    panic!("malformed arguments should produce an object: {input:?}");
                };
                assert!(
                    obj.get("_parse_error")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.starts_with("malformed tool input:")),
                    "expected _parse_error field: {input:?}"
                );
                assert!(
                    obj.contains_key("_raw_input"),
                    "expected _raw_input field: {input:?}"
                );
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn responses_reasoning_item_becomes_thinking_block() {
        let body = r#"{
            "id": "resp-reasoning",
            "model": "o3",
            "status": "completed",
            "output": [{
                "type": "reasoning",
                "id": "rs_1",
                "summary": [
                    { "type": "thinking", "text": "First thought." },
                    { "type": "thinking", "text": "Second thought." }
                ]
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Thinking { thinking, signature } => {
                assert_eq!(thinking, "First thought.\nSecond thought.");
                assert!(signature.is_none());
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn responses_refusal_content_becomes_unsupported_block() {
        let body = r#"{
            "id": "resp-refusal",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{ "type": "refusal", "refusal": "I cannot answer that." }]
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Unsupported {
                kind,
                provider_type,
                detail,
            } => {
                assert_eq!(kind, "content");
                assert_eq!(provider_type.as_deref(), Some("refusal"));
                let detail = detail.as_ref().expect("detail should be present");
                assert_eq!(
                    detail.get("refusal").and_then(|v| v.as_str()),
                    Some("I cannot answer that.")
                );
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn responses_web_search_call_becomes_server_tool_use() {
        let body = r#"{
            "id": "resp-web-search",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "web_search_call",
                "id": "ws_1",
                "status": "completed"
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "ws_1");
                assert_eq!(name, "web_search");
                assert!(input.is_null());
            }
            other => panic!("expected ServerToolUse, got {other:?}"),
        }
    }

    #[test]
    fn responses_file_search_call_becomes_server_tool_use() {
        let body = r#"{
            "id": "resp-file-search",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "file_search_call",
                "id": "fs_1",
                "status": "completed"
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "fs_1");
                assert_eq!(name, "file_search");
                assert!(input.is_null());
            }
            other => panic!("expected ServerToolUse, got {other:?}"),
        }
    }

    #[test]
    fn responses_computer_call_becomes_server_tool_use() {
        let body = r#"{
            "id": "resp-computer",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "computer_call",
                "call_id": "cc_1",
                "action": { "type": "bash", "command": "echo hi" }
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "cc_1");
                assert_eq!(name, "computer");
                assert_eq!(input.get("type").and_then(|v| v.as_str()), Some("bash"));
            }
            other => panic!("expected ServerToolUse, got {other:?}"),
        }
    }

    #[test]
    fn responses_computer_call_output_becomes_web_search_tool_result() {
        let body = r#"{
            "id": "resp-computer-out",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "computer_call_output",
                "call_id": "cc_1",
                "output": { "type": "input_text", "text": "hi" }
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "cc_1");
                assert_eq!(content.get("type").and_then(|v| v.as_str()), Some("input_text"));
            }
            other => panic!("expected WebSearchToolResult, got {other:?}"),
        }
    }

    #[test]
    fn responses_unknown_output_item_becomes_unsupported_block() {
        let body = r#"{
            "id": "resp-unknown",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "future_unknown_item",
                "id": "fu_1",
                "extra_field": 42
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Unsupported {
                kind,
                provider_type,
                detail,
            } => {
                assert_eq!(kind, "output_item");
                assert_eq!(provider_type.as_deref(), Some("future_unknown_item"));
                let detail = detail.as_ref().expect("detail should be present");
                assert_eq!(detail.get("id").and_then(|v| v.as_str()), Some("fu_1"));
                assert_eq!(detail.get("extra_field").and_then(|v| v.as_i64()), Some(42));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn responses_unknown_content_type_becomes_unsupported_block() {
        let body = r#"{
            "id": "resp-unknown-content",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{ "type": "future_unknown_content", "data": "xyz" }]
            }]
        }"#;
        let parsed: ResponsesResponse = serde_json::from_str(body).unwrap();
        let resp = parsed.into_response().unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Unsupported {
                kind,
                provider_type,
                detail,
            } => {
                assert_eq!(kind, "content");
                assert_eq!(provider_type.as_deref(), Some("future_unknown_content"));
                let detail = detail.as_ref().expect("detail should be present");
                assert_eq!(detail.get("data").and_then(|v| v.as_str()), Some("xyz"));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
