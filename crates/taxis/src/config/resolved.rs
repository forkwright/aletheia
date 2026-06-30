//! Resolved configuration types: computed from raw config at runtime.

use std::sync::Arc;

use super::{
    AgencyLevel, AgentBehaviorDefaults, AgentToolGroupPolicy, AletheiaConfig, ExtractionConfig,
    RecallProfile, RecallSettings,
};

/// Resolved model selection for an agent.
#[derive(Debug, Clone)]
pub struct ResolvedModelConfig {
    /// Primary model identifier.
    pub primary: Arc<str>,
    /// Optional provider instance name for the primary model.
    pub primary_provider: Option<Arc<str>>,
    /// Ordered fallback models.
    pub fallbacks: Vec<Arc<str>>,
    /// Optional provider instance names for fallback models, by index.
    pub fallback_providers: Vec<Option<Arc<str>>>,
    /// How many times to retry the current model before trying the next fallback.
    pub retries_before_fallback: u32,
}

struct ResolvedModelSelection {
    primary: Arc<str>,
    primary_provider: Option<Arc<str>>,
    fallbacks: Vec<Arc<str>>,
    fallback_providers: Vec<Option<Arc<str>>>,
    retries_before_fallback: u32,
}

/// Token budget limits for an agent.
#[derive(Debug, Clone)] // kanon:ignore RUST/no-debug-derive-on-public-types
#[rustfmt::skip]
pub struct TokenLimits { // kanon:ignore TOPOLOGY/shallow-struct
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
#[rustfmt::skip]
pub struct AgentCapabilities { // kanon:ignore TOPOLOGY/shallow-struct
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
    pub id: Arc<str>,
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
    /// Whether this agent's workspace is hidden from public discovery.
    pub private: bool,
    /// Episteme knowledge-store cohort for this agent.
    pub episteme_cohort: Arc<str>,
    /// Merged set of permitted filesystem roots.
    pub allowed_roots: Vec<String>,
    /// Resolved tool-group policy.
    pub tool_groups: AgentToolGroupPolicy,
    /// Per-agent tool allowlist.
    pub tool_allowlist: Option<Vec<String>>,
    /// Knowledge domains this agent covers.
    pub domains: Vec<String>,
    /// Resolved recall pipeline settings.
    pub recall: RecallSettings,
    /// Resolved extraction provider settings.
    pub extraction: ExtractionConfig,
    /// Resolved named recall behavior profile.
    pub recall_profile: RecallProfile,
    /// Model used for prosoche heartbeat sessions.
    pub prosoche_model: Arc<str>,
    /// Resolved per-agent behavioral parameters (safety, hooks, distillation, etc.).
    pub behavior: AgentBehaviorDefaults,
}

/// Resolve effective configuration for a specific nous agent.
///
/// Merges `agents.defaults` with the matching entry from `agents.list`.
/// If no matching agent is found, returns defaults with the given id.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "single config-cascade function keeps resolved fields in one auditable place"
)]
pub fn resolve_nous(config: &AletheiaConfig, nous_id: &str) -> ResolvedNousConfig {
    let defaults = &config.agents.defaults;
    let agent = config.agents.list.iter().find(|a| a.id == nous_id);

    let model_selection = match agent.and_then(|a| a.model.as_ref()) {
        Some(spec) => ResolvedModelSelection {
            primary: Arc::from(spec.primary.as_str()),
            primary_provider: spec.primary.provider.as_deref().map(Arc::<str>::from),
            fallbacks: spec
                .fallbacks
                .iter()
                .map(|s| Arc::from(s.as_str()))
                .collect(),
            fallback_providers: spec
                .fallbacks
                .iter()
                .map(|s| s.provider.as_deref().map(Arc::<str>::from))
                .collect(),
            retries_before_fallback: spec.retries_before_fallback,
        },
        None => ResolvedModelSelection {
            primary: Arc::from(defaults.model_defaults.model.primary.as_str()),
            primary_provider: defaults
                .model_defaults
                .model
                .primary
                .provider
                .as_deref()
                .map(Arc::<str>::from),
            fallbacks: defaults
                .model_defaults
                .model
                .fallbacks
                .iter()
                .map(|s| Arc::from(s.as_str()))
                .collect(),
            fallback_providers: defaults
                .model_defaults
                .model
                .fallbacks
                .iter()
                .map(|s| s.provider.as_deref().map(Arc::<str>::from))
                .collect(),
            retries_before_fallback: defaults.model_defaults.model.retries_before_fallback,
        },
    };
    let model = model_selection.primary;
    let model_provider = model_selection.primary_provider;
    let fallbacks = model_selection.fallbacks;
    let fallback_providers = model_selection.fallback_providers;
    let retries_before_fallback = model_selection.retries_before_fallback;

    let agency = agent.and_then(|a| a.agency).unwrap_or(defaults.agency);

    let thinking_enabled = agent
        .and_then(|a| a.thinking_enabled)
        .unwrap_or(defaults.model_defaults.thinking_enabled);

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

    let domains = agent.map_or_else(Vec::new, |agent| agent.domains.clone());
    let tool_groups = agent
        .and_then(|agent| agent.tool_groups.clone())
        .unwrap_or_else(|| defaults.tool_groups.clone());
    let tool_allowlist = agent.and_then(|agent| agent.tool_allowlist.clone());
    let name = agent.and_then(|a| a.name.clone());
    let private = agent.is_some_and(|a| a.private);
    let episteme_cohort = agent
        .and_then(|a| a.episteme_cohort.as_deref())
        .filter(|cohort| !cohort.is_empty())
        .unwrap_or("shared");

    // NOTE: Agent-level recall overrides; falls back to shared defaults.
    let recall = agent
        .and_then(|a| a.recall.clone())
        .unwrap_or_else(|| defaults.recall.clone());

    let recall_profile = agent
        .and_then(|a| a.recall_profile)
        .unwrap_or(RecallProfile::Default);

    // NOTE: Agent-level behavior overrides; falls back to shared defaults.
    // Same cascade pattern as recall: per-agent wins, otherwise defaults apply.
    let mut behavior = agent
        .and_then(|a| a.behavior.clone())
        .unwrap_or_else(|| defaults.behavior.clone());
    behavior.knowledge_extraction_provider = config.knowledge.extraction.provider;

    // WHY: Opus models have a 1M token context window; apply model-aware default
    // when the config still holds the global default (200K). Computed before
    // `model` is moved into `ResolvedModelConfig`.
    let context_tokens = {
        let configured = defaults.model_defaults.context_tokens;
        if configured == koina::defaults::CONTEXT_TOKENS && model.contains("opus") {
            koina::defaults::OPUS_CONTEXT_TOKENS
        } else {
            configured
        }
    };

    ResolvedNousConfig {
        id: Arc::from(nous_id),
        name,
        model: ResolvedModelConfig {
            primary: model,
            primary_provider: model_provider,
            fallbacks,
            fallback_providers,
            retries_before_fallback,
        },
        limits: TokenLimits {
            context_tokens,
            max_output_tokens: defaults.model_defaults.max_output_tokens,
            bootstrap_max_tokens: defaults.model_defaults.bootstrap_max_tokens,
            thinking_budget: defaults.model_defaults.thinking_budget,
            chars_per_token: defaults.model_defaults.chars_per_token,
            history_budget_ratio: defaults.history_budget_ratio,
            max_tool_result_bytes: defaults.model_defaults.max_tool_result_bytes,
        },
        capabilities: AgentCapabilities {
            thinking_enabled,
            agency,
            max_tool_iterations,
            cache_enabled: defaults.caching.enabled && defaults.caching.strategy != "disabled",
        },
        workspace,
        private,
        episteme_cohort: Arc::from(episteme_cohort),
        allowed_roots,
        tool_groups,
        tool_allowlist,
        domains,
        recall,
        extraction: config.knowledge.extraction.clone(),
        recall_profile,
        prosoche_model: Arc::from(defaults.model_defaults.prosoche_model.as_str()),
        behavior,
    }
}
