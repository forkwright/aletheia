//! Anthropic-native types for LLM interaction, with provider adapter support.
//!
//! These types model the Anthropic Messages API surface natively. Other providers
//! get adapter shims that map to what they support.

use serde::{Deserialize, Serialize};

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Message content (text or structured blocks).
    pub content: Content,
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt (Anthropic: separate field, `OpenAI`: system message).
    System,
    /// User message.
    User,
    /// Assistant response.
    Assistant,
}

impl Role {
    /// The lowercase wire-format string for this role.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Message content — either plain text or structured blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Plain text content.
    Text(String),
    /// Structured content blocks (text, tool use, tool result, thinking).
    Blocks(Vec<ContentBlock>),
}

impl Content {
    /// Extract plain text from content (joining blocks if structured).
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    ContentBlock::Thinking { thinking, .. } => Some(thinking.as_str()),
                    _ => None,
                })
                .fold(String::new(), |mut acc, s| {
                    if !acc.is_empty() {
                        acc.push('\n');
                    }
                    acc.push_str(s);
                    acc
                }),
        }
    }
}

/// A structured content block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ContentBlock {
    /// Text content.
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        citations: Option<Vec<Citation>>,
    },

    /// Tool use request from assistant.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Provider-assigned tool use identifier (used to correlate with [`ToolResult`](ContentBlock::ToolResult)).
        id: String,
        /// Tool name matching a registered [`ToolDefinition::name`].
        name: String,
        /// Parsed JSON input arguments for the tool.
        input: serde_json::Value,
    },

    /// Tool result from user.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// The [`ToolUse`](ContentBlock::ToolUse) `id` this result responds to.
        tool_use_id: String,
        /// Tool output content (text or rich content blocks).
        content: ToolResultContent,
        /// Whether the tool execution failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },

    /// Extended thinking content.
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },

    /// Server-side tool use (informational, not dispatched locally).
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Server-side web search tool result (opaque, round-tripped verbatim).
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },

    /// Server-side code execution result.
    ///
    /// Returned by the `code_execution_20250522` server tool. No client `tool_result`
    /// is needed — the server executed the code and returns stdout, stderr, and return code.
    #[serde(rename = "code_execution_result")]
    CodeExecutionResult {
        /// The Python code that was executed.
        code: String,
        /// Standard output from execution.
        stdout: String,
        /// Standard error from execution.
        stderr: String,
        /// Process return code (0 = success).
        return_code: i32,
    },
}

/// Tool result content — simple text or rich content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ToolResultContent {
    /// Simple text result (most common case, backward compatible).
    Text(String),
    /// Rich content blocks (text + images + documents).
    Blocks(Vec<ToolResultBlock>),
}

impl ToolResultContent {
    /// Create a simple text result.
    #[must_use]
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create from rich content blocks.
    #[must_use]
    pub fn blocks(blocks: Vec<ToolResultBlock>) -> Self {
        Self::Blocks(blocks)
    }

    /// Extract a text summary suitable for persistence and logging.
    #[must_use]
    pub fn text_summary(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    ToolResultBlock::Text { text } => text.as_str(),
                    ToolResultBlock::Image { .. } => "[image]",
                    ToolResultBlock::Document { .. } => "[document]",
                })
                .fold(String::new(), |mut acc, s| {
                    if !acc.is_empty() {
                        acc.push('\n');
                    }
                    acc.push_str(s);
                    acc
                }),
        }
    }
}

impl From<String> for ToolResultContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

/// Content block inside a tool result (text, image, or document).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ToolResultBlock {
    /// Text content.
    #[serde(rename = "text")]
    Text { text: String },
    /// Base64-encoded image.
    #[serde(rename = "image")]
    Image { source: ImageSource },
    /// Base64-encoded document (PDF).
    #[serde(rename = "document")]
    Document { source: DocumentSource },
}

/// Image source for vision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    /// Source type (always `"base64"`).
    #[serde(rename = "type")]
    pub source_type: String,
    /// MIME type (`"image/png"`, `"image/jpeg"`, `"image/gif"`, `"image/webp"`).
    pub media_type: String,
    /// Base64-encoded image data.
    pub data: String,
}

/// Document source (PDF).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSource {
    /// Source type (always `"base64"`).
    #[serde(rename = "type")]
    pub source_type: String,
    /// MIME type (always `"application/pdf"`).
    pub media_type: String,
    /// Base64-encoded PDF data.
    pub data: String,
}

/// A server-side tool definition (runs on the API provider's infrastructure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolDefinition {
    /// Server tool type identifier (e.g., `"web_search_20250305"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Display name.
    pub name: String,
    /// Maximum uses per turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    /// Allowed domains for web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// Blocked domains for web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    /// User location hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<serde_json::Value>,
}

