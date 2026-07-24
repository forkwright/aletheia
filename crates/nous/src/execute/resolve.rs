//! Resolution helpers for the execute stage.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::{debug, warn};

use hermeneus::complexity::{ComplexityInput, route_model};
use hermeneus::provider::{
    DeploymentTarget, LlmProvider, ProviderRegistry, ProviderResolutionError,
};
use hermeneus::types::{ContentBlock, ServerToolDefinition};
use koina::id::ToolName;
use organon::types::ToolContext;

use crate::config::{ModelProviderRoute, NousConfig};
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
    resolve_turn_route(ctx, config, providers, tool_count).model
}

/// Resolve the model/provider route to use for this turn.
pub(super) fn resolve_turn_route(
    ctx: &PipelineContext,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tool_count: usize,
) -> ModelProviderRoute {
    let configured_route = ModelProviderRoute {
        model: config.generation.model.clone(),
        provider: config.generation.provider.clone(),
    };
    if !config.generation.complexity.enabled {
        return configured_route;
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
        .resolve_provider(&configured_route.model, configured_route.provider_route())
        .ok()
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
        return configured_route;
    }

    if decision.model == configured_route.model {
        configured_route
    } else {
        ModelProviderRoute::model_only(decision.model)
    }
}

/// Resolve the LLM provider for `route` and verify it is not marked down.
pub(super) fn resolve_provider_checked<'a>(
    providers: &'a ProviderRegistry,
    route: &ModelProviderRoute,
) -> error::Result<&'a dyn LlmProvider> {
    providers
        .resolve_provider(&route.model, route.provider_route())
        .map_err(|err| {
            let message = match err {
                ProviderResolutionError::NoProvider { model } => {
                    format!("no provider for model: {model}")
                }
                ProviderResolutionError::ProviderNotFound { name, model } => {
                    format!("provider '{name}' is not registered for model: {model}")
                }
                ProviderResolutionError::ProviderDoesNotSupportModel { name, model } => {
                    format!("provider '{name}' does not support model: {model}")
                }
                ProviderResolutionError::ProviderUnavailable { name, health } => {
                    format!("provider '{name}' is currently unavailable: {health:?}")
                }
            };
            error::PipelineStageSnafu {
                stage: "execute",
                message,
            }
            .build()
        })
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

    normalize_tool_use_ids(&mut extract.tool_uses);
    extract
}

/// Normalize malformed provider-supplied `tool_use` ids in place.
///
/// WHY: normalize-don't-fail. Empty or duplicate ids are a malformed-provider
/// condition, not a caller bug, and downstream correlation (spawn-isolation
/// enforcement, dispatch policy filtering, hook context) keys entirely on
/// this id. Failing the whole turn over one bad block would be a worse
/// outcome than degrading gracefully with a synthesized/disambiguated id, so
/// this deliberately does not return a turn-level error.
fn normalize_tool_use_ids(tool_uses: &mut [(String, String, serde_json::Value)]) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut dup_counts: HashMap<String, usize> = HashMap::new();

    for (index, (id, name, _)) in tool_uses.iter_mut().enumerate() {
        let name = name.clone();

        if id.is_empty() {
            let synthetic_id = format!("synth-{index}");
            warn!(
                index,
                tool_name = %name,
                "provider tool_use block had an empty id; synthesized {synthetic_id}"
            );
            id.clone_from(&synthetic_id);
            seen.insert(synthetic_id);
            continue;
        }

        if seen.contains(id.as_str()) {
            let dup_count = dup_counts.entry(id.clone()).or_insert(0);
            *dup_count += 1;
            let new_id = format!("{id}-dup{dup_count}");
            warn!(
                tool_use_id = %id,
                tool_name = %name,
                "duplicate provider tool_use id; disambiguated to {new_id}"
            );
            id.clone_from(&new_id);
            seen.insert(new_id);
        } else {
            seen.insert(id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tool_use_ids_synthesizes_empty_id() {
        let mut tool_uses = vec![
            (
                "id-1".to_string(),
                "tool_a".to_string(),
                serde_json::json!({}),
            ),
            (String::new(), "tool_b".to_string(), serde_json::json!({})),
        ];

        normalize_tool_use_ids(&mut tool_uses);

        let ids: Vec<&str> = tool_uses.iter().map(|(id, _, _)| id.as_str()).collect();
        assert_eq!(ids, ["id-1", "synth-1"]);
    }

    #[test]
    fn normalize_tool_use_ids_disambiguates_duplicate_ids() {
        let mut tool_uses = vec![
            (
                "dup-id".to_string(),
                "tool_a".to_string(),
                serde_json::json!({}),
            ),
            (
                "dup-id".to_string(),
                "tool_b".to_string(),
                serde_json::json!({}),
            ),
            (
                "dup-id".to_string(),
                "tool_c".to_string(),
                serde_json::json!({}),
            ),
        ];

        normalize_tool_use_ids(&mut tool_uses);

        let ids: Vec<&str> = tool_uses.iter().map(|(id, _, _)| id.as_str()).collect();
        assert_eq!(ids, ["dup-id", "dup-id-dup1", "dup-id-dup2"]);
    }
}
