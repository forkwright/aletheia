//! Registry-backed model/provider fallback for the execute stage.

use std::ops::ControlFlow;

use hermeneus::anthropic::StreamEvent;
use hermeneus::error as llm_error;
use hermeneus::health::ProviderHealth;
use hermeneus::provider::{
    LlmProvider, ProviderCapabilities, ProviderRegistry, ProviderResolutionError,
};
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
    /// True once at least one stream chunk reached the caller, past which a
    /// route switch would risk duplicated or incoherent output. Always
    /// `false` for a non-streaming attempt, which makes it a no-op in the
    /// shared terminal-error check in [`record_attempt`].
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
    let required = ProviderCapabilities::required_by(request);
    let primary_label = route_label(primary_route);
    let mut last_error = None;
    let mut attempt_errors = Vec::new();
    let mut attempts: u32 = 0;

    if let Some(skip) = capability_gap(providers, primary_route, required) {
        log_capability_skip(nous_id, &skip, "primary");
        attempt_errors.push(skip);
    } else {
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
            let raw = complete_once(providers, primary_route, &routed_request, required).await;
            if let ControlFlow::Break(result) = record_attempt(
                primary_route,
                raw,
                "primary model route failed with retryable error",
                attempt,
                &mut attempts,
                &mut attempt_errors,
                &mut last_error,
            ) {
                return result;
            }
        }
    }

    for fallback_route in &config.fallback_routes {
        if let Some(skip) = capability_gap(providers, fallback_route, required) {
            log_capability_skip(nous_id, &skip, "fallback");
            attempt_errors.push(skip);
            continue;
        }

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

            let raw = complete_once(providers, fallback_route, &routed_request, required).await;
            if let ControlFlow::Break(result) = record_attempt(
                fallback_route,
                raw,
                "fallback model route failed with retryable error",
                fallback_attempt,
                &mut attempts,
                &mut attempt_errors,
                &mut last_error,
            ) {
                return result;
            }
        }
    }

    if attempts == 0 && last_error.is_none() {
        return Err(capability_exhausted_error(&attempt_errors));
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
    let required = ProviderCapabilities::required_by(request);
    let primary_label = route_label(primary_route);
    let mut last_error = None;
    let mut attempt_errors = Vec::new();
    let mut attempts: u32 = 0;

    if let Some(skip) = capability_gap(providers, primary_route, required) {
        log_capability_skip(nous_id, &skip, "primary");
        attempt_errors.push(skip);
    } else {
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
            let raw = complete_streaming_once(
                providers,
                primary_route,
                &routed_request,
                on_event,
                required,
            )
            .await;
            if let ControlFlow::Break(result) = record_attempt(
                primary_route,
                raw,
                "primary streaming model route failed with retryable error before stream output",
                attempt,
                &mut attempts,
                &mut attempt_errors,
                &mut last_error,
            ) {
                return result;
            }
        }
    }

    for fallback_route in &config.fallback_routes {
        if let Some(skip) = capability_gap(providers, fallback_route, required) {
            log_capability_skip(nous_id, &skip, "fallback");
            attempt_errors.push(skip);
            continue;
        }

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

            let raw = complete_streaming_once(
                providers,
                fallback_route,
                &routed_request,
                on_event,
                required,
            )
            .await;
            if let ControlFlow::Break(result) = record_attempt(
                fallback_route,
                raw,
                "fallback streaming model route failed with retryable error before stream output",
                fallback_attempt,
                &mut attempts,
                &mut attempt_errors,
                &mut last_error,
            ) {
                return result;
            }
        }
    }

    if attempts == 0 && last_error.is_none() {
        return Err(capability_exhausted_error(&attempt_errors));
    }

    fallback_chain_error(
        last_error,
        &attempt_errors,
        !config.fallback_routes.is_empty(),
    )
}

/// Fold one raw provider-call attempt into the running fallback state.
///
/// `ControlFlow::Break` carries the result that should propagate
/// immediately: a completed response, or an error that is either
/// non-retryable or arrived after streaming had already committed output to
/// the caller. `ControlFlow::Continue` means the attempt was a retryable
/// failure — it has been logged and recorded — and the caller should retry
/// the route or move on to the next one.
fn record_attempt(
    route: &ModelProviderRoute,
    raw: RawAttempt,
    failure_label: &'static str,
    attempt: u32,
    attempts: &mut u32,
    attempt_errors: &mut Vec<String>,
    last_error: &mut Option<llm_error::Error>,
) -> ControlFlow<llm_error::Result<RegistryFallbackCompletion>> {
    if raw.attempted {
        *attempts += 1;
    }
    match raw.result {
        Ok((response, provider)) => ControlFlow::Break(Ok(RegistryFallbackCompletion {
            response,
            model: route.model.clone(),
            provider,
            attempts: *attempts,
        })),
        Err(e) => {
            if raw.emitted_stream_event || !e.is_retryable() {
                return ControlFlow::Break(Err(e));
            }
            tracing::warn!(
                model = %route.model,
                provider = route.provider.as_deref().unwrap_or("model-only"),
                attempt,
                error = %redact_sensitive(&e.to_string()),
                "{failure_label}"
            );
            attempt_errors.push(format!("{}: {e}", route_label(route)));
            *last_error = Some(e);
            ControlFlow::Continue(())
        }
    }
}

