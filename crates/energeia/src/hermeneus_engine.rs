//! Hermeneus-based dispatch engine with prompt caching support.
//!
//! [`HermeneusEngine`] implements [`DispatchEngine`] by sending completion
//! requests through a hermeneus [`LlmProvider`]. When the [`SessionSpec`]
//! carries [`PromptComponents`](crate::prompt_cache::PromptComponents), the
//! static prefix is placed in the system prompt with
//! `cache_system: true`, enabling Anthropic prompt cache hits.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hermeneus::provider::LlmProvider;
use hermeneus::types::{CompletionRequest, Content, Message, Role, StopReason};

use crate::engine::{
    AgentOptions, DispatchEngine, SessionEvent, SessionHandle, SessionResult, SessionSpec,
};
use crate::error::{self, Result};

// ---------------------------------------------------------------------------
// HermeneusEngine
// ---------------------------------------------------------------------------

/// Dispatch engine backed by hermeneus [`LlmProvider`].
///
/// Supports prompt caching via the [`PromptComponents`](crate::prompt_cache::PromptComponents)
/// split: the static prefix is sent as a cached system prompt and the dynamic
/// suffix as the user message.
pub struct HermeneusEngine {
    provider: Arc<dyn LlmProvider>,
    default_model: String,
}

impl HermeneusEngine {
    /// Create a new engine wrapping the given provider.
    #[must_use]
    pub fn new(provider: Arc<dyn LlmProvider>, default_model: impl Into<String>) -> Self {
        Self {
            provider,
            default_model: default_model.into(),
        }
    }

    /// Build a [`CompletionRequest`] from a [`SessionSpec`] and [`AgentOptions`].
    fn build_request(&self, spec: &SessionSpec, options: &AgentOptions) -> CompletionRequest {
        let system = spec.system_prompt.clone().or_else(|| {
            spec.prompt_components
                .as_ref()
                .map(|c| c.static_prefix.clone())
        });

        let prompt_text = if let Some(ref components) = spec.prompt_components {
            components.dynamic_suffix.clone()
        } else {
            spec.prompt.clone()
        };

        let cache_system = system.is_some()
            && spec
                .prompt_components
                .as_ref()
                .is_some_and(|c| !c.static_prefix.is_empty());

        CompletionRequest {
            model: options
                .model
                .clone()
                .unwrap_or_else(|| self.default_model.clone()),
            system,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text(prompt_text),
                cache_breakpoint: false,
            }],
            max_tokens: options.max_turns.unwrap_or(4096),
            cache_system,
            ..CompletionRequest::default()
        }
    }
}

impl DispatchEngine for HermeneusEngine {
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.build_request(spec, options);
            let response = self.provider.complete(&request).await.map_err(|e| {
                error::EngineSnafu {
                    detail: format!("hermeneus completion failed: {e}"),
                }
                .build()
            })?;

