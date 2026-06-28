//! Agent configuration types.

use std::collections::HashMap;

use eidos::id::FactId;
use eidos::knowledge::MemoryScope;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq as _, Serializer};
use serde::{Deserialize, Serialize};

use super::{AgencyLevel, BookkeepingProviderKind, CompactionStrategyKind};

/// Default value used for `AgentBehaviorDefaults::tool_agent_dispatch_max_tasks`.
pub(crate) const DEFAULT_TOOL_AGENT_DISPATCH_MAX_TASKS: usize = 10;
/// Default value used for `AgentBehaviorDefaults::tool_datalog_default_row_limit`.
pub(crate) const DEFAULT_TOOL_DATALOG_DEFAULT_ROW_LIMIT: usize = 100;
/// Default value used for `AgentBehaviorDefaults::tool_datalog_default_timeout_secs`.
pub(crate) const DEFAULT_TOOL_DATALOG_DEFAULT_TIMEOUT_SECS: f64 = 5.0;
/// Default value used for `AgentBehaviorDefaults::tool_max_image_bytes`.
pub(crate) const DEFAULT_TOOL_MAX_IMAGE_BYTES: u64 = 20_971_520;
/// Default value used for `AgentBehaviorDefaults::tool_max_pdf_bytes`.
pub(crate) const DEFAULT_TOOL_MAX_PDF_BYTES: u64 = 33_554_432;

/// Agent configuration: shared defaults and per-agent definitions.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    /// Shared defaults applied to every agent unless overridden per-agent.
    pub defaults: AgentDefaults,
    /// Individual agent definitions; merged with `defaults` at resolution time.
    pub list: Vec<NousDefinition>,
}

/// Tool-group policy configured for a nous agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AgentToolGroupPolicy {
    /// All tool groups are permitted.
    AllowAll,
    /// Only tools with one of these groups are permitted.
    Groups(Vec<String>),
    /// No tool groups are permitted.
    #[default]
    DenyAll,
}

impl Serialize for AgentToolGroupPolicy {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::AllowAll => serializer.serialize_str("all"),
            Self::DenyAll => serializer.serialize_str("deny"),
            Self::Groups(groups) => {
                let mut seq = serializer.serialize_seq(Some(groups.len()))?;
                for group in groups {
                    seq.serialize_element(group)?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for AgentToolGroupPolicy {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PolicyVisitor;

        impl<'de> Visitor<'de> for PolicyVisitor {
            type Value = AgentToolGroupPolicy;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(r#""all", "deny", or a list of tool group names"#)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "all" => Ok(AgentToolGroupPolicy::AllowAll),
                    "deny" => Ok(AgentToolGroupPolicy::DenyAll),
                    other => Err(E::custom(format!(
                        "unknown tool group policy {other:?}; expected \"all\", \"deny\", or a list"
                    ))),
                }
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut groups = Vec::new();
                while let Some(group) = seq.next_element()? {
                    groups.push(group);
                }
                if groups.is_empty() {
                    Ok(AgentToolGroupPolicy::DenyAll)
                } else {
                    Ok(AgentToolGroupPolicy::Groups(groups))
                }
            }
        }

        deserializer.deserialize_any(PolicyVisitor)
    }
}

/// Per-factor scoring weights for the recall pipeline.
///
/// Lives in taxis so operators can tune them per-agent via TOML without
/// creating a taxis → nous dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecallWeights {
    /// Temporal decay weight (0.0--1.0).
    pub decay: f64,
    /// Content relevance weight (0.0--1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0--1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0--1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0--1.0).
    pub access_frequency: f64,
    /// Graph `PageRank` importance weight (0.0--1.0).
    pub graph_importance: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.3,
            relationship_proximity: 0.1,
            access_frequency: 0.0,
            graph_importance: 0.0,
        }
    }
}

