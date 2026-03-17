use serde::Deserialize;

use super::parse_stop_reason;
use crate::types::{CompletionResponse, ContentBlock, Usage};

#[derive(Debug, Deserialize)]
pub(crate) struct WireResponse {
    pub id: String,
    pub content: Vec<WireContentBlock>,
    pub model: String,
    pub stop_reason: String,
    pub usage: WireUsage,
}

#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "field names match Anthropic API wire format"
)]
pub(crate) struct WireUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum WireContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default)]
        citations: Option<Vec<crate::types::Citation>>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: String },
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(rename = "code_execution_result")]
    CodeExecutionResult {
        code: String,
        stdout: String,
        stderr: String,
        return_code: i32,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireErrorResponse {
    pub error: WireErrorDetail,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl WireResponse {
    pub(crate) fn into_response(self) -> Result<CompletionResponse, String> {
        let stop_reason = parse_stop_reason(&self.stop_reason)?;

        let content = self
            .content
            .into_iter()
            .map(WireContentBlock::into_content_block)
            .collect();

        Ok(CompletionResponse {
            id: self.id,
            model: self.model,
            stop_reason,
            content,
            usage: self.usage.into_usage(),
        })
    }
}

impl WireContentBlock {
    pub(super) fn into_content_block(self) -> ContentBlock {
        match self {
            Self::Text { text, citations } => ContentBlock::Text { text, citations },
            Self::ToolUse { id, name, input } => ContentBlock::ToolUse { id, name, input },
            Self::Thinking {
                thinking,
                signature,
            } => ContentBlock::Thinking {
                thinking,
                signature: Some(signature),
            },
            Self::ServerToolUse { id, name, input } => {
                ContentBlock::ServerToolUse { id, name, input }
            }
            Self::WebSearchToolResult {
                tool_use_id,
                content,
            } => ContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
            },
            Self::CodeExecutionResult {
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
        }
    }
}

impl WireUsage {
    pub(super) fn into_usage(self) -> Usage {
        Usage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_write_tokens: self.cache_creation_input_tokens,
            cache_read_tokens: self.cache_read_input_tokens,
        }
    }
}
