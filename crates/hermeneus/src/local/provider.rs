//! `LocalProvider`: routes LLM calls to a local vLLM endpoint via the
//! OpenAI-compatible Chat Completions API.
//!
//! # Errors
//!
//! Connection failures (server not running) produce [`Error::ApiRequest`] with
//! a descriptive message rather than panicking.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Client;
use tracing::{debug, warn};

use super::stream::parse_openai_stream;
use super::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatFunction, ChatFunctionCall, ChatMessage,
    ChatTool, ChatToolCall,
};
use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::provider::LlmProvider;
use crate::types::{
    CompletionRequest, CompletionResponse, Content, ContentBlock, StopReason, ToolChoice, Usage,
};

/// Default base URL for local vLLM instances.
const DEFAULT_BASE_URL: &str = "http://localhost:8000/v1";

/// Model name prefix that routes requests to this provider.
pub(crate) const LOCAL_MODEL_PREFIX: &str = "local/";

/// Configuration for the local provider.
#[derive(Debug, Clone)]
pub struct LocalProviderConfig {
    /// Base URL of the OpenAI-compatible endpoint (e.g. `http://localhost:8000/v1`).
    pub base_url: String,
    /// Default model to request when not specified.
    pub default_model: String,
    /// HTTP request timeout.
    pub timeout: Duration,
}

impl Default for LocalProviderConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            default_model: "qwen3.5-27b".to_owned(),
            timeout: Duration::from_secs(120),
        }
    }
}

/// OpenAI-compatible LLM provider for local inference (vLLM, etc.).
pub struct LocalProvider {
    // kanon:ignore RUST/pub-visibility
    client: Client,
    base_url: String,
    default_model: String,
}

impl LocalProvider {
    /// Create a new `LocalProvider` from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ProviderInit`] if the HTTP client cannot be constructed.
    #[must_use]
    pub fn new(config: &LocalProviderConfig) -> Result<Self> {
        // kanon:ignore RUST/pub-visibility
        // WHY: reqwest 0.13 with rustls-no-provider requires an explicit crypto provider.
        // install_default() is idempotent: subsequent calls return Err and are ignored.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                error::ProviderInitSnafu {
                    message: format!("failed to build HTTP client: {e}"),
                }
                .build()
            })?;

        let base_url = config.base_url.trim_end_matches('/').to_owned();

        debug!(
            base_url = %base_url,
            default_model = %config.default_model,
            "local provider initialized"
        );

        Ok(Self {
            client,
            base_url,
            default_model: config.default_model.clone(),
        })
    }

    /// Resolve the model name: strip the `local/` prefix and fall back to default.
    fn resolve_model<'a>(&'a self, model: &'a str) -> &'a str {
        let stripped = model.strip_prefix(LOCAL_MODEL_PREFIX).unwrap_or(model);
        if stripped.is_empty() {
            &self.default_model
        } else {
            stripped
        }
    }

    /// Map an Aletheia `CompletionRequest` to a `ChatCompletionRequest`.
    fn build_request<'a>(
        &'a self,
        request: &'a CompletionRequest,
        stream: bool,
    ) -> ChatCompletionRequest<'a> {
        let model = self.resolve_model(&request.model);
        let messages = map_messages(request);

        // Tool definitions.
        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| ChatTool {
                        tool_type: "function",
                        function: ChatFunction {
                            name: &t.name,
                            description: &t.description,
                            parameters: &t.input_schema,
                        },
                    })
                    .collect(),
            )
        };

        let tool_choice = request.tool_choice.as_ref().map(|tc| match tc {
            ToolChoice::Any => "required",
            // NOTE: OpenAI's specific-tool format differs from Anthropic's; fall back to auto.
            ToolChoice::Auto | ToolChoice::Tool { .. } => "auto",
        });

        let stop = if request.stop_sequences.is_empty() {
            None
        } else {
            Some(request.stop_sequences.iter().map(String::as_str).collect())
        };

        ChatCompletionRequest {
            model,
            messages,
            max_tokens: Some(request.max_tokens),
            temperature: request.temperature,
            stop,
            tools,
            tool_choice,
            stream,
        }
    }

    /// Execute a non-streaming completion.
    async fn execute(&self, request: &CompletionRequest) -> Result<CompletionResponse> {
        let chat_request = self.build_request(request, false);
        let url = format!("{}/chat/completions", self.base_url);

        let http_response = self
            .client
            .post(&url)
            .json(&chat_request)
            .send()
            .await
            .map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!(
                        "failed to connect to local provider at {url} \u{2014} \
                         is the server running? ({e})"
                    ),
                }
                .build()
            })?;

        let status = http_response.status();
        if !status.is_success() {
            return Err(map_http_error(status, http_response).await);
        }

        let chat_response: ChatCompletionResponse = http_response.json().await.map_err(|e| {
            error::ApiRequestSnafu {
                message: format!("failed to parse local provider response: {e}"),
            }
            .build()
        })?;

        Ok(map_response(chat_response))
    }

    /// Execute a streaming completion.
    async fn execute_streaming(
        &self,
        request: &CompletionRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CompletionResponse> {
        let chat_request = self.build_request(request, true);
        let url = format!("{}/chat/completions", self.base_url);

        let mut http_response = self
            .client
            .post(&url)
            .json(&chat_request)
            .send()
            .await
            .map_err(|e| {
                error::ApiRequestSnafu {
                    message: format!(
                        "failed to connect to local provider at {url} \u{2014} \
                         is the server running? ({e})"
                    ),
                }
                .build()
            })?;

        let status = http_response.status();
        if !status.is_success() {
            return Err(map_http_error(status, http_response).await);
        }

        let (response, _has_content) = parse_openai_stream(&mut http_response, on_event).await?;
        Ok(response)
    }
}