/// Recall pipeline settings for a nous agent.
///
/// Resolved from taxis config and forwarded to the recall stage via
/// `NousConfig::recall` at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "recall controls are independent operator knobs (enabled, iterative, inject_metadata, late_inject_anchor); not a state machine"
)]
// kanon:ignore RUST/pub-visibility — consumed by sibling crates (nous, pylon, daemon)
pub struct RecallSettings {
    /// Whether semantic recall is enabled for this agent.
    pub enabled: bool,
    /// Maximum number of recalled facts to inject per turn.
    pub max_results: usize,
    /// Minimum relevance score (0.0--1.0) to include a recalled fact.
    pub min_score: f64,
    /// Maximum tokens to allocate for recalled knowledge.
    pub max_recall_tokens: u64,
    /// Enable iterative two-cycle retrieval with terminology discovery.
    pub iterative: bool,
    /// Maximum retrieval cycles when iterative mode is enabled.
    pub max_cycles: usize,
    /// Per-factor scoring weights (factor scores for non-vector signals).
    pub weights: RecallWeights,
    /// Inject factor metadata into recalled knowledge prompts.
    ///
    /// When enabled, each recalled fact includes its factor scores
    /// (vector similarity, decay, relevance, epistemic tier, relationship
    /// proximity, access frequency) so the LLM can weight its reasoning
    /// by provenance quality. Default: false.
    pub inject_metadata: bool,
    /// Fact IDs that should be recalled first when they appear in candidates.
    ///
    /// Pinned facts bypass the `max_results` budget and are slotted before
    /// non-pinned results, but they still count against the token budget.
    #[serde(default)]
    pub pinned_facts: Vec<FactId>,
    /// When true, append recalled knowledge as a system message at the end of
    /// the conversation context instead of injecting it into the system prompt.
    #[serde(default)]
    pub late_inject_anchor: bool,
    /// Per-scope minimum result counts with slack-fill.
    ///
    /// Guarantees fair representation across memory scopes regardless of pure
    /// score ranking. Unused quota from one scope is redistributed (slack-fill)
    /// to others. Default: empty (no quota enforcement).
    #[serde(default)]
    pub scope_quotas: HashMap<MemoryScope, usize>,
    /// URL for an HTTP cross-encoder reranker.
    ///
    /// When set, recall candidates are forwarded to this endpoint for
    /// model-based rescoring. When `None` or when the `reranker` feature is
    /// not enabled, the pipeline falls back to baseline ranking.
    #[serde(default)]
    pub reranker_url: Option<String>,
    /// Filesystem path to a local ONNX cross-encoder model for in-process reranking.
    ///
    /// Reserved for local reranker wiring. Current recall scoring preserves
    /// this value in config, but only the HTTP reranker URL is active.
    /// Default: `None`.
    #[serde(default)]
    pub reranker_model_path: Option<String>,
}

/// Named recall behavior profile for a nous agent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum RecallProfile {
    /// Preserve the explicit recall/extraction/pipeline settings.
    #[default]
    Default,
    /// Favor broad project/reference recall for archival work.
    Archival,
    /// Favor stable identity continuity across turns and sessions.
    IdentityContinuity,
}

impl Default for RecallSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_results: 5,
            min_score: 0.3,
            max_recall_tokens: 2000,
            iterative: false,
            max_cycles: 2,
            weights: RecallWeights::default(),
            inject_metadata: false,
            pinned_facts: Vec::new(),
            late_inject_anchor: false,
            scope_quotas: HashMap::new(),
            reranker_url: None,
            reranker_model_path: None,
        }
    }
}

/// LLM model and generation defaults for agents.
#[derive(Debug, Clone, Serialize, Deserialize)] // kanon:ignore RUST/no-debug-derive-on-public-types
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct AgentModelDefaults {
    /// Primary model and fallback chain.
    pub model: ModelSpec,
    /// Maximum input context window size in tokens.
    pub context_tokens: u32,
    /// Maximum tokens the model may generate per response.
    pub max_output_tokens: u32,
    /// Token budget for bootstrap (system prompt + persona) content.
    pub bootstrap_max_tokens: u32,
    /// Whether extended thinking is enabled by default.
    pub thinking_enabled: bool,
    /// Maximum tokens allocated to extended thinking when enabled.
    pub thinking_budget: u32,
    /// Characters per token for conservative token-budget estimation.
    pub chars_per_token: u32,
    /// Model used for prosoche heartbeat sessions.
    pub prosoche_model: String,
    /// Maximum size in bytes for a single tool result before truncation.
    pub max_tool_result_bytes: u32,
}

