//! Parse OpenAI Chat Completions SSE streams into hermeneus stream events.
//!
//! Accumulates `data: {chunk}` SSE lines (terminated by `[DONE]`) into a
//! [`CompletionResponse`] while emitting [`StreamEvent`]s for live UI.

use std::collections::BTreeMap;

use reqwest::Response;
use serde::Deserialize;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

use super::response::{ResponsesResponse, TokenDetails, parse_arguments};

/// Format an error and its full source chain into a single message string.
///
/// WHY(#4887): reqwest's Display hides the underlying cause ("connection reset
/// by peer"). `is_retryable()` scans for "reset"/"connection", so including
/// the chain makes network-drop errors retryable before content starts.
fn error_chain_message(prefix: &str, err: &dyn std::error::Error) -> String {
    let mut parts = vec![format!("{prefix}: {err}")];
    let mut source = err.source();
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(": ")
}

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
    #[serde(default)]
    prompt_tokens_details: Option<TokenDetails>,
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
    /// Pending tool calls keyed by OpenAI's `index` — deltas arrive interleaved.
    tool_calls: BTreeMap<u32, PendingToolCall>,
    stop_reason: StopReason,
    usage: Usage,
    /// Whether a `ContentBlockStart` has been emitted; suppresses duplicate starts.
    text_block_open: bool,
}

struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
    /// Whether a `ContentBlockStart` has been emitted for this tool call.
    started: bool,
    /// Content block index matching the final [`CompletionResponse::content`] array.
    block_index: u32,
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

    /// Compute the content-block index for a tool call at `tool_calls[index]`.
    ///
    /// WHY: OpenAI Chat Completions exposes tool-call indices local to the
    /// `tool_calls` array, but hermeneus content-block indices are global to the
    /// final `content` array. Text always precedes tool calls in that array, so
    /// shift tool-call indices by one when a text block is present.
    fn tool_block_index(&self, tool_call_index: u32) -> u32 {
        if self.text_block_open || !self.text_buf.is_empty() {
            tool_call_index + 1
        } else {
            tool_call_index
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
            // Match the Anthropic accumulator contract: one MessageStart per stream.
            on_event(StreamEvent::MessageStart { usage: self.usage });
        }

        if let Some(usage) = chunk.usage {
            let cache_read_tokens = usage
                .prompt_tokens_details
                .as_ref()
                .map_or(0, |details| details.cached_tokens);
            self.usage.input_tokens = usage.prompt_tokens.saturating_sub(cache_read_tokens);
            self.usage.output_tokens = usage.completion_tokens;
            self.usage.cache_read_tokens = cache_read_tokens;
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
            let block_index = self.tool_block_index(tc.index);
            let pending = self
                .tool_calls
                .entry(tc.index)
                .or_insert_with(|| PendingToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                    started: false,
                    block_index,
                });
            if !pending.started {
                on_event(StreamEvent::ContentBlockStart {
                    index: pending.block_index,
                    block_type: "tool_use".to_owned(),
                });
                pending.started = true;
            }
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
    ) -> Result<CompletionResponse> {
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
            if tc.started {
                on_event(StreamEvent::ContentBlockStop {
                    index: tc.block_index,
                });
            }
            content.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.name.clone(),
                input: parse_arguments(&tc.arguments, &tc.name)?,
            });
        }

        on_event(StreamEvent::MessageStop { stop_reason, usage });

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

fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "length" => StopReason::MaxTokens,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "content_filter" => StopReason::ContentFiltered,
        // WHY: Collapsing unknown finish reasons into end_turn hides provider
        // drift and safety signals. Preserve them as Unknown.
        _ => StopReason::Unknown,
    }
}

/// Format a short, bounded diagnostic preview of buffered partial content.
///
/// WHY(#5050): When a stream is truncated we include the buffered text length
/// and a small text prefix in the error so operators can distinguish a true
/// empty stream from one that dropped mid-sentence, without leaking the full
/// potentially-large buffer into logs.
fn format_partial_preview(text_buf: &str, tool_call_count: usize) -> String {
    const PREVIEW_LEN: usize = 64;
    let mut text_preview: String = text_buf.chars().take(PREVIEW_LEN).collect();
    // NOTE: `nth(PREVIEW_LEN)` is O(PREVIEW_LEN); avoids a full `count()` pass
    // over potentially-large buffered text.
    if text_buf.chars().nth(PREVIEW_LEN).is_some() {
        text_preview.push('…');
    }
    format!("text_preview={text_preview:?}, tool_calls={tool_call_count}")
}