async fn complete_once(
    providers: &ProviderRegistry,
    route: &ModelProviderRoute,
    request: &CompletionRequest,
    required: ProviderCapabilities,
) -> RawAttempt {
    let provider = match resolve_provider_for_route(providers, route, required) {
        Ok(provider) => provider,
        Err(e) => {
            return RawAttempt {
                result: Err(e),
                attempted: false,
                emitted_stream_event: false,
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
        emitted_stream_event: false,
    }
}

async fn complete_streaming_once(
    providers: &ProviderRegistry,
    route: &ModelProviderRoute,
    request: &CompletionRequest,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
    required: ProviderCapabilities,
) -> RawAttempt {
    let provider = match resolve_provider_for_route(providers, route, required) {
        Ok(provider) => provider,
        Err(e) => {
            return RawAttempt {
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

    RawAttempt {
        result: result.map(|response| (response, provider_name)),
        attempted: true,
        emitted_stream_event,
    }
}

fn resolve_provider_for_route<'a>(
    providers: &'a ProviderRegistry,
    route: &ModelProviderRoute,
    required: ProviderCapabilities,
) -> llm_error::Result<&'a dyn LlmProvider> {
    // WHY(#5253): capability-aware, matching `capability_gap`'s pre-check —
    // a capability-incapable provider must never be the one actually
    // selected for dispatch, even when a co-tier alternative is merely
    // unhealthy (health is preferred in the registry's own reporting, but
    // an incapable provider is never eligible to be *selected* regardless
    // of another candidate's health; see `ProviderRegistry::resolve_model_only`).
    let provider = match providers.resolve_provider_for_request(
        &route.model,
        route.provider_route(),
        required,
    ) {
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
        // WHY(#5253, #5254): reachable only under a health-flip race between
        // `capability_gap`'s pre-check and this call — the pre-check already
        // skips the route (moving straight to the next one, without
        // attempting) whenever it observes a definite capability mismatch.
        // If a health flip removes the last capable-and-healthy candidate in
        // the narrow window between the two calls, this is a permanent
        // (non-retryable) error and — unlike the pre-check's route-level
        // skip — DOES abort the whole chain via `record_attempt`'s
        // non-retryable branch, the same accepted trade-off as the
        // provider-health TOCTOU already below this match.
        Err(ProviderResolutionError::CapabilityMismatch {
            name,
            model,
            capability,
        }) => {
            return Err(llm_error::CapabilityMismatchSnafu {
                provider: name.clone(),
                capability: capability.to_owned(),
                message: format!(
                    "provider '{name}' cannot satisfy required capability \
                     '{capability}' for model: {model}"
                ),
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

/// Whether `route` resolves to a provider that cannot satisfy `required`
/// capabilities, checked once per route rather than once per attempt.
///
/// WHY(#5254): capability is a route-invariant fact — unlike health, no
/// number of retries changes whether a seat-bridged CLI subprocess provider
/// can run aletheia's tool loop. Gating here, before a route's attempt loop
/// even starts, means a tool-bearing turn skips straight past an incapable
/// route to the next one instead of burning its retry budget and then
/// aborting the *whole* chain on a `Permanent`-classified error — capability
/// mismatch is exactly that class (see `error.rs::capability_mismatch_is_not_retryable`),
/// so threading it through `record_attempt`'s retryable/permanent split
/// would abort the chain instead of skipping the route.
///
/// Returns `None` for any other resolution failure (unhealthy, unregistered,
/// wrong model) — those are left to the existing per-attempt path via
/// `resolve_provider_for_route`, which already retries or skips them.
fn capability_gap(
    providers: &ProviderRegistry,
    route: &ModelProviderRoute,
    required: ProviderCapabilities,
) -> Option<String> {
    match providers.resolve_provider_for_request(&route.model, route.provider_route(), required) {
        Err(ProviderResolutionError::CapabilityMismatch {
            name, capability, ..
        }) => Some(format!(
            "{}: provider '{name}' cannot satisfy required capability '{capability}'",
            route_label(route)
        )),
        _ => None,
    }
}

/// Log and account for a route skipped by [`capability_gap`], consistently
/// between the primary and fallback loops in both the streaming and
/// non-streaming fallback functions.
fn log_capability_skip(nous_id: &str, skip_reason: &str, route_kind: &'static str) {
    tracing::warn!(
        route_kind,
        reason = %skip_reason,
        "model route cannot satisfy request capabilities; skipping"
    );
    crate::metrics::record_llm_fallback_attempt(nous_id, "capability_skip");
}

/// Terminal error for a fallback chain where every route — primary and every
/// configured fallback — was skipped by [`capability_gap`] before any
/// provider was actually dispatched to.
///
/// WHY(#5254): without this, the chain would fall through to
/// [`fallback_chain_error`]'s generic "connection unavailable" `ApiRequest`
/// message, which the `"connection"` transient marker misclassifies as
/// retryable — retrying the identical chain against the identical
/// tool-bearing request always fails the same deterministic way. Surfacing
/// the same [`hermeneus::error::Error::CapabilityMismatch`] #4510 defined as
/// its backstop instead reads this as the `Permanent`/`Surface` planning
/// failure it is.
fn capability_exhausted_error(attempt_errors: &[String]) -> llm_error::Error {
    llm_error::CapabilityMismatchSnafu {
        provider: "fallback-chain".to_owned(),
        capability: hermeneus::provider::TOOL_LOOP_CAPABILITY.to_owned(),
        message: format!(
            "no route in the fallback chain can satisfy this request's required capabilities: {}",
            attempt_errors.join("; ")
        ),
    }
    .build()
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
