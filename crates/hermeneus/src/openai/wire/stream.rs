//! Parse OpenAI Chat Completions SSE streams into hermeneus stream events.
//!
//! OpenAI streams `data: {chunk}` lines terminated by `data: [DONE]`. Each
//! chunk carries incremental `delta` fields that mirror the non-streaming
//! `message` shape. This module accumulates them into a
//! [`CompletionResponse`] while emitting [`StreamEvent`]s for live UI.

use std::collections::BTreeMap;

use reqwest::Response;
use serde::Deserialize;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

#[derive(Debug, Deserialize)]
struct ChatStreamChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    choices: Vec<ChatStreamChoice>,
    #[serde(default)]
    usage: Option<ChatStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamChoice {
    delta: ChatStreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatStreamToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamToolCallDelta {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatStreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// Accumulator for an OpenAI SSE stream.
///
/// Mirrors the Anthropic `StreamAccumulator` surface so the caller's
/// `on_event` callback receives the same `StreamEvent` variants regardless
/// of provider.
pub(crate) struct OpenAiStreamAccumulator {
    id: String,
    model: String,
    text_buf: String,
    /// Pending tool calls indexed by their `index` field — OpenAI streams
    /// tool calls in interleaved deltas, each tagged by a stable position.
    tool_calls: BTreeMap<u32, PendingToolCall>,
    stop_reason: StopReason,
    usage: Usage,
    /// Tracks whether a text block has been announced via
    /// `ContentBlockStart` — used to suppress duplicate start events across
    /// chunks.
    text_block_open: bool,
}

struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAiStreamAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            text_buf: String::new(),
            tool_calls: BTreeMap::new(),
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
            text_block_open: false,
        }
    }

    fn process_chunk<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        chunk: ChatStreamChunk,
        on_event: &mut F,
    ) {
        if let Some(id) = chunk.id
            && self.id.is_empty()
        {
            self.id = id;
        }
        if let Some(model) = chunk.model
            && self.model.is_empty()
        {
            self.model = model;
            // Emit a MessageStart so the accumulator contract matches the
            // Anthropic provider — downstream code expects one per stream.
            on_event(StreamEvent::MessageStart { usage: self.usage });
        }

        if let Some(usage) = chunk.usage {
            self.usage.input_tokens = usage.prompt_tokens;
            self.usage.output_tokens = usage.completion_tokens;
        }

        for choice in chunk.choices {
            self.apply_delta(choice, on_event);
        }
    }

    fn apply_delta<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        choice: ChatStreamChoice,
        on_event: &mut F,
    ) {
        // Text delta → TextDelta event + append to buffer.
        if let Some(text) = choice.delta.content
            && !text.is_empty()
        {
            if !self.text_block_open {
                on_event(StreamEvent::ContentBlockStart {
                    index: 0,
                    block_type: "text".to_owned(),
                });
                self.text_block_open = true;
            }
            self.text_buf.push_str(&text);
            on_event(StreamEvent::TextDelta { text });
        }

        // Tool-call deltas → buffered, partial_json emitted per increment.
        for tc in choice.delta.tool_calls {
            let pending = self
                .tool_calls
                .entry(tc.index)
                .or_insert_with(|| PendingToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
            if let Some(id) = tc.id
                && !id.is_empty()
            {
                pending.id = id;
            }
            if let Some(func) = tc.function {
                if let Some(name) = func.name
                    && !name.is_empty()
                {
                    pending.name = name;
                }
                if let Some(args) = func.arguments
                    && !args.is_empty()
                {
                    pending.arguments.push_str(&args);
                    on_event(StreamEvent::InputJsonDelta { partial_json: args });
                }
            }
        }

        if let Some(reason) = choice.finish_reason {
            self.stop_reason = map_finish_reason(&reason);
        }
    }

    pub(crate) fn finish<F: FnMut(StreamEvent) + ?Sized>(
        self,
        on_event: &mut F,
    ) -> CompletionResponse {
        let Self {
            id,
            model,
            text_buf,
            tool_calls,
            stop_reason,
            usage,
            text_block_open,
        } = self;

        if text_block_open {
            on_event(StreamEvent::ContentBlockStop { index: 0 });
        }

        let mut content: Vec<ContentBlock> = Vec::new();
        if !text_buf.is_empty() {
            content.push(ContentBlock::Text {
                text: text_buf,
                citations: None,
            });
        }
        for (_, tc) in tool_calls {
            let input: serde_json::Value = if tc.arguments.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&tc.arguments)
                    .unwrap_or(serde_json::Value::String(tc.arguments.clone()))
            };
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.name,
                input,
            });
        }

        on_event(StreamEvent::MessageStop { stop_reason, usage });

        CompletionResponse {
            id,
            model,
            stop_reason,
            content,
            usage,
        }
    }
}

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "length" => StopReason::MaxTokens,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    }
}