impl Default for AgentModelDefaults {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            model: ModelSpec::default(),
            context_tokens: d::CONTEXT_TOKENS,
            max_output_tokens: d::MAX_OUTPUT_TOKENS,
            bootstrap_max_tokens: d::BOOTSTRAP_MAX_TOKENS,
            thinking_enabled: false,
            thinking_budget: 10_000,
            chars_per_token: d::CHARS_PER_TOKEN,
            prosoche_model: koina::models::task_role_default(koina::models::TaskRole::Prosoche)
                .to_owned(),
            max_tool_result_bytes: d::MAX_TOOL_RESULT_BYTES,
        }
    }
}

/// Default values applied to every agent unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct AgentDefaults {
    /// Model and generation settings.
    #[serde(flatten)]
    pub model_defaults: AgentModelDefaults,
    /// Agent autonomy level. Controls effective tool iteration limits when
    /// `max_tool_iterations` is not explicitly overridden per-agent.
    pub agency: AgencyLevel,
    /// Safety limit on consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Filesystem paths the agent is permitted to access.
    pub allowed_roots: Vec<String>,
    /// Default tool-group policy for agents. Missing or empty configuration denies all groups.
    pub tool_groups: AgentToolGroupPolicy,
    /// Prompt caching configuration.
    pub caching: CachingConfig,
    /// Recall pipeline settings applied to all agents unless overridden.
    pub recall: RecallSettings,
    /// Fraction of the context window reserved for conversation history.
    pub history_budget_ratio: f64,
    /// Default per-agent behavioral parameters (safety, hooks, distillation, etc.).
    pub behavior: AgentBehaviorDefaults,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            model_defaults: AgentModelDefaults::default(),
            agency: AgencyLevel::Standard,
            max_tool_iterations: d::MAX_TOOL_ITERATIONS,
            allowed_roots: Vec::new(),
            tool_groups: AgentToolGroupPolicy::DenyAll,
            caching: CachingConfig::default(),
            recall: RecallSettings::default(),
            history_budget_ratio: d::HISTORY_BUDGET_RATIO,
            behavior: AgentBehaviorDefaults::default(),
        }
    }
}

/// Model specification with primary model and fallbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ModelSpec {
    /// Primary model identifier (e.g. `claude-sonnet-4-6`).
    pub primary: String,
    /// Ordered fallback models tried when the primary is unavailable.
    pub fallbacks: Vec<String>,
    /// How many times to retry the primary model before trying the next fallback.
    pub retries_before_fallback: u32,
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            primary: koina::defaults::DEFAULT_MODEL.to_owned(),
            fallbacks: Vec::new(),
            retries_before_fallback: 2,
        }
    }
}

/// Prompt caching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CachingConfig {
    /// Whether prompt caching is enabled.
    pub enabled: bool,
    /// Caching strategy: `"auto"` or `"disabled"`.
    pub strategy: String,
}

impl Default for CachingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: "auto".to_owned(),
        }
    }
}

/// Definition of a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct NousDefinition {
    /// Unique agent identifier (matches the `nous/{id}/` directory name).
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire/serde config field: id maps to the agent's directory name in TOML, not a runtime domain identifier
    /// Human-readable display name.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether the agent is enabled in the operator surface.
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,
    /// Model override; when `None`, inherits from `AgentDefaults`.
    #[serde(default)]
    pub model: Option<ModelSpec>,
    /// Filesystem path to the agent's workspace directory.
    pub workspace: String,
    /// Thinking override; when `None`, inherits from `AgentDefaults`.
    #[serde(default)]
    pub thinking_enabled: Option<bool>,
    /// Agency level override; when `None`, inherits from [`AgentDefaults::agency`].
    #[serde(default)]
    pub agency: Option<AgencyLevel>,
    /// Additional filesystem roots this agent may access (merged with defaults).
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent specializes in (e.g. `"code"`, `"research"`).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Whether this is the default agent for unrouted messages.
    #[serde(default)]
    pub default: bool,
    /// Whether this agent's workspace is hidden from public discovery.
    #[serde(default)]
    pub private: bool,
    /// Episteme knowledge-store cohort for this agent.
    #[serde(default)]
    pub episteme_cohort: Option<String>,
    /// Recall pipeline override; when `None`, inherits from [`AgentDefaults::recall`].
    #[serde(default)]
    pub recall: Option<RecallSettings>,
    /// Tool allowlist override; when `None`, all tools are enabled.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Tool-group policy override; when `None`, inherits from [`AgentDefaults::tool_groups`].
    #[serde(default)]
    pub tool_groups: Option<AgentToolGroupPolicy>,
    /// Named recall behavior profile; when `None`, resolves to [`RecallProfile::Default`].
    #[serde(default)]
    pub recall_profile: Option<RecallProfile>,
    /// Per-agent behavioral override; when `None`, inherits from [`AgentDefaults::behavior`].
    #[serde(default)]
    pub behavior: Option<AgentBehaviorDefaults>,
}

