//! OpenAI-compatible request and response types for local LLM inference.
//!
//! These types model the subset of the `OpenAI` Chat Completions API that
//! vLLM and other compatible servers implement. They handle serialization
//! for outbound requests and deserialization for inbound responses.

use serde::{Deserialize, Serialize};

/// Chat completion request in the OpenAI-compatible wire format.
#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionRequest<'a> {
    /// Model identifier passed through to the vLLM endpoint.
    pub model: &'a str,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<&'a str>>,
    /// Tool (function) definitions for function calling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool<'a>>>,
    /// Tool choice constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'a str>,
    /// Whether to stream the response.
    pub stream: bool,
}

/// A chat message in the OpenAI-compatible wire format.
///
/// Uses owned strings for `content` and `tool_call_id` because message
/// mapping from Anthropic's block-based format requires constructing
/// new strings (e.g., joining text blocks, extracting tool result summaries).
#[derive(Debug, Serialize)]
pub(crate) struct ChatMessage {
    /// Message role: `"system"`, `"user"`, `"assistant"`, or `"tool"`.
    pub role: String,
    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls made by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    /// Tool call ID this message responds to (for `role: "tool"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool definition in the function-calling wire format.
#[derive(Debug, Serialize)]
pub(crate) struct ChatTool<'a> {
    /// Always `"function"`.
    #[serde(rename = "type")]
    pub tool_type: &'a str,
    /// Function definition.
    pub function: ChatFunction<'a>,
}

/// Function definition within a tool.
#[derive(Debug, Serialize)]
pub(crate) struct ChatFunction<'a> {
    /// Function name.
    pub name: &'a str,
    /// Human-readable description.
    pub description: &'a str,
    /// JSON Schema for the function parameters.
    pub parameters: &'a serde_json::Value,
}

/// A tool call in the response wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatToolCall {
    /// Unique tool call identifier.
    pub id: String,
    /// Always `"function"`.
    #[serde(rename = "type")]
    pub call_type: String,
    /// Function call details.
    pub function: ChatFunctionCall,
}

/// Function call details within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatFunctionCall {
    /// Function name.
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

/// Chat completion response (non-streaming).
#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionResponse {
    /// Response identifier.
    pub id: String,
    /// Model used for the completion.
    pub model: String,
    /// Response choices (typically one).
    pub choices: Vec<ChatChoice>,
    /// Token usage statistics.
    pub usage: Option<ChatUsage>,
}

/// A single choice in the completion response.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatChoice {
    /// The assistant's response message.
    pub message: ChatResponseMessage,
    /// Why generation stopped.
    pub finish_reason: Option<String>,
}

/// The assistant message in a completion response.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatResponseMessage {
    /// Text content.
    pub content: Option<String>,
    /// Tool calls requested by the model.
    pub tool_calls: Option<Vec<ChatToolCall>>,
}

/// Token usage in the completion response.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
}

/// A single streaming chunk in the SSE format.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionChunk {
    /// Chunk identifier (same across all chunks in a stream).
    pub id: String,
    /// Model used.
    pub model: String,
    /// Chunk choices (typically one).
    pub choices: Vec<ChatChunkChoice>,
    /// Usage statistics (only present in the final chunk when requested).
    pub usage: Option<ChatUsage>,
}

/// A choice within a streaming chunk.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunkChoice {
    /// Incremental content delta.
    pub delta: ChatChunkDelta,
    /// Finish reason (present only in the final chunk).
    pub finish_reason: Option<String>,
}

/// Delta content within a streaming chunk choice.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunkDelta {
    /// Incremental text content.
    pub content: Option<String>,
    /// Incremental tool call deltas.
    pub tool_calls: Option<Vec<ChatChunkToolCall>>,
}

/// Tool call delta in a streaming chunk.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunkToolCall {
    /// Zero-based index of the tool call being built.
    pub index: u32,
    /// Tool call ID (present in the first chunk for this tool call).
    #[serde(default)]
    pub id: Option<String>,
    /// Function call delta.
    pub function: Option<ChatChunkFunction>,
}

/// Function call delta in a streaming chunk.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunkFunction {
    /// Function name (present in the first chunk for this tool call).
    pub name: Option<String>,
    /// Incremental JSON arguments fragment.
    pub arguments: Option<String>,
}
