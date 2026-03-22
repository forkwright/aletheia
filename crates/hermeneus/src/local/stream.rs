//! SSE stream parser for OpenAI-compatible streaming responses.
//!
//! Parses `data: {...}\n\n` server-sent events from an HTTP response body,
//! accumulates tool call fragments, and emits [`StreamEvent`]s to a callback.
//! The stream terminates on `data: [DONE]`.

use reqwest::Response;

use super::types::{ChatChunkToolCall, ChatCompletionChunk};
use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::types::{StopReason, Usage};

/// Accumulated state for a tool call being streamed incrementally.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

/// Accumulator for the full streaming response.
pub(crate) struct StreamAccumulator {
    id: String,
    model: String,
    text: String,
    tool_calls: Vec<ToolCallAccumulator>,
    finish_reason: Option<String>,
    usage: Option<(u64, u64)>,
    /// Whether any content has been emitted (for retry safety).
    has_content: bool,
}

impl StreamAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            text: String::new(),
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
            has_content: false,
        }
    }

    /// Whether any content events have been emitted.
    pub(crate) fn has_content(&self) -> bool {
        self.has_content
    }

    /// Process a single chunk, emitting stream events to the callback.
    pub(crate) fn process_chunk(
        &mut self,
        chunk: &ChatCompletionChunk,
        on_event: &mut dyn FnMut(StreamEvent),
    ) {
        if self.id.is_empty() {
            self.id.clone_from(&chunk.id);
            self.model.clone_from(&chunk.model);
            on_event(StreamEvent::MessageStart {
                usage: Usage::default(),
            });
        }

        if let Some(usage) = &chunk.usage {
            self.usage = Some((usage.prompt_tokens, usage.completion_tokens));
        }

        for choice in &chunk.choices {
            if let Some(text) = &choice.delta.content
                && !text.is_empty()
            {
                if !self.has_content {
                    on_event(StreamEvent::ContentBlockStart {
                        index: 0,
                        block_type: "text".to_owned(),
                    });
                    self.has_content = true;
                }
                self.text.push_str(text);
                on_event(StreamEvent::TextDelta { text: text.clone() });
            }

            if let Some(tool_calls) = &choice.delta.tool_calls {
                for tc in tool_calls {
                    self.accumulate_tool_call(tc, on_event);
                }
            }

            if let Some(reason) = &choice.finish_reason {
                self.finish_reason = Some(reason.clone());
            }
        }
    }

    fn accumulate_tool_call(
        &mut self,
        tc: &ChatChunkToolCall,
        on_event: &mut dyn FnMut(StreamEvent),
    ) {
        let idx = usize::try_from(tc.index).unwrap_or(0);

        // Grow the accumulator vec if needed.
        while self.tool_calls.len() <= idx {
            self.tool_calls.push(ToolCallAccumulator::default());
        }

        // SAFETY: We just ensured self.tool_calls.len() > idx in the while loop above.
        #[expect(
            clippy::indexing_slicing,
            reason = "idx is bounded: while loop above grows vec to len > idx"
        )]
        let acc = &mut self.tool_calls[idx]; // kanon:ignore RUST/indexing-slicing

        if let Some(id) = &tc.id {
            acc.id.clone_from(id);
        }

        if let Some(func) = &tc.function {
            if let Some(name) = &func.name {
                acc.name.clone_from(name);
                // WHY: tool_use block index offset by 1 if text block exists.
                let block_index = if self.text.is_empty() { idx } else { idx + 1 };
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "block index is bounded by tool call count, always fits in u32"
                )]
                #[expect(
                    clippy::as_conversions,
                    reason = "block index is bounded by tool call count, safe truncation"
                )]
                let index = block_index as u32; // kanon:ignore RUST/as-cast
                on_event(StreamEvent::ContentBlockStart {
                    index,
                    block_type: "tool_use".to_owned(),
                });
                self.has_content = true;
            }
            if let Some(args) = &func.arguments
                && !args.is_empty()
            {
                acc.arguments.push_str(args);
                on_event(StreamEvent::InputJsonDelta {
                    partial_json: args.clone(),
                });
            }
        }
    }

    /// Build the final stop reason from the accumulated `finish_reason`.
    fn stop_reason(&self) -> StopReason {
        map_finish_reason(self.finish_reason.as_deref())
    }

    /// Finalize and return the accumulated usage.
    fn final_usage(&self) -> Usage {
        match self.usage {
            Some((input, output)) => Usage {
                input_tokens: input,
                output_tokens: output,
                ..Usage::default()
            },
            None => Usage::default(),
        }
    }
}

/// Map a `finish_reason` string to a [`StopReason`].
fn map_finish_reason(reason: Option<&str>) -> StopReason {
    match reason {
        Some("tool_calls") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("stop" | _) | None => StopReason::EndTurn,
    }
}

