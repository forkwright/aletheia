//! SSE event wire shapes.

use serde::Serialize;
use utoipa::ToSchema;

/// SSE event emitted to the client during message streaming.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "type")]
#[non_exhaustive]
pub(crate) enum SseEvent {
    /// Acknowledgment that the message was accepted and a turn is starting.
    ///
    /// Includes `session_id`, `nous_id`, and `turn_id` so the client can
    /// reconnect to the turn event stream using `GET /sessions/{session_id}/turns/{turn_id}/events`
    /// with `Last-Event-ID` (#5163).
    #[serde(rename = "message_start")]
    MessageStart {
        status: String,
        /// Session identifier, supplied so the client can reconnect to this turn.
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Nous identifier for the agent handling this turn.
        #[serde(skip_serializing_if = "Option::is_none")]
        nous_id: Option<String>,
        /// Turn identifier used for reconnection and idempotent replay.
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// Per-request correlation ID for distributed tracing across the pipeline.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },

    /// Incremental text output from the assistant.
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    /// The assistant is invoking a tool.
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Result returned from a tool execution.
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        /// Stable outcome classification (#4558): `"success"` /
        /// `"partial_success"` / `"error"`, or a denial-class string when
        /// the call never ran. Mirrors `nous::pipeline::ToolCall::outcome_label()`.
        /// Additive/backward-compatible: absent on legacy senders, so
        /// clients must treat it as optional.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
    },

