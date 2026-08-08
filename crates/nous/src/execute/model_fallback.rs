//! Registry-backed model/provider fallback for the execute stage.

use hermeneus::anthropic::StreamEvent;
use hermeneus::error as llm_error;
use hermeneus::health::ProviderHealth;
use hermeneus::provider::{LlmProvider, ProviderRegistry, ProviderResolutionError};
use hermeneus::types::{CompletionRequest, CompletionResponse};
use koina::redact::redact_sensitive;

use crate::config::ModelProviderRoute;

/// Fallback chain configured for registry-backed execution.
pub(super) struct RegistryFallbackConfig {
    /// Ordered fallback model/provider routes.
    pub(super) fallback_routes: Vec<ModelProviderRoute>,
    /// How many times to call each route before moving to the next.
    pub(super) retries_before_fallback: u32,
}

/// Successful registry-backed completion with observed model and provider.
pub(super) struct RegistryFallbackCompletion {
    /// Provider response.
    pub(super) response: CompletionResponse,
    /// Request model that completed successfully.
    pub(super) model: String,
    /// Provider instance that served the successful request.
    pub(super) provider: String,
    /// Real provider-call attempts consumed to reach this success, including
    /// the successful call itself.
    ///
    /// WHY(#5372): counts only attempts that actually reached the provider's
    /// `complete`/`complete_streaming` method — a route/health resolution
    /// failure (e.g. an already-`Down` provider skipped without a network
    /// call) does not increment this, matching what `TurnUsage::llm_calls`
    /// is meant to represent: real LLM calls, not resolution attempts.
    pub(super) attempts: u32,
}

/// Outcome of one raw provider call attempt.
struct RawAttempt {
    result: llm_error::Result<(CompletionResponse, String)>,
    /// True once the provider's completion method was actually invoked.
    /// False when route/health resolution failed before any request left
    /// the process — see [`RegistryFallbackCompletion::attempts`].
    attempted: bool,
}

/// Outcome of one raw streaming provider call attempt.
struct RawStreamingAttempt {
    result: llm_error::Result<(CompletionResponse, String)>,
    attempted: bool,
    /// True once at least one stream chunk reached the caller, past which a
    /// route switch would risk duplicated or incoherent output.
    emitted_stream_event: bool,
}

/// Execute a completion request with registry-backed model/provider fallback.
pub(super) async fn complete_with_registry_fallback(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    primary_route: &ModelProviderRoute,
    config: &RegistryFallbackConfig,
    nous_id: &str,
) -> llm_error::Result<RegistryFallbackCompletion> {
    let primary_label = route_label(primary_route);
    let mut last_error = None;
    let mut attempt_errors = Vec::new();
    let mut attempts: u32 = 0;

    for attempt in 0..config.retries_before_fallback.max(1) {
        if attempt > 0 {
            tracing::warn!(
                model = %primary_route.model,
                provider = primary_route.provider.as_deref().unwrap_or("model-only"),
                attempt,
                "retrying primary model route"
            );
            crate::metrics::record_llm_fallback_attempt(nous_id, "primary_retry");
        }

        let routed_request = request_for_route(request, primary_route);
        let raw = complete_once(providers, primary_route, &routed_request).await;
        if raw.attempted {
            attempts += 1;
        }
        match raw.result {
            Ok((response, provider)) => {
                return Ok(RegistryFallbackCompletion {
                    response,
                    model: primary_route.model.clone(),
                    provider,
                    attempts,
                });
            }
            Err(e) => {
                if !e.is_retryable() {
                    return Err(e);
                }
                tracing::warn!(
                    model = %primary_route.model,
                    provider = primary_route.provider.as_deref().unwrap_or("model-only"),
                    attempt,
                    error = %redact_sensitive(&e.to_string()),
                    "primary model route failed with retryable error"
                );
                attempt_errors.push(format!("{primary_label}: {e}"));
                last_error = Some(e);
            }
        }
    }

    for fallback_route in &config.fallback_routes {
        let fallback_label = route_label(fallback_route);
        let routed_request = request_for_route(request, fallback_route);

        for fallback_attempt in 0..config.retries_before_fallback.max(1) {
            if fallback_attempt == 0 {
                tracing::warn!(
                    primary = %primary_label,
                    fallback = %fallback_label,
                    reason = %last_error.as_ref().map_or("unknown", |_| "retryable error on previous model route"),
                    "falling back to alternative model route"
                );
                crate::metrics::record_llm_fallback_attempt(nous_id, "fallback");
            } else {
                tracing::warn!(
                    model = %fallback_route.model,
                    provider = fallback_route.provider.as_deref().unwrap_or("model-only"),
                    attempt = fallback_attempt,
                    "retrying fallback model route"
                );
                crate::metrics::record_llm_fallback_attempt(nous_id, "fallback_retry");
            }

            let raw = complete_once(providers, fallback_route, &routed_request).await;
            if raw.attempted {
                attempts += 1;
            }
            match raw.result {
                Ok((response, provider)) => {
                    return Ok(RegistryFallbackCompletion {
                        response,
                        model: fallback_route.model.clone(),
                        provider,
                        attempts,
                    });
                }
                Err(e) => {
                    if !e.is_retryable() {
                        return Err(e);
                    }
                    tracing::warn!(
                        model = %fallback_route.model,
                        provider = fallback_route.provider.as_deref().unwrap_or("model-only"),
                        attempt = fallback_attempt,
                        error = %redact_sensitive(&e.to_string()),
                        "fallback model route failed with retryable error"
                    );
                    attempt_errors.push(format!("{fallback_label}: {e}"));
                    last_error = Some(e);
                }
            }
        }
    }

    fallback_chain_error(
        last_error,
        &attempt_errors,
        !config.fallback_routes.is_empty(),
    )
}

