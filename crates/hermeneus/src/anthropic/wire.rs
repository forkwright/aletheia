//! Wire types matching the Anthropic Messages API format.

use serde::{Deserialize, Serialize};

use crate::types::{
    CompletionRequest, CompletionResponse, Content, ContentBlock, Role, StopReason, ThinkingConfig,
    Usage,
};

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct WireRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<WireTool<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<WireThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireMessage<'a> {
    pub role: &'a str,
    pub content: &'a Content,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireTool<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub input_schema: &'a serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireThinkingConfig {
    #[serde(rename = "type")]
    pub config_type: &'static str,
    pub budget_tokens: u32,
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

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
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[expect(
            dead_code,
            reason = "captured from API but not exposed in public types until M2"
        )]
        signature: String,
    },
}

// ---------------------------------------------------------------------------
// Error response
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl<'a> WireRequest<'a> {
    pub(crate) fn from_request(req: &'a CompletionRequest, stream: Option<bool>) -> Self {
        // Extract system prompt from messages (Anthropic wants it as a top-level field).
        let system = req.system.clone().or_else(|| {
            let system_texts: Vec<&str> = req
                .messages
                .iter()
                .filter(|m| m.role == Role::System)
                .map(|m| match &m.content {
                    Content::Text(s) => s.as_str(),
                    Content::Blocks(_) => "",
                })
                .filter(|s| !s.is_empty())
                .collect();
            if system_texts.is_empty() {
                None
            } else {
                Some(system_texts.join("\n\n"))
            }
        });

        let messages: Vec<WireMessage<'a>> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| WireMessage {
                role: m.role.as_str(),
                content: &m.content,
            })
            .collect();

        let tools: Vec<WireTool<'a>> = req
            .tools
            .iter()
            .map(|t| WireTool {
                name: &t.name,
                description: &t.description,
                input_schema: &t.input_schema,
            })
            .collect();

        let thinking = req.thinking.as_ref().and_then(|tc| {
            if tc.enabled {
                Some(WireThinkingConfig::from_config(tc))
            } else {
                None
            }
        });

        let stop_sequences: Vec<&str> = req.stop_sequences.iter().map(String::as_str).collect();

        Self {
            model: &req.model,
            max_tokens: req.max_tokens,
            messages,
            system,
            tools,
            temperature: req.temperature,
            stop_sequences,
            thinking,
            stream,
        }
    }
}

impl WireThinkingConfig {
    fn from_config(config: &ThinkingConfig) -> Self {
        Self {
            config_type: "enabled",
            budget_tokens: config.budget_tokens,
        }
    }
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
    fn into_content_block(self) -> ContentBlock {
        match self {
            Self::Text { text } => ContentBlock::Text { text },
            Self::ToolUse { id, name, input } => ContentBlock::ToolUse { id, name, input },
            Self::Thinking {
                thinking,
                signature: _,
            } => ContentBlock::Thinking { thinking },
        }
    }
}

impl WireUsage {
    fn into_usage(self) -> Usage {
        Usage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_write_tokens: self.cache_creation_input_tokens,
            cache_read_tokens: self.cache_read_input_tokens,
        }
    }
}