    /// Turn completed: final event in the stream.
    ///
    /// WHY(#5375): `error` mirrors `TurnOutcome::error` on the turn-stream
    /// protocol's `MessageComplete` so a failed legacy turn's terminal event
    /// is self-describing instead of relying on the client to have also
    /// retained the earlier, separate `Error` event. `stop_reason == "error"`
    /// already signals failure; this field carries *why* on the same event.
    /// `usage` stays zeroed on failure: the legacy `send_message` path calls
    /// the nous actor via a single non-streaming reply (`send_turn_with_cancel`),
    /// so pylon never observes partial usage/text/tool state to report — unlike
    /// `/api/v1/sessions/stream`, which streams events and can preserve them
    /// (see `stream_turn`'s failure branch).
    #[serde(rename = "message_complete")]
    MessageComplete {
        stop_reason: String,
        usage: UsageData,
        /// Provider instance that served the turn, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        /// Per-request correlation ID for distributed tracing across the pipeline.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        /// Failure message when this turn ended in error. `None` on success.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// An error occurred during the turn.
    #[serde(rename = "error")]
    Error {
        code: String,
        message: String,
        /// Per-request correlation ID for cross-system error tracing.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },

    /// Replay cannot be complete because the in-memory turn buffer truncated.
    #[serde(rename = "replay_gap")]
    ReplayGap {
        reason: String,
        dropped_after_seq: u64,
        retained_limit: usize,
    },

    /// Turn was aborted before completion (client disconnect, server shutdown,
    /// timeout, or explicit user cancellation).
    #[serde(rename = "turn_abort")]
    TurnAbort {
        reason: String,
        /// Per-request correlation ID for cross-system tracing.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

/// Token usage summary sent with `message_complete`.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "the `_tokens` suffix names the unit on the wire; dropping it breaks the message_complete API/schema contract"
)]
pub(crate) struct UsageData {
    /// Tokens consumed by the system prompt and conversation history.
    pub input_tokens: u64,
    /// Tokens generated by the model in this turn.
    pub output_tokens: u64,
    /// Tokens read from the provider cache during this turn.
    pub cache_read_tokens: u64,
    /// Tokens written to the provider cache during this turn.
    pub cache_write_tokens: u64,
}

/// SSE events for the turn streaming protocol (`POST /api/v1/sessions/stream`).
///
/// Used by `koilon` and the Signal integration. Event type discriminators
/// (`message_start`/`message_complete`) are shared with `SseEvent`, and all
/// fields use `snake_case` per API.md (#3271).
///
/// WARNING(#5785): `ToolUse` field names diverge from `SseEvent::ToolUse`:
/// this type uses `tool_id`/`tool_name`; `SseEvent` uses `id`/`name`.
/// `ToolResult` also diverges: this type carries `tool_name`, `tool_id`,
/// `result`, `is_error`, `duration_ms`; `SseEvent` carries `tool_use_id`,
/// `content`, `is_error`. Clients consuming both streams must handle both
/// shapes. Unifying the field names is a breaking wire change tracked by
/// #5785.
#[derive(Clone, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub(crate) enum TurnStreamEvent {
    /// Turn accepted - mirrors `SseEvent::MessageStart`.
    #[serde(rename = "message_start")]
    MessageStart {
        session_id: String,
        nous_id: String,
        turn_id: String,
        /// Per-request correlation ID for distributed tracing across the pipeline.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Incremental extended-thinking output.
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { text: String },
    /// Incremental text output.
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    /// Provider-reported message lifecycle start for one LLM call.
    #[serde(rename = "provider_message_start")]
    ProviderMessageStart { usage: UsageData },
    /// Provider-reported content block start.
    #[serde(rename = "provider_content_block_start")]
    ProviderContentBlockStart { index: u32, block_type: String },
    /// Provider tool-input lifecycle delta. The payload is a fixed redaction
    /// marker because partial JSON arrives before per-tool policy can be
    /// resolved; the later `ToolUse` event carries policy-aware input.
    #[serde(rename = "provider_input_json_delta")]
    ProviderInputJsonDelta { partial_json: String },
    /// Provider-reported content block stop.
    #[serde(rename = "provider_content_block_stop")]
    ProviderContentBlockStop { index: u32 },
    /// Provider-reported message lifecycle stop for one LLM call.
    #[serde(rename = "provider_message_stop")]
    ProviderMessageStop {
        stop_reason: String,
        usage: UsageData,
    },
    /// Provider event the adapter could observe but does not yet model.
    #[serde(rename = "provider_unsupported_event")]
    ProviderUnsupportedEvent { event_type: String },
    /// Tool invocation started.
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_name: String,
        tool_id: String,
        input: serde_json::Value,
        /// Canonical turn identity supplied by nous (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// Session that owns the turn (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Gateway request ID for the originating HTTP request (#4853).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Tool execution is awaiting approval.
    #[serde(rename = "tool_approval_required")]
    ToolApprovalRequired {
        turn_id: String,
        tool_name: String,
        tool_id: String,
        input: serde_json::Value,
        risk: String,
        reason: String,
        /// Session that owns the turn (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Gateway request ID for the originating HTTP request (#4853).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Tool approval decision resolved.
    #[serde(rename = "tool_approval_resolved")]
    ToolApprovalResolved {
        tool_id: String,
        decision: String,
        /// Canonical turn identity supplied by nous (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// Session that owns the turn (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Gateway request ID for the originating HTTP request (#4853).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Tool execution result.
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_name: String,
        tool_id: String,
        result: String,
        is_error: bool,
        duration_ms: u64,
        /// Stable outcome classification (#4558): `"success"` /
        /// `"partial_success"` / `"error"`, or a denial-class string when
        /// the call never ran. Mirrors `nous::pipeline::ToolCall::outcome_label()`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<String>,
        /// Canonical turn identity supplied by nous (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// Session that owns the turn (#5016) — additive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// Gateway request ID for the originating HTTP request (#4853).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Turn completed - mirrors `SseEvent::MessageComplete`.
    ///
    /// This is the only terminal error contract for turn streams: errors may
    /// be announced earlier with `Error`, but the stream is not terminal until
    /// `outcome.stop_reason == "error"` and `outcome.error` carries the
    /// message on this completion event.
    #[serde(rename = "message_complete")]
    MessageComplete { outcome: TurnOutcome },
    /// Diagnostic error event. Clients must continue reading for the terminal
    /// `MessageComplete` event.
    #[serde(rename = "error")]
    Error {
        /// Stable machine-readable failure classification.
        code: String,
        message: String,
        /// Per-request correlation ID for cross-system error tracing.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },

    /// Replay cannot be complete because the in-memory turn buffer truncated.
    #[serde(rename = "replay_gap")]
    ReplayGap {
        reason: String,
        dropped_after_seq: u64,
        retained_limit: usize,
    },

    /// Turn was aborted before completion (client disconnect, server shutdown,
    /// timeout, or explicit user cancellation).
    #[serde(rename = "turn_abort")]
    TurnAbort {
        reason: String,
        /// Per-request correlation ID for cross-system tracing.
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

/// Turn completion data emitted in `MessageComplete` events.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TurnOutcome {
    pub text: String,
    pub nous_id: String,
    pub session_id: String,
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub tool_calls: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
