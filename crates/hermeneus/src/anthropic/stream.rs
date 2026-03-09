//! SSE streaming parser for the Anthropic Messages API.
//!
//! Reads server-sent events from an HTTP response body, accumulates content
//! blocks into a final [`CompletionResponse`], and emits [`StreamEvent`]s
//! to a callback for real-time UI updates.

use std::io::BufRead;

use tracing::warn;

use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

use super::wire::{WireContentBlockStart, WireDelta, WireStreamEvent, WireUsage};

/// Event emitted during streaming completion.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StreamEvent {
    /// Incremental text content.
    TextDelta { text: String },
    /// Incremental thinking content.
    ThinkingDelta { thinking: String },
    /// Incremental tool input JSON.
    InputJsonDelta { partial_json: String },
    /// A content block has started.
    ContentBlockStart {
        /// Zero-based position in the response content array.
        index: u32,
        /// Block type: `"text"`, `"tool_use"`, or `"thinking"`.
        block_type: String,
    },
    /// A content block has finished.
    ContentBlockStop {
        /// Zero-based position of the completed block.
        index: u32,
    },
    /// Message started with initial usage.
    MessageStart {
        /// Input token counts reported at message start.
        usage: Usage,
    },
    /// Message finished with final stop reason and usage.
    MessageStop {
        /// Why the model stopped generating.
        stop_reason: StopReason,
        /// Final cumulative token usage for the entire message.
        usage: Usage,
    },
}

/// Accumulator state for building a `CompletionResponse` from SSE events.
pub(crate) struct StreamAccumulator {
    id: String,
    model: String,
    stop_reason: Option<StopReason>,
    blocks: Vec<BlockBuilder>,
    input_tokens: u64,
    output_tokens: u64,
    cache_write_tokens: u64,
    cache_read_tokens: u64,
}

