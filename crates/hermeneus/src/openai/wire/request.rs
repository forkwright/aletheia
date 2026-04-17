//! Translate [`CompletionRequest`] into the OpenAI Chat Completions wire format.
//!
//! Maps Anthropic-native concepts onto OpenAI equivalents, dropping fields
//! the target API cannot express:
//!
//! | Anthropic concept        | OpenAI mapping                         |
//! |--------------------------|----------------------------------------|
//! | `system` top-level       | `{role: "system"}` message prepended   |
//! | `ContentBlock::Text`     | assistant/user `content` string        |
//! | `ContentBlock::ToolUse`  | assistant message `tool_calls[]`       |
//! | `ContentBlock::ToolResult` | `{role: "tool", tool_call_id: ...}`  |
//! | `ContentBlock::Thinking` | dropped (warn at build time)           |
//! | `ToolDefinition`         | `{type: "function", function: {...}}`  |
//! | `ToolChoice`             | `{type: "function"} | "auto" | "any"`  |
//! | `cache_control`          | dropped (warn at build time)           |
//! | `server_tools`           | rejected (returns error)               |

use serde::Serialize;

use crate::error::{self, Result};
use crate::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, ToolChoice, ToolResultContent,
};

/// Top-level OpenAI Chat Completions request body.
#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ChatTool<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<&'a str>,
}

