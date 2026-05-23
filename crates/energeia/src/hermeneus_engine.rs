//! Hermeneus-based dispatch engine with prompt caching support.
//!
//! [`HermeneusEngine`] implements [`DispatchEngine`] by sending completion
//! requests through a hermeneus [`LlmProvider`]. When the [`SessionSpec`]
//! carries [`PromptComponents`](crate::prompt_cache::PromptComponents), the
//! static prefix is placed in the system prompt with
//! `cache_system: true`, enabling Anthropic prompt cache hits.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use hermeneus::provider::{LlmProvider, ModelPricing, ProviderConfig};
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
    pricing: HashMap<String, ModelPricing>,
}

impl HermeneusEngine {
    /// Create a new engine wrapping the given provider.
    #[must_use]
    pub fn new(provider: Arc<dyn LlmProvider>, default_model: impl Into<String>) -> Self {
        Self {
            provider,
            default_model: default_model.into(),
            pricing: ProviderConfig::default().pricing,
        }
    }

    /// Create a new engine with per-model pricing used when provider responses
    /// include token usage but omit precomputed cost metadata.
    #[must_use]
    pub fn with_pricing(
        provider: Arc<dyn LlmProvider>,
        default_model: impl Into<String>,
        pricing: HashMap<String, ModelPricing>,
    ) -> Self {
        Self {
            provider,
            default_model: default_model.into(),
            pricing,
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

    /// Compute cost from usage metadata when the provider did not fill it.
    fn cost_from_response(
        &self,
        response: &hermeneus::types::CompletionResponse,
        model: &str,
    ) -> f64 {
        response
            .cost_usd
            .unwrap_or_else(|| estimate_usage_cost(&self.pricing, model, &response.usage))
    }
}

fn pricing_for_model<'a>(
    pricing: &'a HashMap<String, ModelPricing>,
    model: &str,
) -> Option<&'a ModelPricing> {
    pricing.get(model).or_else(|| {
        pricing
            .iter()
            .find(|(key, _)| {
                model.len() > key.len()
                    && model.starts_with(key.as_str())
                    && model.as_bytes().get(key.len()) == Some(&b'-')
            })
            .map(|(_, pricing)| pricing)
    })
}

#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "u64 token counts to f64 are acceptable for cost estimates"
)]
fn estimate_usage_cost(
    pricing: &HashMap<String, ModelPricing>,
    model: &str,
    usage: &hermeneus::types::Usage,
) -> f64 {
    const CACHE_READ_DISCOUNT: f64 = 0.1;
    const CACHE_WRITE_PREMIUM: f64 = 1.25;

    let Some(pricing) = pricing_for_model(pricing, model) else {
        return 0.0;
    };

    ((usage.input_tokens as f64 // kanon:ignore RUST/as-cast
        + usage.cache_read_tokens as f64 * CACHE_READ_DISCOUNT // kanon:ignore RUST/as-cast
        + usage.cache_write_tokens as f64 * CACHE_WRITE_PREMIUM) // kanon:ignore RUST/as-cast
        * pricing.input_cost_per_mtok
        + usage.output_tokens as f64 * pricing.output_cost_per_mtok) // kanon:ignore RUST/as-cast
        / 1_000_000.0
}

impl DispatchEngine for HermeneusEngine {
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let request = self.build_request(spec, options);
            let started = Instant::now();
            let response = self.provider.complete(&request).await.map_err(|e| {
                error::EngineSnafu {
                    detail: format!("hermeneus completion failed: {e}"),
                }
                .build()
            })?;
            let duration_ms = response.duration_ms.unwrap_or_else(|| {
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
            });

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
                cost_usd: self.cost_from_response(&response, &request.model),
                num_turns: 1,
                duration_ms,
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

            let started = Instant::now();
            let response = self.provider.complete(&request).await.map_err(|e| {
                error::EngineSnafu {
                    detail: format!("hermeneus resume failed: {e}"),
                }
                .build()
            })?;
            let duration_ms = response.duration_ms.unwrap_or_else(|| {
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
            });

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
                cost_usd: self.cost_from_response(&response, &request.model),
                num_turns: 1,
                duration_ms,
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
    use hermeneus::types::Usage;

    use crate::prompt_cache::PromptComponents;

    use super::*;

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

    #[tokio::test]
    async fn session_result_uses_hermeneus_cost_and_duration_metadata() -> Result<()> {
        let mut response = hermeneus::test_utils::make_response("done");
        response.cost_usd = Some(0.123);
        response.duration_ms = Some(456);
        let provider = Arc::new(hermeneus::test_utils::MockProvider::with_responses(vec![
            response,
        ]));
        let engine = HermeneusEngine::new(provider, "claude-sonnet-4");
        let spec = SessionSpec {
            prompt: "do it".to_owned(),
            system_prompt: None,
            cwd: None,
            prompt_components: None,
        };

        let handle = engine.spawn_session(&spec, &AgentOptions::new()).await?;
        let result = handle.wait().await?;

        assert!((result.cost_usd - 0.123).abs() < f64::EPSILON);
        assert_eq!(result.duration_ms, 456);
        assert_eq!(result.num_turns, 1);
        Ok(())
    }

    #[tokio::test]
    async fn session_result_estimates_cost_from_usage_when_provider_omits_cost() -> Result<()> {
        let mut response = hermeneus::test_utils::make_response("done");
        response.cost_usd = None;
        response.usage = Usage {
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 100,
            cache_write_tokens: 40,
        };
        let provider = Arc::new(hermeneus::test_utils::MockProvider::with_responses(vec![
            response,
        ]));
        let pricing = HashMap::from([(
            "gpt-test".to_owned(),
            ModelPricing {
                input_cost_per_mtok: 2.0,
                output_cost_per_mtok: 8.0,
            },
        )]);
        let engine = HermeneusEngine::with_pricing(provider, "gpt-test-2026-05-23", pricing);
        let spec = SessionSpec {
            prompt: "do it".to_owned(),
            system_prompt: None,
            cwd: None,
            prompt_components: None,
        };

        let handle = engine.spawn_session(&spec, &AgentOptions::new()).await?;
        let result = handle.wait().await?;

        let expected = ((1_000.0 + 100.0 * 0.1 + 40.0 * 1.25) * 2.0 + 500.0 * 8.0) / 1_000_000.0;
        assert!((result.cost_usd - expected).abs() < f64::EPSILON);
        Ok(())
    }

    #[tokio::test]
    async fn provider_supplied_cost_takes_precedence_over_usage_estimate() -> Result<()> {
        let mut response = hermeneus::test_utils::make_response("done");
        response.cost_usd = Some(0.321);
        response.usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let provider = Arc::new(hermeneus::test_utils::MockProvider::with_responses(vec![
            response,
        ]));
        let pricing = HashMap::from([(
            "gpt-test".to_owned(),
            ModelPricing {
                input_cost_per_mtok: 99.0,
                output_cost_per_mtok: 99.0,
            },
        )]);
        let engine = HermeneusEngine::with_pricing(provider, "gpt-test", pricing);
        let spec = SessionSpec {
            prompt: "do it".to_owned(),
            system_prompt: None,
            cwd: None,
            prompt_components: None,
        };

        let handle = engine.spawn_session(&spec, &AgentOptions::new()).await?;
        let result = handle.wait().await?;

        assert!((result.cost_usd - 0.321).abs() < f64::EPSILON);
        Ok(())
    }
}