/// Builder for a single content block being streamed.
enum BlockBuilder {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    Thinking {
        text: String,
        signature: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    WebSearchToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
    CodeExecutionResult {
        code: String,
        stdout: String,
        stderr: String,
        return_code: i32,
    },
}

impl StreamAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            stop_reason: None,
            blocks: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    /// Process an SSE event, emitting `StreamEvent`s via the callback.
    /// Returns `Err` if the stream contains an error event.
    #[expect(
        clippy::too_many_lines,
        reason = "SSE event dispatch is inherently branchy"
    )]
    pub(crate) fn process_event(
        &mut self,
        event: WireStreamEvent,
        on_event: &mut impl FnMut(StreamEvent),
    ) -> Result<()> {
        match event {
            WireStreamEvent::MessageStart { message } => {
                self.id.clone_from(&message.id);
                self.model.clone_from(&message.model);
                let usage = convert_wire_usage(&message.usage);
                self.input_tokens = usage.input_tokens;
                self.cache_write_tokens = usage.cache_write_tokens;
                self.cache_read_tokens = usage.cache_read_tokens;
                on_event(StreamEvent::MessageStart { usage });
            }
            WireStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let block_type = match &content_block {
                    WireContentBlockStart::Text { .. } => "text",
                    WireContentBlockStart::ToolUse { .. } => "tool_use",
                    WireContentBlockStart::Thinking { .. } => "thinking",
                    WireContentBlockStart::ServerToolUse { .. } => "server_tool_use",
                    WireContentBlockStart::WebSearchToolResult { .. } => "web_search_tool_result",
                    WireContentBlockStart::CodeExecutionResult { .. } => "code_execution_result",
                };
                on_event(StreamEvent::ContentBlockStart {
                    index,
                    block_type: block_type.to_owned(),
                });

                let builder = match content_block {
                    WireContentBlockStart::Text { text } => BlockBuilder::Text(text),
                    WireContentBlockStart::ToolUse { id, name } => BlockBuilder::ToolUse {
                        id,
                        name,
                        input_json: String::new(),
                    },
                    WireContentBlockStart::Thinking { thinking } => BlockBuilder::Thinking {
                        text: thinking,
                        signature: String::new(),
                    },
                    WireContentBlockStart::ServerToolUse { id, name } => {
                        BlockBuilder::ServerToolUse {
                            id,
                            name,
                            input_json: String::new(),
                        }
                    }
                    WireContentBlockStart::WebSearchToolResult { tool_use_id } => {
                        BlockBuilder::WebSearchToolResult {
                            tool_use_id,
                            content: serde_json::Value::Null,
                        }
                    }
                    WireContentBlockStart::CodeExecutionResult {} => {
                        BlockBuilder::CodeExecutionResult {
                            code: String::new(),
                            stdout: String::new(),
                            stderr: String::new(),
                            return_code: 0,
                        }
                    }
                };
                // Ensure the blocks vec is large enough.
                let idx = index as usize;
                while self.blocks.len() <= idx {
                    self.blocks.push(BlockBuilder::Text(String::new()));
                }
                self.blocks[idx] = builder;
            }
            WireStreamEvent::ContentBlockDelta { index, delta } => {
                let idx = index as usize;
                if idx < self.blocks.len() {
                    match delta {
                        WireDelta::TextDelta { text } => {
                            if let BlockBuilder::Text(ref mut buf) = self.blocks[idx] {
                                buf.push_str(&text);
                            }
                            on_event(StreamEvent::TextDelta { text });
                        }
                        WireDelta::InputJsonDelta { partial_json } => {
                            match &mut self.blocks[idx] {
                                BlockBuilder::ToolUse { input_json, .. }
                                | BlockBuilder::ServerToolUse { input_json, .. } => {
                                    input_json.push_str(&partial_json);
                                }
                                _ => {}
                            }
                            on_event(StreamEvent::InputJsonDelta { partial_json });
                        }
                        WireDelta::ThinkingDelta { thinking } => {
                            if let BlockBuilder::Thinking {
                                text: ref mut buf, ..
                            } = self.blocks[idx]
                            {
                                buf.push_str(&thinking);
                            }
                            on_event(StreamEvent::ThinkingDelta { thinking });
                        }
                        WireDelta::SignatureDelta { signature } => {
                            if let BlockBuilder::Thinking {
                                signature: ref mut buf,
                                ..
                            } = self.blocks[idx]
                            {
                                buf.push_str(&signature);
                            }
                        }
                    }
                }
            }
            WireStreamEvent::ContentBlockStop { index } => {
                on_event(StreamEvent::ContentBlockStop { index });
            }
            WireStreamEvent::MessageDelta { delta, usage } => {
                self.output_tokens = usage.output_tokens;
                let stop_reason = parse_stop_reason_lenient(&delta.stop_reason);
                self.stop_reason = Some(stop_reason);
                on_event(StreamEvent::MessageStop {
                    stop_reason,
                    usage: Usage {
                        input_tokens: self.input_tokens,
                        output_tokens: self.output_tokens,
                        cache_write_tokens: self.cache_write_tokens,
                        cache_read_tokens: self.cache_read_tokens,
                    },
                });
            }
            WireStreamEvent::MessageStop {} | WireStreamEvent::Ping {} => {
                // Final event or keepalive — nothing to accumulate.
            }
            WireStreamEvent::Error { error } => {
                return Err(super::error::map_sse_error(error));
            }
        }
        Ok(())
    }

    /// Build the final `CompletionResponse` from accumulated state.
    pub(crate) fn finish(self) -> CompletionResponse {
        let content = self
            .blocks
            .into_iter()
            .map(|b| match b {
                BlockBuilder::Text(text) => ContentBlock::Text {
                    text,
                    citations: None,
                },
                BlockBuilder::ToolUse {
                    id,
                    name,
                    input_json,
                } => {
                    let input = serde_json::from_str(&input_json).unwrap_or_else(|e| {
                        warn!(error = %e, tool = %name, "failed to parse tool input JSON");
                        serde_json::Value::Object(serde_json::Map::default())
                    });
                    ContentBlock::ToolUse { id, name, input }
                }
                BlockBuilder::Thinking { text, signature } => ContentBlock::Thinking {
                    thinking: text,
                    signature: if signature.is_empty() {
                        None
                    } else {
                        Some(signature)
                    },
                },
                BlockBuilder::ServerToolUse {
                    id,
                    name,
                    input_json,
                } => {
                    let input = serde_json::from_str(&input_json).unwrap_or_else(|e| {
                        warn!(error = %e, tool = %name, "failed to parse server tool input JSON");
                        serde_json::Value::Object(serde_json::Map::default())
                    });
                    ContentBlock::ServerToolUse { id, name, input }
                }
                BlockBuilder::WebSearchToolResult {
                    tool_use_id,
                    content,
                } => ContentBlock::WebSearchToolResult {
                    tool_use_id,
                    content,
                },
                BlockBuilder::CodeExecutionResult {
                    code,
                    stdout,
                    stderr,
                    return_code,
                } => ContentBlock::CodeExecutionResult {
                    code,
                    stdout,
                    stderr,
                    return_code,
                },
            })
            .collect();

        CompletionResponse {
            id: self.id,
            model: self.model,
            stop_reason: self.stop_reason.unwrap_or(StopReason::EndTurn),
            content,
            usage: Usage {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                cache_write_tokens: self.cache_write_tokens,
                cache_read_tokens: self.cache_read_tokens,
            },
        }
    }
}

/// Parse SSE lines from a reader, dispatching events to the accumulator.
pub(crate) fn parse_sse_stream(
    reader: impl BufRead,
    accumulator: &mut StreamAccumulator,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<()> {
    let mut current_event_type = String::new();
    let mut current_data = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("stream read error: {e}"),
            }
            .build()
        })?;

        if line.is_empty() {
            // Empty line = end of event. Dispatch if we have data.
            if !current_data.is_empty() && current_event_type != "ping" {
                let event: WireStreamEvent = serde_json::from_str(&current_data).map_err(|e| {
                    error::ApiRequestSnafu {
                        message: format!("stream parse error: {e}"),
                    }
                    .build()
                })?;
                accumulator.process_event(event, on_event)?;
            }
            current_event_type.clear();
            current_data.clear();
            continue;
        }

        if let Some(event_type) = line.strip_prefix("event: ") {
            event_type.clone_into(&mut current_event_type);
        } else if let Some(data) = line.strip_prefix("data: ") {
            data.clone_into(&mut current_data);
        }
        // Ignore other lines (comments, etc.)
    }

    Ok(())
}

