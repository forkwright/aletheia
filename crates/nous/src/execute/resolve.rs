//! Resolution helpers for the execute stage.

use std::collections::HashSet;
use std::sync::Arc;

use tracing::{debug, warn};

use hermeneus::complexity::{ComplexityInput, route_model};
use hermeneus::health::ProviderHealth;
use hermeneus::provider::{
    DeploymentTarget, LlmProvider, ProviderRegistry, ProviderResolutionError, ProviderRoute,
};
use hermeneus::types::{ContentBlock, ServerToolDefinition};
use koina::id::ToolName;
use mneme::knowledge::FactSensitivity;
use organon::types::ToolContext;

use crate::config::NousConfig;
use crate::error;
use crate::pipeline::PipelineContext;

/// Extracted text, tool uses, server-tool flags, and reasoning from a single LLM response.
#[derive(Default)]
pub(super) struct ResponseExtract {
    pub text_parts: Vec<String>,
    pub tool_uses: Vec<(String, String, serde_json::Value)>,
    pub saw_server_web_search: bool,
    pub saw_server_code_execution: bool,
    pub reasoning_parts: Vec<String>,
}

/// Result of resolving a provider against the current-turn sensitivity policy.
pub(super) enum ProviderAdmission<'a> {
    Admitted(&'a dyn LlmProvider),
    Blocked(String),
    Unavailable(String),
    ResolutionError(error::Error),
}

/// Resolve the model to use for this turn, applying complexity-based routing when enabled.
///
/// WHY: when `complexity.enabled == false` (the default) this returns
/// `config.generation.model` unchanged, preserving existing behaviour bit-for-bit.
/// When enabled, the last user message plus available tool count feed into
/// [`route_model`], which maps a score to a tier model.
pub(super) fn resolve_turn_model(
    ctx: &PipelineContext,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tool_count: usize,
) -> String {
    if !config.generation.complexity.enabled {
        return config.generation.model.clone();
    }

    // WHY: complexity routing scores the most recent user message — the one
    // driving this turn. Fall back to empty text when no user message exists
    // so scoring produces a baseline (Haiku) tier rather than panicking.
    let last_user_text = ctx
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map_or("", |m| m.content.as_str());

    let input = ComplexityInput {
        message_text: last_user_text,
        tool_count,
        message_count: ctx.messages.len(),
        depth: 0,
        tier_override: None,
        model_override: None,
    };

    let decision = route_model(&input, &config.generation.complexity);
    let deployment_target = providers
        .find_provider(&config.generation.model)
        .map_or(DeploymentTarget::Cloud, LlmProvider::deployment_target);
    let configured_local = matches!(
        deployment_target,
        DeploymentTarget::LocalHosted | DeploymentTarget::Embedded
    );
    let routed_deployment_target = providers
        .find_provider(&decision.model)
        .map(LlmProvider::deployment_target);
    let routed_local = matches!(
        routed_deployment_target,
        Some(DeploymentTarget::LocalHosted | DeploymentTarget::Embedded)
    );
    if configured_local && !routed_local && decision.model != config.generation.model {
        debug!(
            configured_model = config.generation.model,
            routed_model = decision.model,
            deployment_target = deployment_target.as_str(),
            routed_deployment_target = routed_deployment_target
                .map_or("unregistered", DeploymentTarget::as_str),
            complexity_score = decision.complexity.score,
            complexity_tier = %decision.complexity.tier,
            "complexity routing preserved local deployment target"
        );
        return config.generation.model.clone();
    }

    decision.model
}

