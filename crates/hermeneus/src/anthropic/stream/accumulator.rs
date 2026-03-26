//! `StreamAccumulator` state machine for SSE event processing.

use tracing::warn;

use super::super::wire::{WireContentBlockStart, WireDelta, WireStreamEvent, WireUsage};
use crate::error::Result;
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

use super::StreamEvent;

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

fn convert_wire_usage(wire: &WireUsage) -> Usage {
    Usage {
        input_tokens: wire.input_tokens,
        output_tokens: wire.output_tokens,
        cache_write_tokens: wire.cache_creation_input_tokens,
        cache_read_tokens: wire.cache_read_input_tokens,
    }
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
                // INVARIANT: blocks vec must be at least index+1 long before assignment.
                // index is a u32 content block index from the API; usize is at least 32 bits
                #[expect(
                    clippy::as_conversions,
                    reason = "u32→usize: content block indices are small"
                )]
                let idx = index as usize; // kanon:ignore RUST/as-cast
                while self.blocks.len() <= idx {
                    self.blocks.push(BlockBuilder::Text(String::new()));
                }
                #[expect(
                    clippy::indexing_slicing,
                    reason = "while loop above ensures blocks.len() > idx"
                )]
                {
                    self.blocks[idx] = builder; // kanon:ignore RUST/indexing-slicing
                }
            }
            WireStreamEvent::ContentBlockDelta { index, delta } => {
                // index is a u32 content block index from the API; usize is at least 32 bits
                #[expect(
                    clippy::as_conversions,
                    reason = "u32→usize: content block indices are small"
                )]
                let idx = index as usize; // kanon:ignore RUST/as-cast
                #[expect(
                    clippy::indexing_slicing,
                    reason = "idx < self.blocks.len() is checked by the if-guard"
                )]
                if idx < self.blocks.len() {
                    match delta {
                        WireDelta::TextDelta { text } => {
                            if let BlockBuilder::Text(ref mut buf) = self.blocks[idx] {
                                // kanon:ignore RUST/indexing-slicing
                                buf.push_str(&text);
                            }
                            on_event(StreamEvent::TextDelta { text });
                        }
                        WireDelta::InputJsonDelta { partial_json } => {
                            match &mut self.blocks[idx] {
                                // kanon:ignore RUST/indexing-slicing
                                BlockBuilder::ToolUse { input_json, .. }
                                | BlockBuilder::ServerToolUse { input_json, .. } => {
                                    input_json.push_str(&partial_json);
                                }
                                _ => {
                                    // NOTE: InputJsonDelta for non-tool blocks is ignored
                                }
                            }
                            on_event(StreamEvent::InputJsonDelta { partial_json });
                        }
                        WireDelta::ThinkingDelta { thinking } => {
                            if let BlockBuilder::Thinking {
                                text: ref mut buf, ..
                            } = self.blocks[idx]
                            // kanon:ignore RUST/indexing-slicing
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
                            // kanon:ignore RUST/indexing-slicing
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
                // NOTE: Cache token deltas are reported in message_delta, not message_start.
                self.cache_write_tokens += usage.cache_creation_input_tokens;
                self.cache_read_tokens += usage.cache_read_input_tokens;
                let stop_reason = delta
                    .stop_reason
                    .parse::<StopReason>()
                    .unwrap_or(StopReason::EndTurn);
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
                // NOTE: Final event or keepalive -- nothing to accumulate.
            }
            WireStreamEvent::Error { error } => {
                return Err(super::super::error::map_sse_error(error));
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
                    // WHY: An empty string means no input_json_delta events were sent: the
                    // tool takes no arguments. Skip parsing to avoid a spurious WARN.
                    let input = if input_json.is_empty() {
                        serde_json::Value::Object(serde_json::Map::default())
                    } else {
                        match serde_json::from_str(&input_json) {
                            Ok(v) => v,
                            Err(e) => {
                                warn!(
                                    error = %e,
                                    tool = %name,
                                    raw_json = %input_json,
                                    "tool input JSON parse failed; returning error object to agent"
                                );
                                serde_json::json!({
                                    "_parse_error": format!("malformed tool input: {e}"),
                                    "_raw_input": input_json,
                                })
                            }
                        }
                    };
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
                    // WHY: Same empty-input guard as ToolUse above.
                    let input = if input_json.is_empty() {
                        serde_json::Value::Object(serde_json::Map::default())
                    } else {
                        match serde_json::from_str(&input_json) {
                            Ok(v) => v,
                            Err(e) => {
                                warn!(
                                    error = %e,
                                    tool = %name,
                                    raw_json = %input_json,
                                    "server tool input JSON parse failed; returning error object to agent"
                                );
                                serde_json::json!({
                                    "_parse_error": format!("malformed tool input: {e}"),
                                    "_raw_input": input_json,
                                })
                            }
                        }
                    };
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