/// Parse OpenAI-format SSE events from a live HTTP response stream.
///
/// Reads the response body chunk-by-chunk, parses `data: {...}` lines,
/// and emits [`StreamEvent`]s to the callback. The stream terminates
/// on `data: [DONE]` or EOF.
///
/// # Errors
///
/// Returns an error on transport failure or malformed JSON in a chunk.
pub(crate) async fn parse_openai_stream(
    response: &mut Response,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<(crate::types::CompletionResponse, bool)> {
    let mut acc = StreamAccumulator::new();
    let mut line_buf: Vec<u8> = Vec::with_capacity(512);

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

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        line_buf.clear();
                        // Emit final stop events.
                        let stop_reason = acc.stop_reason();
                        let usage = acc.final_usage();
                        on_event(StreamEvent::MessageStop { stop_reason, usage });
                        return Ok((build_response(&acc), acc.has_content()));
                    }

                    let chunk: ChatCompletionChunk = serde_json::from_str(data).map_err(|e| {
                        error::ApiRequestSnafu {
                            message: format!("stream chunk parse error: {e}"),
                        }
                        .build()
                    })?;
                    acc.process_chunk(&chunk, on_event);
                }
                // NOTE: Ignore non-data lines (event:, id:, comments, blank).
                line_buf.clear();
            } else {
                line_buf.push(byte);
            }
        }
    }

    // EOF without [DONE] — build response from what we have.
    let stop_reason = acc.stop_reason();
    let usage = acc.final_usage();
    on_event(StreamEvent::MessageStop { stop_reason, usage });
    Ok((build_response(&acc), acc.has_content()))
}

/// Parse OpenAI-format SSE events from a byte reader (for tests).
#[cfg(test)]
pub(crate) fn parse_openai_stream_sync(
    mut reader: impl std::io::BufRead,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<crate::types::CompletionResponse> {
    let mut acc = StreamAccumulator::new();
    let mut raw_line: Vec<u8> = Vec::new();

    loop {
        raw_line.clear();
        let n = reader.read_until(b'\n', &mut raw_line).map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("stream read error: {e}"),
            }
            .build()
        })?;

        if n == 0 {
            break;
        }

        let line_cow = String::from_utf8_lossy(&raw_line);
        let line = line_cow.trim_end_matches(['\n', '\r']);

        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                let stop_reason = acc.stop_reason();
                let usage = acc.final_usage();
                on_event(StreamEvent::MessageStop { stop_reason, usage });
                return Ok(build_response(&acc));
            }

            let chunk: ChatCompletionChunk = serde_json::from_str(data).map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!("stream chunk parse error: {e}"),
                }
                .build()
            })?;
            acc.process_chunk(&chunk, on_event);
        }
    }

    let stop_reason = acc.stop_reason();
    let usage = acc.final_usage();
    on_event(StreamEvent::MessageStop { stop_reason, usage });
    Ok(build_response(&acc))
}

/// Build a [`CompletionResponse`] from accumulated stream state.
fn build_response(acc: &StreamAccumulator) -> crate::types::CompletionResponse {
    use crate::types::{CompletionResponse, ContentBlock};

    let mut content = Vec::new();

    if !acc.text.is_empty() {
        content.push(ContentBlock::Text {
            text: acc.text.clone(),
            citations: None,
        });
    }

    for tc in &acc.tool_calls {
        // WHY: Parse arguments as JSON; fall back to empty object on parse failure
        // (vLLM can send empty arguments for zero-parameter tools).
        let input = serde_json::from_str(&tc.arguments)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        content.push(ContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input,
        });
    }

    CompletionResponse {
        id: acc.id.clone(),
        model: acc.model.clone(),
        stop_reason: acc.stop_reason(),
        content,
        usage: acc.final_usage(),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: index 0 is valid after asserting content.len()"
)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, StopReason};

    #[test]
    fn parses_simple_text_stream() {
        let sse = "\
data: {\"id\":\"chatcmpl-1\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-1\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-1\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
\n\
data: [DONE]\n\
\n";

        let mut events = Vec::new();
        let response =
            parse_openai_stream_sync(std::io::Cursor::new(sse), &mut |e| events.push(e)).unwrap();

        assert_eq!(response.id, "chatcmpl-1");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Hello world"),
            other => panic!("expected Text, got: {other:?}"),
        }

        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta { text } if text == "Hello"))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta { text } if text == " world"))
        );
    }

    #[test]
    fn parses_tool_call_stream() {
        let sse = "\
data: {\"id\":\"chatcmpl-2\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"exec\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-2\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\"\"}}]},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-2\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\": \\\"ls\\\"}\"}}]},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-2\",\"model\":\"local/qwen\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\
\n\
data: [DONE]\n\
\n";

        let mut events = Vec::new();
        let response =
            parse_openai_stream_sync(std::io::Cursor::new(sse), &mut |e| events.push(e)).unwrap();

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "exec");
                assert_eq!(input["cmd"], "ls");
            }
            other => panic!("expected ToolUse, got: {other:?}"),
        }
    }

    #[test]
    fn malformed_json_returns_error() {
        let sse = "data: not valid json\n\n";
        let result = parse_openai_stream_sync(std::io::Cursor::new(sse), &mut |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn empty_stream_returns_defaults() {
        let mut events = Vec::new();
        let response =
            parse_openai_stream_sync(std::io::Cursor::new(""), &mut |e| events.push(e)).unwrap();
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert!(response.content.is_empty());
    }
}