/// Parser state for an OpenAI Chat Completions SSE stream.
///
/// WHY(#5050): The parser must distinguish between a provider-terminal
/// `[DONE]` marker and a premature EOF. Keeping the line buffer and terminal
/// flag alongside the accumulator lets tests feed raw byte chunks to the same
/// code path used in production.
struct ChatSseParser {
    accumulator: OpenAiStreamAccumulator,
    line_buf: Vec<u8>,
    current_data: String,
    done: bool,
}

impl ChatSseParser {
    fn new() -> Self {
        Self {
            accumulator: OpenAiStreamAccumulator::new(),
            line_buf: Vec::with_capacity(256),
            current_data: String::new(),
            done: false,
        }
    }

    fn feed<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        bytes: &[u8],
        on_event: &mut F,
    ) -> Result<()> {
        for &byte in bytes {
            if byte == b'\n' {
                let line_cow = String::from_utf8_lossy(&self.line_buf);
                let line = line_cow.trim_end_matches('\r');
                if line.is_empty() {
                    if !self.current_data.is_empty() {
                        if self.current_data.trim() == "[DONE]" {
                            self.done = true;
                        } else {
                            match serde_json::from_str::<ChatStreamChunk>(&self.current_data) {
                                Ok(chunk) => self.accumulator.process_chunk(chunk, on_event),
                                Err(e) => {
                                    return Err(error::ApiRequestSnafu {
                                        message: format!("stream parse error: {e}"),
                                    }
                                    .build());
                                }
                            }
                        }
                    }
                    self.current_data.clear();
                } else if let Some(data) = line.strip_prefix("data: ") {
                    if !self.current_data.is_empty() {
                        self.current_data.push('\n');
                    }
                    self.current_data.push_str(data);
                } else if let Some(data) = line.strip_prefix("data:") {
                    // llama.cpp emits `data:{...}` without the space.
                    if !self.current_data.is_empty() {
                        self.current_data.push('\n');
                    }
                    self.current_data.push_str(data);
                }
                // Ignore comments, empty `:` lines, event:, id:, retry:
                self.line_buf.clear();
            } else {
                self.line_buf.push(byte);
            }
        }
        Ok(())
    }

    fn is_terminal(&self) -> bool {
        self.done
    }

    fn finish<F: FnMut(StreamEvent) + ?Sized>(
        self,
        on_event: &mut F,
    ) -> Result<CompletionResponse> {
        if !self.done {
            let partial = format_partial_preview(
                &self.accumulator.text_buf,
                self.accumulator.tool_calls.len(),
            );
            return Err(error::StreamIncompleteSnafu {
                message: "SSE stream ended without [DONE] marker".to_owned(),
                partial_content: partial,
            }
            .build());
        }
        self.accumulator.finish(on_event)
    }
}

/// Parse an OpenAI SSE stream from a `reqwest::Response`, emitting
/// [`StreamEvent`]s and returning the finalized [`CompletionResponse`].
#[tracing::instrument(skip_all)]
pub(crate) async fn parse_chat_sse_response(
    response: &mut Response,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<CompletionResponse> {
    let mut parser = ChatSseParser::new();
    loop {
        let chunk = response.chunk().await.map_err(|e| {
            error::ApiRequestSnafu {
                message: error_chain_message("stream read error", &e),
            }
            .build()
        })?;
        let Some(bytes) = chunk else { break };
        parser.feed(&bytes, on_event)?;
        if parser.is_terminal() {
            break;
        }
    }
    parser.finish(on_event)
}

#[derive(Debug, Deserialize)]
struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    response: Option<ResponsesResponse>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
}

struct ResponsesStreamAccumulator {
    id: String,
    model: String,
    text_buf: String,
    tool_calls: BTreeMap<String, PendingResponsesToolCall>,
    /// Order in which each `call_id`/`item_id` first appeared in the stream.
    /// WHY: The Responses API does not assign a numeric tool-call index; we
    /// synthesize stable content-block indices from first-arrival order.
    call_order: Vec<String>,
    completed: Option<CompletionResponse>,
    usage: Usage,
    text_block_open: bool,
    message_started: bool,
}

struct PendingResponsesToolCall {
    id: String,
    name: String,
    arguments: String,
    /// Whether a `ContentBlockStart` has been emitted for this tool call.
    started: bool,
    /// Whether a `ContentBlockStop` has been emitted for this tool call.
    stopped: bool,
    /// Content block index matching the final [`CompletionResponse::content`] array.
    block_index: u32,
}

