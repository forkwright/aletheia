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
    CompletionRequest, Content, ContentBlock, Message, OutputFormat, Role, ToolChoice,
    ToolResultContent,
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
    #[serde(skip_serializing_if = "Option::is_none", rename = "response_format")]
    pub output_format: Option<WireResponseFormat>,
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

/// Top-level OpenAI Responses request body.
#[derive(Debug, Serialize)]
pub(crate) struct ResponsesRequest<'a> {
    pub model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub input: Vec<ResponsesInputItem>,
    pub max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ResponsesTool<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<&'a str>,
}

/// Wire representation of `response_format` for OpenAI Chat Completions.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum WireResponseFormat {
    #[serde(rename = "json_schema")]
    JsonSchema {
        #[serde(rename = "json_schema")]
        definition: WireJsonSchema,
    },
    #[serde(rename = "text")]
    Text,
}

#[derive(Debug, Serialize)]
pub(crate) struct WireJsonSchema {
    pub name: String,
    pub schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ResponsesInputItem {
    Message {
        role: &'static str,
        content: String,
    },
    FunctionCall {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        output: String,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponsesTool<'a> {
    #[serde(rename = "type")]
    pub tool_type: &'static str,
    pub name: &'a str,
    pub description: &'a str,
    pub parameters: &'a serde_json::Value,
    pub strict: bool,
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

        // WHY: Chat Completions has no separate thinking budget, so we drop it
        // and warn instead of inventing wire-format fields.
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

        if req.output_format.is_some() {
            tracing::debug!(
                "output_format set on request routed to OpenAI-compatible provider; \
                 passing through response_format"
            );
        }

        let mut messages: Vec<ChatMessage> = Vec::with_capacity(req.messages.len() + 1);

        // WHY: OpenAI expects `system` as the first message rather than a top-level field.
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
        let output_format = req.output_format.as_ref().map(translate_output_format);

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
            output_format,
        })
    }
}

