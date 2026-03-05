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
                .collect::<Vec<_>>()
                .join("\n"),
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
}

/// Tool result content — simple text or rich content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
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
                .collect::<Vec<_>>()
                .join("\n"),
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

/// A tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name (must match what the model calls).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the input parameters.
    pub input_schema: serde_json::Value,
}

/// Cache control directive for prompt caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub control_type: String,
}

impl CacheControl {
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            control_type: "ephemeral".to_owned(),
        }
    }
}

/// Control tool use behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
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

/// Token count result from the `count_tokens` endpoint.
#[derive(Debug, Clone)]
pub struct TokenCount {
    pub input_tokens: u64,
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
    /// Available tools.
    pub tools: Vec<ToolDefinition>,
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
            temperature: None,
            thinking: None,
            stop_sequences: Vec::new(),
            cache_system: false,
            cache_tools: false,
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
mod tests {
    use super::*;

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn stop_reason_serde_roundtrip() {
        for reason in [
            StopReason::EndTurn,
            StopReason::ToolUse,
            StopReason::MaxTokens,
            StopReason::StopSequence,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn content_text_extraction() {
        let text = Content::Text("hello world".to_owned());
        assert_eq!(text.text(), "hello world");

        let blocks = Content::Blocks(vec![
            ContentBlock::Thinking {
                thinking: "let me think".to_owned(),
                signature: None,
            },
            ContentBlock::Text {
                text: "the answer is 42".to_owned(),
                citations: None,
            },
        ]);
        assert!(blocks.text().contains("let me think"));
        assert!(blocks.text().contains("the answer is 42"));
    }

    #[test]
    fn tool_use_block_serde() {
        let block = ContentBlock::ToolUse {
            id: "tool_123".to_owned(),
            name: "exec".to_owned(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("tool_use"));
        assert!(json.contains("exec"));

        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        match back {
            ContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "tool_123");
                assert_eq!(name, "exec");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn tool_result_block_serde() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tool_123".to_owned(),
            content: ToolResultContent::text("file.txt\ndir/"),
            is_error: Some(false),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        match back {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool_123");
                assert_eq!(content.text_summary(), "file.txt\ndir/");
                assert_eq!(is_error, Some(false));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_text_serializes_as_string() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".to_owned(),
            content: ToolResultContent::text("hello"),
            is_error: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            v["content"].is_string(),
            "Text should serialize as bare string"
        );
        assert_eq!(v["content"], "hello");
    }

    #[test]
    fn tool_result_blocks_serializes_as_array() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".to_owned(),
            content: ToolResultContent::blocks(vec![
                ToolResultBlock::Text {
                    text: "description".to_owned(),
                },
                ToolResultBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_owned(),
                        media_type: "image/png".to_owned(),
                        data: "iVBOR".to_owned(),
                    },
                },
            ]),
            is_error: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["content"].is_array(), "Blocks should serialize as array");
        assert_eq!(v["content"].as_array().unwrap().len(), 2);
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][1]["type"], "image");
    }

    #[test]
    fn tool_result_content_text_deserializes_from_string() {
        let json = r#"{"type":"tool_result","tool_use_id":"t1","content":"hello"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.text_summary(), "hello");
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_content_blocks_deserializes_from_array() {
        let json = r#"{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"hi"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}]}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.text_summary(), "hi\n[image]");
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn image_source_serde_roundtrip() {
        let source = ImageSource {
            source_type: "base64".to_owned(),
            media_type: "image/png".to_owned(),
            data: "iVBOR".to_owned(),
        };
        let json = serde_json::to_string(&source).unwrap();
        let back: ImageSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_type, "base64");
        assert_eq!(back.media_type, "image/png");
        assert_eq!(back.data, "iVBOR");
    }

    #[test]
    fn document_source_serde_roundtrip() {
        let source = DocumentSource {
            source_type: "base64".to_owned(),
            media_type: "application/pdf".to_owned(),
            data: "JVBERi0".to_owned(),
        };
        let json = serde_json::to_string(&source).unwrap();
        let back: DocumentSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_type, "base64");
        assert_eq!(back.media_type, "application/pdf");
        assert_eq!(back.data, "JVBERi0");
    }

    #[test]
    fn tool_result_content_from_string() {
        let content: ToolResultContent = "hello".to_owned().into();
        assert_eq!(content.text_summary(), "hello");
    }

    #[test]
    fn usage_total() {
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 800,
            cache_write_tokens: 200,
        };
        assert_eq!(usage.total(), 1500);
    }

    #[test]
    fn citation_char_location_serde() {
        let citation = Citation::CharLocation {
            document_index: 0,
            start_char_index: 10,
            end_char_index: 50,
            cited_text: "some text".to_owned(),
        };
        let json = serde_json::to_string(&citation).unwrap();
        let back: Citation = serde_json::from_str(&json).unwrap();
        match back {
            Citation::CharLocation {
                document_index,
                start_char_index,
                ..
            } => {
                assert_eq!(document_index, 0);
                assert_eq!(start_char_index, 10);
            }
            _ => panic!("expected CharLocation"),
        }
    }

    #[test]
    fn thinking_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "deep thoughts".to_owned(),
            signature: Some("sig_xyz".to_owned()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        match back {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "deep thoughts");
                assert_eq!(signature.as_deref(), Some("sig_xyz"));
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn thinking_no_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "brief".to_owned(),
            signature: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        match back {
            ContentBlock::Thinking { signature, .. } => {
                assert!(signature.is_none());
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn completion_request_default() {
        let req = CompletionRequest::default();
        assert!(req.model.is_empty());
        assert!(req.system.is_none());
        assert!(req.messages.is_empty());
        assert_eq!(req.max_tokens, 4096);
        assert!(!req.cache_system);
        assert!(!req.cache_tools);
        assert!(req.tool_choice.is_none());
        assert!(req.metadata.is_none());
        assert!(req.citations.is_none());
    }

    #[test]
    fn tool_choice_serde() {
        let auto = ToolChoice::Auto;
        let json = serde_json::to_string(&auto).unwrap();
        assert!(json.contains("\"type\":\"auto\""));

        let tool = ToolChoice::Tool {
            name: "exec".to_owned(),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"type\":\"tool\""));
        assert!(json.contains("\"name\":\"exec\""));
    }

    #[test]
    fn text_block_with_citations_serde() {
        let block = ContentBlock::Text {
            text: "cited text".to_owned(),
            citations: Some(vec![Citation::CharLocation {
                document_index: 0,
                start_char_index: 0,
                end_char_index: 10,
                cited_text: "source".to_owned(),
            }]),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        match back {
            ContentBlock::Text { citations, .. } => {
                assert_eq!(citations.unwrap().len(), 1);
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn completion_response_serde() {
        let response = CompletionResponse {
            id: "msg_123".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Hello!".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                ..Usage::default()
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        let back: CompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "msg_123");
        assert_eq!(back.stop_reason, StopReason::EndTurn);
    }
}
