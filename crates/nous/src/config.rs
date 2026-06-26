//! Nous agent configuration.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hermeneus::complexity::ComplexityConfig;
use mneme::knowledge::{EpistemicTier, MemoryScope};
use mneme::workspace::ProjectId;
use serde::{Deserialize, Serialize};
use taxis::config::AgentBehaviorDefaults;
use tracing::warn;

use crate::recall::RecallConfig;

const CONSECUTIVE_MISTAKE_LIMIT_ENV: &str = "KOINA_CONSECUTIVE_MISTAKE_LIMIT";

/// Serde helpers for `Arc<str>`.
mod arc_str {
    use std::sync::Arc;

    use serde::{Deserialize, Serialize};

    pub fn serialize<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        value.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(Arc::from(s))
    }
}

/// LLM generation settings for a nous agent.
// kanon:ignore RUST/no-debug-derive-on-public-types — NousGenerationConfig contains no secrets (model names, token limits, flags)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NousGenerationConfig {
    /// Default model for this agent.
    pub model: String,
    /// Ordered fallback models to try after the primary model fails transiently.
    pub fallback_models: Vec<String>,
    /// Number of primary-model attempts before moving to the fallback chain.
    pub retries_before_fallback: u32,
    /// Maximum context window tokens.
    pub context_window: u32,
    /// Maximum output tokens per turn.
    pub max_output_tokens: u32,
    /// Maximum tokens allocated to bootstrap context.
    pub bootstrap_max_tokens: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Characters per token for conservative token-budget estimation.
    pub chars_per_token: u32,
    /// Model to use for prosoche heartbeat sessions instead of the primary model.
    pub prosoche_model: String,
    /// Complexity-based model routing.
    ///
    /// WHY: when `complexity.enabled == true`, the turn model is chosen per
    /// message by scoring query complexity and mapping to a configured tier
    /// (haiku/sonnet/opus). When `false` (the default), `model` is used for
    /// every turn — preserving existing behaviour.
    pub complexity: ComplexityConfig,
    /// Override for the knowledge extraction model (#3740).
    ///
    /// Extraction and distillation are obvious "fast tier" workloads that
    /// should route to a small model (Qwen3-4B class) on local multi-model
    /// deployments regardless of turn routing. When `None`, extraction falls
    /// back to the turn model — preserving existing behaviour.
    #[serde(default)]
    pub extraction_model: Option<String>,
    /// Override for the context distillation model (#3740).
    ///
    /// See `extraction_model`. Same tier / same fallback shape. When `None`,
    /// distillation falls back to the turn model.
    #[serde(default)]
    pub distillation_model: Option<String>,
}

impl Default for NousGenerationConfig {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            model: koina::models::tier_default(koina::models::ModelTier::Opus).to_owned(),
            fallback_models: Vec::new(),
            retries_before_fallback: 2,
            context_window: d::CONTEXT_TOKENS,
            max_output_tokens: d::MAX_OUTPUT_TOKENS,
            bootstrap_max_tokens: d::BOOTSTRAP_MAX_TOKENS,
            thinking_enabled: false,
            thinking_budget: 10_000,
            chars_per_token: default_chars_per_token(),
            prosoche_model: default_prosoche_model(),
            complexity: ComplexityConfig::default(),
            extraction_model: None,
            distillation_model: None,
        }
    }
}

