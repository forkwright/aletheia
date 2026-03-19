//! Resolved configuration types: computed from raw config at runtime.

use super::{AgencyLevel, AletheiaConfig, RecallSettings};

/// Resolved model selection for an agent.
#[derive(Debug, Clone)]
pub struct ResolvedModelConfig {
    /// Primary model identifier.
    pub primary: String,
    /// Ordered fallback models.
    pub fallbacks: Vec<String>,
    /// How many times to retry the current model before trying the next fallback.
    pub retries_before_fallback: u32,
}

/// Token budget limits for an agent.
#[derive(Debug, Clone)]
pub struct TokenLimits {
    /// Maximum input context window in tokens.
    pub context_tokens: u32,
    /// Maximum output tokens per response.
    pub max_output_tokens: u32,
    /// Token budget for bootstrap content.
    pub bootstrap_max_tokens: u32,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Characters per token for token-budget estimation.
    pub chars_per_token: u32,
    /// Fraction of the context window reserved for conversation history.
    pub history_budget_ratio: f64,
    /// Maximum tool result size in bytes before truncation (0 = disabled).
    pub max_tool_result_bytes: u32,
}

/// Behavioral capabilities for an agent.
#[derive(Debug, Clone)]
pub struct AgentCapabilities {
    /// Whether extended thinking is enabled for this agent.
    pub thinking_enabled: bool,
    /// Effective agency level for this agent.
    pub agency: AgencyLevel,
    /// Maximum consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Whether prompt caching is enabled.
    pub cache_enabled: bool,
}

/// Resolved configuration for a specific nous agent.
///
/// Produced by merging [`super::AgentDefaults`] with a matching [`super::NousDefinition`].
#[derive(Debug, Clone)]
pub struct ResolvedNousConfig {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name (from the agent definition, if set).
    pub name: Option<String>,
    /// Resolved model selection.
    pub model: ResolvedModelConfig,
    /// Token budget limits.
    pub limits: TokenLimits,
    /// Behavioral capabilities.
    pub capabilities: AgentCapabilities,
    /// Resolved workspace directory path.
    pub workspace: String,
    /// Merged set of permitted filesystem roots.
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent covers.
    pub domains: Vec<String>,
    /// Resolved recall pipeline settings.
    pub recall: RecallSettings,
    /// Model used for prosoche heartbeat sessions.
    pub prosoche_model: String,
}

/// Resolve effective configuration for a specific nous agent.
///
/// Merges `agents.defaults` with the matching entry from `agents.list`.
/// If no matching agent is found, returns defaults with the given id.
#[must_use]
pub fn resolve_nous(config: &AletheiaConfig, nous_id: &str) -> ResolvedNousConfig {
    let defaults = &config.agents.defaults;
    let agent = config.agents.list.iter().find(|a| a.id == nous_id);

    let (model, fallbacks, retries_before_fallback) = match agent.and_then(|a| a.model.as_ref()) {
        Some(spec) => (
            spec.primary.clone(),
            spec.fallbacks.clone(),
            spec.retries_before_fallback,
        ),
        None => (
            defaults.model.primary.clone(),
            defaults.model.fallbacks.clone(),
            defaults.model.retries_before_fallback,
        ),
    };

    let agency = agent.and_then(|a| a.agency).unwrap_or(defaults.agency);

    let thinking_enabled = agent
        .and_then(|a| a.thinking_enabled)
        .unwrap_or(defaults.thinking_enabled);

    let max_tool_iterations = match agency {
        AgencyLevel::Unrestricted => 10_000,
        AgencyLevel::Standard => defaults.max_tool_iterations,
        AgencyLevel::Restricted => 50,
    };

    let workspace = agent.map_or_else(
        || format!("instance/nous/{nous_id}"),
        |a| a.workspace.clone(),
    );

    let mut allowed_roots = defaults.allowed_roots.clone();
    if let Some(agent) = agent {
        for root in &agent.allowed_roots {
            if !allowed_roots.contains(root) {
                allowed_roots.push(root.clone());
            }
        }
    }

    let domains = agent.map(|a| a.domains.clone()).unwrap_or_default();
    let name = agent.and_then(|a| a.name.clone());

    // NOTE: Agent-level recall overrides; falls back to shared defaults.
    let recall = agent
        .and_then(|a| a.recall.clone())
        .unwrap_or_else(|| defaults.recall.clone());

    ResolvedNousConfig {
        id: nous_id.to_owned(),
        name,
        model: ResolvedModelConfig {
            primary: model,
            fallbacks,
            retries_before_fallback,
        },
        limits: TokenLimits {
            context_tokens: defaults.context_tokens,
            max_output_tokens: defaults.max_output_tokens,
            bootstrap_max_tokens: defaults.bootstrap_max_tokens,
            thinking_budget: defaults.thinking_budget,
            chars_per_token: defaults.chars_per_token,
            history_budget_ratio: defaults.history_budget_ratio,
            max_tool_result_bytes: defaults.max_tool_result_bytes,
        },
        capabilities: AgentCapabilities {
            thinking_enabled,
            agency,
            max_tool_iterations,
            cache_enabled: defaults.caching.enabled && defaults.caching.strategy != "disabled",
        },
        workspace,
        allowed_roots,
        domains,
        recall,
        prosoche_model: defaults.prosoche_model.clone(),
    }
}