impl ResponsesStreamAccumulator {
    fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            text_buf: String::new(),
            tool_calls: BTreeMap::new(),
            call_order: Vec::new(),
            completed: None,
            usage: Usage::default(),
            text_block_open: false,
            message_started: false,
        }
    }

    /// Compute the content-block index for a tool call at `call_order[position]`.
    ///
    /// WHY: Text blocks precede tool calls in the final `content` array, so
    /// shift tool-call indices by one when a text block has been opened.
    fn tool_block_index(&self, position: u32) -> u32 {
        if self.text_block_open || !self.text_buf.is_empty() {
            position + 1
        } else {
            position
        }
    }

    /// Return the stable content-block position for `key`, allocating one if new.
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "WHY(#5049): content block positions bounded by stream output size, fits u32"
    )]
    fn position_for_call(&mut self, key: &str) -> u32 {
        if let Some(pos) = self.call_order.iter().position(|id| id == key) {
            pos as u32 // kanon:ignore RUST/as-cast
        } else {
            self.call_order.push(key.to_owned());
            (self.call_order.len() - 1) as u32 // kanon:ignore RUST/as-cast
        }
    }

    fn process_event<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        event: ResponsesStreamEvent,
        on_event: &mut F,
    ) -> Result<()> {
        match event.event_type.as_str() {
            "response.created" | "response.in_progress" => {
                if let Some(response) = event.response {
                    self.capture_response_metadata(&response, on_event);
                }
            }
            "response.output_text.delta" => {
                let delta = event.delta.unwrap_or_default();
                if !delta.is_empty() {
                    self.start_text(on_event);
                    self.text_buf.push_str(&delta);
                    on_event(StreamEvent::TextDelta { text: delta });
                }
            }
            "response.function_call_arguments.delta" => {
                self.handle_fn_call_delta(
                    event.call_id,
                    event.item_id,
                    event.name,
                    event.delta,
                    on_event,
                );
            }
            "response.function_call_arguments.done" => {
                self.handle_fn_call_done(
                    event.call_id,
                    event.item_id,
                    event.name,
                    event.arguments,
                    on_event,
                );
            }
            "response.completed" => {
                if let Some(response) = event.response {
                    self.capture_response_metadata(&response, on_event);
                    self.completed = Some(response.into_response()?);
                }
            }
            "response.failed" => {
                return Err(error::ApiRequestSnafu {
                    message: "OpenAI Responses stream failed".to_owned(),
                }
                .build());
            }
            "error" => {
                return Err(error::ApiRequestSnafu {
                    message: "OpenAI Responses stream emitted an error".to_owned(),
                }
                .build());
            }
            _ => {
                // Ignore Responses event types this adapter does not surface.
            }
        }
        Ok(())
    }

    fn handle_fn_call_delta<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        call_id: Option<String>,
        item_id: Option<String>,
        name: Option<String>,
        delta: Option<String>,
        on_event: &mut F,
    ) {
        let key = call_id.or(item_id).unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<String>.or(Option<String>), not Result — empty string fallback is correct
        let position = self.position_for_call(&key);
        let block_index = self.tool_block_index(position);
        let pending =
            self.tool_calls
                .entry(key.clone())
                .or_insert_with(|| PendingResponsesToolCall {
                    id: key,
                    name: name.unwrap_or_default(),
                    arguments: String::new(),
                    started: false,
                    stopped: false,
                    block_index,
                });
        if !pending.started {
            on_event(StreamEvent::ContentBlockStart {
                index: pending.block_index,
                block_type: "tool_use".to_owned(),
            });
            pending.started = true;
        }
        if let Some(d) = delta
            && !d.is_empty()
        {
            pending.arguments.push_str(&d);
            on_event(StreamEvent::InputJsonDelta { partial_json: d });
        }
    }

    fn handle_fn_call_done<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        call_id: Option<String>,
        item_id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
        on_event: &mut F,
    ) {
        let key = call_id.clone().or(item_id).unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: Option<String>.or(Option<String>), not Result — empty string fallback is correct
        let position = self.position_for_call(&key);
        let block_index = self.tool_block_index(position);
        let pending =
            self.tool_calls
                .entry(key.clone())
                .or_insert_with(|| PendingResponsesToolCall {
                    id: key,
                    name: String::new(),
                    arguments: String::new(),
                    started: false,
                    stopped: false,
                    block_index,
                });
        if let Some(id) = call_id
            && !id.is_empty()
        {
            pending.id = id;
        }
        if let Some(n) = name
            && !n.is_empty()
        {
            pending.name = n;
        }
        if let Some(args) = arguments {
            pending.arguments = args;
        }
        if !pending.started {
            on_event(StreamEvent::ContentBlockStart {
                index: pending.block_index,
                block_type: "tool_use".to_owned(),
            });
            pending.started = true;
        }
        on_event(StreamEvent::ContentBlockStop {
            index: pending.block_index,
        });
        pending.stopped = true;
    }

    fn capture_response_metadata<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        response: &ResponsesResponse,
        on_event: &mut F,
    ) {
        if self.id.is_empty() {
            self.id.clone_from(&response.id);
        }
        if self.model.is_empty() {
            self.model.clone_from(&response.model);
        }
        if let Some(usage) = &response.usage {
            let cache_read_tokens = usage
                .input_tokens_details
                .as_ref()
                .map_or(0, |details| details.cached_tokens);
            self.usage.input_tokens = usage.input_tokens.saturating_sub(cache_read_tokens);
            self.usage.output_tokens = usage.output_tokens;
            self.usage.cache_read_tokens = cache_read_tokens;
        }
        self.start_message(on_event);
    }

    fn start_message<F: FnMut(StreamEvent) + ?Sized>(&mut self, on_event: &mut F) {
        if !self.message_started {
            on_event(StreamEvent::MessageStart { usage: self.usage });
            self.message_started = true;
        }
    }

    fn start_text<F: FnMut(StreamEvent) + ?Sized>(&mut self, on_event: &mut F) {
        self.start_message(on_event);
        if !self.text_block_open {
            on_event(StreamEvent::ContentBlockStart {
                index: 0,
                block_type: "text".to_owned(),
            });
            self.text_block_open = true;
        }
    }

    fn finish<F: FnMut(StreamEvent) + ?Sized>(
        self,
        on_event: &mut F,
    ) -> Result<CompletionResponse> {
        let Self {
            id,
            model,
            text_buf,
            tool_calls,
            call_order,
            completed,
            usage,
            text_block_open,
            ..
        } = self;

        if text_block_open {
            on_event(StreamEvent::ContentBlockStop { index: 0 });
        }

        // WHY: Emit ContentBlockStop for any tool call whose `done` event was
        // missing or arrived out of order. Iterate in first-arrival order so
        // indices stay aligned with the ContentBlockStart events already emitted.
        for key in &call_order {
            if let Some(pending) = tool_calls.get(key)
                && pending.started
                && !pending.stopped
            {
                on_event(StreamEvent::ContentBlockStop {
                    index: pending.block_index,
                });
            }
        }

        let resp = if let Some(resp) = completed {
            resp
        } else {
            let mut content = Vec::new();
            if !text_buf.is_empty() {
                content.push(ContentBlock::Text {
                    text: text_buf,
                    citations: None,
                });
            }
            // WHY: Preserve the first-arrival order used for streaming content-block
            // indices so that emitted indices match the final `content` array.
            for key in &call_order {
                if let Some(call) = tool_calls.get(key) {
                    content.push(ContentBlock::ToolUse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        input: parse_arguments(&call.arguments, &call.name)?,
                    });
                }
            }
            let stop_reason = if content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
            {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            };
            CompletionResponse {
                id,
                model,
                stop_reason,
                content,
                usage,
                cost_usd: None,
                duration_ms: None,
            }
        };

        on_event(StreamEvent::MessageStop {
            stop_reason: resp.stop_reason,
            usage: resp.usage,
        });
        Ok(resp)
    }
}