/// Resource and safety limits for a nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NousLimits {
    /// Maximum tool execution iterations per turn.
    pub max_tool_iterations: u32,
    /// Loop detection threshold (identical tool calls before detection).
    pub loop_detection_threshold: u32,
    /// Consecutive error threshold (same-tool errors before detection).
    pub consecutive_error_threshold: u32,
    /// Maximum loop warnings before escalating to halt.
    pub loop_max_warnings: u32,
    /// Maximum cumulative tokens (input + output) allowed per session.
    ///
    /// Once a session exceeds this budget the guard stage rejects further
    /// turns with a `GuardResult::Rejected` response. Set to `0` to disable
    /// the cap (default: 500,000).
    pub session_token_cap: u64,
    /// Maximum size in bytes for a single tool result before truncation.
    ///
    /// Results exceeding this limit are truncated with an indicator showing
    /// the original and truncated sizes. Set to `0` to disable. Default:
    /// 32 768 bytes (32 KB).
    pub max_tool_result_bytes: u32,
    /// Maximum consecutive LLM iterations that produce only tool calls
    /// without any reasoning text before a think-first prompt is injected.
    ///
    /// WHY: Without this limit, agents can fire long bursts of tool calls
    /// before producing any reasoning, wasting tokens and obscuring intent.
    /// When the limit is hit, a system message is injected asking the agent
    /// to explain its reasoning before making more tool calls. Set to `0` to
    /// disable. Default: 3.
    pub max_consecutive_tool_only_iterations: u32,
    /// Consecutive no-progress turn limit before the mistake brake fires.
    ///
    /// A turn with zero tool calls increments the counter; a turn with any
    /// tool call resets it to zero. When the limit is reached, execution
    /// pauses and the agent asks the operator for intervention. The counter
    /// resets on the next user message.
    ///
    /// Operator-tunable via `KOINA_CONSECUTIVE_MISTAKE_LIMIT` environment
    /// variable. Default: 5.
    pub consecutive_mistake_limit: u32,
    /// Sliding window size for loop-detection history. Default: 50.
    ///
    /// Matches `taxis::config::NousBehaviorConfig::loop_detection_window`.
    pub loop_detection_window: usize,
    /// Maximum cycle length checked during loop detection. Default: 10.
    ///
    /// Matches `taxis::config::NousBehaviorConfig::cycle_detection_max_len`.
    pub cycle_detection_max_len: usize,
}

impl Default for NousLimits {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            max_tool_iterations: d::MAX_TOOL_ITERATIONS,
            loop_detection_threshold: 3,
            consecutive_error_threshold: 4,
            loop_max_warnings: 2,
            session_token_cap: default_session_token_cap(),
            max_tool_result_bytes: default_max_tool_result_bytes(),
            max_consecutive_tool_only_iterations: 3,
            consecutive_mistake_limit: default_consecutive_mistake_limit(),
            loop_detection_window: 50,
            cycle_detection_max_len: 10,
        }
    }
}

/// Named recall behavior profile for a nous agent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecallProfile {
    /// Preserve explicit recall/extraction/pipeline settings.
    #[default]
    Default,
    /// Favor broad project/reference recall for archival work.
    Archival,
    /// Favor identity continuity using late anchors, curated pins, and reflection.
    IdentityContinuity,
}

impl From<taxis::config::RecallProfile> for RecallProfile {
    fn from(value: taxis::config::RecallProfile) -> Self {
        match value {
            taxis::config::RecallProfile::Archival => Self::Archival,
            taxis::config::RecallProfile::IdentityContinuity => Self::IdentityContinuity,
            _ => Self::Default,
        }
    }
}

impl RecallProfile {
    /// Apply this profile to recall, extraction, and pipeline configuration.
    pub fn apply(
        self,
        recall: &mut RecallConfig,
        extraction: &mut mneme::extract::ExtractionConfig,
        pipeline: &mut PipelineConfig,
    ) {
        match self {
            Self::Default => {}
            Self::Archival => {
                recall.iterative = true;
                recall.max_results = recall.max_results.max(12);
                recall.max_recall_tokens = recall.max_recall_tokens.max(4_000);
                recall.scope_quotas =
                    HashMap::from([(MemoryScope::Project, 4), (MemoryScope::Reference, 4)]);
            }
            Self::IdentityContinuity => {
                recall.late_inject_anchor = true;
                recall.max_results = recall.max_results.max(8);
                recall.pinned_facts.truncate(3);
                recall.scope_quotas = HashMap::from([
                    (MemoryScope::User, 3),
                    (MemoryScope::Feedback, 2),
                    (MemoryScope::Project, 1),
                ]);
                extraction.extract_self_facts = false;
                extraction.events_only_prompt = false;
                extraction.default_tier = EpistemicTier::Reflected;
                pipeline.reflection_enabled = true;
            }
        }
    }

    const fn needs_extraction(self) -> bool {
        matches!(self, Self::IdentityContinuity)
    }
}