            let session_id = format!("hermeneus-{}", koina::ulid::Ulid::new());
            let text = response
                .content
                .iter()
                .filter_map(|block| match block {
                    hermeneus::types::ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");

            let success = matches!(
                response.stop_reason,
                StopReason::EndTurn | StopReason::StopSequence
            );

            let result = SessionResult {
                session_id: session_id.clone(),
                cost_usd: 0.0, // hermeneus responses don't include cost directly
                num_turns: 1,
                duration_ms: 0,
                success,
                result_text: Some(text.clone()),
                model: Some(request.model),
                cache_hit_tokens: response.usage.cache_read_tokens,
                cache_miss_tokens: response.usage.cache_write_tokens,
            };

            let handle = HermeneusSessionHandle {
                session_id,
                text: Some(text),
                result: Some(result),
            };

            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }

    fn resume_session<'a>(
        &'a self,
        _session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        // WHY: Resume is modelled as a new completion with the resume message
        // as the user prompt. No conversation history is retained.
        Box::pin(async move {
            let request = CompletionRequest {
                model: options
                    .model
                    .clone()
                    .unwrap_or_else(|| self.default_model.clone()),
                messages: vec![Message {
                    role: Role::User,
                    content: Content::Text(prompt.to_owned()),
                    cache_breakpoint: false,
                }],
                max_tokens: options.max_turns.unwrap_or(4096),
                ..CompletionRequest::default()
            };

            let response = self.provider.complete(&request).await.map_err(|e| {
                error::EngineSnafu {
                    detail: format!("hermeneus resume failed: {e}"),
                }
                .build()
            })?;

            let session_id = format!("hermeneus-resume-{}", koina::ulid::Ulid::new());
            let text = response
                .content
                .iter()
                .filter_map(|block| match block {
                    hermeneus::types::ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");

            let success = matches!(
                response.stop_reason,
                StopReason::EndTurn | StopReason::StopSequence
            );

            let result = SessionResult {
                session_id: session_id.clone(),
                cost_usd: 0.0,
                num_turns: 1,
                duration_ms: 0,
                success,
                result_text: Some(text.clone()),
                model: Some(request.model),
                cache_hit_tokens: response.usage.cache_read_tokens,
                cache_miss_tokens: response.usage.cache_write_tokens,
            };

            let handle = HermeneusSessionHandle {
                session_id,
                text: Some(text),
                result: Some(result),
            };

            let boxed: Box<dyn SessionHandle> = Box::new(handle);
            Ok(boxed)
        })
    }
}

// ---------------------------------------------------------------------------
// HermeneusSessionHandle
// ---------------------------------------------------------------------------

/// Session handle for a hermeneus-backed completion.
///
/// Yields a single [`SessionEvent::TextDelta`] with the response text,
/// then returns `None`. `wait()` returns the pre-built [`SessionResult`].
struct HermeneusSessionHandle {
    session_id: String,
    text: Option<String>,
    result: Option<SessionResult>,
}

impl SessionHandle for HermeneusSessionHandle {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>> {
        Box::pin(async move {
            self.text
                .take()
                .map(|t| SessionEvent::TextDelta { text: t })
        })
    }

    fn wait(mut self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>> {
        Box::pin(async move {
            self.result.take().ok_or_else(|| {
                error::EngineSnafu {
                    detail: "HermeneusSessionHandle: wait() called more than once",
                }
                .build()
            })
        })
    }

    fn abort<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::prompt_cache::PromptComponents;

    #[test]
    fn build_request_with_components_enables_cache_system() {
        let provider = Arc::new(hermeneus::test_utils::MockProvider::new("done"));
        let engine = HermeneusEngine::new(provider, "claude-sonnet-4");

        let spec = SessionSpec {
            prompt: "dynamic".to_owned(),
            system_prompt: Some("static".to_owned()),
            cwd: None,
            prompt_components: Some(PromptComponents {
                static_prefix: "static".to_owned(),
                dynamic_suffix: "dynamic".to_owned(),
            }),
        };

        let request = engine.build_request(&spec, &AgentOptions::new());
        assert!(request.cache_system);
        assert_eq!(request.system.as_deref(), Some("static"));
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content.text(), "dynamic");
    }

    #[test]
    fn build_request_without_components_disables_cache_system() {
        let provider = Arc::new(hermeneus::test_utils::MockProvider::new("done"));
        let engine = HermeneusEngine::new(provider, "claude-sonnet-4");

        let spec = SessionSpec {
            prompt: "plain prompt".to_owned(),
            system_prompt: None,
            cwd: None,
            prompt_components: None,
        };

        let request = engine.build_request(&spec, &AgentOptions::new());
        assert!(!request.cache_system);
        assert_eq!(request.system, None);
    }

    #[test]
    fn build_request_empty_static_prefix_disables_cache_system() {
        let provider = Arc::new(hermeneus::test_utils::MockProvider::new("done"));
        let engine = HermeneusEngine::new(provider, "claude-sonnet-4");

        let spec = SessionSpec {
            prompt: "dynamic".to_owned(),
            system_prompt: Some(String::new()),
            cwd: None,
            prompt_components: Some(PromptComponents {
                static_prefix: String::new(),
                dynamic_suffix: "dynamic".to_owned(),
            }),
        };

        let request = engine.build_request(&spec, &AgentOptions::new());
        assert!(!request.cache_system);
    }
}