/// Parser state for an OpenAI Responses SSE stream.
///
/// WHY(#5050): Responses streams may terminate with `response.completed` or
/// the legacy `[DONE]` marker. This parser tracks both and refuses to emit a
/// synthesized completion when EOF arrives before either terminal signal.
struct ResponsesSseParser {
    accumulator: ResponsesStreamAccumulator,
    line_buf: Vec<u8>,
    current_data: String,
    done: bool,
}

impl ResponsesSseParser {
    fn new() -> Self {
        Self {
            accumulator: ResponsesStreamAccumulator::new(),
            line_buf: Vec::with_capacity(256),
            current_data: String::new(),
            done: false,
        }
    }

    fn feed<F: FnMut(StreamEvent) + ?Sized>(
        &mut self,
        bytes: &[u8],
        on_event: &mut F,
    ) -> Result<()> {
        for &byte in bytes {
            if byte == b'\n' {
                let line_cow = String::from_utf8_lossy(&self.line_buf);
                let line = line_cow.trim_end_matches('\r');
                if line.is_empty() {
                    if !self.current_data.is_empty() {
                        if self.current_data.trim() == "[DONE]" {
                            self.done = true;
                        } else {
                            let event: ResponsesStreamEvent =
                                serde_json::from_str(&self.current_data).map_err(|e| {
                                    error::ApiRequestSnafu {
                                        message: format!("Responses stream parse error: {e}"),
                                    }
                                    .build()
                                })?;
                            self.accumulator.process_event(event, on_event)?;
                        }
                    }
                    self.current_data.clear();
                } else if let Some(data) = line.strip_prefix("data: ") {
                    if !self.current_data.is_empty() {
                        self.current_data.push('\n');
                    }
                    self.current_data.push_str(data);
                } else if let Some(data) = line.strip_prefix("data:") {
                    if !self.current_data.is_empty() {
                        self.current_data.push('\n');
                    }
                    self.current_data.push_str(data);
                }
                self.line_buf.clear();
            } else {
                self.line_buf.push(byte);
            }
        }
        Ok(())
    }