/// Configuration for a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NousConfig {
    /// Agent identifier (e.g. "syn", "demiurge").
    #[serde(with = "arc_str")]
    pub id: Arc<str>,
    /// Human-readable display name (e.g. "Syn"). Falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// LLM generation settings (model, token limits, thinking).
    #[serde(flatten)]
    pub generation: NousGenerationConfig,
    /// Resource and safety limits.
    #[serde(flatten)]
    pub limits: NousLimits,
    /// Domain tags for this agent (static config + pack overlays).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Whether this agent's workspace is hidden from public discovery.
    #[serde(default)]
    pub private: bool,
    /// Episteme knowledge-store cohort for this agent.
    #[serde(default = "default_episteme_cohort", with = "arc_str")]
    pub episteme_cohort: Arc<str>,
    /// Filesystem workspace used by local tools and hooks.
    #[serde(default = "default_workspace")]
    pub workspace: PathBuf,
    /// Canonical filesystem roots that local tools may access.
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    /// Server-side tools available for lazy activation through `enable_tool`.
    #[serde(default)]
    pub server_tool_config: organon::types::ServerToolConfig,
    /// Server-side tools to include in API requests from the start of a turn.
    #[serde(default)]
    pub server_tools: Vec<hermeneus::types::ServerToolDefinition>,
    /// Whether prompt caching is enabled for this agent.
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    /// Per-agent recall pipeline configuration.
    #[serde(default)]
    pub recall: RecallConfig,
    /// Named recall behavior profile.
    #[serde(default)]
    pub recall_profile: RecallProfile,
    /// Tool allowlist for sub-agent role enforcement.
    ///
    /// When `Some`, only the listed tool names are available during execution.
    /// When `None`, all registered tools are available. Set by role templates
    /// during ephemeral sub-agent spawning.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Tool-group policy for role-based gating.
    ///
    /// Only tools permitted by this policy are visible to the LLM and
    /// executable. Absent or empty configuration resolves to deny-all.
    #[serde(default)]
    pub tool_groups: organon::types::ToolGroupPolicy,
    /// Turn-level hook configuration.
    #[serde(default)]
    pub hooks: HookConfig,
    /// Resolved per-agent behavioral parameters (distillation, competence, drift, etc.).
    ///
    /// Populated at startup from taxis config cascade and passed through the
    /// pipeline for all behavioral threshold reads.
    #[serde(default)]
    pub behavior: AgentBehaviorDefaults,
}

/// Configuration for turn-level behavior hooks.
///
/// Controls which built-in hooks are enabled and their parameters.
/// All hooks are enabled by default.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag toggles an independent hook (cost control, scope, corrections, audit); they are not a state machine"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HookConfig {
    /// Enable the cost control hook (priority 10).
    pub cost_control_enabled: bool,
    /// Maximum tokens allowed per turn before cost control aborts.
    /// 0 = unlimited (default: 0).
    pub turn_token_budget: u64,
    /// Enable the scope enforcement hook (priority 20).
    pub scope_enforcement_enabled: bool,
    /// Enable the correction hooks (injector priority 30, detector priority 90).
    ///
    /// When enabled, operator corrections ("don't", "always", "never", etc.)
    /// are detected, persisted to the agent workspace, and injected into
    /// the system prompt on subsequent turns.
    pub correction_hooks_enabled: bool,
    /// Enable the audit logging hook (priority 100).
    pub audit_logging_enabled: bool,
    /// Enable post-turn self-audit checks.
    pub self_audit_enabled: bool,
    /// Enable the working checkpoint hook (priority 40).
    ///
    /// When enabled, agent-curated `<key_info>` checkpoints written via
    /// `update_working_checkpoint` are injected into the system prompt at
    /// the start of each turn.
    pub working_checkpoint_enabled: bool,
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            cost_control_enabled: true,
            turn_token_budget: 0,
            scope_enforcement_enabled: true,
            correction_hooks_enabled: true,
            audit_logging_enabled: true,
            self_audit_enabled: true,
            working_checkpoint_enabled: true,
        }
    }
}

fn default_cache_enabled() -> bool {
    true
}

fn default_episteme_cohort() -> Arc<str> {
    Arc::from("shared")
}

fn default_session_token_cap() -> u64 {
    500_000
}

fn default_chars_per_token() -> u32 {
    koina::defaults::CHARS_PER_TOKEN
}

/// Default prosoche model: Haiku-tier for cheap heartbeat checks.
fn default_prosoche_model() -> String {
    koina::models::task_role_default(koina::models::TaskRole::Prosoche).to_owned()
}

fn default_max_tool_result_bytes() -> u32 {
    koina::defaults::MAX_TOOL_RESULT_BYTES
}

