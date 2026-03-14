use serde::Deserialize;

use super::response::{WireErrorDetail, WireUsage};

// ---------------------------------------------------------------------------
// Streaming wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum WireStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: WireMessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: WireContentBlockStart,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: WireDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: WireMessageDeltaBody,
        usage: WireMessageDeltaUsage,
    },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: WireErrorDetail },
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireMessageStart {
    pub id: String,
    pub model: String,
    pub usage: WireUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum WireContentBlockStart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "server_tool_use")]
    ServerToolUse { id: String, name: String },
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult { tool_use_id: String },
    #[serde(rename = "code_execution_result")]
    CodeExecutionResult {},
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[expect(
    clippy::enum_variant_names,
    reason = "variant names match Anthropic SSE delta types"
)]
pub(crate) enum WireDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireMessageDeltaBody {
    pub stop_reason: String,
}

#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "field names mirror the Anthropic wire format exactly"
)]
pub(crate) struct WireMessageDeltaUsage {
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}
