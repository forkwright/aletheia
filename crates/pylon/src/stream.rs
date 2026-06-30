//! SSE event types and hermeneus→SSE bridge.

#[path = "stream_dto.rs"]
mod stream_dto;
pub(crate) use stream_dto::{SseEvent, TurnOutcome, TurnStreamEvent, UsageData};

impl SseEvent {
    /// SSE event name for the `event:` field.
    #[must_use]
    pub(crate) fn event_type(&self) -> &'static str {
        match self {
            Self::MessageStart { .. } => "message_start",
            Self::TextDelta { .. } => "text_delta",
            Self::ToolUse { .. } => "tool_use",
            Self::ToolResult { .. } => "tool_result",
            Self::MessageComplete { .. } => "message_complete",
            Self::Error { .. } => "error",
            Self::ReplayGap { .. } => "replay_gap",
            Self::TurnAbort { .. } => "turn_abort",
        }
    }
}

impl TurnStreamEvent {
    /// SSE event name for the `event:` field.
    #[must_use]
    pub(crate) fn event_type(&self) -> &'static str {
        match self {
            Self::MessageStart { .. } => "message_start",
            Self::ThinkingDelta { .. } => "thinking_delta",
            Self::TextDelta { .. } => "text_delta",
            Self::ProviderMessageStart { .. } => "provider_message_start",
            Self::ProviderContentBlockStart { .. } => "provider_content_block_start",
            Self::ProviderInputJsonDelta { .. } => "provider_input_json_delta",
            Self::ProviderContentBlockStop { .. } => "provider_content_block_stop",
            Self::ProviderMessageStop { .. } => "provider_message_stop",
            Self::ProviderUnsupportedEvent { .. } => "provider_unsupported_event",
            Self::ToolUse { .. } => "tool_use",
            Self::ToolApprovalRequired { .. } => "tool_approval_required",
            Self::ToolApprovalResolved { .. } => "tool_approval_resolved",
            Self::ToolResult { .. } => "tool_result",
            Self::MessageComplete { .. } => "message_complete",
            Self::Error { .. } => "error",
            Self::ReplayGap { .. } => "replay_gap",
            Self::TurnAbort { .. } => "turn_abort",
        }
    }
}
