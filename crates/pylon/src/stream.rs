//! SSE event types and hermeneus→SSE bridge.

use serde::Serialize;

/// SSE event emitted to the client during message streaming.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    #[serde(rename = "message_complete")]
    MessageComplete {
        stop_reason: String,
        usage: UsageData,
    },

    #[serde(rename = "error")]
    Error { code: String, message: String },
}

/// Token usage summary sent with `message_complete`.
#[derive(Debug, Clone, Serialize)]
pub struct UsageData {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl SseEvent {
    /// SSE event name for the `event:` field.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::TextDelta { .. } => "text_delta",
            Self::ThinkingDelta { .. } => "thinking_delta",
            Self::ToolUse { .. } => "tool_use",
            Self::ToolResult { .. } => "tool_result",
            Self::MessageComplete { .. } => "message_complete",
            Self::Error { .. } => "error",
        }
    }
}
