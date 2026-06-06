# L3 API Index: taxis

Crate path: `crates/taxis`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/cascade.rs`

```rust
pub enum Tier {
    /// Agent-specific (most specific).
    Nous,
    /// Shared across all agents.
    Shared,
    /// Human + agent collaborative (least specific).
    Theke,
}
```

```rust
pub struct CascadeEntry {
    /// Absolute file path.
    pub path: PathBuf,
    /// Which tier it came from.
    pub tier: Tier,
    /// Filename (basename).
    pub name: String,
}
```

```rust
pub fn discover (
    oikos: &Oikos,
    nous_id: &str,
    subdir: &str,
    ext: Option<&str>,
) -> Vec<CascadeEntry>
```

```rust
pub fn discover_with (
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    subdir: &str,
    ext: Option<&str>,
) -> Vec<CascadeEntry>
```

```rust
pub fn resolve (
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Option<PathBuf>
```

```rust
pub fn resolve_with (
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Option<PathBuf>
```

```rust
pub fn resolve_all (
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Vec<CascadeEntry>
```

```rust
pub fn resolve_all_with (
    fs: &impl FileSystem,
    oikos: &Oikos,
    nous_id: &str,
    filename: &str,
    subdir: Option<&str>,
) -> Vec<CascadeEntry>
```

## `src/config/agents.rs`

```rust
pub struct AgentsConfig {
    /// Shared defaults applied to every agent unless overridden per-agent.
    pub defaults: AgentDefaults,
    /// Individual agent definitions; merged with `defaults` at resolution time.
    pub list: Vec<NousDefinition>,
}
```

```rust
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
```

```rust
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
    /// When set alongside `reranker_url`, the URL takes precedence.  This path
    /// is only consulted when `reranker_url` is `None` and the
    /// `local-reranker` feature is enabled.  Default: `None`.
    #[serde(default)]
    pub reranker_model_path: Option<String>,
}
```

```rust
pub enum RecallProfile {
    /// Preserve the explicit recall/extraction/pipeline settings.
    #[default]
    Default,
    /// Favor broad project/reference recall for archival work.
    Archival,
    /// Favor stable identity continuity across turns and sessions.
    IdentityContinuity,
}
```

```rust
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
```

```rust
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
    /// Prompt caching configuration.
    pub caching: CachingConfig,
    /// Recall pipeline settings applied to all agents unless overridden.
    pub recall: RecallSettings,
    /// Fraction of the context window reserved for conversation history.
    pub history_budget_ratio: f64,
    /// Default per-agent behavioral parameters (safety, hooks, distillation, etc.).
    pub behavior: AgentBehaviorDefaults,
}
```

```rust
pub struct ModelSpec {
    /// Primary model identifier (e.g. `claude-sonnet-4-6`).
    pub primary: String,
    /// Ordered fallback models tried when the primary is unavailable.
    pub fallbacks: Vec<String>,
    /// How many times to retry the primary model before trying the next fallback.
    pub retries_before_fallback: u32,
}
```

```rust
pub struct CachingConfig {
    /// Whether prompt caching is enabled.
    pub enabled: bool,
    /// Caching strategy: `"auto"` or `"disabled"`.
    pub strategy: String,
}
```

```rust
pub struct NousDefinition {
    /// Unique agent identifier (matches the `nous/{id}/` directory name).
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire/serde config field: id maps to the agent's directory name in TOML, not a runtime domain identifier
    /// Human-readable display name.
    #[serde(default)]
    pub name: Option<String>,
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
    /// Named recall behavior profile; when `None`, resolves to [`RecallProfile::Default`].
    #[serde(default)]
    pub recall_profile: Option<RecallProfile>,
    /// Per-agent behavioral override; when `None`, inherits from [`AgentDefaults::behavior`].
    #[serde(default)]
    pub behavior: Option<AgentBehaviorDefaults>,
}
```

```rust
pub struct AgentBehaviorDefaults { // kanon:ignore RUST/struct-too-many-fields
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
    /// Mirrors `nous::distillation::CONTEXT_TOKEN_TRIGGER`.
    pub distillation_context_token_trigger: u64,
    /// Message count that triggers distillation. Default: 150.
    /// Mirrors `nous::distillation::MESSAGE_COUNT_TRIGGER`.
    pub distillation_message_count_trigger: u64,
    /// Days idle before a session is considered stale for distillation. Default: 7.
    /// Mirrors `nous::distillation::STALE_SESSION_DAYS`.
    pub distillation_stale_session_days: u64,
    /// Minimum messages required for stale-session distillation. Default: 20.
    /// Mirrors `nous::distillation::STALE_SESSION_MIN_MESSAGES`.
    pub distillation_stale_min_messages: u64,
    /// Message count trigger for sessions never distilled. Default: 30.
    /// Mirrors `nous::distillation::NEVER_DISTILLED_MESSAGE_TRIGGER`.
    pub distillation_never_distilled_trigger: u64,
    /// Minimum messages for legacy distillation threshold. Default: 10.
    /// Mirrors `nous::distillation::LEGACY_THRESHOLD_MIN_MESSAGES`.
    pub distillation_legacy_min_messages: u64,
    /// Maximum backoff turns before distillation is forced. Default: 8.
    /// Mirrors `melete::distill::MAX_BACKOFF_TURNS`.
    pub distillation_max_backoff_turns: u32,

    // --- Competence scoring ---
    /// Competence score penalty per correction. Default: 0.05.
    /// Mirrors `nous::competence::CORRECTION_PENALTY`.
    pub competence_correction_penalty: f64,
    /// Competence score bonus per successful turn. Default: 0.02.
    /// Mirrors `nous::competence::SUCCESS_BONUS`.
    pub competence_success_bonus: f64,
    /// Competence score penalty per user disagreement. Default: 0.01.
    /// Mirrors `nous::competence::DISAGREEMENT_PENALTY`.
    pub competence_disagreement_penalty: f64,
    /// Competence score floor. Default: 0.1.
    /// Mirrors `nous::competence::MIN_SCORE`.
    pub competence_min_score: f64,
    /// Competence score ceiling. Default: 0.95.
    /// Mirrors `nous::competence::MAX_SCORE`.
    pub competence_max_score: f64,
    /// Initial competence score for a new agent. Default: 0.5.
    /// Mirrors `nous::competence::DEFAULT_SCORE`.
    pub competence_default_score: f64,
    /// Competence score below which escalation fires. Default: 0.30.
    /// Mirrors `nous::competence::ESCALATION_FAILURE_THRESHOLD`.
    pub competence_escalation_failure_threshold: f64,
    /// Minimum samples before escalation threshold is evaluated. Default: 5.
    /// Mirrors `nous::competence::ESCALATION_MIN_SAMPLES`.
    pub competence_escalation_min_samples: u32,

    // --- Drift detection ---
    /// Sliding window size for response-quality drift detection. Default: 20.
    /// Mirrors `nous::drift::DEFAULT_WINDOW_SIZE`.
    pub drift_window_size: usize,
    /// Comparison window for recent vs. historical drift. Default: 5.
    /// Mirrors `nous::drift::DEFAULT_RECENT_SIZE`.
    pub drift_recent_size: usize,
    /// Standard deviations required to flag drift. Default: 2.0.
    /// Mirrors `nous::drift::DEFAULT_DEVIATION_THRESHOLD`.
    pub drift_deviation_threshold: f64,
    /// Minimum samples before drift detection activates. Default: 8.
    /// Mirrors `nous::drift::MIN_SAMPLES`.
    pub drift_min_samples: usize,

    // --- Uncertainty calibration ---
    /// Maximum calibration data points retained for the uncertainty model. Default: 1000.
    /// Mirrors `nous::uncertainty::MAX_CALIBRATION_POINTS`.
    pub uncertainty_max_calibration_points: usize,

    // --- Manifest ---
    /// Maximum memory entries in a single manifest for side-query pre-filtering. Default: 200.
    /// Mirrors `episteme::manifest::MAX_MEMORY_ENTRIES`.
    pub manifest_max_entries: usize,

    // --- Skills ---
    /// Maximum number of skills loadable per agent. Default: 5.
    pub skills_max_skills: usize,
    /// Maximum chars from context used when matching skills. Default: 200.
    /// Mirrors `nous::skills::MAX_CONTEXT_CHARS`.
    pub skills_max_context_chars: usize,

    // --- Working state ---
    /// Working-state TTL in seconds before expiry. Default: 604800 (7 days).
    pub working_state_ttl_secs: u64,
    /// Maximum task stack depth before oldest entries are evicted. Default: 10.
    /// Mirrors `nous::working_state::MAX_TASK_STACK`.
    pub working_state_max_task_stack: usize,

    // --- Planning ---
    /// Maximum planning iterations per planning cycle. Default: 10.
    /// Mirrors `dianoia::plan::DEFAULT_MAX_ITERATIONS`.
    pub planning_max_iterations: u32,
    /// History turns inspected for stuck-detection. Default: 20.
    /// Mirrors `dianoia::stuck::DEFAULT_HISTORY_WINDOW`.
    pub planning_stuck_history_window: u32,
    /// Repeated errors before agent is flagged stuck. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_REPEATED_ERROR_THRESHOLD`.
    pub planning_stuck_repeated_error_threshold: u32,
    /// Identical-argument tool calls before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_SAME_ARGS_THRESHOLD`.
    pub planning_stuck_same_args_threshold: u32,
    /// Alternating tool-call pairs before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_ALTERNATING_THRESHOLD`.
    pub planning_stuck_alternating_threshold: u32,
    /// Escalating retry pattern depth before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_ESCALATING_RETRY_THRESHOLD`.
    pub planning_stuck_escalating_retry_threshold: u32,
    /// Seconds of timestamp difference treated as "in sync" during reconciliation. Default: 5.
    /// Mirrors `dianoia::reconciler::TIMESTAMP_TOLERANCE_SECS`.
    pub planning_reconciler_timestamp_tolerance_secs: i64,

    // --- Knowledge tuning (instinct / surprise / rules / dedup) ---
    /// Minimum observations before an instinct is eligible. Default: 5.
    pub knowledge_instinct_min_observations: u32,
    /// Minimum success rate for an instinct to surface. Default: 0.80.
    pub knowledge_instinct_min_success_rate: f64,
    /// Minimum stability hours before an instinct is surfaced. Default: 168.0.
    pub knowledge_instinct_stability_hours: f64,
    /// Standard deviations above baseline for surprise detection. Default: 2.0.
    /// Mirrors `episteme::surprise::DEFAULT_THRESHOLD`.
    pub knowledge_surprise_threshold: f64,
    /// EMA alpha for surprise baseline. Default: 0.3.
    /// Mirrors `episteme::surprise::EMA_ALPHA`.
    pub knowledge_surprise_ema_alpha: f64,
    /// Minimum observations before a rule proposal is eligible. Default: 5.
    /// Mirrors `episteme::rule_proposals::MIN_OBSERVATIONS`.
    pub knowledge_rule_min_observations: u32,
    /// Minimum confidence for a rule proposal to surface. Default: 0.60.
    /// Mirrors `episteme::rule_proposals::MIN_CONFIDENCE`.
    pub knowledge_rule_min_confidence: f64,
    /// Weight of name similarity in dedup scoring. Default: 0.4.
    /// Mirrors `episteme::dedup::WEIGHT_NAME`.
    pub knowledge_dedup_weight_name: f64,
    /// Weight of embedding similarity in dedup scoring. Default: 0.3.
    /// Mirrors `episteme::dedup::WEIGHT_EMBED`.
    pub knowledge_dedup_weight_embed: f64,
    /// Weight of fact-type match in dedup scoring. Default: 0.2.
    /// Mirrors `episteme::dedup::WEIGHT_TYPE`.
    pub knowledge_dedup_weight_type: f64,
    /// Weight of alias similarity in dedup scoring. Default: 0.1.
    /// Mirrors `episteme::dedup::WEIGHT_ALIAS`.
    pub knowledge_dedup_weight_alias: f64,
    /// Jaro-Winkler score above which strings are considered similar. Default: 0.85.
    /// Mirrors `episteme::dedup::JW_THRESHOLD`.
    pub knowledge_dedup_jw_threshold: f64,
    /// Cosine similarity above which embeddings are considered similar. Default: 0.80.
    /// Mirrors `episteme::dedup::EMBED_THRESHOLD`.
    pub knowledge_dedup_embed_threshold: f64,
    /// Bookkeeping provider selected for extraction. Default: `llm`.
    pub knowledge_extraction_provider: BookkeepingProviderKind,
    // --- Compaction ---
    /// Preserved-tail compaction strategy used during full context compaction.
    pub compaction_strategy: CompactionStrategyKind,

    // --- Fact lifecycle ---
    /// Confidence above which a fact is considered Active. Default: 0.7.
    /// Mirrors `eidos::knowledge::fact::STAGE_ACTIVE_THRESHOLD`.
    pub fact_active_threshold: f64,
    /// Confidence below which a fact is considered Fading. Default: 0.3.
    /// Mirrors `eidos::knowledge::fact::STAGE_FADING_THRESHOLD`.
    pub fact_fading_threshold: f64,
    /// Confidence below which a fact is considered Dormant. Default: 0.1.
    /// Mirrors `eidos::knowledge::fact::STAGE_DORMANT_THRESHOLD`.
    pub fact_dormant_threshold: f64,

    // --- Similarity ---
    /// Similarity score threshold for recall deduplication. Default: 0.85.
    pub similarity_threshold: f64,
    /// Minimum token length to include in Jaccard similarity comparison. Default: 3.
    /// Mirrors `melete::similarity::MIN_TOKEN_LEN`.
    pub similarity_min_token_len: usize,

    // --- Distillation prompt ---
    /// Maximum character length for truncated tool results in distillation prompts. Default: 500.
    /// Mirrors `melete::prompt::MAX_TOOL_RESULT_LEN`.
    pub distillation_max_tool_result_len: usize,

    // --- Auto-dream consolidation ---
    /// Minimum hours between auto-dream consolidation runs. Default: 24.
    /// Mirrors `melete::dream::DEFAULT_MIN_HOURS`.
    pub dream_min_hours: u64,
    /// Minimum sessions required to trigger auto-dream consolidation. Default: 5.
    /// Mirrors `melete::dream::DEFAULT_MIN_SESSIONS`.
    pub dream_min_sessions: usize,
    /// Session scan throttle interval in seconds. Default: 600.
    /// Mirrors `melete::dream::SCAN_THROTTLE_SECS`.
    pub dream_scan_throttle_secs: i64,
    /// Stale lock threshold in seconds for auto-dream. Default: 3600.
    /// Mirrors `melete::dream::DEFAULT_STALE_THRESHOLD_SECS`.
    pub dream_stale_threshold_secs: i64,

    // --- Tool behavior ---
    /// Maximum concurrent agent-dispatch tasks. Default: 10.
    /// Mirrors `organon::builtins::agent::MAX_DISPATCH_TASKS`.
    pub tool_agent_dispatch_max_tasks: usize,
    /// Default row limit for Datalog memory queries. Default: 100.
    /// Mirrors `organon::builtins::memory::datalog::DEFAULT_ROW_LIMIT`.
    pub tool_datalog_default_row_limit: u32,
    /// Default query timeout in seconds for the Datalog memory tool. Default: 5.0.
    /// Mirrors `organon::builtins::memory::datalog::DEFAULT_TIMEOUT_SECS`.
    pub tool_datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool. Default: 20971520 (20 MiB).
    /// Mirrors `organon::builtins::view_file::MAX_IMAGE_BYTES`.
    pub tool_max_image_bytes: usize,
    /// Maximum PDF file size in bytes for the view-file tool. Default: 33554432 (32 MiB).
    /// Mirrors `organon::builtins::view_file::MAX_PDF_BYTES`.
    pub tool_max_pdf_bytes: usize,

    // --- Bootstrap ---
    /// Minimum token budget remaining before attempting section truncation.
    /// Below this threshold the section is dropped rather than truncated. Default: 200.
    /// Mirrors `nous::bootstrap::MIN_TRUNCATION_BUDGET`.
    pub bootstrap_min_truncation_budget: u64,

    // --- Corrections ---
    /// Maximum correction entries stored per agent. Default: 50.
    /// Mirrors `nous::hooks::builtins::correction::MAX_CORRECTIONS`.
    pub corrections_max_corrections: usize,

    // --- Self-tuning ---
    /// Whether this agent participates in the self-tuning loop. Default: true.
    ///
    /// When false, the agent's metrics are collected but no proposals are
    /// generated. Combined with the global `TuningConfig::enabled` kill switch.
    pub tuning_eligible: bool,
}
```

## `src/config/behavior/api.rs`

```rust
pub struct ApiLimitsConfig {
    /// Maximum characters in a session name. Default: 255.
    /// Mirrors `pylon::handlers::sessions::MAX_SESSION_NAME_LEN`.
    pub max_session_name_len: usize,
    /// Maximum bytes in a session identifier. Default: 256.
    /// Mirrors `pylon::handlers::sessions::MAX_IDENTIFIER_BYTES`.
    pub max_identifier_bytes: usize,
    /// Maximum messages returned by the history endpoint. Default: 1000.
    /// Mirrors `pylon::handlers::sessions::MAX_HISTORY_LIMIT`.
    pub max_history_limit: u32,
    /// Default messages returned by the history endpoint. Default: 50.
    /// Mirrors `pylon::handlers::sessions::DEFAULT_HISTORY_LIMIT`.
    pub default_history_limit: u32,
    /// Maximum bytes per streaming message body. Default: 262144 (256 KiB).
    /// Mirrors `pylon::handlers::sessions::streaming::MAX_MESSAGE_BYTES`.
    pub max_message_bytes: usize,
    /// Maximum facts returned by a single knowledge list request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_FACTS_LIMIT`.
    pub max_facts_limit: usize,
    /// Maximum results for a single knowledge search request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_SEARCH_LIMIT`.
    pub max_search_limit: usize,
    /// Maximum facts in a single bulk-import request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::bulk_import::MAX_IMPORT_BATCH_SIZE`.
    pub max_import_batch_size: usize,
    /// TTL in seconds for idempotency key cache entries. Default: 300.
    /// Mirrors `pylon::idempotency::DEFAULT_TTL`.
    pub idempotency_ttl_secs: u64,
    /// Maximum idempotency cache entries (LRU cap). Default: 10000.
    /// Mirrors `pylon::idempotency::DEFAULT_CAPACITY`.
    pub idempotency_capacity: usize,
    /// Maximum character length of an idempotency key. Default: 64.
    pub idempotency_max_key_length: usize,
    /// Acceptable clock skew in seconds before token expiry check warns. Default: 30.
    /// Mirrors `pylon::handlers::health::CLOCK_SKEW_LEEWAY`.
    pub clock_skew_leeway_secs: u64,
    /// Time in seconds before token expiry that triggers a warning. Default: 3600.
    /// Mirrors `pylon::handlers::health::EXPIRY_WARNING_THRESHOLD`.
    pub expiry_warning_threshold_secs: u64,
}
```

## `src/config/behavior/daemon.rs`

```rust
pub struct DaemonBehaviorConfig {
    /// Base duration in seconds for watchdog restart backoff. Default: 2.
    /// Mirrors `daemon::watchdog::BACKOFF_BASE`.
    pub watchdog_backoff_base_secs: u64,
    /// Maximum watchdog restart backoff duration in seconds. Default: 300.
    /// Mirrors `daemon::watchdog::BACKOFF_CAP`.
    pub watchdog_backoff_cap_secs: u64,
    /// Samples used for anomaly detection in prosoche attention check. Default: 15.
    /// Mirrors `daemon::prosoche::ANOMALY_SAMPLE_SIZE`.
    pub prosoche_anomaly_sample_size: usize,
    /// Lines from task output head to include in brief summary. Default: 5.
    /// Mirrors `daemon::runner::output::BRIEF_HEAD_LINES`.
    pub runner_output_brief_head_lines: usize,
    /// Lines from task output tail to include in brief summary. Default: 3.
    /// Mirrors `daemon::runner::output::BRIEF_TAIL_LINES`.
    pub runner_output_brief_tail_lines: usize,
}
```

## `src/config/behavior/dispatch.rs`

```rust
pub struct DispatchConfig {
    /// Recurring cron-dispatched tasks.
    pub cron_tasks: Vec<CronTaskConfig>,
}
```

```rust
pub struct CronTaskConfig {
    /// Unique task name.
    pub name: String,
    /// Cron expression (e.g., "0 2 * * *").
    pub schedule: String,
    /// Jitter in seconds (+/-).
    pub jitter_secs: u64,
    /// Whether this task is registered with the scheduler. Defaults to `true`
    /// so that defining a task in config implies the operator wants it run;
    /// set `enabled = false` to leave the task in the config without firing.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// What to dispatch.
    pub dispatch_spec: DispatchSpecConfig,
}
```

```rust
pub struct DispatchSpecConfig {
    /// Prompt numbers to execute.
    pub prompt_numbers: Vec<u32>,
    /// Project identifier.
    pub project: String,
    /// Optional DAG reference.
    #[serde(default)]
    pub dag_ref: Option<String>,
    /// Maximum parallelism.
    #[serde(default)]
    pub max_parallel: Option<u32>,
    /// Maximum turns per initial session.
    #[serde(default)]
    pub max_turns: Option<u32>,
}
```

## `src/config/behavior/jwt.rs`

```rust
pub struct JwtSettings {
    /// Clock skew tolerance in seconds applied when checking the `exp`
    /// claim. A token whose `exp` lies up to this many seconds in the past
    /// is still accepted. Valid range: 0–300. Default: 30.
    pub clock_skew_leeway_secs: u64,
}
```

## `src/config/behavior/knowledge.rs`

```rust
pub struct KnowledgeConfig {
    /// Maximum LLM calls per fact during conflict resolution. Default: 3.
    /// Mirrors `episteme::conflict::MAX_LLM_CALLS_PER_FACT`.
    pub conflict_max_llm_calls_per_fact: usize,
    /// Similarity threshold above which intra-batch candidates are merged. Default: 0.95.
    /// Mirrors `episteme::conflict::INTRA_BATCH_DEDUP_THRESHOLD`.
    pub conflict_intra_batch_dedup_threshold: f64,
    /// Maximum vector distance for a fact to be a conflict candidate. Default: 0.28.
    /// Mirrors `episteme::conflict::CANDIDATE_DISTANCE_THRESHOLD`.
    pub conflict_candidate_distance_threshold: f64,
    /// Maximum conflict candidates evaluated per fact. Default: 5.
    /// Mirrors `episteme::conflict::MAX_CANDIDATES`.
    pub conflict_max_candidates: usize,
    /// Confidence boost per reinforcement event. Default: 0.02.
    /// Mirrors `episteme::decay::REINFORCEMENT_BOOST`.
    pub decay_reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    /// Mirrors `episteme::decay::MAX_REINFORCEMENT_BONUS`.
    pub decay_max_reinforcement_bonus: f64,
    /// Confidence bonus per additional corroborating agent. Default: 0.15.
    /// Mirrors `episteme::decay::CROSS_AGENT_BONUS_PER_AGENT`.
    pub decay_cross_agent_bonus_per_agent: f64,
    /// Cap on total cross-agent multiplier. Default: 1.75.
    /// Mirrors `episteme::decay::MAX_CROSS_AGENT_MULTIPLIER`.
    pub decay_max_cross_agent_multiplier: f64,
    /// Minimum confidence for a fact to pass extraction filtering. Default: 0.3.
    pub extraction_confidence_threshold: f64,
    /// Minimum character length for an extracted fact. Default: 10.
    pub extraction_min_fact_length: usize,
    /// Maximum character length for an extracted fact. Default: 500.
    pub extraction_max_fact_length: usize,
    /// Provider selection for the extraction bookkeeping pass.
    pub extraction: ExtractionConfig,
    /// Minimum tool calls before operational instinct scoring fires. Default: 5.
    /// Mirrors `episteme::ops_facts::MIN_TOOL_CALLS`.
    pub instinct_min_tool_calls: u64,
    /// Maximum length for parameter values before truncation. Default: 200.
    /// Mirrors `episteme::instinct::MAX_PARAM_VALUE_LEN`.
    pub instinct_max_param_value_len: usize,
    /// Maximum length for context summaries. Default: 100.
    /// Mirrors `episteme::instinct::MAX_CONTEXT_SUMMARY_LEN`.
    pub instinct_max_context_summary_len: usize,
    /// Maximum byte length for fact content strings. Default: 102400 (100 KiB).
    /// Mirrors `eidos::knowledge::fact::MAX_CONTENT_LENGTH`.
    pub max_content_length: usize,
    /// Default maximum entries returned by a single side-query. Default: 5.
    /// Mirrors `episteme::side_query::DEFAULT_MAX_RESULTS`.
    pub side_query_max_results: usize,
    /// Default cache time-to-live in seconds for side-query. Default: 300.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_TTL_SECS`.
    pub side_query_cache_ttl_secs: u64,
    /// Default maximum cache entries for side-query. Default: 64.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_CAPACITY`.
    pub side_query_cache_capacity: usize,
    /// Decay score below which a skill is flagged for review. Default: 0.3.
    /// Mirrors `episteme::skill::decay::NEEDS_REVIEW_THRESHOLD`.
    pub skill_decay_needs_review_threshold: f64,
    /// Decay score below which a skill is auto-retired. Default: 0.08.
    /// Mirrors `episteme::skill::decay::RETIRE_THRESHOLD`.
    pub skill_decay_retire_threshold: f64,
    /// Days of inactivity before decay reaches review threshold (low-usage skills). Default: 28.
    /// Mirrors `episteme::skill::decay::DEFAULT_STALE_DAYS`.
    pub skill_decay_stale_days: u32,
    /// Usage count above which a skill decays slower. Default: 10.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_THRESHOLD`.
    pub skill_decay_high_usage_threshold: u32,
    /// Multiplier applied to decay half-life for high-usage skills. Default: 3.0.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_DECAY_FACTOR`.
    pub skill_decay_high_usage_factor: f64,
    /// Surprise threshold (nats) for episode boundary detection. Default: 2.0.
    /// Mirrors `episteme::surprise::DEFAULT_THRESHOLD`.
    pub surprise_threshold: f64,
    /// EMA alpha for surprise baseline adaptation. Default: 0.3.
    /// Mirrors `episteme::surprise::DEFAULT_EMA_ALPHA`.
    pub surprise_ema_alpha: f64,
    /// Recall weight for Bayesian surprise contribution. Default: 0.0 (inert).
    ///
    /// Non-zero values blend the session `SurpriseCalculator`'s KL-divergence
    /// signal (via `RecallEngine::score_surprise`) into recall scoring, so
    /// candidates whose content diverges from the running session topic rank
    /// higher. Threaded into `RecallWeights::surprise` at engine construction
    /// (`aletheia::runtime::nous_config` → `RecallConfig::surprise_weight`).
    ///
    /// WARNING: this is a novelty/serendipity signal, not a relevance booster —
    /// it surfaces cross-topic memories (high topic-shift surprise), trading
    /// relevance for diversity. Keep it small relative to `vector_similarity`.
    pub recall_surprise_weight: f64,
    /// Recall weight for evidence-gap coverage. Default: 0.0 (inert).
    ///
    /// Non-zero values boost candidates whose `source_id` answers a decomposed
    /// query gap (via `RecallEngine::score_evidence_coverage`) during the
    /// iterative-retrieval path. Threaded into `RecallWeights::evidence_coverage`
    /// at engine construction.
    pub recall_evidence_coverage_weight: f64,
    /// Recall weight for consolidated-fact convergence. Default: 0.0 (inert).
    ///
    /// Non-zero values boost facts assembled from more independent converging
    /// observations, scored as `log(1 + source_count)` from the
    /// `fact_multiplicity` side-index (via `RecallEngine::score_convergence`).
    /// Threaded into `RecallWeights::convergence` at engine construction.
    pub recall_convergence_weight: f64,
    /// Admission policy applied to every `insert_fact` call. Default: `default` (admit-all).
    ///
    /// Set to `structured` to activate the five-factor A-MAC gate
    /// (`StructuredAdmissionPolicy`). Operators can tune the threshold and
    /// min-confidence via `episteme::admission::StructuredAdmissionConfig`
    /// defaults; those knobs are not yet surfaced in the TOML cascade.
    pub admission_policy: AdmissionPolicyKind,
}
```

```rust
pub struct ExtractionConfig {
    /// Bookkeeping provider implementation. Default: `llm`.
    pub provider: BookkeepingProviderKind,
}
```

```rust
pub enum BookkeepingProviderKind {
    /// Compatibility LLM prompt + parser path.
    #[default]
    Llm,
    /// `GLiNER` ONNX entity adapter with LLM fallback.
    Gliner,
}
```

```rust
pub enum CompactionStrategyKind {
    /// Keep the preserved tail as whole messages.
    #[default]
    UniformTail,
    /// Keep the last two steps full and compact earlier preserved steps.
    StepPositional,
}
```

```rust
pub enum AdmissionPolicyKind {
    /// Admit-all policy: every fact that passes basic validation is stored.
    ///
    /// This is the pre-admission-control behavior. Use this when the
    /// extraction pipeline is already well-filtered or when behavioral
    /// compatibility with existing deployments is required.
    #[default]
    Default,
    /// Five-factor A-MAC policy (arxiv 2603.04549): utility, confidence,
    /// novelty, recency, and content-type prior. Facts whose combined
    /// weighted score falls below the configured threshold are rejected.
    Structured,
}
```

## `src/config/behavior/messaging.rs`

```rust
pub struct MessagingConfig {
    /// How often Semeion polls for new channel messages in milliseconds. Default: 2000.
    /// Mirrors `agora::semeion::DEFAULT_POLL_INTERVAL`.
    pub poll_interval_ms: u64,
    /// Inbound message buffer size per channel. Default: 100.
    /// Mirrors `agora::semeion::DEFAULT_BUFFER_CAPACITY`.
    pub buffer_capacity: usize,
    /// Consecutive channel errors before the channel is halted. Default: 5.
    /// Mirrors `agora::semeion::CIRCUIT_BREAKER_THRESHOLD`.
    pub circuit_breaker_threshold: u32,
    /// How often a halted channel is health-checked in seconds. Default: 60.
    /// Mirrors `agora::semeion::HALTED_HEALTH_CHECK_INTERVAL`.
    pub halted_health_check_interval_secs: u64,
    /// Timeout in seconds for Semeion RPC calls. Default: 10.
    /// Mirrors `agora::semeion::client::RPC_TIMEOUT`.
    pub rpc_timeout_secs: u64,
    /// Timeout in seconds for Semeion health-check requests. Default: 2.
    /// Mirrors `agora::semeion::client::HEALTH_TIMEOUT`.
    pub health_timeout_secs: u64,
    /// Timeout in seconds waiting to receive a Semeion response. Default: 15.
    /// Mirrors `agora::semeion::client::RECEIVE_TIMEOUT`.
    pub receive_timeout_secs: u64,
    /// Default timeout in seconds for agent-dispatch tool calls. Default: 300.
    /// Mirrors `organon::builtins::agent::DEFAULT_TIMEOUT_SECS`.
    pub agent_dispatch_timeout_secs: u64,
    /// Maximum concurrent inbound-message handler tasks. Default: 64.
    /// Mirrors `agora::listener::ChannelListener::MAX_CONCURRENT_HANDLERS`.
    pub max_concurrent_handlers: usize,
}
```

## `src/config/behavior/nous.rs`

```rust
pub struct NousBehaviorConfig {
    /// Panics within the window that trigger degraded mode. Default: 5.
    /// Mirrors `nous::actor::DEGRADED_PANIC_THRESHOLD`.
    pub degraded_panic_threshold: u32,
    /// Window in seconds for counting panics toward degraded threshold. Default: 600.
    /// Mirrors `nous::actor::DEGRADED_WINDOW`.
    pub degraded_window_secs: u64,
    /// Actor inbox receive timeout in seconds before a warning is logged. Default: 30.
    /// Mirrors `nous::actor::INBOX_RECV_TIMEOUT`.
    pub inbox_recv_timeout_secs: u64,
    /// Consecutive receive timeouts before a warning log is emitted. Default: 3.
    /// Mirrors `nous::actor::CONSECUTIVE_TIMEOUT_WARN_THRESHOLD`.
    pub consecutive_timeout_warn_threshold: u32,
    /// Maximum number of concurrently spawned tasks per agent. Default: 8.
    pub max_spawned_tasks: usize,
    /// Completed-task garbage collection interval in seconds. Default: 300.
    /// Mirrors `nous::tasks::gc::DEFAULT_GC_INTERVAL`.
    pub gc_interval_secs: u64,
    /// Consecutive failed pings before marking an agent dead. Default: 3.
    /// Mirrors `nous::manager::DEAD_THRESHOLD`.
    pub manager_dead_threshold: u32,
    /// Cap on exponential restart backoff in seconds. Default: 300.
    /// Mirrors `nous::manager::MAX_RESTART_BACKOFF`.
    pub manager_max_restart_backoff_secs: u64,
    /// Drain timeout in seconds before forcing an agent restart. Default: 30.
    /// Mirrors `nous::manager::RESTART_DRAIN_TIMEOUT`.
    pub manager_restart_drain_timeout_secs: u64,
    /// Window in seconds over which the failure count decays to zero. Default: 3600.
    /// Mirrors `nous::manager::RESTART_DECAY_WINDOW`.
    pub manager_restart_decay_window_secs: u64,
    /// Agent health poll interval in seconds. Default: 30.
    /// Mirrors `nous::manager::DEFAULT_HEALTH_INTERVAL`.
    pub manager_health_interval_secs: u64,
    /// Timeout in seconds for health-ping responses. Default: 5.
    /// Mirrors `nous::manager::DEFAULT_PING_TIMEOUT`.
    pub manager_ping_timeout_secs: u64,
    /// Maximum seconds a turn may be active before the health check considers
    /// the actor stuck. An `active_turn` flag alone cannot distinguish a legitimately
    /// busy actor from one hung on an infinite loop or deadlock. Default: 600 (10 min).
    /// WHY: Without a timeout, a stuck `active_turn` flag prevents the health check
    /// from ever restarting the actor, making a single hung pipeline permanently
    /// block all subsequent messages. (#3254)
    pub stuck_turn_timeout_secs: u64,
    /// Number of recent tool calls scanned for loop detection. Default: 50.
    /// Mirrors `nous::pipeline::DEFAULT_LOOP_WINDOW`.
    pub loop_detection_window: usize,
    /// Maximum sequence length examined for repeating cycles. Default: 10.
    /// Mirrors `nous::pipeline::CYCLE_DETECTION_MAX_LEN`.
    pub cycle_detection_max_len: usize,
    /// Events accumulated before self-audit runs. Default: 50.
    /// Mirrors `nous::self_audit::DEFAULT_EVENT_THRESHOLD`.
    pub self_audit_event_threshold: u32,
    /// TTL in seconds for the bootstrap workspace file cache. Default: 60.
    ///
    /// // WHY: bootstrap files (SOUL.md, USER.md, etc.) change rarely relative
    /// // to turn frequency. Caching their content and token estimates for up
    /// // to this many seconds avoids redundant disk reads per turn (#3388).
    /// // mtime-based invalidation catches operator edits immediately, so the
    /// // TTL is a backstop rather than the primary freshness mechanism.
    /// // Set to 0 to disable the cache.
    pub bootstrap_cache_ttl_secs: u64,
    /// Maximum seconds `NousManager::shutdown_all` waits for actors to finish
    /// their current turn before aborting their tasks. Default: 30.
    ///
    /// WHY: Without a timeout, a long-running turn (e.g. a stuck LLM call or
    /// deadlocked tool) blocks graceful shutdown indefinitely. When the
    /// timeout expires, remaining actor tasks are aborted via
    /// `JoinHandle::abort()` so the process can exit. (#3382)
    pub shutdown_timeout_secs: u64,
}
```

## `src/config/behavior/provider.rs`

```rust
pub struct ProviderBehaviorConfig {
    /// Timeout in seconds for non-streaming LLM requests. Default: 120.
    /// Mirrors `hermeneus::anthropic::client::NON_STREAMING_TIMEOUT`.
    pub non_streaming_timeout_secs: u64,
    /// Default retry delay from SSE stream retry field in milliseconds. Default: 1000.
    /// Mirrors `hermeneus::anthropic::error::SSE_DEFAULT_RETRY_MS`.
    pub sse_default_retry_ms: u64,
    /// EWMA smoothing factor for adaptive concurrency limiter. Default: 0.8.
    /// Mirrors `hermeneus::concurrency::DEFAULT_EWMA_ALPHA`.
    pub concurrency_ewma_alpha: f64,
    /// Latency threshold in seconds above which concurrency limit is reduced. Default: 30.0.
    /// Mirrors `hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS`.
    pub concurrency_latency_threshold_secs: f64,
    /// Complexity score below which Haiku-class model is selected. Default: 30.
    /// Mirrors `hermeneus::complexity::DEFAULT_LOW_THRESHOLD`.
    pub complexity_low_threshold: u32,
    /// Complexity score above which Opus-class model is selected. Default: 70.
    /// Mirrors `hermeneus::complexity::DEFAULT_HIGH_THRESHOLD`.
    pub complexity_high_threshold: u32,
}
```

```rust
pub struct AnthropicConfig {
    /// Prompt cache policy (#3410).
    ///
    /// Controls whether outgoing requests carry `cache_control` markers that
    /// let Anthropic store operator system prompts, tool definitions, and
    /// recent conversation turns on their side for reuse. `"disabled"` (the
    /// default) strips every marker so operator content never enters the
    /// Anthropic prompt cache; `"ephemeral"` opts in to the standard 5-minute
    /// cache; `"extended"` reserves the slot for the 1-hour cache wire format
    /// and currently behaves the same as `"ephemeral"`.
    ///
    /// Tradeoff: enabling caching lowers per-turn token spend at the cost of
    /// storing the operator's system prompt on Anthropic infrastructure for
    /// the cache lifetime.
    pub prompt_cache_mode: PromptCacheMode,
}
```

```rust
pub enum PromptCacheMode {
    /// No `cache_control` markers emitted — operator content never enters
    /// Anthropic's prompt cache. Sovereignty default.
    #[default]
    Disabled,
    /// Standard 5-minute ephemeral cache.
    Ephemeral,
    /// Extended 1-hour cache (reserved; behaves like `Ephemeral` until the
    /// wire format for extended TTL is plumbed through).
    Extended,
}
```

```rust
pub enum DeploymentTarget {
    /// Third-party cloud API (e.g., api.anthropic.com, api.openai.com).
    /// Facts marked sensitive are filtered before the request is sent.
    #[default]
    Cloud,
    /// Self-hosted endpoint reachable over the local network (e.g., a
    /// colocated llama.cpp server on the same subnet). Trusted with
    /// operator-sensitive content but not with personally-identifiable data.
    #[serde(alias = "local_hosted", alias = "local-hosted")]
    LocalHosted,
    /// Runs on the same machine as aletheia (loopback llama.cpp / ollama
    /// / vllm). Trusted with every fact the operator would trust to disk.
    Embedded,
}
```

```rust
pub enum ProviderKind {
    /// Anthropic Messages API native client.
    Anthropic,
    /// `OpenAI` Chat Completions API native endpoint.
    #[serde(rename = "openai", alias = "open-ai")]
    OpenAi,
    /// `OpenAI` Chat Completions-compatible HTTP client. Works with
    /// `OpenAI`, llama.cpp, ollama, vllm, and any other server exposing the
    /// same wire format.
    #[serde(alias = "openai-compatible")]
    OpenAiCompatible,
    /// Claude Code subprocess adapter (delegates to the `claude` CLI).
    /// Requires the `cc-provider` feature flag on hermeneus.
    ClaudeCode,
    /// Codex CLI subprocess adapter (delegates to the `codex` CLI).
    /// Requires the `codex-provider` feature flag on hermeneus.
    #[serde(rename = "codex_oauth", alias = "codex-oauth")]
    CodexOauth,
}
```

```rust
pub enum OpenAiApiFamily {
    /// `OpenAI` `/v1/chat/completions` and compatible local/proxy endpoints.
    ChatCompletions,
    /// `OpenAI` first-party `/v1/responses` endpoint.
    Responses,
}
```

```rust
pub struct LlmProviderConfig {
    /// Operator-facing label for logs and diagnostics (e.g., `"local-qwen"`,
    /// `"anthropic-cloud"`). Must be unique across the provider list.
    pub name: String,
    /// Which concrete provider implementation to instantiate.
    #[serde(rename = "providerType")]
    pub kind: ProviderKind,
    /// HTTP base URL override. Required for OpenAI-compatible providers
    /// (e.g., `http://127.0.0.1:8088/v1` for local llama.cpp). Optional for
    /// Anthropic (defaults to `https://api.anthropic.com`). Ignored for
    /// subprocess adapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Environment variable name holding the API key. Read at startup via
    /// `std::env::var`. Optional for loopback / embedded providers that do
    /// not require authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// `OpenAI` API family to use. If omitted, `providerType = "openai"`
    /// defaults to `responses`, while `openai-compatible` defaults to
    /// `chat-completions` for local/proxy compatibility. Ignored for
    /// subprocess adapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_family: Option<OpenAiApiFamily>,
    /// Where this provider's traffic terminates. Drives the
    /// factsensitivity filter (#3414) and air-gapped mode.
    #[serde(default)]
    pub deployment_target: DeploymentTarget,
    /// Model identifiers this provider advertises support for. Used by the
    /// provider registry for routing: the first provider in list order that
    /// claims the requested model wins.
    #[serde(default)]
    pub models: Vec<String>,
}
```

## `src/config/behavior/timeouts.rs`

```rust
pub struct TimeoutsConfig {
    /// Maximum wall-clock seconds for a single LLM API call (Anthropic or CC provider).
    ///
    /// Requests exceeding this limit are cancelled and may trigger a retry.
    /// Valid range: 30–3600. Default: 300.
    pub llm_call_secs: u32,
}
```

```rust
pub struct CapacityConfig {
    /// Maximum bytes returned by a single tool call before the output is
    /// truncated with an indicator showing the original size.
    ///
    /// Applies to all built-in tools (filesystem, workspace, shell). Set to
    /// `0` to disable truncation. Valid range: 0–10 MiB. Default: 51200 (50 KiB).
    pub max_tool_output_bytes: usize,
}
```

```rust
pub struct RetrySettings {
    /// Maximum number of retry attempts after an initial transient failure.
    ///
    /// The total number of LLM calls is `max_attempts + 1`. Set to `0` to
    /// disable retries. Valid range: 0–10. Default: 3.
    pub max_attempts: u32,
    /// Initial exponential backoff delay in milliseconds.
    ///
    /// Each successive retry doubles this delay until `backoff_max_ms` is
    /// reached. Valid range: 100–30000. Default: 1000.
    pub backoff_base_ms: u64,
    /// Maximum backoff delay cap in milliseconds.
    ///
    /// No retry will wait longer than this value regardless of how many
    /// attempts have failed. Valid range: `backoff_base_ms`–300000. Default: 30000.
    pub backoff_max_ms: u64,
}
```

## `src/config/behavior/tools.rs`

```rust
pub struct ToolLimitsConfig {
    /// Maximum character length for glob patterns. Default: 1000.
    /// Mirrors `organon::builtins::filesystem::MAX_PATTERN_LENGTH`.
    pub max_pattern_length: usize,
    /// Timeout in seconds for filesystem subprocess commands. Default: 60.
    /// Mirrors `organon::builtins::filesystem::SUBPROCESS_TIMEOUT`.
    pub subprocess_timeout_secs: u64,
    /// Maximum bytes per workspace write operation. Default: 10485760 (10 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_WRITE_BYTES`.
    pub max_write_bytes: usize,
    /// Maximum bytes per workspace read operation. Default: 52428800 (50 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_READ_BYTES`.
    pub max_read_bytes: u64,
    /// Maximum character length of a shell command. Default: 10000.
    /// Mirrors `organon::builtins::workspace::MAX_COMMAND_LENGTH`.
    pub max_command_length: usize,
    /// Maximum characters per intra-session message. Default: 4000.
    /// Mirrors `organon::builtins::communication::MESSAGE_MAX_LEN`.
    pub message_max_len: usize,
    /// Maximum characters per inter-session message. Default: 100000.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN`.
    pub inter_session_max_message_len: usize,
    /// Maximum wait timeout in seconds for inter-session messages. Default: 300.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS`.
    pub inter_session_max_timeout_secs: u64,
    /// Maximum concurrent agent-dispatch tasks. Default: 10.
    /// Also present in `AgentBehaviorDefaults::tool_agent_dispatch_max_tasks`.
    pub max_dispatch_tasks: usize,
    /// Default timeout in seconds for spawned sub-agents. Default: 300.
    pub agent_dispatch_timeout_secs: u64,
    /// Default row limit for Datalog memory queries. Default: 100.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_row_limit`.
    pub datalog_default_row_limit: usize,
    /// Default query timeout in seconds for the Datalog memory tool. Default: 5.0.
    /// Also present in `AgentBehaviorDefaults::tool_datalog_default_timeout_secs`.
    pub datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool. Default: 20971520 (20 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_image_bytes`.
    pub max_image_bytes: u64,
    /// Maximum PDF file size in bytes for the view-file tool. Default: 33554432 (32 MiB).
    /// Also present in `AgentBehaviorDefaults::tool_max_pdf_bytes`.
    pub max_pdf_bytes: u64,
}
```

## `src/config/behavior/tuning.rs`

```rust
pub struct TuningConfig {
    /// Global kill switch for self-tuning. Default: false.
    ///
    /// When false, no tuning proposals are generated or applied regardless
    /// of per-agent settings.
    pub enabled: bool,
    /// Maximum parameter changes applied per prosoche cycle. Default: 3.
    ///
    /// Limits the blast radius of a single tuning cycle. Additional proposals
    /// beyond this limit are deferred to the next cycle.
    pub max_changes_per_cycle: u32,
    /// Minimum metric observations required before a proposal is generated. Default: 20.
    ///
    /// Below this threshold, evidence is considered insufficient and the
    /// proposal is rejected.
    pub evidence_min_samples: u32,
    /// Significance threshold in standard deviations. Default: 1.5.
    ///
    /// The observed delta must exceed `significance_threshold * stddev` for
    /// the evidence to be considered statistically significant.
    pub significance_threshold: f64,
}
```

## `src/config/gateway.rs`

```rust
pub struct GatewayConfig {
    /// TCP port the gateway listens on.
    pub port: u16,
    /// Bind mode: `"localhost"` for loopback only, `"lan"` for all interfaces.
    pub bind: String,
    /// Authentication configuration.
    pub auth: GatewayAuthConfig,
    /// TLS termination settings.
    pub tls: TlsConfig,
    /// Cross-origin resource sharing policy.
    pub cors: CorsConfig,
    /// Request body size limit.
    pub body_limit: BodyLimitConfig,
    /// CSRF protection settings.
    pub csrf: CsrfConfig,
    /// Rate limiting settings.
    pub rate_limit: RateLimitConfig,
    /// SSE heartbeat interval for event subscription streams, in seconds.
    pub sse_heartbeat_interval_secs: u64,
}
```

```rust
pub struct GatewayAuthConfig {
    /// Auth mode: `"token"` (bearer token), `"none"` (disabled), `"jwt"` (explicit JWT).
    pub mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    /// Valid values: `"readonly"`, `"agent"`, `"operator"`, `"admin"`. Defaults to `"admin"`.
    pub none_role: String,
    /// JWT signing key. If `None`, falls back to `ALETHEIA_JWT_SECRET` env var.
    /// Startup fails when auth mode requires JWT and this is still the default placeholder.
    ///
    /// WHY: `SecretString` prevents accidental logging of the key value. Closes #1631.
    pub signing_key: Option<SecretString>,
}
```

```rust
pub struct TlsConfig {
    /// Whether TLS termination is active.
    pub enabled: bool,
    /// Path to the PEM-encoded certificate file.
    pub cert_path: Option<String>,
    /// Path to the PEM-encoded private key file.
    pub key_path: Option<String>,
}
```

```rust
pub struct CorsConfig {
    /// Allowed origins. Empty or `["*"]` means permissive (dev mode).
    pub allowed_origins: Vec<String>,
    /// Preflight cache duration in seconds.
    pub max_age_secs: u64,
}
```

```rust
pub struct BodyLimitConfig {
    /// Maximum request body size in bytes.
    pub max_bytes: usize,
}
```

```rust
pub struct CsrfConfig {
    /// Whether CSRF header checking is active.
    pub enabled: bool,
    /// Required header name (e.g. `x-requested-with`).
    pub header_name: String,
    /// Required header value (e.g. `aletheia`).
    pub header_value: String,
}
```

```rust
pub struct RateLimitConfig {
    /// Whether rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute per client IP (global rate limit).
    pub requests_per_minute: u32,
    /// Per-user rate limiting settings keyed by authenticated identity.
    pub per_user: PerUserRateLimitConfig,
}
```

```rust
pub struct PerUserRateLimitConfig {
    /// Whether per-user rate limiting is active.
    pub enabled: bool,
    /// Default requests per minute for general API endpoints.
    pub default_rpm: u32,
    /// Burst allowance above the sustained rate for general endpoints.
    pub default_burst: u32,
    /// Requests per minute for LLM/chat endpoints (more expensive).
    pub llm_rpm: u32,
    /// Burst allowance for LLM endpoints.
    pub llm_burst: u32,
    /// Requests per minute for tool execution endpoints.
    pub tool_rpm: u32,
    /// Burst allowance for tool execution endpoints.
    pub tool_burst: u32,
    /// Seconds after which an idle user's rate limit state is evicted.
    pub stale_after_secs: u64,
}
```

## `src/config/maintenance.rs`

```rust
pub struct MaintenanceSettings {
    /// Trace log file rotation and compression.
    pub trace_rotation: TraceRotationSettings,
    /// Filesystem drift detection against expected instance layout.
    pub drift_detection: DriftDetectionSettings,
    /// Database size monitoring and alerting.
    pub db_monitoring: DbMonitoringSettings,
    /// Proactive disk space monitoring and write protection.
    pub disk_space: DiskSpaceSettings,
    /// Automatic data retention enforcement.
    pub retention: RetentionSettings,
    /// Whether background knowledge graph maintenance tasks are enabled.
    #[serde(default)]
    pub knowledge_maintenance_enabled: bool,
    /// Watchdog process monitor settings.
    pub watchdog: WatchdogSettings,
    /// Periodic cron task settings (evolution, reflection, graph cleanup).
    pub cron_tasks: CronTaskSettings,
    /// Fjall knowledge store backup settings.
    pub backup: BackupSettings,
}
```

```rust
pub struct TraceRotationSettings {
    /// Whether automatic trace rotation runs.
    pub enabled: bool,
    /// Delete trace files older than this many days.
    pub max_age_days: u32,
    /// Maximum total trace directory size in MB before pruning.
    pub max_total_size_mb: u64,
    /// Whether to gzip-compress rotated trace files.
    pub compress: bool,
    /// Maximum number of compressed archive files to retain.
    pub max_archives: usize,
}
```

```rust
pub struct DriftDetectionSettings {
    /// Whether drift detection runs during maintenance.
    pub enabled: bool,
    /// Emit warnings for files missing from the expected layout.
    pub alert_on_missing: bool,
    /// Glob patterns for paths to ignore during drift checks entirely.
    pub ignore_patterns: Vec<String>,
    /// Glob patterns for optional scaffolding files. Missing files matching these
    /// patterns are reported at info level rather than warn level.
    pub optional_patterns: Vec<String>,
}
```

```rust
pub struct DbMonitoringSettings {
    /// Whether database size monitoring runs.
    pub enabled: bool,
    /// Emit a warning when any database exceeds this size in MB.
    pub warn_threshold_mb: u64,
    /// Emit an alert when any database exceeds this size in MB.
    pub alert_threshold_mb: u64,
}
```

```rust
pub struct DiskSpaceSettings {
    /// Whether disk space monitoring is active.
    pub enabled: bool,
    /// Emit a warning when available space drops below this value (MB).
    pub warning_threshold_mb: u64,
    /// Reject non-essential writes when available space drops below this value (MB).
    pub critical_threshold_mb: u64,
    /// Seconds between background disk space checks.
    pub check_interval_secs: u64,
}
```

```rust
pub struct RetentionSettings {
    /// Whether automatic retention enforcement (session cleanup) runs.
    pub enabled: bool,
}
```

```rust
pub struct SandboxSettings {
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcementMode,
    /// Default filesystem root granted read access.
    ///
    /// Defaults to `~` (HOME). Operators can set this to a stricter path.
    /// The `~` prefix is expanded to the HOME environment variable at runtime.
    ///
    /// WHY: without a home-directory default, agents cannot read user files:
    /// closes #1823.
    pub allowed_root: PathBuf,
    /// Additional filesystem paths granted read access.
    pub extra_read_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted read+write access.
    pub extra_write_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted execute access.
    ///
    /// Values may begin with `~` which is expanded to the HOME environment
    /// variable at policy-build time.
    pub extra_exec_paths: Vec<PathBuf>,
    /// Network egress policy for child processes spawned by the exec tool.
    pub egress: EgressPolicy,
    /// CIDR ranges or addresses permitted when `egress = "allowlist"`.
    pub egress_allowlist: Vec<String>,
    /// Maximum number of processes (`RLIMIT_NPROC`) for exec child processes.
    ///
    /// WHY: `RLIMIT_NPROC` counts ALL processes for the user, not just sandbox
    /// children. Default: 256. Closes #1984.
    pub nproc_limit: u32,
}
```

```rust
pub struct CredentialConfig {
    /// Credential source strategy: `"auto"`, `"api-key"`, or `"claude-code"`.
    pub source: String,
    /// Override path to the Claude Code credentials file.
    /// Defaults to `~/.claude/.credentials.json`.
    pub claude_code_credentials: Option<String>,
    /// Refresh when token has less than this many seconds remaining. Default: 3600 (1 hour).
    /// Mirrors `symbolon::credential::REFRESH_THRESHOLD_SECS`.
    pub refresh_threshold_secs: u64,
    /// Circuit breaker settings for OAuth token refresh.
    pub circuit_breaker: CircuitBreakerSettings,
}
```

```rust
pub struct CircuitBreakerSettings {
    /// Number of failures within the window to trip the circuit.
    pub failure_threshold: u32,
    /// Sliding window (seconds) for failure counting.
    pub failure_window_secs: u64,
    /// Base cooldown (seconds) before probing recovery.
    pub cooldown_secs: u64,
    /// Maximum cooldown (seconds) after exponential backoff.
    pub max_cooldown_secs: u64,
}
```

```rust
pub struct LoggingSettings {
    /// Directory where daily log files are written.
    ///
    /// Relative paths are resolved from the instance root. `None` (the
    /// default) resolves to `{instance}/logs/`.
    pub log_dir: Option<String>,
    /// Number of days to retain log files before they are deleted.
    ///
    /// Cleanup is performed once daily at server startup and every 24 hours
    /// thereafter. Default: 14 days.
    pub retention_days: u32,
    /// Minimum log level written to log files.
    ///
    /// Accepts any `tracing` filter directive (e.g. `"warn"`, `"error"`,
    /// `"aletheia=debug,warn"`). Default: `"warn"`, which captures WARN and
    /// ERROR events from all crates regardless of the console log level.
    pub level: String,
    /// Redaction settings for tracing spans and events.
    pub redaction: RedactionSettings,
}
```

```rust
pub struct RedactionSettings {
    /// Primary switch for the redaction layer. Default: `true`.
    pub enabled: bool,
    /// Field names whose values are replaced with `[REDACTED]`.
    pub redact_fields: Vec<String>,
    /// Field names whose values are truncated to `truncate_length` chars.
    pub truncate_fields: Vec<String>,
    /// Maximum character length for truncated fields. Default: 200.
    pub truncate_length: usize,
}
```

```rust
pub struct WatchdogSettings {
    /// Whether the watchdog monitor is enabled.
    pub enabled: bool,
    /// Seconds without a heartbeat before a process is declared hung.
    pub heartbeat_timeout_secs: u64,
    /// Seconds between watchdog health check sweeps.
    pub check_interval_secs: u64,
    /// Maximum restart attempts before abandoning a process.
    pub max_restarts: u32,
}
```

```rust
pub struct BackupSettings {
    /// Whether automatic fjall backups are enabled.
    pub enabled: bool,
    /// Hours between automatic backups.
    pub backup_interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub backup_retention_count: usize,
}
```

```rust
pub struct CronTaskSettings {
    /// Evolution: periodic configuration variant search.
    pub evolution: CronTaskEntry,
    /// Reflection: periodic self-reflection prompt.
    pub reflection: CronTaskEntry,
    /// Graph cleanup: periodic knowledge graph orphan removal.
    pub graph_cleanup: CronTaskEntry,
}
```

```rust
pub struct CronTaskEntry {
    /// Whether this cron task is enabled.
    pub enabled: bool,
    /// Interval between runs in seconds.
    pub interval_secs: u64,
}
```

```rust
pub struct PromptAuditSettings {
    /// Whether outbound requests are recorded. Default: `true`.
    ///
    /// WHY default-on: this is a sovereignty feature — operators need
    /// visibility into what the system sends to external providers without
    /// opting in. The log stores hashes and IDs, not content, so the cost
    /// of enabling it is small.
    pub enabled: bool,
    /// Directory for daily JSONL files. When `None`, resolves to
    /// `{instance}/logs/prompt-audit/` at startup.
    pub log_dir: Option<PathBuf>,
    /// Days to retain JSONL files before the daemon prunes them.
    pub retention_days: u32,
    /// Whether the IDs of facts filtered by the sensitivity policy (#3404)
    /// are included in each record. Default: `true`.
    pub include_filtered_ids: bool,
}
```

```rust
pub struct McpConfig {
    /// Per-session rate limiting for MCP tool calls.
    pub rate_limit: McpRateLimitConfig,
    /// Knowledge graph MCP surface configuration.
    pub knowledge_graph: KnowledgeGraphMcpConfig,
    /// Repomix MCP surface configuration.
    pub repomix: RepomixMcpConfig,
}
```

```rust
pub struct KnowledgeGraphMcpConfig {
    /// Whether the knowledge graph MCP tools are enabled.
    ///
    /// Defaults to `false` — operators must explicitly opt in.
    pub enabled: bool,
    /// Maximum number of results returned by `knowledge.search`.
    ///
    /// Defaults to 50.
    pub max_search_results: u32,
    /// Maximum graph traversal depth for `knowledge.graph_neighbors`.
    ///
    /// Capped at 4 to prevent unbounded Datalog recursion.
    pub max_graph_depth: u32,
}
```

```rust
pub struct RepomixMcpConfig {
    /// Whether the repomix MCP tools are enabled.
    ///
    /// Defaults to `false` — operators must explicitly opt in.
    pub enabled: bool,
    /// Maximum output tokens for a packed context.
    ///
    /// Defaults to `128_000` (Claude 3.5 Sonnet context window).
    pub max_output_tokens: u32,
    /// Directory containing custom `.repomix` template files.
    ///
    /// When `None`, built-in templates (`single_crate`, `crate_with_deps`,
    /// `cross_crate`) are used.
    pub templates_dir: Option<String>,
}
```

```rust
pub struct McpRateLimitConfig {
    /// Whether MCP rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute for expensive operations.
    pub message_requests_per_minute: u32,
    /// Maximum requests per minute for read/status operations.
    pub read_requests_per_minute: u32,
}
```

## `src/config/mod.rs`

```rust
pub struct ObservabilitySettings {
    /// Install the episteme trace-ingest subscriber layer and flush ops facts
    /// into the knowledge store. Default: true.
    #[serde(alias = "trace_ingest")]
    pub trace_ingest: bool,
}
```

```rust
pub struct AletheiaConfig {
    /// Agent definitions and shared defaults.
    pub agents: AgentsConfig,
    /// HTTP gateway settings (port, bind address, auth, TLS, CORS).
    pub gateway: GatewayConfig,
    /// Messaging transport configuration (Signal, etc.).
    pub channels: ChannelsConfig,
    /// Routes mapping channel sources to nous agents.
    pub bindings: Vec<ChannelBinding>,
    /// Embedding provider configuration for the recall pipeline.
    pub embedding: EmbeddingSettings,
    /// External domain pack paths (directories containing pack.toml).
    pub packs: Vec<PathBuf>,
    /// Periodic maintenance task configuration (trace rotation, drift detection, etc.).
    pub maintenance: MaintenanceSettings,
    /// Per-model pricing for LLM cost metrics. Keyed by model name.
    pub pricing: HashMap<String, ModelPricing>,
    /// Sandbox configuration for tool execution.
    pub sandbox: SandboxSettings,
    /// Credential resolution configuration.
    pub credential: CredentialConfig,
    /// Structured file logging configuration.
    pub logging: LoggingSettings,
    /// Runtime observability feature toggles.
    pub observability: ObservabilitySettings,
    /// MCP server configuration.
    pub mcp: McpConfig,
    /// Training data capture configuration.
    pub training: eidos::training::TrainingConfig,
    /// Deployment-tunable timeout thresholds.
    ///
    /// WHY configurable: LLM call timeouts vary by provider and network
    /// conditions; operators running behind proxies or on slow links need to
    /// adjust without code changes.
    pub timeouts: TimeoutsConfig,
    /// Deployment-tunable capacity limits for tool output and context windows.
    ///
    /// WHY configurable: tool output truncation and Opus context upgrade
    /// thresholds depend on host hardware and model provider limits.
    pub capacity: CapacityConfig,
    /// Deployment-tunable LLM retry and backoff parameters.
    ///
    /// WHY configurable: retry aggressiveness must adapt to provider SLAs and
    /// cost constraints; operators may want zero retries in latency-sensitive
    /// deployments or more retries behind rate-limited providers.
    pub retry: RetrySettings,
    /// Nous actor/manager health, restart, GC, and loop-detection settings.
    ///
    /// WHY configurable: actor inbox sizes, session caps, and health poll
    /// intervals depend on workload characteristics and host resources.
    pub nous_behavior: NousBehaviorConfig,
    /// Episteme conflict resolution, decay, and extraction parameters.
    ///
    /// WHY configurable: knowledge extraction thresholds and conflict
    /// resolution aggressiveness vary by deployment use case (research vs
    /// production, single-agent vs multi-agent).
    pub knowledge: KnowledgeConfig,
    /// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
    ///
    /// WHY configurable: non-streaming timeouts and concurrency limits depend
    /// on provider rate limits and latency characteristics. Complexity
    /// thresholds control model routing (Haiku vs Opus) which affects cost.
    pub provider_behavior: ProviderBehaviorConfig,
    /// Pylon request size and idempotency limits.
    ///
    /// WHY configurable: API body size limits, idempotency cache capacity,
    /// and history pagination defaults vary by deployment scale and client
    /// requirements.
    pub api_limits: ApiLimitsConfig,
    /// Daemon watchdog, prosoche, and runner output settings.
    ///
    /// WHY configurable: watchdog backoff and anomaly detection sensitivity
    /// depend on system stability requirements and agent workload patterns.
    pub daemon_behavior: DaemonBehaviorConfig,
    /// Recurring dispatch task configuration (cron-scheduled prompt runs).
    #[serde(default)]
    pub dispatch: DispatchConfig,
    /// Organon tool size and timeout limits.
    ///
    /// WHY configurable: filesystem write caps, subprocess timeouts, and
    /// message size limits must match the deployment's security posture and
    /// resource constraints.
    pub tool_limits: ToolLimitsConfig,
    /// Agora messaging transport poll, buffer, and circuit-breaker settings.
    ///
    /// WHY configurable: poll intervals and buffer sizes depend on channel
    /// message volume; circuit-breaker thresholds must balance reliability
    /// against false positives in flaky network conditions.
    pub messaging: MessagingConfig,
    /// Self-tuning feedback loop configuration.
    ///
    /// WHY configurable: tuning is disabled by default (experimental). The
    /// global kill switch and evidence thresholds let operators enable and
    /// tune the feedback loop incrementally.
    pub tuning: TuningConfig,
    /// Anthropic-specific sovereignty and privacy settings (#3410, #3406, #3409).
    ///
    /// WHY configurable: prompt caching stores operator system prompts on
    /// Anthropic servers. The default (`disabled`) is sovereignty-first;
    /// operators who accept the tradeoff may opt in to reduce per-turn token
    /// cost.
    pub anthropic: AnthropicConfig,
    /// JWT validation tuning (clock-skew leeway, etc.).
    ///
    /// WHY configurable: clock drift between issuer and validator can
    /// immediately invalidate fresh tokens. Default 30s leeway tolerates
    /// typical NTP drift; operators on tightly synchronized hosts may
    /// lower this, and those behind mis-synced proxies may raise it.
    pub jwt: JwtSettings,
    /// LLM provider definitions (#3424, #3414).
    ///
    /// Ordered list of backends — the provider registry routes each request
    /// to the first entry that claims the requested model. Empty by default
    /// for backward compatibility: when empty, the runtime falls back to the
    /// legacy single-Anthropic setup driven by [`Self::anthropic`] and the
    /// top-level credential chain. Populate this to enable OpenAI-compatible
    /// endpoints (local llama.cpp/ollama/vllm, other cloud APIs) or to
    /// declare explicit deployment targets for the factsensitivity filter.
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    /// Prompt audit log: operator visibility into outbound LLM requests (#3411).
    ///
    /// WHY configurable: operators can disable the log or tune retention and
    /// filtered-ID inclusion. Default is on with 90-day retention because
    /// the log is a sovereignty feature — operators should be able to see
    /// what the system sent out without opting in.
    pub prompt_audit: PromptAuditSettings,
}
```

```rust
pub enum SandboxEnforcementMode {
    /// Reject tool calls that violate sandbox policy.
    Enforcing,
    /// Allow tool calls that violate sandbox policy but log a warning.
    Permissive,
}
```

```rust
pub enum EgressPolicy {
    /// Block all outbound network from child processes.
    Deny,
    /// No egress filtering; child processes have full network access.
    #[default]
    Allow,
    /// Permit only connections to listed destinations.
    Allowlist,
}
```

```rust
pub enum AgencyLevel {
    /// No practical limits on tool iterations (10 000 cap).
    Unrestricted,
    /// Balanced defaults for typical agent use.
    #[default]
    Standard,
    /// Conservative limits matching pre-expansion behavior.
    Restricted,
}
```

```rust
pub struct ModelPricing {
    /// Cost per million input tokens (USD).
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens (USD).
    pub output_cost_per_mtok: f64,
}
```

```rust
pub struct ChannelBinding {
    /// Channel type (e.g., "signal").
    pub channel: String,
    /// Source pattern: phone number, group ID, or "*" for default.
    pub source: String,
    /// Nous ID to route to.
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde config field: nous_id is a TOML routing string, not a runtime domain identifier
    pub nous_id: String,
    /// Session key pattern. Supports `{source}` and `{group}` placeholders.
    #[serde(default = "default_session_pattern")]
    // kanon:ignore RUST/plain-string-secret
    pub session_key: String,
}
```

```rust
pub struct EmbeddingSettings {
    /// Provider type: "mock", "candle".
    pub provider: String,
    /// Provider-specific model name.
    pub model: Option<String>,
    /// Output vector dimension (must match knowledge store HNSW index).
    pub dimension: usize,
}
```

```rust
pub struct ChannelsConfig {
    /// Signal messenger transport configuration.
    pub signal: SignalConfig,
    /// Matrix messenger transport configuration.
    pub matrix: MatrixConfig,
}
```

```rust
pub struct SignalConfig {
    /// Whether the Signal channel is active.
    pub enabled: bool,
    /// Named Signal accounts keyed by account label.
    pub accounts: HashMap<String, SignalAccountConfig>,
}
```

```rust
pub struct SignalAccountConfig {
    /// Whether this account is active.
    pub enabled: bool,
    /// Hostname for the signal-cli JSON-RPC HTTP interface.
    pub http_host: String,
    /// Port for the signal-cli JSON-RPC HTTP interface.
    pub http_port: u16,
    /// Whether to auto-start the receive loop for this account on server boot.
    pub auto_start: bool,
}
```

```rust
pub struct MatrixConfig {
    /// Whether the Matrix channel is active.
    pub enabled: bool,
    /// Named Matrix accounts keyed by account label.
    pub accounts: HashMap<String, MatrixAccountConfig>,
}
```

```rust
pub struct MatrixAccountConfig {
    /// Whether this account is active.
    pub enabled: bool,
    /// Matrix homeserver base URL, e.g. `https://matrix.example.org`.
    pub homeserver: String,
    /// Environment variable that contains the Matrix access token.
    // kanon:ignore RUST/plain-string-secret
    pub access_token_env: String,
    /// Matrix user ID for this account. Used to ignore echoed self messages.
    pub user_id: Option<String>,
    /// Whether to auto-start the `/sync` receive loop on server boot.
    pub auto_start: bool,
    /// Optional initial `/sync` since token.
    pub initial_since: Option<String>,
}
```

## `src/config/resolved.rs`

```rust
pub struct ResolvedModelConfig {
    /// Primary model identifier.
    pub primary: Arc<str>,
    /// Ordered fallback models.
    pub fallbacks: Vec<Arc<str>>,
    /// How many times to retry the current model before trying the next fallback.
    pub retries_before_fallback: u32,
}
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
pub fn resolve_nous (config: &AletheiaConfig, nous_id: &str) -> ResolvedNousConfig
```

## `src/encrypt.rs`

```rust
pub fn primary_key_path () -> Option<PathBuf>
```

```rust
pub fn load_primary_key (path: &Path) -> Result<Option<[u8; KEY_LEN]>>
```

```rust
pub fn generate_primary_key (path: &Path) -> Result<()>
```

```rust
pub fn encrypt_config_file (toml_path: &Path, primary_key: &[u8; KEY_LEN]) -> Result<usize>
```

## `src/error.rs`

```rust
pub enum Error {
    /// The instance root directory does not exist.
    #[snafu(display("instance root not found: {}", path.display()))]
    InstanceNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A required configuration file was not found.
    #[snafu(display("config not found: {}", path.display()))]
    ConfigNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read a configuration file.
    #[snafu(display("failed to read config from {}", path.display()))]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse JSON configuration.
    #[snafu(display("failed to parse JSON config at {}", path.display()))]
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// TOML parse error during configuration loading.
    #[snafu(display("failed to parse TOML config: {source}"))]
    ParseToml {
        source: toml::de::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Configuration loading failed (cascade merge or deserialisation).
    #[snafu(display("configuration load failed: {reason}: {source}"))]
    ConfigLoad {
        reason: String,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Configuration loading failed with a free-form reason.
    #[snafu(display("configuration load failed: {reason}"))]
    Load {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize configuration to TOML.
    #[snafu(display("failed to serialize config to TOML: {reason}"))]
    SerializeToml {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write configuration to disk.
    #[snafu(display("failed to write config to {}", path.display()))]
    WriteConfig {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The instance root directory does not exist (startup validation).
    #[snafu(display(
        "instance root not found: {}\n  help: set ALETHEIA_ROOT or run `aletheia init`",
        path.display()
    ))]
    InstanceRootNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A required subdirectory (config/ or data/) is missing from the instance root.
    #[snafu(display(
        "required directory missing: {}\n  help: run `aletheia init` to create the instance layout",
        path.display()
    ))]
    RequiredDirMissing {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The data directory is not writable.
    #[snafu(display(
        "data directory is not writable: {}\n  help: check permissions or run `aletheia init`",
        path.display()
    ))]
    NotWritable {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A workspace path from agent config does not resolve to a directory.
    #[snafu(display(
        "agent workspace path does not exist: {}\n  help: create the directory or update the workspace path in config",
        path.display()
    ))]
    WorkspacePathInvalid {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The primary key file is invalid (wrong length, bad hex).
    #[snafu(display("invalid primary key at {}: {reason}", path.display()))]
    InvalidPrimaryKey {
        path: PathBuf,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The primary key file already exists.
    #[snafu(display(
        "primary key already exists at {}\n  help: delete the file first if you want to regenerate",
        path.display()
    ))]
    PrimaryKeyExists {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Encryption operation failed.
    #[snafu(display("encryption failed: {reason}"))]
    Encrypt {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Decryption operation failed.
    #[snafu(display("decryption failed: {reason}"))]
    Decrypt {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Encrypted config fields found but decryption key is missing.
    #[snafu(display(
        "encrypted config fields cannot be decrypted (key not found): {fields}. Run 'aletheia config init-key'."
    ))]
    ConfigDecrypt {
        fields: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A `${VAR:?message}` expression in the config resolved to an unset variable.
    ///
    /// Emitted when the TOML config contains `${VAR:?some message}` and `VAR`
    /// is not present in the environment. Startup aborts with the user-supplied message.
    #[snafu(display("required env var `{var}` is not set: {message}"))]
    EnvVarRequired {
        var: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An unterminated env-var expression was found in the configuration file.
    ///
    /// Emitted when a `${` opener has no matching `}`.
    #[snafu(display("unterminated env-var expression in config file near: {}", excerpt))]
    EnvVarUnterminated {
        excerpt: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/interpolate.rs`

```rust
pub fn interpolate_env_vars (content: &str) -> Result<String>
```

## `src/loader.rs`

```rust
pub fn load_config (oikos: &Oikos) -> Result<AletheiaConfig>
```

```rust
pub fn load_config_with (oikos: &Oikos, fs: &impl FileSystem) -> Result<AletheiaConfig>
```

```rust
pub fn parse_toml_file (path: &std::path::Path) -> Result<toml::Value>
```

```rust
pub fn parse_toml_file_with (path: &std::path::Path, fs: &impl FileSystem) -> Result<toml::Value>
```

```rust
pub fn write_config (oikos: &Oikos, config: &AletheiaConfig) -> Result<()>
```

## `src/oikos.rs`

```rust
pub struct Oikos {
    root: PathBuf,
}
```

```rust
impl Oikos {
    pub fn from_root (root: impl Into<PathBuf>) -> Self;
    pub fn discover () -> Self;
    pub fn discover_with (env: &impl Environment) -> Self;
    pub fn root (&self) -> &Path;
    pub fn theke (&self) -> PathBuf;
    pub fn shared (&self) -> PathBuf;
    pub fn nous_dir (&self, id: &str) -> PathBuf;
    pub fn nous_file (&self, id: &str, filename: &str) -> PathBuf;
    pub fn config (&self) -> PathBuf;
    pub fn credentials (&self) -> PathBuf;
    pub fn data (&self) -> PathBuf;
    pub fn sessions_db (&self) -> PathBuf;
    pub fn knowledge_db (&self) -> PathBuf;
    pub fn knowledge_cohort_db (&self, cohort: &str) -> PathBuf;
    pub fn backups (&self) -> PathBuf;
    pub fn archive (&self) -> PathBuf;
    pub fn logs (&self) -> PathBuf;
    pub fn traces (&self) -> PathBuf;
    pub fn trace_archive (&self) -> PathBuf;
    pub fn validate (&self) -> crate::error::Result<()>;
    pub fn validate_workspace_path (&self, workspace: &str) -> crate::error::Result<()>;
}
```

## `src/preflight.rs`

```rust
pub struct PreconditionError {
    /// Human-readable failure messages identifying each failing resource.
    pub failures: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}
```

> Run all resource precondition checks before service initialization.
> 
> Checks disk space on the data directory, gateway port availability, and
> read/write permissions on key instance directories.
> 
> Call this after [`crate::oikos::Oikos::validate`] and config loading, but
> before starting the HTTP server or any actors.
> 
> # Errors
> 
> Returns [`PreconditionError`] with all collected failures when any check
> does not pass. The error message is human-readable and actionable.
```rust
pub fn check_preconditions (
    config: &AletheiaConfig,
    oikos: &Oikos,
) -> Result<(), PreconditionError>
```

## `src/redact.rs`

```rust
pub fn redact (config: &AletheiaConfig) -> Value
```

## `src/registry.rs`

```rust
pub enum ParameterTier {
    /// Operator sets at deployment time; not self-tunable.
    Deployment,
    /// Operator or agent may override per-agent via config.
    PerAgent,
    /// Eligible for automated tuning by the self-tuning loop.
    SelfTuning,
}
```

```rust
pub enum TuningDirection {
    /// Increasing the value generally improves the outcome signal.
    Higher,
    /// Decreasing the value generally improves the outcome signal.
    Lower,
    /// Optimal direction depends on deployment context.
    Contextual,
}
```

```rust
pub enum ParameterValue {
    /// Integer parameter.
    Int(i64),
    /// Floating-point parameter.
    Float(f64),
    /// Boolean toggle.
    Bool(bool),
    /// String-valued parameter.
    Str(&'static str),
    /// Duration in the unit described by the parameter key (seconds, milliseconds, etc.).
    Duration(u64),
}
```

```rust
pub struct ParameterSpec {
    /// Dotted config key (e.g. `"agents.defaults.behavior.distillationContextTokenTrigger"`).
    pub key: &'static str,
    /// Config section this parameter lives in.
    pub section: &'static str,
    /// Who should tune this parameter.
    pub tier: ParameterTier,
    /// Default value compiled into the binary.
    pub default: ParameterValue,
    /// Optional `(min, max)` numeric bounds.
    pub bounds: Option<(f64, f64)>,
    /// Whether the parameter can be changed without restarting.
    pub hot_reloadable: bool,
    /// Human-readable description.
    pub description: &'static str,
    /// Which subsystem behavior this parameter affects.
    pub affects: &'static str,
    /// Outcome signal that a tuning loop should optimise for.
    pub outcome_signal: &'static str,
    /// What evidence is needed before changing this parameter.
    pub evidence_required: &'static str,
    /// Hint for which direction improves the outcome signal.
    pub direction_hint: TuningDirection,
}
```

```rust
pub fn all_specs () -> &'static [ParameterSpec]
```

```rust
pub fn specs_by_section (section: &str) -> Vec<&'static ParameterSpec>
```

```rust
pub fn specs_affecting (outcome: &str) -> Vec<&'static ParameterSpec>
```

```rust
pub fn spec_by_key (key: &str) -> Option<&'static ParameterSpec>
```

## `src/reload.rs`

```rust
pub fn restart_prefixes () -> &'static [&'static str]
```

> Return `staged` with every restart-required changed path restored from `current`.
> 
> The returned config is the live/effective view. Callers may still persist the
> staged config to disk, but must not broadcast cold values as live runtime state.
> 
> # Errors
> 
> Returns [`serde_json::Error`] if the restored JSON cannot deserialize back
> into [`AletheiaConfig`].
```rust
pub fn preserve_restart_required_values (
    current: &AletheiaConfig,
    staged: &AletheiaConfig,
    diff: &ConfigDiff,
) -> Result<AletheiaConfig, serde_json::Error>
```

```rust
pub struct ConfigChange {
    /// Dotted path to the changed field (e.g. `agents.defaults.thinkingBudget`).
    pub path: String,
    /// Whether this change requires a restart to take effect.
    pub restart_required: bool,
}
```

```rust
pub struct ConfigDiff {
    /// Fields that changed between old and new config.
    pub changes: Vec<ConfigChange>,
}
```

```rust
impl ConfigDiff {
    pub fn is_empty (&self) -> bool;
    pub fn hot_changes (&self) -> Vec<&ConfigChange>;
    pub fn cold_changes (&self) -> Vec<&ConfigChange>;
}
```

```rust
pub fn diff_configs (old: &AletheiaConfig, new: &AletheiaConfig) -> ConfigDiff
```

> Log all changes from a config diff at appropriate levels.
> 
> Cold changes (those requiring restart) are logged at `warn` level with
> an explicit message that the new value is staged but not yet effective.
> This satisfies the observability contract: the system's reported state
> must reflect its actual state.
```rust
pub fn log_diff (diff: &ConfigDiff)
```

```rust
pub enum ReloadError {
    /// Failed to load config from disk.
    #[snafu(display("failed to load config: {source}"))]
    Load {
        source: crate::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// New config failed validation; old config is preserved.
    #[snafu(display("config validation failed: {source}"))]
    Validation {
        source: crate::validate::ValidationError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Outcome of a successful reload preparation.
```rust
pub struct ReloadOutcome {
    /// The validated new config ready to be swapped in.
    pub new_config: AletheiaConfig,
    /// Diff between the old (current) and new config.
    pub diff: ConfigDiff,
}
```

```rust
pub fn prepare_reload (
    oikos: &Oikos,
    current: &AletheiaConfig,
) -> Result<ReloadOutcome, ReloadError>
```

## `src/test_support.rs`

> RAII fixture that owns a fresh temp directory and restores any env vars
> it set (or cleared) when it is dropped.
> 
> WHY: replaces `figment::Jail` after #3447 (figment replacement). The jail
> protects a test's env-var mutations from leaking to sibling tests.
```rust
pub struct EnvJail {
    _lock: MutexGuard<'static, ()>,
    dir: TempDir,
    saved: HashMap<OsString, Option<OsString>>,
    canonical_dir: PathBuf,
}
```

```rust
impl EnvJail {
    pub fn new () -> Self;
    pub fn directory (&self) -> &Path;
    pub fn set_env <K: AsRef<str>, V: AsRef<str>> (&mut self, key: K, value: V);
    pub fn remove_env <K: AsRef<str>> (&mut self, key: K);
    pub fn create_file <P: AsRef<Path>> (&self, rel: P, contents: &str);
}
```

## `src/validate.rs`

```rust
pub struct ValidationError {
    /// Collected validation error messages.
    pub errors: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}
```

```rust
pub fn validate_startup (config: &AletheiaConfig, oikos: &Oikos) -> Result<(), ValidationError>
```

```rust
pub fn validate_section (section: &str, value: &Value) -> Result<(), ValidationError>
```

> Environment variable operators must set to `1` in order to accept a config
> write that sets `gateway.auth.mode = "none"`.
> 
> WHY: Disabling authentication removes all access control from the HTTP API.
> Requiring an explicit opt-in prevents a remote PUT /api/v1/config/gateway
> from silently turning off auth. (#3383)
```rust
pub const ALLOW_AUTH_NONE_ENV: &str = "ALETHEIA_ALLOW_AUTH_NONE";
```

> Environment variable operators must set to `1` to allow the server to bind
> to a non-localhost address while running with `gateway.auth.mode = "none"`.
> 
> WHY: disabled-auth on localhost is a supported local-dev shape; disabled-auth
> on a LAN or Tailscale bind is an insecure-by-default posture we refuse to
> boot into. Operators who genuinely want unauthenticated LAN access must
> flip this knob explicitly. The variable is distinct from
> [`ALLOW_AUTH_NONE_ENV`] because disabling auth locally is a meaningfully
> smaller blast radius than disabling auth and exposing it to the tailnet.
> (#3716)
```rust
pub const ALLOW_AUTH_NONE_LAN_ENV: &str = "ALETHEIA_ALLOW_AUTH_NONE_LAN";
```

```rust
pub fn is_loopback_bind (addr: &str) -> bool
```

```rust
pub fn auth_none_lan_opt_in_enabled () -> bool
```

```rust
pub fn validate_auth_mode_policy (gateway_value: &Value) -> Result<(), ValidationError>
```

```rust
pub fn auth_none_opt_in_enabled () -> bool
```

> Emit a loud startup warning when authentication is disabled.
> 
> Called after the initial config load. Emits a single `warn!` event with the
> prefix `SECURITY: auth disabled` so operators running with `auth_mode = "none"`
>  -  even intentionally  -  see the consequence in every log aggregator. (#3383)
```rust
pub fn warn_if_auth_disabled (config: &AletheiaConfig)
```

## `src/workspace_schema.rs`

```rust
pub enum RequirementKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
}
```

```rust
pub struct WorkspaceRequirement {
    /// Path relative to the workspace root.
    pub path: &'static str,
    /// Whether the path must be a file or a directory.
    pub kind: RequirementKind,
}
```

```rust
pub struct WorkspaceSchemaError {
    /// Path to the workspace root that failed validation.
    pub workspace: PathBuf,
    /// Human-readable description of each missing entry.
    pub failures: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}
```

```rust
pub struct WorkspaceSchema {
    requirements: Vec<WorkspaceRequirement>,
}
```

```rust
impl WorkspaceSchema {
    pub fn new () -> Self;
    pub fn standard () -> Self;
    pub fn require (mut self, req: WorkspaceRequirement) -> Self;
    pub fn validate (&self, workspace: &Path) -> Result<(), WorkspaceSchemaError>;
}
```

## `tests/common/mod.rs`

> Create a temp instance root with the minimal `config/`, `data/`, and
> `nous/` subdirectories that `Oikos::validate` requires.
```rust
pub fn make_valid_instance () -> TempDir
```

> Write a config file into `<instance>/config/aletheia.toml`.
```rust
pub fn write_toml (instance: &Path, body: &str)
```