impl Default for NousDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: None,
            enabled: true,
            model: None,
            workspace: String::new(),
            thinking_enabled: None,
            agency: None,
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
            private: false,
            episteme_cohort: None,
            recall: None,
            tool_allowlist: None,
            tool_groups: None,
            recall_profile: None,
            behavior: None,
        }
    }
}

fn default_agent_enabled() -> bool {
    true
}

/// Per-agent behavioral parameters: safety, hooks, distillation, competence,
/// drift, uncertainty, skills, planning, knowledge tuning, fact lifecycle,
/// similarity, tool behavior, and correction limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "hook toggles are a genuine set of independent feature flags, not a state machine"
)]
#[rustfmt::skip]
// kanon:ignore RUST/pub-visibility — consumed by sibling crates (nous, pylon, daemon, dianoia)
pub struct AgentBehaviorDefaults { // kanon:ignore RUST/struct-too-many-fields — many independent deployment knobs; splitting would fragment config locality
    // --- Safety ---
    /// Consecutive identical tool-call sequences before loop detection fires. Default: 3.
    pub safety_loop_detection_threshold: u32,
    /// Consecutive errors before the pipeline aborts with a safety interrupt. Default: 4.
    pub safety_consecutive_error_threshold: u32,
    /// Maximum loop-detection warnings before the session is halted. Default: 2.
    pub safety_loop_max_warnings: u32,
    /// Hard token cap for a single session. Default: 500000.
    pub safety_session_token_cap: u64,
    /// Maximum consecutive tool-only iterations before forcing a text response. Default: 3.
    pub safety_max_consecutive_tool_only_iterations: u32,

    // --- Hooks ---
    /// Whether cost-control hooks are active. Default: true.
    pub hooks_cost_control_enabled: bool,
    /// Per-turn token budget (0 = unlimited). Default: 0.
    pub hooks_turn_token_budget: u64,
    /// Whether scope-enforcement hooks are active. Default: true.
    pub hooks_scope_enforcement_enabled: bool,
    /// Whether correction hooks are active. Default: true.
    pub hooks_correction_hooks_enabled: bool,
    /// Whether audit logging hooks are active. Default: true.
    pub hooks_audit_logging_enabled: bool,

    // --- Distillation ---
    /// Context token count that triggers automatic distillation. Default: 120000.
    pub distillation_context_token_trigger: u64,
    /// Message count that triggers distillation. Default: 150.
    pub distillation_message_count_trigger: u64,
    /// Days idle before a session is considered stale for distillation. Default: 7.
    pub distillation_stale_session_days: u64,
    /// Minimum messages required for stale-session distillation. Default: 20.
    pub distillation_stale_min_messages: u64,
    /// Message count trigger for sessions never distilled. Default: 30.
    pub distillation_never_distilled_trigger: u64,
    /// Minimum messages for legacy distillation threshold. Default: 10.
    pub distillation_legacy_min_messages: u64,
    /// Maximum backoff turns before distillation is forced. Default: 8.
    pub distillation_max_backoff_turns: u32,