fn parse_u32_env_override(var_name: &str, raw_value: Option<String>, default: u32) -> u32 {
    let Some(value) = raw_value else {
        return default;
    };

    match value.parse::<u32>() {
        Ok(parsed) => parsed,
        Err(error) => {
            warn!(
                env_var = var_name,
                value = %value,
                default,
                error = %error,
                "invalid numeric environment override, using default"
            );
            default
        }
    }
}

fn resolve_consecutive_mistake_limit(raw: Option<&str>) -> u32 {
    const DEFAULT: u32 = koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT;
    parse_u32_env_override(
        CONSECUTIVE_MISTAKE_LIMIT_ENV,
        raw.map(str::to_owned),
        DEFAULT,
    )
}

fn default_consecutive_mistake_limit() -> u32 {
    resolve_consecutive_mistake_limit(std::env::var(CONSECUTIVE_MISTAKE_LIMIT_ENV).ok().as_deref())
}

impl Default for NousConfig {
    fn default() -> Self {
        Self {
            id: Arc::from("default"),
            name: None,
            generation: NousGenerationConfig::default(),
            limits: NousLimits::default(),
            domains: Vec::new(),
            private: false,
            episteme_cohort: default_episteme_cohort(),
            workspace: default_workspace(),
            allowed_roots: Vec::new(),
            server_tool_config: organon::types::ServerToolConfig::default(),
            server_tools: Vec::new(),
            cache_enabled: true,
            recall: RecallConfig::default(),
            recall_profile: RecallProfile::Default,
            tool_allowlist: None,
            tool_groups: organon::types::ToolGroupPolicy::DenyAll,
            hooks: HookConfig::default(),
            behavior: AgentBehaviorDefaults::default(),
        }
    }
}

fn default_workspace() -> PathBuf {
    PathBuf::from(".")
}

impl NousConfig {
    /// Apply the configured recall profile to this agent's pipeline settings.
    pub fn apply_recall_profile(&mut self, pipeline: &mut PipelineConfig) {
        let had_extraction = pipeline.extraction.is_some();
        let mut extraction;
        if let Some(config) = pipeline.extraction.take() {
            extraction = config;
        } else {
            extraction = mneme::extract::ExtractionConfig::default();
        }
        self.recall_profile
            .apply(&mut self.recall, &mut extraction, pipeline);
        if had_extraction || self.recall_profile.needs_extraction() {
            pipeline.extraction = Some(extraction);
        }
    }
}

/// Turn-history loading policy.
///
/// Determines how many recent messages are loaded into the turn context,
/// how many tokens are reserved for the current user message, and whether
/// tool-result messages are included.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only operator-owned policy knobs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurnHistoryPolicy {
    /// Maximum number of history messages to load.
    pub max_messages: usize,
    /// Reserve tokens for the user's current message.
    pub reserve_for_current: i64,
    /// Whether to include tool-result messages.
    pub include_tool_messages: bool,
}

impl Default for TurnHistoryPolicy {
    fn default() -> Self {
        Self {
            max_messages: 50,
            reserve_for_current: 4000,
            include_tool_messages: true,
        }
    }
}

/// Pipeline configuration: controls stage behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    /// Token budget for history (remaining after bootstrap).
    pub history_budget_ratio: f64,
    /// Git-remote-derived project partition for behavioral observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    /// Knowledge extraction configuration (None = disabled).
    #[serde(default)]
    pub extraction: Option<mneme::extract::ExtractionConfig>,
    /// Per-stage time budgets.
    #[serde(default)]
    pub stage_budget: StageBudget,
    /// Training data capture configuration.
    #[serde(default)]
    pub training: crate::training::TrainingConfig,
    /// Whether the reflection stage runs after finalize.
    #[serde(default)]
    pub reflection_enabled: bool,
    /// Turn-history loading policy.
    #[serde(default)]
    pub history: TurnHistoryPolicy,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            history_budget_ratio: 0.6,
            project_id: None,
            extraction: None,
            stage_budget: StageBudget::default(),
            training: crate::training::TrainingConfig::default(),
            reflection_enabled: false,
            history: TurnHistoryPolicy::default(),
        }
    }
}