/// Parse an OpenAI SSE stream from a `reqwest::Response`, emitting
/// [`StreamEvent`]s and returning the finalized [`CompletionResponse`].
#[tracing::instrument(skip_all)]
pub(crate) async fn parse_sse_response(
    response: &mut Response,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<CompletionResponse> {
    let mut accumulator = OpenAiStreamAccumulator::new();
    let mut line_buf: Vec<u8> = Vec::with_capacity(256);
    let mut current_data = String::new();
    let mut done = false;

    loop {
        let chunk = response.chunk().await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("stream read error: {e}"),
            }
            .build()
        })?;
        let Some(bytes) = chunk else { break };

        for &byte in &bytes {
            if byte == b'\n' {
                let line_cow = String::from_utf8_lossy(&line_buf);
                let line = line_cow.trim_end_matches('\r');
                if line.is_empty() {
                    if !current_data.is_empty() {
                        if current_data.trim() == "[DONE]" {
                            done = true;
                        } else {
                            match serde_json::from_str::<ChatStreamChunk>(&current_data) {
                                Ok(chunk) => accumulator.process_chunk(chunk, on_event),
                                Err(e) => {
                                    return Err(error::ApiRequestSnafu {
                                        message: format!("stream parse error: {e}"),
                                    }
                                    .build());
                                }
                            }
                        }
                    }
                    current_data.clear();
                } else if let Some(data) = line.strip_prefix("data: ") {
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(data);
                } else if let Some(data) = line.strip_prefix("data:") {
                    // llama.cpp emits `data:{...}` without the space.
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(data);
                }
                // Ignore comments, empty `:` lines, event:, id:, retry:
                line_buf.clear();
            } else {
                line_buf.push(byte);
            }
        }

        if done {
            break;
        }
    }

    Ok(accumulator.finish(on_event))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
mod tests {
    use super::*;

    fn process_chunks(chunks: &[&str]) -> (Vec<StreamEvent>, CompletionResponse) {
        let mut acc = OpenAiStreamAccumulator::new();
        let mut events = Vec::new();
        for chunk_json in chunks {
            let chunk: ChatStreamChunk = serde_json::from_str(chunk_json).unwrap();
            acc.process_chunk(chunk, &mut |e| events.push(e));
        }
        let resp = acc.finish(&mut |e| events.push(e));
        (events, resp)
    }

    #[test]
    fn accumulates_text_deltas() {
        let (events, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"content":"Hel"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"content":"lo"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        ]);
        match &resp.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("expected Text"),
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta { text } if text == "Hel")),
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta { text } if text == "lo")),
        );
    }

    #[test]
    fn accumulates_tool_call_deltas() {
        let (_, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{\"a\":"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "f");
                assert_eq!(input["a"], 1);
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn usage_propagates_when_present() {
        let (_, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"content":"hi"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1}}"#,
        ]);
        assert_eq!(resp.usage.input_tokens, 4);
        assert_eq!(resp.usage.output_tokens, 1);
    }
}