/// A tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name (must match what the model calls).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the input parameters.
    pub input_schema: serde_json::Value,
    /// When true, the model returns `tool_use` blocks but does not execute them.
    /// The client must execute the tool and return a `tool_result`.
    /// This prevents the model from calling the tool via server-side passthrough.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_passthrough: Option<bool>,
}

/// Cache control directive for prompt caching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CacheControl {
    #[serde(rename = "type")]
    pub kind: CacheControlType,
}

/// The type of cache control.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CacheControlType {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

impl CacheControl {
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            kind: CacheControlType::Ephemeral,
        }
    }
}

/// Caching strategy for prompt caching.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub(crate) enum CachingStrategy {
    #[default]
    Auto,
    Disabled,
}

/// Configuration for prompt caching behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "prompt caching wire types; not yet wired into provider"
    )
)]
pub(crate) struct CachingConfig {
    pub enabled: bool,
    #[serde(default)]
    pub strategy: CachingStrategy,
}

impl Default for CachingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: CachingStrategy::Auto,
        }
    }
}

/// Control tool use behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "any")]
    Any,
    #[serde(rename = "tool")]
    Tool { name: String },
}

/// Optional request metadata for tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Citation configuration for requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationConfig {
    pub enabled: bool,
}

/// A source citation in a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Citation {
    #[serde(rename = "char_location")]
    CharLocation {
        document_index: u32,
        start_char_index: u32,
        end_char_index: u32,
        cited_text: String,
    },
    #[serde(rename = "page_location")]
    PageLocation {
        document_index: u32,
        start_page: u32,
        end_page: u32,
        cited_text: String,
    },
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation {
        url: String,
        title: Option<String>,
        cited_text: String,
    },
}

/// Request to the LLM provider.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// Model identifier (e.g. `claude-opus-4-20250514`).
    pub model: String,
    /// System prompt.
    pub system: Option<String>,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum output tokens.
    pub max_tokens: u32,
    /// Available user-defined tools.
    pub tools: Vec<ToolDefinition>,
    /// Server-side tools (e.g., web search) that execute on the provider's infrastructure.
    pub server_tools: Vec<ServerToolDefinition>,
    /// Temperature (0.0–1.0).
    pub temperature: Option<f32>,
    /// Whether to enable extended thinking.
    pub thinking: Option<ThinkingConfig>,
    /// Stop sequences.
    pub stop_sequences: Vec<String>,
    /// When true, system prompt gets `cache_control: ephemeral`.
    pub cache_system: bool,
    /// When true, last tool definition gets `cache_control: ephemeral`.
    pub cache_tools: bool,
    /// When true, recent non-current conversation turns get `cache_control: ephemeral`.
    pub cache_turns: bool,
    /// Control tool use behavior (auto/any/specific tool).
    pub tool_choice: Option<ToolChoice>,
    /// Request metadata for tracking.
    pub metadata: Option<RequestMetadata>,
    /// Enable citation tracking in responses.
    pub citations: Option<CitationConfig>,
}

impl Default for CompletionRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            system: None,
            messages: Vec::new(),
            max_tokens: 4096,
            tools: Vec::new(),
            server_tools: Vec::new(),
            temperature: None,
            thinking: None,
            stop_sequences: Vec::new(),
            cache_system: false,
            cache_tools: false,
            cache_turns: false,
            tool_choice: None,
            metadata: None,
            citations: None,
        }
    }
}

/// Configuration for extended thinking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Whether thinking is enabled.
    pub enabled: bool,
    /// Maximum thinking tokens.
    pub budget_tokens: u32,
}

/// Response from the LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    /// Response ID.
    pub id: String,
    /// Model used.
    pub model: String,
    /// Why the model stopped generating.
    pub stop_reason: StopReason,
    /// Response content blocks.
    pub content: Vec<ContentBlock>,
    /// Token usage.
    pub usage: Usage,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StopReason {
    /// Normal end of turn.
    EndTurn,
    /// Model wants to use a tool.
    ToolUse,
    /// Hit max tokens limit.
    MaxTokens,
    /// Hit a stop sequence.
    StopSequence,
}

impl StopReason {
    /// The `snake_case` wire-format string for this stop reason.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::ToolUse => "tool_use",
            Self::MaxTokens => "max_tokens",
            Self::StopSequence => "stop_sequence",
        }
    }
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for StopReason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "end_turn" => Ok(Self::EndTurn),
            "tool_use" => Ok(Self::ToolUse),
            "max_tokens" => Ok(Self::MaxTokens),
            "stop_sequence" => Ok(Self::StopSequence),
            other => Err(format!("unknown stop_reason: {other}")),
        }
    }
}

/// Token usage for a completion.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens generated.
    pub output_tokens: u64,
    /// Tokens read from cache.
    pub cache_read_tokens: u64,
    /// Tokens written to cache.
    pub cache_write_tokens: u64,
}

impl Usage {
    /// Total tokens (input + output).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
