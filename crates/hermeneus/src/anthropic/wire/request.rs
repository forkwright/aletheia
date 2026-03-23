use serde::Serialize;

use super::{compute_turn_cache_indices, content_with_cache_control};
use crate::types::{CacheControl, CompletionRequest, Content, Role, ThinkingConfig, ToolChoice};

#[derive(Debug, Serialize)]
pub(crate) struct WireRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<WireToolEntry<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<WireThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'a ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<&'a crate::types::RequestMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<&'a crate::types::CitationConfig>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireMessage<'a> {
    pub role: &'a str,
    pub content: WireContent<'a>,
}

#[derive(Debug)]
pub(crate) enum WireContent<'a> {
    Borrowed(&'a Content),
    WithCacheControl(serde_json::Value),
}

impl serde::Serialize for WireContent<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Borrowed(content) => content.serialize(serializer),
            Self::WithCacheControl(value) => value.serialize(serializer),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct WireTool<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub input_schema: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    /// When true, the model returns `tool_use` blocks without executing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_passthrough: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireServerTool<'a> {
    #[serde(rename = "type")]
    pub tool_type: &'a str,
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<&'a serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum WireToolEntry<'a> {
    UserDefined(WireTool<'a>),
    ServerSide(WireServerTool<'a>),
}

#[derive(Debug, Serialize)]
pub(crate) struct WireThinkingConfig {
    #[serde(rename = "type")]
    pub config_type: &'static str,
    pub budget_tokens: u32,
}

impl<'a> WireRequest<'a> {
    #[expect(
        clippy::too_many_lines,
        reason = "request construction with caching logic"
    )]
    pub(crate) fn from_request(req: &'a CompletionRequest, stream: Option<bool>) -> Self {
        // WHY: Anthropic API requires system prompt as a top-level field, not in messages.
        let system_text = req.system.clone().or_else(|| {
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

        // WHY: Anthropic caching requires system as an array with cache_control on the last block.
        // codequality:ignore — system_text is the LLM system prompt, not a credential
        let system = system_text.map(|text| {
            if req.cache_system {
                serde_json::json!([{
                    "type": "text",
                    "text": text,
                    "cache_control": {"type": "ephemeral"}
                }])
            } else {
                serde_json::Value::String(text)
            }
        });

        let non_system: Vec<&crate::types::Message> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .collect();

        let cached_indices = if req.cache_turns {
            compute_turn_cache_indices(&non_system)
        } else {
            Vec::new()
        };

        let messages: Vec<WireMessage<'a>> = non_system
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let content = if cached_indices.contains(&i) {
                    WireContent::WithCacheControl(content_with_cache_control(&m.content))
                } else {
                    WireContent::Borrowed(&m.content)
                };
                WireMessage {
                    role: m.role.as_str(),
                    content,
                }
            })
            .collect();

        let user_tool_count = req.tools.len();
        let mut tools: Vec<WireToolEntry<'a>> = req
            .tools
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let cache_control = if req.cache_tools && i == user_tool_count - 1 {
                    Some(CacheControl::ephemeral())
                } else {
                    None
                };
                WireToolEntry::UserDefined(WireTool {
                    name: &t.name,
                    description: &t.description,
                    input_schema: &t.input_schema,
                    cache_control,
                    disable_passthrough: t.disable_passthrough,
                })
            })
            .collect();

        for st in &req.server_tools {
            tools.push(WireToolEntry::ServerSide(WireServerTool {
                tool_type: &st.tool_type,
                name: &st.name,
                max_uses: st.max_uses,
                allowed_domains: st.allowed_domains.as_deref(),
                blocked_domains: st.blocked_domains.as_deref(),
                user_location: st.user_location.as_ref(),
            }));
        }

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
            tool_choice: req.tool_choice.as_ref(),
            metadata: req.metadata.as_ref(),
            citations: req.citations.as_ref(),
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