/// Single chat message. `role` is `"system" | "user" | "assistant" | "tool"`.
#[derive(Debug, Serialize)]
pub(crate) struct ChatMessage {
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: &'static str,
    pub function: ChatFunctionCall,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatFunctionCall {
    pub name: String,
    /// OpenAI requires `arguments` to be a **JSON-encoded string**, not
    /// the raw object — matches `json.dumps(...)` on the Python side.
    pub arguments: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatTool<'a> {
    #[serde(rename = "type")]
    pub tool_type: &'static str,
    pub function: ChatFunctionDef<'a>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatFunctionDef<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub parameters: &'a serde_json::Value,
}

impl<'a> ChatCompletionRequest<'a> {
    /// Build an OpenAI-format request from a hermeneus [`CompletionRequest`].
    ///
    /// # Errors
    ///
    /// Returns an error when the request carries features the OpenAI wire
    /// format cannot express and that would silently change behavior
    /// (currently: `server_tools` — the caller must route these through an
    /// Anthropic provider).
    pub(crate) fn from_request(req: &'a CompletionRequest, stream: Option<bool>) -> Result<Self> {
        if !req.server_tools.is_empty() {
            return Err(error::ProviderInitSnafu {
                message: "OpenAI-compatible providers do not support server-side tools \
                          (web_search, code_execution). Route these requests to an \
                          Anthropic provider, or remove the server_tools from the \
                          request."
                    .to_owned(),
            }
            .build());
        }

        // Thinking: the OpenAI Chat Completions wire format has no equivalent
        // concept. The model is never given a separate thinking budget, so the
        // request is sent verbatim and the operator's budget_tokens is dropped.
        if let Some(thinking) = &req.thinking
            && thinking.enabled
        {
            tracing::warn!(
                budget_tokens = thinking.budget_tokens,
                "thinking budget set on request routed to OpenAI-compatible provider; \
                 dropping (no OpenAI equivalent for extended thinking)"
            );
        }

        if req.cache_system || req.cache_tools || req.cache_turns {
            tracing::warn!(
                cache_system = req.cache_system,
                cache_tools = req.cache_tools,
                cache_turns = req.cache_turns,
                "prompt-cache flags set on request routed to OpenAI-compatible provider; \
                 dropping (no OpenAI equivalent for cache_control markers)"
            );
        }

        if req.citations.is_some() {
            tracing::debug!(
                "citations config set on request routed to OpenAI-compatible provider; \
                 ignored (no OpenAI equivalent)"
            );
        }

        let mut messages: Vec<ChatMessage> = Vec::with_capacity(req.messages.len() + 1);

        // Prepend system message. Anthropic carries `system` at the top level;
        // OpenAI expects it as the first message with role="system".
        if let Some(system) = req.system.as_deref()
            && !system.is_empty()
        {
            messages.push(ChatMessage {
                role: "system",
                content: Some(system.to_owned()),
                tool_calls: Vec::new(),
                tool_call_id: None,
            });
        }

        for msg in &req.messages {
            translate_message(msg, &mut messages);
        }

        let tools: Vec<ChatTool<'a>> = req
            .tools
            .iter()
            .map(|t| ChatTool {
                tool_type: "function",
                function: ChatFunctionDef {
                    name: &t.name,
                    description: &t.description,
                    parameters: &t.input_schema,
                },
            })
            .collect();

        let tool_choice = req.tool_choice.as_ref().map(translate_tool_choice);

        let stop: Vec<&str> = req.stop_sequences.iter().map(String::as_str).collect();

        let user = req.metadata.as_ref().and_then(|m| m.user_id.as_deref());

        Ok(Self {
            model: &req.model,
            messages,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stop,
            tools,
            tool_choice,
            stream,
            user,
        })
    }
}

/// Translate one hermeneus [`Message`] into one or more [`ChatMessage`]s.
///
/// Messages with mixed content (text + `tool_use` + `tool_result`) expand
/// into multiple chat messages: OpenAI requires each tool result to live
/// in its own `role: "tool"` message correlated by `tool_call_id`.
#[expect(
    clippy::too_many_lines,
    reason = "one function per Anthropic role keeps the wire mapping readable in one place"
)]
#[expect(
    clippy::collapsible_match,
    reason = "nested if lets read more clearly here than a combined match guard"
)]
fn translate_message(msg: &Message, out: &mut Vec<ChatMessage>) {
    match msg.role {
        Role::System => {
            // System messages inside the messages vec (rather than at top
            // level) get merged as plain system prefaces.
            out.push(ChatMessage {
                role: "system",
                content: Some(msg.content.text()),
                tool_calls: Vec::new(),
                tool_call_id: None,
            });
        }
        Role::User => {
            // User messages with tool_result blocks split into one
            // `role: "tool"` per result, plus any remaining text as a
            // follow-up user message.
            match &msg.content {
                Content::Text(text) => {
                    out.push(ChatMessage {
                        role: "user",
                        content: Some(text.clone()),
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                    });
                }
                Content::Blocks(blocks) => {
                    let mut text_parts: Vec<String> = Vec::new();
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text, .. } => {
                                if !text.is_empty() {
                                    text_parts.push(text.clone());
                                }
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => {
                                out.push(ChatMessage {
                                    role: "tool",
                                    content: Some(tool_result_to_string(content)),
                                    tool_calls: Vec::new(),
                                    tool_call_id: Some(tool_use_id.clone()),
                                });
                            }
                            // Thinking / server-side blocks have no inbound
                            // user-side equivalent; skip.
                            _ => {}
                        }
                    }
                    if !text_parts.is_empty() {
                        out.push(ChatMessage {
                            role: "user",
                            content: Some(text_parts.join("\n")),
                            tool_calls: Vec::new(),
                            tool_call_id: None,
                        });
                    }
                }
            }
        }
        Role::Assistant => {
            // Assistant messages coalesce text + tool_use into a single
            // message with `content` and `tool_calls`.
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<ChatToolCall> = Vec::new();
            match &msg.content {
                Content::Text(text) => {
                    text_parts.push(text.clone());
                }
                Content::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text, .. } => {
                                if !text.is_empty() {
                                    text_parts.push(text.clone());
                                }
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                // WHY: OpenAI requires arguments as a JSON
                                // *string*, not object. Round-trip through
                                // serde_json::to_string to encode.
                                let arguments = serde_json::to_string(input)
                                    .unwrap_or_else(|_| "{}".to_owned());
                                tool_calls.push(ChatToolCall {
                                    id: id.clone(),
                                    call_type: "function",
                                    function: ChatFunctionCall {
                                        name: name.clone(),
                                        arguments,
                                    },
                                });
                            }
                            // WHY: thinking blocks and server-side blocks
                            // have no OpenAI equivalent; skipped silently
                            // (thinking is warned once at request build
                            // time, server_tools is rejected outright).
                            _ => {}
                        }
                    }
                }
            }
            let content = if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            };
            // OpenAI rejects assistant messages where both `content` and
            // `tool_calls` are empty. Synthesize a single space to keep
            // the payload valid — matches openai-python behavior.
            let content = if content.is_none() && tool_calls.is_empty() {
                Some(String::new())
            } else {
                content
            };
            out.push(ChatMessage {
                role: "assistant",
                content,
                tool_calls,
                tool_call_id: None,
            });
        }
    }
}

fn tool_result_to_string(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Blocks(_) => {
            // OpenAI tool role accepts only text content; collapse via the
            // existing summary helper (images / documents become tags).
            content.text_summary()
        }
    }
}