impl<'a> ResponsesRequest<'a> {
    /// Build an OpenAI Responses request from a hermeneus [`CompletionRequest`].
    ///
    /// # Errors
    ///
    /// Returns an error when the request carries provider-side server tools
    /// whose OpenAI Responses equivalents are not represented in
    /// [`ServerToolDefinition`](crate::types::ServerToolDefinition) yet.
    pub(crate) fn from_request(req: &'a CompletionRequest, stream: Option<bool>) -> Result<Self> {
        if !req.server_tools.is_empty() {
            return Err(error::ProviderInitSnafu {
                message: "OpenAI Responses providers do not yet support hermeneus \
                          server-side tool definitions. Route these requests to an \
                          Anthropic provider, or remove the server_tools from the \
                          request."
                    .to_owned(),
            }
            .build());
        }

        if let Some(thinking) = &req.thinking
            && thinking.enabled
        {
            tracing::warn!(
                budget_tokens = thinking.budget_tokens,
                "thinking budget set on request routed to OpenAI Responses provider; \
                 dropping until the Responses reasoning knob is represented in hermeneus"
            );
        }

        if req.cache_system || req.cache_tools || req.cache_turns {
            tracing::warn!(
                cache_system = req.cache_system,
                cache_tools = req.cache_tools,
                cache_turns = req.cache_turns,
                "prompt-cache flags set on request routed to OpenAI Responses provider; \
                 dropping (no hermeneus mapping for Responses prompt cache controls)"
            );
        }

        if req.citations.is_some() {
            tracing::debug!(
                "citations config set on request routed to OpenAI Responses provider; \
                 ignored until Responses annotations are mapped"
            );
        }

        if req.output_format.is_some() {
            tracing::warn!(
                "output_format set on request routed to OpenAI Responses provider; \
                 dropping (Responses text.format not yet mapped in hermeneus)"
            );
        }

        let mut instructions = Vec::new();
        if let Some(system) = req.system.as_deref()
            && !system.is_empty()
        {
            instructions.push(system.to_owned());
        }

        let mut input = Vec::with_capacity(req.messages.len());
        for msg in &req.messages {
            translate_responses_message(msg, &mut instructions, &mut input);
        }

        let tools = req
            .tools
            .iter()
            .map(|tool| ResponsesTool {
                tool_type: "function",
                name: &tool.name,
                description: &tool.description,
                parameters: &tool.input_schema,
                strict: false,
            })
            .collect();

        let user = req.metadata.as_ref().and_then(|m| m.user_id.as_deref());

        Ok(Self {
            model: &req.model,
            instructions: (!instructions.is_empty()).then(|| instructions.join("\n")),
            input,
            max_output_tokens: req.max_tokens,
            temperature: req.temperature,
            tools,
            tool_choice: req
                .tool_choice
                .as_ref()
                .map(translate_responses_tool_choice),
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
fn translate_message(msg: &Message, out: &mut Vec<ChatMessage>) {
    match msg.role {
        Role::System => {
            out.push(ChatMessage {
                role: "system",
                content: Some(msg.content.text()),
                tool_calls: Vec::new(),
                tool_call_id: None,
            });
        }
        Role::User => match &msg.content {
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
                        ContentBlock::Text { text, .. } if !text.is_empty() => {
                            text_parts.push(text.clone());
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
        },
        Role::Assistant => {
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<ChatToolCall> = Vec::new();
            match &msg.content {
                Content::Text(text) => {
                    text_parts.push(text.clone());
                }
                Content::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text, .. } if !text.is_empty() => {
                                text_parts.push(text.clone());
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
            // WHY: OpenAI rejects assistant messages where both `content` and
            // `tool_calls` are empty, so synthesize an empty string to keep the
            // payload valid.
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
            // WHY: OpenAI tool-role content must be text; summarize non-text blocks.
            content.text_summary()
        }
    }
}

fn translate_responses_message(
    msg: &Message,
    instructions: &mut Vec<String>,
    out: &mut Vec<ResponsesInputItem>,
) {
    match msg.role {
        Role::System => {
            let text = msg.content.text();
            if !text.is_empty() {
                instructions.push(text);
            }
        }
        Role::User => translate_responses_user_message(msg, out),
        Role::Assistant => translate_responses_assistant_message(msg, out),
    }
}

fn translate_responses_user_message(msg: &Message, out: &mut Vec<ResponsesInputItem>) {
    match &msg.content {
        Content::Text(text) => {
            out.push(ResponsesInputItem::Message {
                role: "user",
                content: text.clone(),
            });
        }
        Content::Blocks(blocks) => {
            let mut text_parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text, .. } if !text.is_empty() => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => out.push(ResponsesInputItem::FunctionCallOutput {
                        item_type: "function_call_output",
                        call_id: tool_use_id.clone(),
                        output: tool_result_to_string(content),
                    }),
                    _ => {
                        // NOTE: Images/documents have no Responses input mapping yet.
                    }
                }
            }
            if !text_parts.is_empty() {
                out.push(ResponsesInputItem::Message {
                    role: "user",
                    content: text_parts.join("\n"),
                });
            }
        }
    }
}

fn translate_responses_assistant_message(msg: &Message, out: &mut Vec<ResponsesInputItem>) {
    let mut text_parts = Vec::new();
    match &msg.content {
        Content::Text(text) => text_parts.push(text.clone()),
        Content::Blocks(blocks) => {
            for block in blocks {
                match block {
                    ContentBlock::Text { text, .. } if !text.is_empty() => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let arguments =
                            serde_json::to_string(input).unwrap_or_else(|_| "{}".to_owned());
                        out.push(ResponsesInputItem::FunctionCall {
                            item_type: "function_call",
                            call_id: id.clone(),
                            name: name.clone(),
                            arguments,
                        });
                    }
                    _ => {
                        // NOTE: Tool results and non-text media are not assistant output items.
                    }
                }
            }
        }
    }
    if !text_parts.is_empty() {
        out.push(ResponsesInputItem::Message {
            role: "assistant",
            content: text_parts.join("\n"),
        });
    }
}

fn translate_output_format(format: &OutputFormat) -> WireResponseFormat {
    match format {
        OutputFormat::JsonSchema {
            name,
            schema,
            strict,
        } => WireResponseFormat::JsonSchema {
            definition: WireJsonSchema {
                name: name.clone(),
                schema: schema.clone(),
                strict: *strict,
            },
        },
        OutputFormat::Text => WireResponseFormat::Text,
    }
}

fn translate_tool_choice(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::Value::String("auto".to_owned()),
        // NOTE: OpenAI uses "required" to force any tool; Anthropic calls this "any".
        ToolChoice::Any => serde_json::Value::String("required".to_owned()),
        ToolChoice::Tool { name } => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

fn translate_responses_tool_choice(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::Value::String("auto".to_owned()),
        ToolChoice::Any => serde_json::Value::String("required".to_owned()),
        ToolChoice::Tool { name } => serde_json::json!({
            "type": "function",
            "name": name
        }),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
#[path = "request_tests.rs"]
mod tests;