/// Resolve a provider for `model` that may receive the current live prompt.
pub(super) fn resolve_admitted_provider<'a>(
    ctx: &PipelineContext,
    providers: &'a ProviderRegistry,
    model: &str,
) -> ProviderAdmission<'a> {
    let provider = match providers.resolve_provider(model, ProviderRoute::ModelOnly) {
        Ok(provider) => provider,
        Err(ProviderResolutionError::ProviderUnavailable { name, health }) => {
            return ProviderAdmission::Unavailable(format!(
                "provider '{name}' is currently unavailable: {health:?}"
            ));
        }
        Err(ProviderResolutionError::NoProvider { model }) => {
            return ProviderAdmission::ResolutionError(
                error::PipelineStageSnafu {
                    stage: "execute",
                    message: format!("no provider for model: {model}"),
                }
                .build(),
            );
        }
    };

    if admits_current_turn(ctx, provider) {
        return ProviderAdmission::Admitted(provider);
    }

    if let Some(alternative) = find_admitted_alternative(ctx, providers, model) {
        warn!(
            model,
            blocked_provider = provider.name(),
            blocked_deployment_target = provider.deployment_target().as_str(),
            selected_provider = alternative.name(),
            selected_deployment_target = alternative.deployment_target().as_str(),
            sensitivity = current_turn_sensitivity(ctx).as_str(),
            "execute: routed current-turn prompt to eligible provider"
        );
        return ProviderAdmission::Admitted(alternative);
    }

    let message = provider_admission_message(ctx, provider, model);
    warn!(
        model,
        provider = provider.name(),
        deployment_target = provider.deployment_target().as_str(),
        sensitivity = current_turn_sensitivity(ctx).as_str(),
        max_sensitivity = max_sensitivity_for_target(provider.deployment_target()).as_str(),
        "execute: blocked provider dispatch by current-turn sensitivity policy"
    );
    ProviderAdmission::Blocked(message)
}

/// Resolve a provider for `model`, returning a pipeline error when policy blocks it.
pub(super) fn resolve_admitted_provider_checked<'a>(
    ctx: &PipelineContext,
    providers: &'a ProviderRegistry,
    model: &str,
) -> error::Result<&'a dyn LlmProvider> {
    match resolve_admitted_provider(ctx, providers, model) {
        ProviderAdmission::Admitted(provider) => Ok(provider),
        ProviderAdmission::Blocked(message) | ProviderAdmission::Unavailable(message) => {
            Err(error::PipelineStageSnafu {
                stage: "execute",
                message,
            }
            .build())
        }
        ProviderAdmission::ResolutionError(err) => Err(err),
    }
}

/// Resolve an eligible streaming provider for the current turn.
pub(super) fn resolve_admitted_streaming_provider<'a>(
    ctx: &PipelineContext,
    providers: &'a ProviderRegistry,
    model: &str,
) -> Option<&'a dyn LlmProvider> {
    find_admitted_provider(ctx, providers, model, true)
}

fn find_admitted_alternative<'a>(
    ctx: &PipelineContext,
    providers: &'a ProviderRegistry,
    model: &str,
) -> Option<&'a dyn LlmProvider> {
    find_admitted_provider(ctx, providers, model, false)
}

fn find_admitted_provider<'a>(
    ctx: &PipelineContext,
    providers: &'a ProviderRegistry,
    model: &str,
    require_streaming: bool,
) -> Option<&'a dyn LlmProvider> {
    let target_kind = providers
        .providers()
        .into_iter()
        .filter_map(|provider| provider.match_specificity(model))
        .max()?;

    providers.providers().into_iter().find(|provider| {
        provider.match_specificity(model) == Some(target_kind)
            && (!require_streaming || provider.supports_streaming())
            && provider_is_available(providers, *provider)
            && admits_current_turn(ctx, *provider)
    })
}

fn provider_is_available(providers: &ProviderRegistry, provider: &dyn LlmProvider) -> bool {
    providers
        .provider_health(provider.name())
        .is_some_and(|health| {
            matches!(health, ProviderHealth::Up | ProviderHealth::Degraded { .. })
        })
}

fn admits_current_turn(ctx: &PipelineContext, provider: &dyn LlmProvider) -> bool {
    current_turn_sensitivity(ctx) <= max_sensitivity_for_target(provider.deployment_target())
}

fn provider_admission_message(
    ctx: &PipelineContext,
    provider: &dyn LlmProvider,
    model: &str,
) -> String {
    let sensitivity = current_turn_sensitivity(ctx);
    let target = provider.deployment_target();
    let max_allowed = max_sensitivity_for_target(target);
    format!(
        "provider '{}' (model '{}', deployment target '{}') may not receive current-turn '{}' prompt; maximum allowed sensitivity is '{}'",
        provider.name(),
        model,
        target.as_str(),
        sensitivity.as_str(),
        max_allowed.as_str()
    )
}

