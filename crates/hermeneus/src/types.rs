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
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::Thinking { thinking } => Some(thinking.as_str()),
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
    Text { text: String },

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
        /// Tool output content (text).
        content: String,
        /// Whether the tool execution failed.
        is_error: Option<bool>,
    },

    /// Extended thinking content.
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
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
            },
            ContentBlock::Text {
                text: "the answer is 42".to_owned(),
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
            content: "file.txt\ndir/".to_owned(),
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
                assert_eq!(content, "file.txt\ndir/");
                assert_eq!(is_error, Some(false));
            }
            _ => panic!("expected ToolResult"),
        }
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
    fn completion_response_serde() {
        let response = CompletionResponse {
            id: "msg_123".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Hello!".to_owned(),
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