fn convert_wire_usage(wire: &WireUsage) -> Usage {
    Usage {
        input_tokens: wire.input_tokens,
        output_tokens: wire.output_tokens,
        cache_write_tokens: wire.cache_creation_input_tokens,
        cache_read_tokens: wire.cache_read_input_tokens,
    }
}

fn parse_stop_reason_lenient(s: &str) -> StopReason {
    match s {
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_events(sse_text: &str) -> (Vec<StreamEvent>, CompletionResponse) {
        let reader = std::io::Cursor::new(sse_text);
        let mut acc = StreamAccumulator::new();
        let mut events = Vec::new();
        parse_sse_stream(reader, &mut acc, &mut |e| events.push(e)).unwrap();
        let response = acc.finish();
        (events, response)
    }

    #[test]
    fn parses_simple_text_stream() {
        let sse = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let (events, response) = collect_events(sse);

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
        assert_eq!(response.id, "msg_1");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Hello world"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn parses_tool_use_stream() {
        let sse = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":20,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"exec\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\": \\\"ls\\\"}\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":15}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let (_, response) = collect_events(sse);

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        match &response.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "exec");
                assert_eq!(input["cmd"], "ls");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn parses_thinking_stream() {
        let sse = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_3\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":30,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me think about this.\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"The answer is 42.\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":25}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let (_, response) = collect_events(sse);

        assert_eq!(response.content.len(), 2);
        match &response.content[0] {
            ContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "Let me think about this.");
            }
            _ => panic!("expected Thinking block"),
        }
        match &response.content[1] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "The answer is 42."),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn handles_ping_events() {
        let sse = "\
event: ping\n\
data: {\"type\":\"ping\"}\n\
\n\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_4\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let (_, response) = collect_events(sse);
        assert_eq!(response.id, "msg_4");
    }

    #[test]
    fn stream_error_event_returns_err() {
        let sse = "\
event: error\n\
data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\
\n";

        let reader = std::io::Cursor::new(sse);
        let mut acc = StreamAccumulator::new();
        let result = parse_sse_stream(reader, &mut acc, &mut |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn overloaded_sse_error_is_rate_limited() {
        use crate::error::Error;

        let sse = "\
event: error\n\
data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\
\n";

        let reader = std::io::Cursor::new(sse);
        let mut acc = StreamAccumulator::new();
        let result = parse_sse_stream(reader, &mut acc, &mut |_| {});
        let err = result.unwrap_err();
        assert!(
            matches!(err, Error::RateLimited { .. }),
            "expected RateLimited, got: {err:?}"
        );
    }

    #[test]
    fn malformed_json_data_returns_error() {
        let sse = "\
event: message_start\n\
data: this is not valid json\n\
\n";

        let reader = std::io::Cursor::new(sse);
        let mut acc = StreamAccumulator::new();
        let result = parse_sse_stream(reader, &mut acc, &mut |_| {});
        assert!(
            result.is_err(),
            "malformed JSON data should produce an error"
        );
    }

    #[test]
    fn non_retryable_sse_error_is_api_error() {
        use crate::error::Error;

        let sse = "\
event: error\n\
data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bad input\"}}\n\
\n";

        let reader = std::io::Cursor::new(sse);
        let mut acc = StreamAccumulator::new();
        let err = parse_sse_stream(reader, &mut acc, &mut |_| {}).expect_err("should error");
        assert!(
            matches!(err, Error::ApiError { status: 0, .. }),
            "expected ApiError, got: {err:?}"
        );
    }

    #[test]
    fn parses_server_tool_use_stream() {
        let sse = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_srv\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srvtoolu_1\",\"name\":\"web_search\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\": \\\"rust async\\\"}\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"web_search_tool_result\",\"tool_use_id\":\"srvtoolu_1\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"Based on my search...\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":2}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":20}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

        let (events, response) = collect_events(sse);

        // Verify block_type events emitted correctly
        let block_starts: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStart { block_type, .. } => Some(block_type.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            block_starts,
            vec!["server_tool_use", "web_search_tool_result", "text"]
        );

        // Verify content blocks in response
        assert_eq!(response.content.len(), 3);
        match &response.content[0] {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "srvtoolu_1");
                assert_eq!(name, "web_search");
                assert_eq!(input["query"], "rust async");
            }
            _ => panic!("expected ServerToolUse"),
        }
        assert!(matches!(
            &response.content[1],
            ContentBlock::WebSearchToolResult { .. }
        ));
        match &response.content[2] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Based on my search..."),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn empty_stream_returns_ok_with_defaults() {
        let (events, response) = collect_events("");
        assert!(events.is_empty(), "no events from empty stream");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert!(response.content.is_empty());
    }
}