fn current_turn_sensitivity(ctx: &PipelineContext) -> FactSensitivity {
    ctx.triage_result
        .as_ref()
        .map_or(FactSensitivity::Public, |triage| triage.sensitivity)
}

fn max_sensitivity_for_target(target: DeploymentTarget) -> FactSensitivity {
    // WHY(#4621): match recall's sovereignty boundary for live prompts. Future
    // deployment targets default to Public until the policy is explicitly extended.
    match target {
        DeploymentTarget::LocalHosted => FactSensitivity::Internal,
        DeploymentTarget::Embedded => FactSensitivity::Confidential,
        _ => FactSensitivity::Public,
    }
}

/// Read the current active-tools set and derive server-tool definitions.
///
/// Returns `(active_set, server_tools)` so callers can filter local tool
/// definitions against the same snapshot of `active` while reusing the
/// server-tool `Arc` when nothing changed (#3389).
///
/// The `config_server_tools` argument is an `Arc` of the config's static
/// server-tool list, hoisted out of the per-iteration loop by the caller so
/// the backward-compatibility clone pays once per turn instead of once per
/// LLM iteration. When the session has no dynamically-activated server tools
/// and the call site has no [`ToolServices`], the same `Arc` is returned
/// without allocation.
pub(super) fn resolve_active_server_tools(
    tool_ctx: &ToolContext,
    config_server_tools: &Arc<Vec<ServerToolDefinition>>,
) -> (Arc<HashSet<ToolName>>, Arc<Vec<ServerToolDefinition>>) {
    // WHY: the std::sync::RwLock is held only long enough to clone the inner
    // HashSet into an Arc. Downstream iteration reads the Arc without the lock,
    // which means enable_tool can take the write lock without blocking on
    // long-running tool iterations.
    let active_snapshot = tool_ctx
        .active_tools
        .read()
        .unwrap_or_else(|poisoned| {
            warn!("active_tools lock poisoned by prior panic, recovering with last value");
            poisoned.into_inner()
        })
        .clone();
    let active = Arc::new(active_snapshot);

    // WHY: fast path — no ToolServices means server tools come solely from
    // static config, which we already hold as an Arc. Skip the Vec allocation
    // and return the shared handle unchanged.
    let Some(services) = tool_ctx.services.as_deref() else {
        return (active, Arc::clone(config_server_tools));
    };

    let dynamic = services.server_tool_config.active_definitions(&active);

    // WHY: fast path — no dynamically-activated server tools (the common case
    // when no enable_tool call has fired) reuses the config Arc as-is.
    if dynamic.is_empty() {
        return (active, Arc::clone(config_server_tools));
    }

    // WHY: combine dynamic and static definitions in a fresh Vec exactly when
    // the dynamic list is non-empty. Wrapping in Arc keeps the return type
    // uniform so callers don't branch on cardinality.
    let mut combined = dynamic;
    combined.extend_from_slice(config_server_tools.as_slice());
    (active, Arc::new(combined))
}

/// Extract text, tool uses, and reasoning parts from a completion response.
pub(super) fn process_response_blocks(content: &[ContentBlock]) -> ResponseExtract {
    let mut extract = ResponseExtract::default();

    for block in content {
        match block {
            ContentBlock::Text { text, .. } => extract.text_parts.push(text.clone()),
            ContentBlock::ToolUse { id, name, input } => {
                extract
                    .tool_uses
                    .push((id.clone(), name.clone(), input.clone()));
            }
            ContentBlock::Thinking { thinking, .. } => {
                debug!(len = thinking.len(), "thinking block received");
                extract.reasoning_parts.push(thinking.clone());
            }
            ContentBlock::ServerToolUse { name, .. } if name == "web_search" => {
                extract.saw_server_web_search = true;
            }
            ContentBlock::ServerToolUse { name, .. } if name == "code_execution" => {
                extract.saw_server_code_execution = true;
            }
            ContentBlock::CodeExecutionResult {
                code, return_code, ..
            } => {
                extract.saw_server_code_execution = true;
                debug!(
                    code_len = code.len(),
                    return_code, "server code execution result received"
                );
            }
            _ => {
                // NOTE: other content block types (images, etc.) are not tracked in extraction
            }
        }
    }

    extract
}