    // --- Competence scoring ---
    /// Competence score penalty per correction. Default: 0.05.
    pub competence_correction_penalty: f64,
    /// Competence score bonus per successful turn. Default: 0.02.
    pub competence_success_bonus: f64,
    /// Competence score penalty per user disagreement. Default: 0.01.
    pub competence_disagreement_penalty: f64,
    /// Competence score floor. Default: 0.1.
    pub competence_min_score: f64,
    /// Competence score ceiling. Default: 0.95.
    pub competence_max_score: f64,
    /// Initial competence score for a new agent. Default: 0.5.
    pub competence_default_score: f64,
    /// Competence score below which escalation fires. Default: 0.30.
    pub competence_escalation_failure_threshold: f64,
    /// Minimum samples before escalation threshold is evaluated. Default: 5.
    pub competence_escalation_min_samples: u32,

    // --- Drift detection ---
    /// Sliding window size for response-quality drift detection. Default: 20.
    pub drift_window_size: usize,
    /// Comparison window for recent vs. historical drift. Default: 5.
    pub drift_recent_size: usize,
    /// Standard deviations required to flag drift. Default: 2.0.
    pub drift_deviation_threshold: f64,
    /// Minimum samples before drift detection activates. Default: 8.
    pub drift_min_samples: usize,

    // --- Uncertainty calibration ---
    /// Maximum calibration data points retained for the uncertainty model. Default: 1000.
    pub uncertainty_max_calibration_points: usize,

    // --- Manifest ---
    /// Maximum memory entries in a single manifest for side-query pre-filtering.
    pub manifest_max_entries: usize,

    // --- Skills ---
    /// Maximum number of skills loadable per agent. Default: 5.
    pub skills_max_skills: usize,
    /// Maximum chars from context used when matching skills. Default: 200.
    pub skills_max_context_chars: usize,

    // --- Working state ---
    /// Working-state TTL in seconds before expiry. Default: 604800 (7 days).
    pub working_state_ttl_secs: u64,
    /// Maximum task stack depth before oldest entries are evicted. Default: 10.
    pub working_state_max_task_stack: usize,

    // --- Planning ---
    /// Maximum planning iterations per planning cycle.
    pub planning_max_iterations: u32,
    /// History turns inspected for stuck-detection. Default: 20.
    pub planning_stuck_history_window: u32,
    /// Repeated errors before agent is flagged stuck. Default: 3.
    pub planning_stuck_repeated_error_threshold: u32,
    /// Identical-argument tool calls before stuck detection fires. Default: 3.
    pub planning_stuck_same_args_threshold: u32,
    /// Alternating tool-call pairs before stuck detection fires. Default: 3.
    pub planning_stuck_alternating_threshold: u32,
    /// Escalating retry pattern depth before stuck detection fires. Default: 3.
    pub planning_stuck_escalating_retry_threshold: u32,
    /// Seconds of timestamp difference treated as "in sync" during reconciliation. Default: 5.
    pub planning_reconciler_timestamp_tolerance_secs: i64,

    // --- Knowledge tuning (instinct / surprise / rules / dedup) ---
    /// Minimum observations before an instinct is eligible. Default: 5.
    pub knowledge_instinct_min_observations: u32,
    /// Minimum success rate for an instinct to surface. Default: 0.80.
    pub knowledge_instinct_min_success_rate: f64,
    /// Minimum stability hours before an instinct is surfaced. Default: 168.0.
    pub knowledge_instinct_stability_hours: f64,
    /// Standard deviations above baseline for surprise detection.
    pub knowledge_surprise_threshold: f64,
    /// EMA alpha for surprise baseline. Default: 0.3.
    pub knowledge_surprise_ema_alpha: f64,
    /// Minimum observations before a rule proposal is eligible. Default: 5.
    pub knowledge_rule_min_observations: u32,
    /// Minimum confidence for a rule proposal to surface. Default: 0.60.
    pub knowledge_rule_min_confidence: f64,
    /// Weight of name similarity in dedup scoring. Default: 0.4.
    pub knowledge_dedup_weight_name: f64,
    /// Weight of embedding similarity in dedup scoring. Default: 0.3.
    pub knowledge_dedup_weight_embed: f64,
    /// Weight of fact-type match in dedup scoring. Default: 0.2.
    pub knowledge_dedup_weight_type: f64,
    /// Weight of alias similarity in dedup scoring. Default: 0.1.
    pub knowledge_dedup_weight_alias: f64,
    /// Jaro-Winkler score above which strings are considered similar. Default: 0.85.
    pub knowledge_dedup_jw_threshold: f64,
    /// Cosine similarity above which embeddings are considered similar. Default: 0.80.
    pub knowledge_dedup_embed_threshold: f64,
    /// Bookkeeping provider selected for extraction. Default: `llm`.
    pub knowledge_extraction_provider: BookkeepingProviderKind,
    // --- Compaction ---
    /// Preserved-tail compaction strategy used during full context compaction.
    pub compaction_strategy: CompactionStrategyKind,

