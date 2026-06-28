//! Registry-backed model fallback for the execute stage.

use std::future::Future;
use std::pin::Pin;

use hermeneus::anthropic::StreamEvent;
use hermeneus::error as llm_error;
use hermeneus::fallback::{FallbackCompletion, FallbackConfig, complete_with_fallback_observed};
use hermeneus::health::ProviderHealth;
use hermeneus::provider::{LlmProvider, ProviderRegistry, ProviderResolutionError, ProviderRoute};
use hermeneus::types::{CompletionRequest, CompletionResponse};

struct RegistryFallbackProvider<'a> {
    providers: &'a ProviderRegistry,
}

impl LlmProvider for RegistryFallbackProvider<'_> {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = llm_error::Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(async move {
            let model = request.model.clone();
            let provider = resolve_provider_for_model(self.providers, &model)?;
            let provider_name = provider.name().to_owned();
            match provider.complete(request).await {
                Ok(resp) => {
                    self.providers.record_success(&provider_name);
                    Ok(resp)
                }
                Err(e) => {
                    self.providers.record_error(&provider_name, &e);
                    Err(e)
                }
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        &[]
    }

    fn name(&self) -> &'static str {
        "provider-registry"
    }
}

/// Execute a completion request with registry-backed model fallback.
///
/// Returns the successful [`CompletionResponse`] alongside the model identifier
/// that produced it. The model may differ from `request.model` when a fallback
/// model succeeded.
pub(super) async fn complete_with_registry_fallback(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    config: &FallbackConfig,
) -> llm_error::Result<FallbackCompletion> {
    let fallback_provider = RegistryFallbackProvider { providers };
    complete_with_fallback_observed(&fallback_provider, request, config).await
}

/// Execute a streaming completion request with registry-backed model fallback.
///
/// Fallback is only safe before any stream event has been delivered to the
/// caller. Once the callback fires, the consumer may have rendered partial SSE
/// state; a model switch after that point would risk duplicated or incoherent
/// output, so retryable errors are returned as terminal failures.
pub(super) async fn complete_streaming_with_registry_fallback(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    config: &FallbackConfig,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> llm_error::Result<FallbackCompletion> {
    let primary = &request.model;
    let mut last_error = None;
    let mut attempt_errors = Vec::new();

    for attempt in 0..config.retries_before_fallback.max(1) {
        if attempt > 0 {
            tracing::warn!(
                model = %primary,
                attempt,
                "retrying primary model streaming request"
            );
        }

        match complete_streaming_once(providers, request, on_event).await {
            (Ok(response), _) => {
                return Ok(FallbackCompletion {
                    model: primary.clone(),
                    response,
                });
            }
            (Err(e), emitted_stream_event) => {
                if emitted_stream_event || !e.is_retryable() {
                    return Err(e);
                }
                tracing::warn!(
                    model = %primary,
                    attempt,
                    error = %e,
                    "primary streaming model failed with retryable error before stream output"
                );
                attempt_errors.push(format!("{primary}: {e}"));
                last_error = Some(e);
            }
        }
    }

    for fallback_model in &config.fallback_models {
        let mut fallback_req = request.clone();
        fallback_req.model = fallback_model.clone();

        for fallback_attempt in 0..config.retries_before_fallback.max(1) {
            if fallback_attempt == 0 {
                tracing::warn!(
                    primary = %primary,
                    fallback = %fallback_model,
                    reason = %last_error.as_ref().map_or("unknown", |_| "retryable error on previous streaming model"),
                    "falling back to alternative streaming model"
                );
            } else {
                tracing::warn!(
                    model = %fallback_model,
                    attempt = fallback_attempt,
                    "retrying fallback streaming model"
                );
            }

            match complete_streaming_once(providers, &fallback_req, on_event).await {
                (Ok(response), _) => {
                    return Ok(FallbackCompletion {
                        model: fallback_model.clone(),
                        response,
                    });
                }
                (Err(e), emitted_stream_event) => {
                    if emitted_stream_event || !e.is_retryable() {
                        return Err(e);
                    }
                    tracing::warn!(
                        model = %fallback_model,
                        attempt = fallback_attempt,
                        error = %e,
                        "fallback streaming model failed with retryable error before stream output"
                    );
                    attempt_errors.push(format!("{fallback_model}: {e}"));
                    last_error = Some(e);
                }
            }
        }
    }

    if !attempt_errors.is_empty() && !config.fallback_models.is_empty() {
        return Err(llm_error::ApiRequestSnafu {
            message: format!(
                "connection unavailable: all models in fallback chain failed: {}",
                attempt_errors.join("; ")
            ),
        }
        .build());
    }

    Err(last_error.unwrap_or_else(|| {
        llm_error::ApiRequestSnafu {
            message: "all models in fallback chain failed".to_owned(),
        }
        .build()
    }))
}

async fn complete_streaming_once(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> (llm_error::Result<CompletionResponse>, bool) {
    let model = request.model.clone();
    let provider = match resolve_provider_for_model(providers, &model) {
        Ok(provider) => provider,
        Err(e) => return (Err(e), false),
    };
    let provider_name = provider.name().to_owned();
    let mut emitted_stream_event = false;

    let result = {
        let mut guarded_on_event = |event: StreamEvent| {
            emitted_stream_event = true;
            on_event(event);
        };
        provider
            .complete_streaming(request, &mut guarded_on_event)
            .await
    };

    match &result {
        Ok(_) => providers.record_success(&provider_name),
        Err(e) => providers.record_error(&provider_name, e),
    }

    (result, emitted_stream_event)
}

fn resolve_provider_for_model<'a>(
    providers: &'a ProviderRegistry,
    model: &str,
) -> llm_error::Result<&'a dyn LlmProvider> {
    let provider = match providers.resolve_provider(model, ProviderRoute::ModelOnly) {
        Ok(provider) => provider,
        Err(ProviderResolutionError::NoProvider { .. }) => {
            return Err(llm_error::UnsupportedModelSnafu {
                model: model.to_owned(),
            }
            .build());
        }
        Err(ProviderResolutionError::ProviderUnavailable { name, health }) => {
            return Err(llm_error::ApiRequestSnafu {
                message: format!("provider '{name}' is currently unavailable: {health:?}"),
            }
            .build());
        }
    };

    if let Some(health) = providers.provider_health(provider.name())
        && matches!(health, ProviderHealth::Down { .. })
    {
        return Err(llm_error::ApiRequestSnafu {
            message: format!("provider '{}' is currently unavailable", provider.name()),
        }
        .build());
    }

    Ok(provider)
}