/// Execute a streaming completion request with registry-backed model/provider fallback.
///
/// Fallback is only safe before any stream event has been delivered to the
/// caller. Once the callback fires, the consumer may have rendered partial SSE
/// state; a route switch after that point would risk duplicated or incoherent
/// output, so retryable errors are returned as terminal failures.
pub(super) async fn complete_streaming_with_registry_fallback(
    providers: &ProviderRegistry,
    request: &CompletionRequest,
    primary_route: &ModelProviderRoute,
    config: &RegistryFallbackConfig,
    nous_id: &str,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> llm_error::Result<RegistryFallbackCompletion> {
    let primary_label = route_label(primary_route);
    let mut last_error = None;
    let mut attempt_errors = Vec::new();
    let mut attempts: u32 = 0;

    for attempt in 0..config.retries_before_fallback.max(1) {
        if attempt > 0 {
            tracing::warn!(
                model = %primary_route.model,
                provider = primary_route.provider.as_deref().unwrap_or("model-only"),
                attempt,
                "retrying primary streaming model route"
            );
            crate::metrics::record_llm_fallback_attempt(nous_id, "primary_retry");
        }

        let routed_request = request_for_route(request, primary_route);
        let raw = complete_streaming_once(providers, primary_route, &routed_request, on_event).await;
        if raw.attempted {
            attempts += 1;
        }
        match raw.result {
            Ok((response, provider)) => {
                return Ok(RegistryFallbackCompletion {
                    response,
                    model: primary_route.model.clone(),
                    provider,
                    attempts,
                });
            }
            Err(e) => {
                if raw.emitted_stream_event || !e.is_retryable() {
                    return Err(e);
                }
                tracing::warn!(
                    model = %primary_route.model,
                    provider = primary_route.provider.as_deref().unwrap_or("model-only"),
                    attempt,
                    error = %redact_sensitive(&e.to_string()),
                    "primary streaming model route failed with retryable error before stream output"
                );
                attempt_errors.push(format!("{primary_label}: {e}"));
                last_error = Some(e);
            }
        }
    }

    for fallback_route in &config.fallback_routes {
        let fallback_label = route_label(fallback_route);
        let routed_request = request_for_route(request, fallback_route);

        for fallback_attempt in 0..config.retries_before_fallback.max(1) {
            if fallback_attempt == 0 {
                tracing::warn!(
                    primary = %primary_label,
                    fallback = %fallback_label,
                    reason = %last_error.as_ref().map_or("unknown", |_| "retryable error on previous streaming model route"),
                    "falling back to alternative streaming model route"
                );
                crate::metrics::record_llm_fallback_attempt(nous_id, "fallback");
            } else {
                tracing::warn!(
                    model = %fallback_route.model,
                    provider = fallback_route.provider.as_deref().unwrap_or("model-only"),
                    attempt = fallback_attempt,
                    "retrying fallback streaming model route"
                );
                crate::metrics::record_llm_fallback_attempt(nous_id, "fallback_retry");
            }

            let raw =
                complete_streaming_once(providers, fallback_route, &routed_request, on_event).await;
            if raw.attempted {
                attempts += 1;
            }
            match raw.result {
                Ok((response, provider)) => {
                    return Ok(RegistryFallbackCompletion {
                        response,
                        model: fallback_route.model.clone(),
                        provider,
                        attempts,
                    });
                }
                Err(e) => {
                    if raw.emitted_stream_event || !e.is_retryable() {
                        return Err(e);
                    }
                    tracing::warn!(
                        model = %fallback_route.model,
                        provider = fallback_route.provider.as_deref().unwrap_or("model-only"),
                        attempt = fallback_attempt,
                        error = %redact_sensitive(&e.to_string()),
                        "fallback streaming model route failed with retryable error before stream output"
                    );
                    attempt_errors.push(format!("{fallback_label}: {e}"));
                    last_error = Some(e);
                }
            }
        }
    }

    fallback_chain_error(
        last_error,
        &attempt_errors,
        !config.fallback_routes.is_empty(),
    )
}

async fn complete_once(
    providers: &ProviderRegistry,
    route: &ModelProviderRoute,
    request: &CompletionRequest,
) -> RawAttempt {
    let provider = match resolve_provider_for_route(providers, route) {
        Ok(provider) => provider,
        Err(e) => {
            return RawAttempt {
                result: Err(e),
                attempted: false,
            };
        }
    };
    let provider_name = provider.name().to_owned();
    let result = match provider.complete(request).await {
        Ok(resp) => {
            providers.record_success(&provider_name);
            Ok((resp, provider_name))
        }
        Err(e) => {
            providers.record_error(&provider_name, &e);
            Err(e)
        }
    };
    RawAttempt {
        result,
        attempted: true,
    }
}

async fn complete_streaming_once(
    providers: &ProviderRegistry,
    route: &ModelProviderRoute,
    request: &CompletionRequest,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> RawStreamingAttempt {
    let provider = match resolve_provider_for_route(providers, route) {
        Ok(provider) => provider,
        Err(e) => {
            return RawStreamingAttempt {
                result: Err(e),
                attempted: false,
                emitted_stream_event: false,
            };
        }
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

    RawStreamingAttempt {
        result: result.map(|response| (response, provider_name)),
        attempted: true,
        emitted_stream_event,
    }
}

fn resolve_provider_for_route<'a>(
    providers: &'a ProviderRegistry,
    route: &ModelProviderRoute,
) -> llm_error::Result<&'a dyn LlmProvider> {
    let provider = match providers.resolve_provider(&route.model, route.provider_route()) {
        Ok(provider) => provider,
        Err(ProviderResolutionError::NoProvider { .. }) => {
            return Err(llm_error::UnsupportedModelSnafu {
                model: route.model.clone(),
            }
            .build());
        }
        Err(ProviderResolutionError::ProviderNotFound { name, model }) => {
            return Err(llm_error::ApiRequestSnafu {
                message: format!("provider '{name}' is not registered for model: {model}"),
            }
            .build());
        }
        Err(ProviderResolutionError::ProviderDoesNotSupportModel { name, model }) => {
            return Err(llm_error::ApiRequestSnafu {
                message: format!("provider '{name}' does not support model: {model}"),
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

fn request_for_route(request: &CompletionRequest, route: &ModelProviderRoute) -> CompletionRequest {
    let mut routed_request = request.clone();
    routed_request.model.clone_from(&route.model);
    routed_request
}

fn route_label(route: &ModelProviderRoute) -> String {
    route.provider.as_ref().map_or_else(
        || route.model.clone(),
        |provider| format!("{} via {}", route.model, provider),
    )
}

fn fallback_chain_error(
    last_error: Option<llm_error::Error>,
    attempt_errors: &[String],
    has_fallbacks: bool,
) -> llm_error::Result<RegistryFallbackCompletion> {
    if !attempt_errors.is_empty() && has_fallbacks {
        return Err(llm_error::ApiRequestSnafu {
            message: format!(
                "connection unavailable: all model routes in fallback chain failed: {}",
                attempt_errors.join("; ")
            ),
        }
        .build());
    }

    Err(last_error.unwrap_or_else(|| {
        llm_error::ApiRequestSnafu {
            message: "all model routes in fallback chain failed".to_owned(),
        }
        .build()
    }))
}