    // --- Fact lifecycle ---
    /// Confidence above which a fact is considered Active. Default: 0.7.
    pub fact_active_threshold: f64,
    /// Confidence below which a fact is considered Fading. Default: 0.3.
    pub fact_fading_threshold: f64,
    /// Confidence below which a fact is considered Dormant. Default: 0.1.
    pub fact_dormant_threshold: f64,

    // --- Similarity ---
    /// Similarity score threshold for recall deduplication. Default: 0.85.
    pub similarity_threshold: f64,
    /// Minimum token length to include in Jaccard similarity comparison. Default: 3.
    pub similarity_min_token_len: usize,

    // --- Distillation prompt ---
    /// Maximum character length for truncated tool results in distillation prompts. Default: 500.
    pub distillation_max_tool_result_len: usize,

    // --- Auto-dream consolidation ---
    /// Minimum hours between auto-dream consolidation runs.
    pub dream_min_hours: u64,
    /// Minimum sessions required to trigger auto-dream consolidation.
    pub dream_min_sessions: usize,
    /// Session scan throttle interval in seconds. Default: 600.
    pub dream_scan_throttle_secs: i64,
    /// Stale lock threshold in seconds for auto-dream.
    pub dream_stale_threshold_secs: i64,

    // --- Tool behavior ---
    /// Maximum concurrent agent-dispatch tasks.
    pub tool_agent_dispatch_max_tasks: usize,
    /// Default row limit for Datalog memory queries.
    pub tool_datalog_default_row_limit: usize,
    /// Default query timeout in seconds for the Datalog memory tool.
    pub tool_datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool.
    pub tool_max_image_bytes: u64,
    /// Maximum PDF file size in bytes for the view-file tool.
    pub tool_max_pdf_bytes: u64,

    // --- Bootstrap ---
    /// Minimum token budget remaining before attempting section truncation.
    /// Below this threshold the section is dropped rather than truncated. Default: 200.
    pub bootstrap_min_truncation_budget: u64,

    // --- Corrections ---
    /// Maximum correction entries stored per agent. Default: 50.
    pub corrections_max_corrections: usize,

    // --- Self-tuning ---
    /// Whether this agent participates in the self-tuning loop. Default: true.
    ///
    /// When false, the agent's metrics are collected but no proposals are
    /// generated. Combined with the global `TuningConfig::enabled` kill switch.
    pub tuning_eligible: bool,
}