fn parse_stop_reason(s: &str) -> Result<StopReason, String> {
    match s {
        "end_turn" => Ok(StopReason::EndTurn),
        "tool_use" => Ok(StopReason::ToolUse),
        "max_tokens" => Ok(StopReason::MaxTokens),
        "stop_sequence" => Ok(StopReason::StopSequence),
        other => Err(format!("unknown stop_reason: {other}")),
    }
}

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
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireMessageDeltaBody {
    pub stop_reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WireMessageDeltaUsage {
    pub output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, ToolDefinition};

    #[test]
    fn wire_response_deserializes() {
        let json = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello"}],
            "model": "claude-opus-4-20250514",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;
        let resp: WireResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "msg_123");
        assert_eq!(resp.stop_reason, "end_turn");
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.cache_creation_input_tokens, 0);
    }

    #[test]
    fn wire_response_with_cache_tokens() {
        let json = r#"{
            "id": "msg_456",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi"}],
            "model": "claude-opus-4-20250514",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 200,
                "cache_read_input_tokens": 80
            }
        }"#;
        let resp: WireResponse = serde_json::from_str(json).unwrap();
        let converted = resp.into_response().unwrap();
        assert_eq!(converted.usage.cache_write_tokens, 200);
        assert_eq!(converted.usage.cache_read_tokens, 80);
    }

    #[test]
    fn wire_content_block_tool_use() {
        let json = r#"{"type":"tool_use","id":"toolu_1","name":"exec","input":{"cmd":"ls"}}"#;
        let block: WireContentBlock = serde_json::from_str(json).unwrap();
        let converted = block.into_content_block();
        match converted {
            ContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "exec");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn wire_content_block_thinking() {
        let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig_abc"}"#;
        let block: WireContentBlock = serde_json::from_str(json).unwrap();
        let converted = block.into_content_block();
        match converted {
            ContentBlock::Thinking { thinking } => {
                assert_eq!(thinking, "let me think");
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn wire_error_response_deserializes() {
        let json = r#"{
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "bad input"}
        }"#;
        let err: WireErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.message, "bad input");
    }

    #[test]
    fn wire_request_extracts_system_prompt() {
        let req = CompletionRequest {
            model: "claude-opus-4-20250514".to_owned(),
            system: Some("You are helpful.".to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
            }],
            max_tokens: 1024,
            tools: vec![],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
        };
        let wire = WireRequest::from_request(&req, None);
        assert_eq!(wire.system.as_deref(), Some("You are helpful."));
        assert_eq!(wire.messages.len(), 1);
        assert_eq!(wire.messages[0].role, "user");
    }

    #[test]
    fn wire_request_extracts_system_from_messages() {
        let req = CompletionRequest {
            model: "claude-opus-4-20250514".to_owned(),
            system: None,
            messages: vec![
                Message {
                    role: Role::System,
                    content: Content::Text("Be concise.".to_owned()),
                },
                Message {
                    role: Role::User,
                    content: Content::Text("hello".to_owned()),
                },
            ],
            max_tokens: 1024,
            tools: vec![],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
        };
        let wire = WireRequest::from_request(&req, None);
        assert_eq!(wire.system.as_deref(), Some("Be concise."));
        // System messages must not appear in the messages array
        assert_eq!(wire.messages.len(), 1);
    }

    #[test]
    fn wire_request_serializes_thinking_config() {
        let req = CompletionRequest {
            model: "claude-opus-4-20250514".to_owned(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("think hard".to_owned()),
            }],
            max_tokens: 16384,
            tools: vec![],
            temperature: None,
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: 8192,
            }),
            stop_sequences: vec![],
        };
        let wire = WireRequest::from_request(&req, None);
        let json = serde_json::to_value(&wire).unwrap();
        let thinking = json.get("thinking").unwrap();
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 8192);
    }

    #[test]
    fn wire_request_serializes_tools() {
        let req = CompletionRequest {
            model: "claude-opus-4-20250514".to_owned(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("run ls".to_owned()),
            }],
            max_tokens: 1024,
            tools: vec![ToolDefinition {
                name: "exec".to_owned(),
                description: "Execute a command".to_owned(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"]
                }),
            }],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
        };
        let wire = WireRequest::from_request(&req, None);
        let json = serde_json::to_value(&wire).unwrap();
        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "exec");
    }

    #[test]
    fn wire_stream_event_deserializes() {
        let json = r#"{"type":"message_start","message":{"id":"msg_1","model":"claude-opus-4-20250514","usage":{"input_tokens":10,"output_tokens":0}}}"#;
        let event: WireStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            WireStreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "msg_1");
            }
            _ => panic!("expected MessageStart"),
        }
    }

    #[test]
    fn wire_stream_delta_deserializes() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event: WireStreamEvent = serde_json::from_str(json).unwrap();
        match event {
            WireStreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    WireDelta::TextDelta { text } => assert_eq!(text, "Hello"),
                    _ => panic!("expected TextDelta"),
                }
            }
            _ => panic!("expected ContentBlockDelta"),
        }
    }

    #[test]
    fn parse_stop_reason_all_variants() {
        assert_eq!(parse_stop_reason("end_turn").unwrap(), StopReason::EndTurn);
        assert_eq!(parse_stop_reason("tool_use").unwrap(), StopReason::ToolUse);
        assert_eq!(
            parse_stop_reason("max_tokens").unwrap(),
            StopReason::MaxTokens
        );
        assert_eq!(
            parse_stop_reason("stop_sequence").unwrap(),
            StopReason::StopSequence
        );
        assert!(parse_stop_reason("unknown").is_err());
    }
}