/// Per-stage time budget configuration (seconds).
///
/// Each field is a maximum wall-clock duration for that pipeline stage.
/// `total_secs` is a hard cap: if elapsed time exceeds it, remaining
/// stages are skipped and a partial result is returned.
/// A value of 0 means no limit for that stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageBudget {
    /// Context assembly stage limit.
    pub context_secs: u32,
    /// Semantic recall stage limit.
    pub recall_secs: u32,
    /// History retrieval stage limit.
    pub history_secs: u32,
    /// Guard evaluation stage limit.
    pub guard_secs: u32,
    /// LLM execution stage limit (0 = unlimited, provider controls timeout).
    pub execute_secs: u32,
    /// Finalization stage limit.
    pub finalize_secs: u32,
    /// Reflection stage limit.
    #[serde(default = "default_reflection_secs")]
    pub reflection_secs: u32,
    /// Hard cap on total pipeline wall-clock time.
    pub total_secs: u32,
}

impl Default for StageBudget {
    fn default() -> Self {
        Self {
            context_secs: 10,
            recall_secs: 15,
            history_secs: 5,
            guard_secs: 2,
            execute_secs: 0,
            finalize_secs: 10,
            reflection_secs: default_reflection_secs(),
            total_secs: 300,
        }
    }
}

const fn default_reflection_secs() -> u32 {
    30
}
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test assertions may panic on failure"
)]
mod tests {
    use super::*;