fn translate_tool_choice(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::Value::String("auto".to_owned()),
        // OpenAI uses "required" to force any tool; Anthropic calls this "any".
        ToolChoice::Any => serde_json::Value::String("required".to_owned()),
        ToolChoice::Tool { name } => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
mod tests {
    use super::*;
    use crate::types::{
        CompletionRequest, Content, ContentBlock, Message, Role, ThinkingConfig, ToolDefinition,
        ToolResultContent,
    };

    #[test]
    fn system_prompt_becomes_first_system_message() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            system: Some("You are a helpful assistant.".to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hi".to_owned()),
            }],
            max_tokens: 128,
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        assert_eq!(wire.messages.len(), 2);
        assert_eq!(wire.messages[0].role, "system");
        assert_eq!(
            wire.messages[0].content.as_deref(),
            Some("You are a helpful assistant.")
        );
        assert_eq!(wire.messages[1].role, "user");
    }

    #[test]
    fn tool_definitions_map_to_function_tools() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("use a tool".to_owned()),
            }],
            max_tokens: 128,
            tools: vec![ToolDefinition {
                name: "get_weather".to_owned(),
                description: "Fetch weather for a city".to_owned(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } }
                }),
                disable_passthrough: None,
            }],
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        assert_eq!(wire.tools.len(), 1);
        let tool = &wire.tools[0];
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.function.name, "get_weather");
    }

    #[test]
    fn assistant_tool_use_block_becomes_tool_calls() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: Content::Text("call get_weather".to_owned()),
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Blocks(vec![
                        ContentBlock::Text {
                            text: "Sure".to_owned(),
                            citations: None,
                        },
                        ContentBlock::ToolUse {
                            id: "call_1".to_owned(),
                            name: "get_weather".to_owned(),
                            input: serde_json::json!({ "city": "Paris" }),
                        },
                    ]),
                },
            ],
            max_tokens: 128,
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        let assistant = wire
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        assert_eq!(assistant.tool_calls.len(), 1);
        assert_eq!(assistant.tool_calls[0].id, "call_1");
        assert_eq!(assistant.tool_calls[0].function.name, "get_weather");
        assert!(assistant.tool_calls[0].function.arguments.contains("Paris"));
    }

    #[test]
    fn user_tool_result_block_becomes_role_tool_message() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_owned(),
                    content: ToolResultContent::Text("sunny".to_owned()),
                    is_error: None,
                }]),
            }],
            max_tokens: 128,
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        let tool_msg = wire.messages.iter().find(|m| m.role == "tool").unwrap();
        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_msg.content.as_deref(), Some("sunny"));
    }

    #[test]
    fn thinking_block_is_dropped_and_warned() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hi".to_owned()),
            }],
            max_tokens: 128,
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: 1024,
            }),
            ..Default::default()
        };
        // No thinking field in the wire request; confirm it serializes.
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        let json = serde_json::to_string(&wire).unwrap();
        assert!(!json.contains("thinking"));
        assert!(!json.contains("budget_tokens"));
    }

    #[test]
    fn server_tools_rejected_with_clear_error() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hi".to_owned()),
            }],
            max_tokens: 128,
            server_tools: vec![crate::types::ServerToolDefinition {
                tool_type: "web_search_20250305".to_owned(),
                name: "web_search".to_owned(),
                max_uses: Some(3),
                allowed_domains: None,
                blocked_domains: None,
                user_location: None,
            }],
            ..Default::default()
        };
        let err = ChatCompletionRequest::from_request(&req, None).unwrap_err();
        assert!(err.to_string().contains("server-side tools"));
    }

    #[test]
    fn cache_flags_dropped_without_error() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            system: Some("sys".to_owned()),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hi".to_owned()),
            }],
            max_tokens: 128,
            cache_system: true,
            cache_tools: true,
            cache_turns: true,
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        let json = serde_json::to_string(&wire).unwrap();
        assert!(!json.contains("cache_control"));
    }

    #[test]
    fn tool_choice_any_maps_to_required() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hi".to_owned()),
            }],
            max_tokens: 128,
            tool_choice: Some(ToolChoice::Any),
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        assert_eq!(
            wire.tool_choice,
            Some(serde_json::Value::String("required".to_owned()))
        );
    }

    #[test]
    fn tool_arguments_serialized_as_json_string() {
        let req = CompletionRequest {
            model: "qwen".to_owned(),
            messages: vec![Message {
                role: Role::Assistant,
                content: Content::Blocks(vec![ContentBlock::ToolUse {
                    id: "c1".to_owned(),
                    name: "f".to_owned(),
                    input: serde_json::json!({ "x": 1 }),
                }]),
            }],
            max_tokens: 64,
            ..Default::default()
        };
        let wire = ChatCompletionRequest::from_request(&req, None).unwrap();
        let tc = &wire.messages[0].tool_calls[0];
        // arguments must be a JSON-encoded string containing the object.
        let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
        assert_eq!(parsed["x"], 1);
    }
}