impl Default for AgentBehaviorDefaults {
    fn default() -> Self {
        Self {
            // Safety
            safety_loop_detection_threshold: 3,
            safety_consecutive_error_threshold: 4,
            safety_loop_max_warnings: 2,
            safety_session_token_cap: 500_000,
            safety_max_consecutive_tool_only_iterations: 3,
            // Hooks
            hooks_cost_control_enabled: true,
            hooks_turn_token_budget: 0,
            hooks_scope_enforcement_enabled: true,
            hooks_correction_hooks_enabled: true,
            hooks_audit_logging_enabled: true,
            // Distillation
            distillation_context_token_trigger: 120_000,
            distillation_message_count_trigger: 150,
            distillation_stale_session_days: 7,
            distillation_stale_min_messages: 20,
            distillation_never_distilled_trigger: 30,
            distillation_legacy_min_messages: 10,
            distillation_max_backoff_turns: 8,
            // Competence
            competence_correction_penalty: 0.05,
            competence_success_bonus: 0.02,
            competence_disagreement_penalty: 0.01,
            competence_min_score: 0.1,
            competence_max_score: 0.95,
            competence_default_score: 0.5,
            competence_escalation_failure_threshold: 0.30,
            competence_escalation_min_samples: 5,
            // Drift
            drift_window_size: 20,
            drift_recent_size: 5,
            drift_deviation_threshold: 2.0,
            drift_min_samples: 8,
            // Uncertainty
            uncertainty_max_calibration_points: 1_000,
            // Manifest
            manifest_max_entries: episteme::manifest::MAX_MEMORY_ENTRIES,
            // Skills
            skills_max_skills: 5,
            skills_max_context_chars: 200,
            // Working state
            working_state_ttl_secs: 604_800,
            working_state_max_task_stack: 10,
            // Planning
            planning_max_iterations: dianoia::plan::DEFAULT_MAX_ITERATIONS,
            planning_stuck_history_window: 20,
            planning_stuck_repeated_error_threshold: 3,
            planning_stuck_same_args_threshold: 3,
            planning_stuck_alternating_threshold: 3,
            planning_stuck_escalating_retry_threshold: 3,
            planning_reconciler_timestamp_tolerance_secs: 5,
            // Knowledge tuning
            knowledge_instinct_min_observations: 5,
            knowledge_instinct_min_success_rate: 0.80,
            knowledge_instinct_stability_hours: 168.0,
            knowledge_surprise_threshold: episteme::surprise::DEFAULT_THRESHOLD,
            knowledge_surprise_ema_alpha: 0.3,
            knowledge_rule_min_observations: 5,
            knowledge_rule_min_confidence: 0.60,
            knowledge_dedup_weight_name: 0.4,
            knowledge_dedup_weight_embed: 0.3,
            knowledge_dedup_weight_type: 0.2,
            knowledge_dedup_weight_alias: 0.1,
            knowledge_dedup_jw_threshold: 0.85,
            knowledge_dedup_embed_threshold: 0.80,
            knowledge_extraction_provider: BookkeepingProviderKind::Llm,
            compaction_strategy: CompactionStrategyKind::UniformTail,
            // Fact lifecycle
            fact_active_threshold: 0.7,
            fact_fading_threshold: 0.3,
            fact_dormant_threshold: 0.1,
            // Similarity
            similarity_threshold: 0.85,
            similarity_min_token_len: 3,
            // Distillation prompt
            distillation_max_tool_result_len: 500,
            // Auto-dream
            dream_min_hours: melete::dream::DEFAULT_MIN_HOURS,
            dream_min_sessions: melete::dream::DEFAULT_MIN_SESSIONS,
            dream_scan_throttle_secs: 600,
            dream_stale_threshold_secs: melete::dream::DEFAULT_STALE_THRESHOLD_SECS,
            // Tool behavior
            tool_agent_dispatch_max_tasks: DEFAULT_TOOL_AGENT_DISPATCH_MAX_TASKS,
            tool_datalog_default_row_limit: DEFAULT_TOOL_DATALOG_DEFAULT_ROW_LIMIT,
            tool_datalog_default_timeout_secs: DEFAULT_TOOL_DATALOG_DEFAULT_TIMEOUT_SECS,
            tool_max_image_bytes: DEFAULT_TOOL_MAX_IMAGE_BYTES,
            tool_max_pdf_bytes: DEFAULT_TOOL_MAX_PDF_BYTES,
            // Bootstrap
            bootstrap_min_truncation_budget: 200,
            // Corrections
            corrections_max_corrections: 50,
            // Self-tuning
            tuning_eligible: true,
        }
    }
}

#[cfg(test)]
const _: () =
    assert!(DEFAULT_TOOL_AGENT_DISPATCH_MAX_TASKS == organon::builtins::agent::MAX_DISPATCH_TASKS);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_TOOL_DATALOG_DEFAULT_ROW_LIMIT == organon::builtins::memory::datalog::DEFAULT_ROW_LIMIT
);
#[cfg(test)]
const _: () = assert!(
    DEFAULT_TOOL_DATALOG_DEFAULT_TIMEOUT_SECS
        == organon::builtins::memory::datalog::DEFAULT_TIMEOUT_SECS
);
#[cfg(test)]
const _: () =
    assert!(DEFAULT_TOOL_MAX_IMAGE_BYTES == organon::builtins::view_file::MAX_IMAGE_BYTES);
#[cfg(test)]
const _: () = assert!(DEFAULT_TOOL_MAX_PDF_BYTES == organon::builtins::view_file::MAX_PDF_BYTES);