    fn is_terminal(&self) -> bool {
        self.done || self.accumulator.completed.is_some()
    }

    fn finish<F: FnMut(StreamEvent) + ?Sized>(
        self,
        on_event: &mut F,
    ) -> Result<CompletionResponse> {
        if !self.is_terminal() {
            let partial = format_partial_preview(
                &self.accumulator.text_buf,
                self.accumulator.tool_calls.len(),
            );
            return Err(error::StreamIncompleteSnafu {
                message: "Responses SSE stream ended without completion marker".to_owned(),
                partial_content: partial,
            }
            .build());
        }
        self.accumulator.finish(on_event)
    }
}

/// Parse an OpenAI Responses SSE stream from a `reqwest::Response`, emitting
/// [`StreamEvent`]s and returning the finalized [`CompletionResponse`].
#[tracing::instrument(skip_all)]
pub(crate) async fn parse_responses_sse_response(
    response: &mut Response,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<CompletionResponse> {
    let mut parser = ResponsesSseParser::new();
    loop {
        let chunk = response.chunk().await.map_err(|e| {
            error::ApiRequestSnafu {
                message: error_chain_message("stream read error", &e),
            }
            .build()
        })?;
        let Some(bytes) = chunk else { break };
        parser.feed(&bytes, on_event)?;
        if parser.is_terminal() {
            break;
        }
    }
    parser.finish(on_event)
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

    fn process_chunks(chunks: &[&str]) -> (Vec<StreamEvent>, CompletionResponse) {
        let mut acc = OpenAiStreamAccumulator::new();
        let mut events = Vec::new();
        for chunk_json in chunks {
            let chunk: ChatStreamChunk = serde_json::from_str(chunk_json).unwrap();
            acc.process_chunk(chunk, &mut |e| events.push(e));
        }
        let resp = acc
            .finish(&mut |e| events.push(e))
            .expect("finish should succeed");
        (events, resp)
    }

    fn process_responses_events(chunks: &[&str]) -> (Vec<StreamEvent>, CompletionResponse) {
        let mut acc = ResponsesStreamAccumulator::new();
        let mut events = Vec::new();
        for event_json in chunks {
            let event: ResponsesStreamEvent = serde_json::from_str(event_json).unwrap();
            acc.process_event(event, &mut |e| events.push(e)).unwrap();
        }
        let resp = acc
            .finish(&mut |e| events.push(e))
            .expect("finish should succeed");
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
    fn malformed_tool_call_deltas_reject_parse_error() {
        let mut acc = OpenAiStreamAccumulator::new();
        let mut events = Vec::new();
        for chunk_json in [
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{not"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":" json"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ] {
            let chunk: ChatStreamChunk = serde_json::from_str(chunk_json).unwrap();
            acc.process_chunk(chunk, &mut |e| events.push(e));
        }
        let err = acc
            .finish(&mut |e| events.push(e))
            .expect_err("malformed tool arguments should fail finish");
        assert!(
            matches!(err, error::Error::MalformedToolArguments { .. }),
            "expected MalformedToolArguments, got {err:?}"
        );
    }

    #[test]
    fn content_filter_finish_reason_maps_to_content_filtered() {
        let (_, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"content":"Par"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"content":"tial"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ContentFiltered);
    }

    #[test]
    fn usage_propagates_when_present() {
        let (_, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"content":"hi"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":3}}}"#,
        ]);
        assert_eq!(resp.usage.input_tokens, 1);
        assert_eq!(resp.usage.output_tokens, 1);
        assert_eq!(resp.usage.cache_read_tokens, 3);
    }

    #[test]
    fn responses_stream_usage_extracts_cached_input_tokens() {
        let (_, resp) = process_responses_events(&[r#"{
            "type": "response.completed",
            "response": {
                "id": "resp-cache",
                "model": "gpt-5",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "content": [{ "type": "output_text", "text": "cached" }]
                }],
                "usage": {
                    "input_tokens": 9,
                    "output_tokens": 2,
                    "total_tokens": 11,
                    "input_tokens_details": { "cached_tokens": 6 }
                }
            }
        }"#]);
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.usage.output_tokens, 2);
        assert_eq!(resp.usage.cache_read_tokens, 6);
    }

    use crate::error::Error;

    fn run_chat_parser(chunks: &[&[u8]]) -> Result<(Vec<StreamEvent>, CompletionResponse)> {
        let mut parser = ChatSseParser::new();
        let mut events = Vec::new();
        for chunk in chunks {
            parser.feed(chunk, &mut |e| events.push(e))?;
        }
        let resp = parser.finish(&mut |e| events.push(e))?;
        Ok((events, resp))
    }

    fn run_responses_parser(chunks: &[&[u8]]) -> Result<(Vec<StreamEvent>, CompletionResponse)> {
        let mut parser = ResponsesSseParser::new();
        let mut events = Vec::new();
        for chunk in chunks {
            parser.feed(chunk, &mut |e| events.push(e))?;
        }
        let resp = parser.finish(&mut |e| events.push(e))?;
        Ok((events, resp))
    }

    #[test]
    fn chat_sse_clean_completion_finishes_with_text() {
        let (events, resp) = run_chat_parser(&[
            b"data: {\"id\":\"chat-1\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n\n".as_slice(),
            b"data: [DONE]\n\n".as_slice(),
        ])
        .unwrap();
        assert_eq!(resp.content.len(), 1);
        assert!(
            matches!(&resp.content[0], ContentBlock::Text { text, .. } if text == "Hello"),
            "expected text content, got {resp:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::TextDelta { text } if text == "Hello")),
            "expected a TextDelta event"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStop { .. })),
            "expected a MessageStop event"
        );
    }

    #[test]
    fn chat_sse_eof_without_done_is_incomplete() {
        let err = run_chat_parser(&[b"data: {\"id\":\"chat-1\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Partial\"}}]}\n\n".as_slice()])
            .unwrap_err();
        assert!(
            matches!(err, Error::StreamIncomplete { ref message, .. } if message.contains("[DONE]")),
            "expected StreamIncomplete for EOF without [DONE], got {err:?}"
        );
    }

    #[test]
    fn chat_sse_eof_after_partial_tool_call_is_incomplete() {
        let err = run_chat_parser(&[b"data: {\"id\":\"chat-1\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tc1\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\n".as_slice()])
            .unwrap_err();
        assert!(
            matches!(err, Error::StreamIncomplete { .. }),
            "expected StreamIncomplete for truncated tool stream, got {err:?}"
        );
    }

    #[test]
    fn chat_sse_provider_error_event_returns_parse_error() {
        let mut parser = ChatSseParser::new();
        let mut events = Vec::new();
        parser
            .feed(
                b"data: {\"id\":\"chat-1\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Partial\"}}]}\n\n",
                &mut |e| events.push(e),
            )
            .unwrap();
        let err = parser
            .feed(
                b"data: {\"error\":{\"message\":\"rate limit\"}}\n\n",
                &mut |e| events.push(e),
            )
            .unwrap_err();
        assert!(
            matches!(err, Error::ApiRequest { ref message, .. } if message.contains("stream parse error")),
            "expected ApiRequest parse error after provider error event, got {err:?}"
        );
    }

    #[test]
    fn responses_sse_clean_completion_finishes() {
        let (events, resp) = run_responses_parser(&[
            b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\",\"model\":\"gpt-5\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]}]}}\n\n".as_slice(),
        ])
        .unwrap();
        assert_eq!(resp.content.len(), 1);
        assert!(
            matches!(&resp.content[0], ContentBlock::Text { text, .. } if text == "Hello"),
            "expected text content, got {resp:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStop { .. })),
            "expected a MessageStop event"
        );
    }

    #[test]
    fn responses_sse_eof_without_completion_is_incomplete() {
        let err = run_responses_parser(&[
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\n".as_slice(),
        ])
        .unwrap_err();
        assert!(
            matches!(err, Error::StreamIncomplete { ref message, .. } if message.contains("completion marker")),
            "expected StreamIncomplete for EOF without response.completed, got {err:?}"
        );
    }

    #[test]
    fn responses_sse_eof_after_partial_function_call_is_incomplete() {
        let err = run_responses_parser(&[
            b"data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"fc1\",\"name\":\"get_weather\",\"delta\":\"{\\\"city\\\":\"}\n\n".as_slice(),
        ])
        .unwrap_err();
        assert!(
            matches!(err, Error::StreamIncomplete { .. }),
            "expected StreamIncomplete for truncated function-call stream, got {err:?}"
        );
    }

    #[test]
    fn responses_sse_error_event_returns_error() {
        let mut parser = ResponsesSseParser::new();
        let mut events = Vec::new();
        parser
            .feed(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\n",
                &mut |e| events.push(e),
            )
            .unwrap();
        let err = parser
            .feed(b"data: {\"type\":\"error\"}\n\n", &mut |e| events.push(e))
            .unwrap_err();
        assert!(
            matches!(err, Error::ApiRequest { ref message, .. } if message.contains("error")),
            "expected ApiRequest error after error event, got {err:?}"
        );
    }

    #[test]
    fn emits_tool_call_content_block_lifecycle() {
        let (events, resp) = process_chunks(&[
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
        let start_idx = events.iter().position(|e| {
            matches!(e, StreamEvent::ContentBlockStart { index: 0, block_type } if block_type == "tool_use")
        });
        let stop_idx = events
            .iter()
            .position(|e| matches!(e, StreamEvent::ContentBlockStop { index: 0 }));
        assert!(start_idx.is_some(), "expected tool-use ContentBlockStart");
        assert!(stop_idx.is_some(), "expected tool-use ContentBlockStop");
        assert!(
            start_idx < stop_idx,
            "ContentBlockStart must precede ContentBlockStop"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::InputJsonDelta { partial_json } if partial_json == "{\"a\":"
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::InputJsonDelta { partial_json } if partial_json == "1}"
        )));
    }

    #[test]
    fn emits_lifecycle_for_multiple_interleaved_tool_calls() {
        let (events, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f1","arguments":"{\"a\":"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"f2","arguments":"{\"b\":"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"2}"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.content.len(), 2);
        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStart { index, block_type }
                    if block_type == "tool_use" =>
                {
                    Some(*index)
                }
                _ => None,
            })
            .collect();
        let stops: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStop { index } => Some(*index),
                _ => None,
            })
            .collect();
        assert!(starts.contains(&0));
        assert!(starts.contains(&1));
        assert!(stops.contains(&0));
        assert!(stops.contains(&1));
    }

    #[test]
    fn tool_call_block_index_shifts_when_text_precedes() {
        let (events, resp) = process_chunks(&[
            r#"{"id":"x","model":"m","choices":[{"index":0,"delta":{"content":"Using tool"}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{\"a\":1}"}}]}}]}"#,
            r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.content.len(), 2);
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::ContentBlockStart { index: 0, block_type } if block_type == "text"
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::ContentBlockStart { index: 1, block_type } if block_type == "tool_use"
        )));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ContentBlockStop { index: 1 }))
        );
    }

    #[test]
    fn responses_emits_tool_call_content_block_lifecycle() {
        let (events, resp) = process_responses_events(&[
            r#"{"type":"response.in_progress","response":{"id":"r1","model":"gpt-5","status":"in_progress","output":[],"usage":{"input_tokens":2,"output_tokens":0,"total_tokens":2}}}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f","delta":"{\"a\":"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f","delta":"1}"}"#,
            r#"{"type":"response.function_call_arguments.done","call_id":"call-1","name":"f","arguments":"{\"a\":1}"}"#,
            r#"{"type":"response.completed","response":{"id":"r1","model":"gpt-5","status":"completed","output":[{"type":"function_call","call_id":"call-1","name":"f","arguments":"{\"a\":1}"}],"usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7}}}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "f");
                assert_eq!(input["a"], 1);
            }
            _ => panic!("expected ToolUse"),
        }
        let start_idx = events.iter().position(|e| {
            matches!(e, StreamEvent::ContentBlockStart { index: 0, block_type } if block_type == "tool_use")
        });
        let stop_idx = events
            .iter()
            .position(|e| matches!(e, StreamEvent::ContentBlockStop { index: 0 }));
        assert!(start_idx.is_some(), "expected tool-use ContentBlockStart");
        assert!(stop_idx.is_some(), "expected tool-use ContentBlockStop");
        assert!(
            start_idx < stop_idx,
            "ContentBlockStart must precede ContentBlockStop"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            StreamEvent::InputJsonDelta { partial_json } if partial_json == "{\"a\":"
        )));
    }

    #[test]
    fn responses_emits_lifecycle_for_multiple_tool_calls() {
        let (events, resp) = process_responses_events(&[
            r#"{"type":"response.in_progress","response":{"id":"r1","model":"gpt-5","status":"in_progress","output":[],"usage":{"input_tokens":2,"output_tokens":0,"total_tokens":2}}}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f1","delta":"{\"a\":"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-2","name":"f2","delta":"{\"b\":"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f1","delta":"1}"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-2","name":"f2","delta":"2}"}"#,
            r#"{"type":"response.function_call_arguments.done","call_id":"call-1","name":"f1","arguments":"{\"a\":1}"}"#,
            r#"{"type":"response.function_call_arguments.done","call_id":"call-2","name":"f2","arguments":"{\"b\":2}"}"#,
            r#"{"type":"response.completed","response":{"id":"r1","model":"gpt-5","status":"completed","output":[{"type":"function_call","call_id":"call-1","name":"f1","arguments":"{\"a\":1}"},{"type":"function_call","call_id":"call-2","name":"f2","arguments":"{\"b\":2}"}],"usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7}}}"#,
        ]);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.content.len(), 2);
        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStart { index, block_type }
                    if block_type == "tool_use" =>
                {
                    Some(*index)
                }
                _ => None,
            })
            .collect();
        let stops: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ContentBlockStop { index } => Some(*index),
                _ => None,
            })
            .collect();
        assert!(starts.contains(&0));
        assert!(starts.contains(&1));
        assert!(stops.contains(&0));
        assert!(stops.contains(&1));
    }

    #[test]
    fn responses_no_duplicate_content_block_stop_when_done_fires() {
        // WHY: handle_fn_call_done emits ContentBlockStop; finish_inner must not
        // re-emit it for the same tool call, producing exactly one stop event.
        let (events, _resp) = process_responses_events(&[
            r#"{"type":"response.in_progress","response":{"id":"r1","model":"gpt-5","status":"in_progress","output":[],"usage":{"input_tokens":2,"output_tokens":0,"total_tokens":2}}}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f","delta":"{\"a\":"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"call-1","name":"f","delta":"1}"}"#,
            r#"{"type":"response.function_call_arguments.done","call_id":"call-1","name":"f","arguments":"{\"a\":1}"}"#,
            r#"{"type":"response.completed","response":{"id":"r1","model":"gpt-5","status":"completed","output":[{"type":"function_call","call_id":"call-1","name":"f","arguments":"{\"a\":1}"}],"usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7}}}"#,
        ]);
        let stop_count = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::ContentBlockStop { index: 0 }))
            .count();
        assert_eq!(
            stop_count, 1,
            "expected exactly one ContentBlockStop for index 0, got {stop_count}"
        );
    }

    #[test]
    fn malformed_responses_function_arguments_reject_parse_error() {
        let mut acc = ResponsesStreamAccumulator::new();
        let mut events = Vec::new();
        for event_json in [
            r#"{"type":"response.function_call_arguments.delta","call_id":"c1","name":"f","delta":"{not"}"#,
            r#"{"type":"response.function_call_arguments.delta","call_id":"c1","delta":" json"}"#,
            r#"{"type":"response.function_call_arguments.done","call_id":"c1","name":"f","arguments":"{not json"}"#,
        ] {
            let event: ResponsesStreamEvent = serde_json::from_str(event_json).unwrap();
            acc.process_event(event, &mut |e| events.push(e)).unwrap();
        }
        let err = acc
            .finish(&mut |e| events.push(e))
            .expect_err("malformed Responses function arguments should fail finish");
        assert!(
            matches!(err, error::Error::MalformedToolArguments { .. }),
            "expected MalformedToolArguments, got {err:?}"
        );
    }
}