    #[test]
    fn nous_config_defaults() {
        let config = NousConfig::default();
        assert_eq!(config.generation.context_window, 200_000);
        assert_eq!(
            config.limits.max_tool_iterations,
            koina::defaults::MAX_TOOL_ITERATIONS,
            "default should match koina::defaults"
        );
        assert!(!config.generation.thinking_enabled);
        assert_eq!(
            config.limits.max_consecutive_tool_only_iterations, 3,
            "default tool-only iteration limit should be 3"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn malformed_consecutive_mistake_limit_warns_and_falls_back() {
        let default = koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT;

        for value in ["abc", "-1"] {
            let parsed = parse_u32_env_override(
                CONSECUTIVE_MISTAKE_LIMIT_ENV,
                Some(value.to_owned()),
                default,
            );
            assert_eq!(
                parsed, default,
                "malformed override {value:?} must fall back to default"
            );
        }

        assert!(logs_contain(CONSECUTIVE_MISTAKE_LIMIT_ENV));
        assert!(logs_contain("abc"));
        assert!(logs_contain("-1"));
        assert!(logs_contain(&default.to_string()));
        assert!(logs_contain(
            "invalid numeric environment override, using default"
        ));
    }

    #[test]
    fn pipeline_config_defaults() {
        let config = PipelineConfig::default();
        assert!((config.history_budget_ratio - 0.6).abs() < f64::EPSILON);
        assert!(config.extraction.is_none());
        assert!(!config.training.enabled);
        assert_eq!(config.training.path, "data/training");
        assert!(
            !config.reflection_enabled,
            "reflection should be disabled by default"
        );
        assert_eq!(config.history.max_messages, 50);
        assert_eq!(config.history.reserve_for_current, 4000);
        assert!(config.history.include_tool_messages);
    }

    #[test]
    fn identity_continuity_profile_applies_recall_extraction_and_reflection_knobs() {
        let mut recall = RecallConfig {
            pinned_facts: vec![
                mneme::id::FactId::new("identity-pin-1").expect("valid"),
                mneme::id::FactId::new("identity-pin-2").expect("valid"),
                mneme::id::FactId::new("identity-pin-3").expect("valid"),
                mneme::id::FactId::new("identity-pin-4").expect("valid"),
            ],
            ..RecallConfig::default()
        };
        let mut extraction = mneme::extract::ExtractionConfig::default();
        let mut pipeline = PipelineConfig::default();

        RecallProfile::IdentityContinuity.apply(&mut recall, &mut extraction, &mut pipeline);

        assert!(recall.late_inject_anchor);
        assert_eq!(
            recall.pinned_facts.len(),
            3,
            "identity profile should use the operator-curated top three pins"
        );
        assert_eq!(recall.scope_quotas.get(&MemoryScope::User), Some(&3));
        assert_eq!(recall.scope_quotas.get(&MemoryScope::Feedback), Some(&2));
        assert!(!extraction.extract_self_facts);
        assert_eq!(extraction.default_tier, EpistemicTier::Reflected);
        assert!(pipeline.reflection_enabled);
    }

    #[test]
    fn apply_recall_profile_preserves_absent_extraction_for_default_profile() {
        let mut config = NousConfig::default();
        let mut pipeline = PipelineConfig::default();

        config.apply_recall_profile(&mut pipeline);

        assert!(pipeline.extraction.is_none());
        assert!(!pipeline.reflection_enabled);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = NousConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: NousConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.id.as_ref(), back.id.as_ref());
        assert_eq!(config.generation.model, back.generation.model);
    }

    #[test]
    fn pipeline_config_serde_roundtrip() {
        let config = PipelineConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: PipelineConfig = serde_json::from_str(&json).unwrap();
        assert!((back.history_budget_ratio - 0.6).abs() < f64::EPSILON);
        assert!(back.extraction.is_none());
        assert!(!back.training.enabled);
        assert!(
            !back.reflection_enabled,
            "serde roundtrip should preserve reflection_enabled=false"
        );
    }

    #[test]
    fn stage_budget_defaults() {
        let budget = StageBudget::default();
        assert_eq!(budget.context_secs, 10);
        assert_eq!(budget.recall_secs, 15);
        assert_eq!(
            budget.reflection_secs, 30,
            "default reflection budget should be 30s"
        );
        assert_eq!(budget.execute_secs, 0);
        assert_eq!(budget.total_secs, 300);
    }

    #[test]
    fn nous_config_custom_values() {
        let config = NousConfig {
            id: Arc::from("analyst"),
            name: Some("Analyst".to_owned()),
            generation: NousGenerationConfig {
                model: koina::models::tier_default(koina::models::ModelTier::Haiku).to_owned(),
                fallback_models: Vec::new(),
                retries_before_fallback: 2,
                context_window: 100_000,
                max_output_tokens: 8_192,
                bootstrap_max_tokens: 20_000,
                thinking_enabled: true,
                thinking_budget: 5_000,
                chars_per_token: koina::defaults::CHARS_PER_TOKEN,
                prosoche_model: koina::models::task_role_default(koina::models::TaskRole::Prosoche)
                    .to_owned(),
                complexity: ComplexityConfig::default(),
                extraction_model: None,
                distillation_model: None,
            },
            limits: NousLimits {
                max_tool_iterations: 10,
                loop_detection_threshold: 5,
                consecutive_error_threshold: 4,
                loop_max_warnings: 2,
                session_token_cap: 250_000,
                max_tool_result_bytes: 32_768,
                max_consecutive_tool_only_iterations: 3,
                consecutive_mistake_limit: 5,
                loop_detection_window: 50,
                cycle_detection_max_len: 10,
            },
            domains: vec!["medical".to_owned()],
            private: true,
            episteme_cohort: std::sync::Arc::from("shared"),
            workspace: std::path::PathBuf::from("/tmp/analyst"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            server_tool_config: organon::types::ServerToolConfig::default(),
            server_tools: Vec::new(),
            cache_enabled: false,
            recall: RecallConfig::default(),
            recall_profile: RecallProfile::Default,
            tool_allowlist: None,
            tool_groups: organon::types::ToolGroupPolicy::DenyAll,
            hooks: HookConfig::default(),
            behavior: AgentBehaviorDefaults::default(),
        };
        assert_eq!(config.name.as_deref(), Some("Analyst"));
        assert!(config.generation.thinking_enabled);
        assert_eq!(config.domains.len(), 1);
        assert!(config.private);
        assert!(!config.cache_enabled);
    }

    #[test]
    fn prosoche_model_defaults_to_haiku() {
        let config = NousConfig::default();
        assert_eq!(
            config.generation.prosoche_model,
            koina::models::task_role_default(koina::models::TaskRole::Prosoche)
        );
    }

    #[test]
    fn consecutive_mistake_limit_env_override_honored_when_valid() {
        assert_eq!(resolve_consecutive_mistake_limit(Some("7")), 7);
        assert_eq!(resolve_consecutive_mistake_limit(Some("0")), 0);
    }

    #[test]
    fn consecutive_mistake_limit_env_override_falls_back_when_malformed() {
        let default = koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT;
        assert_eq!(resolve_consecutive_mistake_limit(None), default);
        assert_eq!(resolve_consecutive_mistake_limit(Some("")), default);
        assert_eq!(
            resolve_consecutive_mistake_limit(Some("not-a-number")),
            default
        );
        assert_eq!(resolve_consecutive_mistake_limit(Some("-3")), default);
        assert_eq!(resolve_consecutive_mistake_limit(Some(" 5 ")), default);
    }
}
