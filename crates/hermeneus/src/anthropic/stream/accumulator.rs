//! `StreamAccumulator` state machine for SSE event processing.

use tracing::warn;

use super::super::wire::{WireContentBlockStart, WireDelta, WireStreamEvent, WireUsage};
use crate::error::{Error, MalformedToolArgumentsSnafu, Result};
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
    ///
    /// // WHY: The accumulator maintains a Vec<BlockBuilder> indexed by the
    /// // API's content block index. The while-loop ensures the vec is grown
    /// // to accommodate out-of-order or sparse indices from the stream.
    #[expect(
        clippy::too_many_lines,
        reason = "SSE event dispatch is inherently branchy"
    )]
    pub(crate) fn process_event(
        &mut self,
        event: WireStreamEvent,
        on_event: &mut impl FnMut(StreamEvent),
        sse_retry_ms: u64,
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
                // NOTE: u32→usize conversion is safe: content block indices are small
                // and usize is at least 32 bits on all supported platforms.
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
                // INVARIANT: Per the Anthropic streaming API, cache_creation_input_tokens
                // and cache_read_input_tokens are reported in message_start.usage (assigned in
                // the MessageStart arm above), not message_delta.usage. WireMessageDeltaUsage
                // carries these fields with #[serde(default)] so they are always zero in
                // practice; += is additive for forward compatibility but must produce a
                // zero increment from this arm under the current protocol.
                self.cache_write_tokens += usage.cache_creation_input_tokens;
                self.cache_read_tokens += usage.cache_read_input_tokens;
                // WHY: Anthropic stop_reason strings we do not recognize are
                // provider drift, not a clean end_turn. Preserve the signal as
                // Unknown so downstream callers can surface degraded output.
                let stop_reason = delta
                    .stop_reason
                    .parse::<StopReason>()
                    .unwrap_or(StopReason::Unknown);
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
                return Err(super::super::error::map_sse_error(error, sse_retry_ms));
            }
        }
        Ok(())
    }

    /// Build the final `CompletionResponse` from accumulated state.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MalformedToolArguments`] when a streamed tool's
    /// accumulated `input_json` is not valid JSON. This prevents malformed
    /// provider output from reaching Nous tool dispatch as a normal tool input.
    #[expect(
        clippy::too_many_lines,
        reason = "stream accumulator converts every supported Anthropic block variant"
    )]
    pub(crate) fn finish(self) -> Result<CompletionResponse, Error> {
        let mut content = Vec::with_capacity(self.blocks.len());
        for block in self.blocks {
            let converted = match block {
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
                        serde_json::from_str(&input_json).map_err(|e| {
                            warn!(
                                error = %e,
                                tool = %name,
                                raw_json = %input_json,
                                "tool input JSON parse failed; rejecting provider tool call"
                            );
                            MalformedToolArgumentsSnafu {
                                tool: name.clone(),
                                source: e,
                            }
                            .build()
                        })?
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
                        serde_json::from_str(&input_json).map_err(|e| {
                            warn!(
                                error = %e,
                                tool = %name,
                                raw_json = %input_json,
                                "server tool input JSON parse failed; rejecting provider tool call"
                            );
                            MalformedToolArgumentsSnafu {
                                tool: name.clone(),
                                source: e,
                            }
                            .build()
                        })?
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
            };
            content.push(converted);
        }

        Ok(CompletionResponse {
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
            cost_usd: None,
            duration_ms: None,
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "tests assert against known-length content vecs"
)]
mod tests {
    use super::super::super::wire::{
        WireContentBlockStart, WireDelta, WireMessageDeltaBody, WireMessageDeltaUsage,
        WireMessageStart, WireStreamEvent, WireUsage,
    };
    use super::*;

    fn usage(input: u64, cache_write: u64, cache_read: u64) -> WireUsage {
        WireUsage {
            input_tokens: input,
            output_tokens: 0,
            cache_creation_input_tokens: cache_write,
            cache_read_input_tokens: cache_read,
        }
    }

    fn start_event(id: &str, model: &str) -> WireStreamEvent {
        WireStreamEvent::MessageStart {
            message: WireMessageStart {
                id: id.to_owned(),
                model: model.to_owned(),
                usage: usage(10, 0, 0),
            },
        }
    }

    fn delta_event(delta: WireDelta, index: u32) -> WireStreamEvent {
        WireStreamEvent::ContentBlockDelta { index, delta }
    }

    fn stop_event(reason: &str, output_tokens: u64) -> WireStreamEvent {
        WireStreamEvent::MessageDelta {
            delta: WireMessageDeltaBody {
                stop_reason: reason.to_owned(),
            },
            usage: WireMessageDeltaUsage {
                output_tokens,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            },
        }
    }

    #[test]
    fn accumulator_captures_message_metadata() {
        let mut acc = StreamAccumulator::new();
        let mut events = Vec::new();
        acc.process_event(
            start_event("msg_01", "claude-opus"),
            &mut |e| {
                events.push(e);
            },
            1000,
        )
        .expect("process should succeed");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.id, "msg_01");
        assert_eq!(response.model, "claude-opus");
        assert_eq!(response.usage.input_tokens, 10);
    }

    #[test]
    fn accumulator_builds_text_block_from_deltas() {
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_02", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 0,
                content_block: WireContentBlockStart::Text {
                    text: String::new(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("block start");
        acc.process_event(
            delta_event(
                WireDelta::TextDelta {
                    text: "Hello, ".to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("delta 1");
        acc.process_event(
            delta_event(
                WireDelta::TextDelta {
                    text: "world!".to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("delta 2");
        acc.process_event(
            WireStreamEvent::ContentBlockStop { index: 0 },
            &mut on_event,
            1000,
        )
        .expect("block stop");
        acc.process_event(stop_event("end_turn", 42), &mut on_event, 1000)
            .expect("message stop");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Hello, world!"),
            other => panic!("expected Text, got {other:?}"),
        }
        assert_eq!(response.usage.output_tokens, 42);
    }

    #[test]
    fn accumulator_builds_tool_use_from_json_deltas() {
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_03", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 0,
                content_block: WireContentBlockStart::ToolUse {
                    id: "toolu_1".to_owned(),
                    name: "Read".to_owned(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("tool start");
        acc.process_event(
            delta_event(
                WireDelta::InputJsonDelta {
                    partial_json: r#"{"path":"/tmp/"#.to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("delta 1");
        acc.process_event(
            delta_event(
                WireDelta::InputJsonDelta {
                    partial_json: r#"foo.txt"}"#.to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("delta 2");
        acc.process_event(stop_event("tool_use", 5), &mut on_event, 1000)
            .expect("stop");

        let response = acc.finish().expect("finish should succeed");
        match &response.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "Read");
                assert_eq!(input["path"].as_str(), Some("/tmp/foo.txt"));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn accumulator_handles_empty_tool_input() {
        // WHY: a tool with zero arguments sends no InputJsonDelta events;
        // the builder's input_json stays empty and should materialize as {}.
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_04", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 0,
                content_block: WireContentBlockStart::ToolUse {
                    id: "toolu_2".to_owned(),
                    name: "NoArgTool".to_owned(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("tool start");
        acc.process_event(stop_event("tool_use", 2), &mut on_event, 1000)
            .expect("stop");

        let response = acc.finish().expect("finish should succeed");
        match &response.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert!(input.is_object(), "empty input should be empty object");
                assert_eq!(input.as_object().expect("object").len(), 0);
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn accumulator_rejects_malformed_tool_json() {
        // WHY: a malformed JSON delta must not be dispatched as normal tool
        // input; it is a provider contract failure.
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_05", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 0,
                content_block: WireContentBlockStart::ToolUse {
                    id: "toolu_3".to_owned(),
                    name: "BadTool".to_owned(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("tool start");
        acc.process_event(
            delta_event(
                WireDelta::InputJsonDelta {
                    partial_json: r#"{"broken":"#.to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("delta");
        acc.process_event(stop_event("tool_use", 3), &mut on_event, 1000)
            .expect("stop");

        let err = acc
            .finish()
            .expect_err("malformed tool JSON should fail finish");
        assert!(
            matches!(err, Error::MalformedToolArguments { .. }),
            "expected MalformedToolArguments, got {err:?}"
        );
    }

    #[test]
    fn accumulator_builds_thinking_block() {
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_06", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 0,
                content_block: WireContentBlockStart::Thinking {
                    thinking: String::new(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("block start");
        acc.process_event(
            delta_event(
                WireDelta::ThinkingDelta {
                    thinking: "Let me think... ".to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("thinking");
        acc.process_event(
            delta_event(
                WireDelta::SignatureDelta {
                    signature: "sig123".to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("signature");
        acc.process_event(stop_event("end_turn", 10), &mut on_event, 1000)
            .expect("stop");

        let response = acc.finish().expect("finish should succeed");
        match &response.content[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me think... ");
                assert_eq!(signature.as_deref(), Some("sig123"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn accumulator_supports_sparse_block_indices() {
        // WHY: the stream can send block index 2 before 1, and the accumulator
        // should grow its blocks vec to accommodate. The gap should be filled
        // with empty text blocks.
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_07", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(
            WireStreamEvent::ContentBlockStart {
                index: 2,
                content_block: WireContentBlockStart::Text {
                    text: "third".to_owned(),
                },
            },
            &mut on_event,
            1000,
        )
        .expect("block 2");
        acc.process_event(stop_event("end_turn", 5), &mut on_event, 1000)
            .expect("stop");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(
            response.content.len(),
            3,
            "should have 3 blocks (gaps filled)"
        );
    }

    #[test]
    fn accumulator_cache_tokens_from_message_start_survive_to_finish() {
        // WHY: The Anthropic API sends cache token counts in message_start.usage,
        // not message_delta.usage. This verifies values assigned in the MessageStart
        // arm survive through the MessageDelta arm (which carries zero cache counts
        // per the current protocol) and into the final CompletionResponse.
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(
            WireStreamEvent::MessageStart {
                message: WireMessageStart {
                    id: "msg_08".to_owned(),
                    model: "claude-opus".to_owned(),
                    usage: WireUsage {
                        input_tokens: 10,
                        output_tokens: 0,
                        cache_creation_input_tokens: 100,
                        cache_read_input_tokens: 200,
                    },
                },
            },
            &mut on_event,
            1000,
        )
        .expect("start");
        // MessageDelta carries zero cache counts per current Anthropic streaming API.
        acc.process_event(
            WireStreamEvent::MessageDelta {
                delta: WireMessageDeltaBody {
                    stop_reason: "end_turn".to_owned(),
                },
                usage: WireMessageDeltaUsage {
                    output_tokens: 50,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
            },
            &mut on_event,
            1000,
        )
        .expect("delta");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.usage.output_tokens, 50);
        assert_eq!(response.usage.cache_write_tokens, 100);
        assert_eq!(response.usage.cache_read_tokens, 200);
    }

    #[test]
    fn accumulator_message_delta_cache_fields_are_additive() {
        // WHY: WireMessageDeltaUsage has cache fields with #[serde(default)], always
        // zero under the current Anthropic streaming protocol. If they ever become
        // non-zero (future API evolution), += must not clobber the value already
        // assigned from message_start. This verifies the additive invariant.
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(
            WireStreamEvent::MessageStart {
                message: WireMessageStart {
                    id: "msg_08b".to_owned(),
                    model: "claude-opus".to_owned(),
                    usage: WireUsage {
                        input_tokens: 10,
                        output_tokens: 0,
                        cache_creation_input_tokens: 50,
                        cache_read_input_tokens: 100,
                    },
                },
            },
            &mut on_event,
            1000,
        )
        .expect("start");
        acc.process_event(
            WireStreamEvent::MessageDelta {
                delta: WireMessageDeltaBody {
                    stop_reason: "end_turn".to_owned(),
                },
                usage: WireMessageDeltaUsage {
                    output_tokens: 20,
                    cache_creation_input_tokens: 10,
                    cache_read_input_tokens: 5,
                },
            },
            &mut on_event,
            1000,
        )
        .expect("delta");

        let response = acc.finish();
        // start(50) + delta(10) = 60; start(100) + delta(5) = 105
        assert_eq!(response.usage.cache_write_tokens, 60);
        assert_eq!(response.usage.cache_read_tokens, 105);
    }

    #[test]
    fn accumulator_rejects_error_event() {
        use super::super::super::wire::WireErrorDetail;

        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        let result = acc.process_event(
            WireStreamEvent::Error {
                error: WireErrorDetail {
                    error_type: "overloaded_error".to_owned(),
                    message: "server is at capacity".to_owned(),
                },
            },
            &mut on_event,
            1000,
        );
        assert!(result.is_err(), "error event should propagate as Err");
    }

    #[test]
    fn accumulator_ignores_ping_and_message_stop() {
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_09", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        acc.process_event(WireStreamEvent::Ping {}, &mut on_event, 1000)
            .expect("ping");
        acc.process_event(WireStreamEvent::MessageStop {}, &mut on_event, 1000)
            .expect("message stop");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.id, "msg_09");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn accumulator_finish_without_stop_defaults_to_end_turn() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(start_event("msg_10", "claude-opus"), &mut |_| {}, 1000)
            .expect("start");
        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn delta_for_nonexistent_block_index_ignored() {
        let mut acc = StreamAccumulator::new();
        let mut on_event = |_: StreamEvent| {};

        acc.process_event(start_event("msg_11", "claude-opus"), &mut on_event, 1000)
            .expect("start");
        // No ContentBlockStart — delta references non-existent index
        acc.process_event(
            delta_event(
                WireDelta::TextDelta {
                    text: "orphan".to_owned(),
                },
                0,
            ),
            &mut on_event,
            1000,
        )
        .expect("orphan delta should not error");

        let response = acc.finish().expect("finish should succeed");
        assert_eq!(response.content.len(), 0);
    }
}