/// Map Aletheia messages to chat messages.
///
/// Handles the translation from Anthropic's block-based content model to
/// the flat message model:
/// - System prompts become `role: "system"` messages
/// - `Content::Text` maps directly
/// - `Content::Blocks` are decomposed: text blocks are joined, tool results
///   become `role: "tool"` messages, and tool use blocks become `tool_calls`
fn map_messages(request: &CompletionRequest) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    // System prompt as a system message.
    if let Some(system) = &request.system {
        messages.push(ChatMessage {
            role: "system".to_owned(),
            content: Some(system.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &request.messages {
        match &msg.content {
            Content::Text(text) => {
                messages.push(ChatMessage {
                    role: msg.role.as_str().to_owned(),
                    content: Some(text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            Content::Blocks(blocks) => {
                map_block_message(&mut messages, msg.role.as_str(), blocks);
            }
        }
    }

    messages
}

/// Map a block-based message into one or more chat messages.
fn map_block_message(messages: &mut Vec<ChatMessage>, role: &str, blocks: &[ContentBlock]) {
    let mut text_parts: Vec<&str> = Vec::new();
    let mut tool_calls: Vec<ChatToolCall> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text, .. } => {
                text_parts.push(text);
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ChatToolCall {
                    id: id.clone(),
                    call_type: "function".to_owned(),
                    function: ChatFunctionCall {
                        name: name.clone(),
                        arguments: input.to_string(),
                    },
                });
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                // Flush accumulated text before emitting tool result.
                if !text_parts.is_empty() {
                    messages.push(ChatMessage {
                        role: role.to_owned(),
                        content: Some(text_parts.join("\n")),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    text_parts.clear();
                }
                messages.push(ChatMessage {
                    role: "tool".to_owned(),
                    content: Some(content.text_summary()),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id.clone()),
                });
            }
            // Thinking, server tool use, web search results, code execution
            // are Anthropic-specific and have no equivalent in this wire format.
            ContentBlock::Thinking { .. }
            | ContentBlock::ServerToolUse { .. }
            | ContentBlock::WebSearchToolResult { .. }
            | ContentBlock::CodeExecutionResult { .. } => {}
        }
    }

    // Flush remaining text and/or tool calls.
    if !text_parts.is_empty() || !tool_calls.is_empty() {
        messages.push(ChatMessage {
            role: role.to_owned(),
            content: if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        });
    }
}

/// Map an HTTP error response to a hermeneus error.
async fn map_http_error(status: reqwest::StatusCode, response: reqwest::Response) -> error::Error {
    let body = response.text().await.unwrap_or_default();
    if status.as_u16() == 429 {
        return error::RateLimitedSnafu {
            retry_after_ms: 1000_u64,
        }
        .build();
    }
    error::ApiRequestSnafu {
        message: format!("local provider returned {status}: {body}"),
    }
    .build()
}

/// Map a `finish_reason` string to a [`StopReason`].
fn map_finish_reason(reason: Option<&str>) -> StopReason {
    match reason {
        Some("tool_calls") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("stop" | _) | None => StopReason::EndTurn,
    }
}

/// Map a chat completion response to Aletheia's canonical response type.
fn map_response(response: ChatCompletionResponse) -> CompletionResponse {
    let choice = response.choices.into_iter().next();
    let (content, stop_reason) = match choice {
        Some(c) => {
            let mut blocks = Vec::new();

            if let Some(text) = c.message.content
                && !text.is_empty()
            {
                blocks.push(ContentBlock::Text {
                    text,
                    citations: None,
                });
            }

            if let Some(tool_calls) = c.message.tool_calls {
                for tc in tool_calls {
                    let input = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|e| {
                        warn!(
                            error = %e,
                            arguments = %tc.function.arguments,
                            "failed to parse tool call arguments, using empty object"
                        );
                        serde_json::Value::Object(serde_json::Map::new())
                    });
                    blocks.push(ContentBlock::ToolUse {
                        id: tc.id,
                        name: tc.function.name,
                        input,
                    });
                }
            }

            let reason = map_finish_reason(c.finish_reason.as_deref());
            (blocks, reason)
        }
        None => (Vec::new(), StopReason::EndTurn),
    };

    let usage = response.usage.map_or(Usage::default(), |u| Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        ..Usage::default()
    });

    CompletionResponse {
        id: response.id,
        model: response.model,
        stop_reason,
        content,
        usage,
    }
}

impl std::fmt::Debug for LocalProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalProvider")
            .field("base_url", &self.base_url)
            .field("default_model", &self.default_model)
            .finish_non_exhaustive()
    }
}

impl LlmProvider for LocalProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute(request))
    }

    fn supported_models(&self) -> &[&str] {
        // WHY: LocalProvider matches any model with the "local/" prefix via
        // `supports_model` override. Returning an empty slice here is correct
        // because the set of local models is dynamic (whatever vLLM serves).
        &[]
    }

    fn supports_model(&self, model: &str) -> bool {
        model.starts_with(LOCAL_MODEL_PREFIX)
    }

    fn name(&self) -> &'static str {
        "local"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn complete_streaming<'a>(
        &'a self,
        request: &'a CompletionRequest,
        on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(self.execute_streaming(request, on_event))
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: index bounds verified by preceding assertions"
)]
#[path = "provider_tests.rs"]
mod tests;
