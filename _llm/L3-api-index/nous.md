# L3 API Index: nous

Crate path: `crates/nous`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/actor/mod.rs`

> Default bounded channel capacity for the actor inbox.
```rust
pub const DEFAULT_INBOX_CAPACITY: usize = 32;
```

> A single nous agent running as a Tokio actor.
> 
> Each actor owns its mutable state and processes messages sequentially
> from a bounded inbox. External code interacts via [`NousHandle`](crate::handle::NousHandle).
```rust
pub struct NousActor {
    id: String,
    config: NousConfig,
    pipeline_config: PipelineConfig,
    extra_bootstrap: Vec<BootstrapSection>,
    channel: ActorChannel,
    sessions: HashMap<String, SessionState>,
    active_session: Option<String>,
    services: ActorServices,
    stores: ActorStores,
    runtime: ActorRuntime,
    /// Per-session quality drift detectors keyed by session key.
    ///
    /// // WHY: Drift is tracked per-session, not globally, because different
    /// // sessions may have different quality baselines. A coding session
    /// // naturally has different tool-error patterns than a research session.
    drift_detectors: HashMap<String, DriftDetector>,
    /// Deployment-level behavioral configuration (panic thresholds, timeouts).
    pub(crate) nous_behavior: taxis::config::NousBehaviorConfig,
}
```

```rust
impl NousActor {
    pub async fn run (mut self);
}
```

## `src/actor/spawn.rs`

> Parameters for daemon-initiated child agent spawning.
> 
> WHY: the daemon coordinator needs to spawn child agents with a subset of
> the parent's runtime dependencies. This struct collects the required
> parameters so the binary crate can wire daemon spawns through to the
> nous actor system.
```rust
pub struct DaemonSpawnParams {
    /// Agent configuration for the child.
    pub config: NousConfig,
    /// Pipeline configuration.
    pub pipeline_config: PipelineConfig,
    /// LLM provider registry (shared with parent).
    pub providers: Arc<ProviderRegistry>,
    /// Tool registry (shared with parent).
    pub tools: Arc<ToolRegistry>,
    /// Workspace organization.
    pub oikos: Arc<Oikos>,
    /// Optional embedding provider (shared with parent).
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Optional vector search (shared with parent).
    pub vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    /// Optional session store (shared with parent).
    pub session_store: Option<Arc<Mutex<SessionStore>>>,
    /// Optional knowledge store (shared with parent).
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Optional tool services (shared with parent).
    pub tool_services: Option<Arc<organon::types::ToolServices>>,
    /// Additional bootstrap sections for the child agent.
    pub extra_bootstrap: Vec<BootstrapSection>,
}
```

```rust
pub fn spawn_for_daemon (
    params: DaemonSpawnParams,
    cancel: CancellationToken,
) -> (
    NousHandle,
    tokio::task::JoinHandle<()>,
    Arc<AtomicBool>,
    Arc<AtomicU64>,
)
```

## `src/adapters.rs`

> Adapts `SessionStore` note methods to the `NoteStore` trait.
> 
> The inner lock guards `SQLite` write access; acquired via `block_in_place`
> to avoid holding it across async boundaries.
```rust
pub struct SessionNoteAdapter(pub Arc<Mutex<SessionStore>>);
```

> Adapts `SessionStore` blackboard methods to the `BlackboardStore` trait.
> 
> The inner lock guards `SQLite` write access; acquired via `block_in_place`
> to avoid holding it across async boundaries.
```rust
pub struct SessionBlackboardAdapter(pub Arc<Mutex<SessionStore>>);
```

## `src/audit.rs`

```rust
pub enum PromptAuditError {
    /// Failed to create the audit log directory.
    #[snafu(display("failed to create prompt audit directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the daily JSONL file for appending.
    #[snafu(display("failed to open prompt audit file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a record to JSON.
    #[snafu(display("failed to serialize prompt audit record: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a record to the JSONL file.
    #[snafu(display("failed to write prompt audit record to {}: {source}", path.display()))]
    WriteRecord {
        path: PathBuf,
        source: std::io::Error,
    },
}
```

> Result alias for prompt audit operations.
```rust
pub type Result<T> = std::result::Result<T, PromptAuditError>;
```

> Deployment target classification for a request.
> 
> WHY(stub): #3404 will introduce a proper `DeploymentTarget` enum shared with
> eidos/hermeneus for the fact sensitivity recall filter. Until then, the
> audit log carries a simple string default of `"cloud"` so the schema is
> stable and the follow-up PR only has to swap the field type.
```rust
pub type DeploymentTarget = String;
```

> Sensitivity classification of a fact that was filtered from recall.
> 
> WHY(stub): Same as [`DeploymentTarget`]  -  #3404 owns the canonical enum.
> The audit log keeps the field in the schema now so `PromptAuditRecord` is
> stable; the filtered-facts vector defaults to empty until the filter lands.
```rust
pub type FactSensitivity = String;
```

```rust
pub struct FilteredFact {
    /// Fact identifier from the knowledge store.
    pub id: String,
    /// Sensitivity tier that caused the filter to exclude the fact.
    pub sensitivity: FactSensitivity,
}
```

```rust
pub struct PromptAuditRecord {
    /// When the request was assembled (UTC).
    pub timestamp: Timestamp,
    /// Nous agent identifier (e.g. `"syn"`).
    pub nous_id: String,
    /// Session identifier.
    pub session_id: String,
    /// Turn identifier (ULID). Stable across actor restarts for a given turn.
    pub turn_id: String,
    /// LLM provider name (`"anthropic"`, `"cc"`, etc.).
    pub provider: String,
    /// Deployment target (cloud/local/…). See [`DeploymentTarget`].
    pub deployment_target: DeploymentTarget,
    /// Model identifier passed to the provider.
    pub model: String,
    /// SHA-256 of the system prompt (hex). Empty string when no system prompt.
    pub system_prompt_hash: String,
    /// Byte length of the system prompt.
    pub system_prompt_bytes: usize,
    /// Number of conversation messages in the request.
    pub message_count: usize,
    /// Rough token count estimate for the request.
    pub token_count_estimate: u32,
    /// Fact IDs whose content was included in the recall section.
    pub fact_ids_included: Vec<String>,
    /// Facts excluded from recall by the sensitivity filter (#3404).
    #[serde(default)]
    pub fact_ids_filtered: Vec<FilteredFact>,
    /// Names of tools exposed to the model for this request.
    pub tool_names: Vec<String>,
    /// Request identifier propagated from pylon middleware (#3384).
    #[serde(default)]
    pub request_id: Option<String>,
}
```

```rust
pub fn hash_system_prompt (prompt: Option<&str>) -> String
```

```rust
pub struct PromptAuditLog {
    inner: Mutex<PromptAuditLogInner>,
    /// Whether logging is active. When `false`, [`PromptAuditLog::log_request`]
    /// is a no-op that does not touch the filesystem.
    enabled: bool,
    log_dir: PathBuf,
}
```

```rust
impl PromptAuditLog {
    pub fn new (log_dir: PathBuf, enabled: bool) -> Self;
    pub fn log_dir (&self) -> &Path;
    pub fn enabled (&self) -> bool;
    pub fn log_request (&self, record: &PromptAuditRecord) -> Result<()>;
}
```

## `src/bootstrap/mod.rs`

> Default TTL for bootstrap file cache entries when no operator override is set.
> 
> // WHY: 60s balances freshness (operator edits to SOUL.md/USER.md should
> // surface within about a minute) against the cost of re-reading every
> // workspace file on every turn. mtime-based invalidation catches edits
> // sooner when they happen.
```rust
pub const DEFAULT_BOOTSTRAP_CACHE_TTL_SECS: u64 = 60;
```

```rust
pub struct BootstrapFileCache {
    entries: RwLock<HashMap<PathBuf, CachedFile>>,
    ttl: Duration,
}
```

```rust
impl BootstrapFileCache {
    pub fn new (ttl: Duration) -> Self;
    pub fn with_ttl_secs (ttl_secs: u64) -> Self;
    pub fn clear (&self);
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
}
```

```rust
pub enum SectionPriority {
    /// Must be included. Missing = error.
    Required = 0,
    /// Should be included if present. Missing = skip silently.
    Important = 1,
    /// Can be truncated (oldest content removed first).
    Flexible = 2,
    /// Dropped first under budget pressure.
    Optional = 3,
}
```

```rust
pub enum TaskHint {
    /// Load all workspace files. Default for backward compatibility.
    #[default]
    General,
    /// Coding task: loads TOOLS, CHECKLIST, MEMORY.
    Coding,
    /// Research or information gathering: loads GOALS, CONTEXT, MEMORY.
    Research,
    /// Planning or architecture: loads GOALS, AGENTS, CONTEXT.
    Planning,
    /// Quick question or casual conversation: identity files only.
    Conversation,
}
```

```rust
pub enum LlmRecipe {
    /// Load L1 as Required, L3 as Optional. Used on first turn (cold start).
    #[default]
    ColdStart,
    /// Load L1 as Optional, L3 as Optional. Used for general in-session turns.
    InSession,
    /// Load L1 as Important, L3 as Important. Used for planning and refactoring.
    Refactor,
    /// Skip all `_llm/` content.
    None,
}
```

```rust
impl LlmRecipe {
    pub fn from_task_hint (task_hint: TaskHint, is_cold_start: bool) -> Self;
}
```

```rust
pub struct BootstrapSection {
    /// Section name (e.g. "SOUL.md", "tools-summary").
    pub name: String,
    /// Priority level.
    pub priority: SectionPriority,
    /// The text content.
    pub content: String,
    /// Estimated token count.
    pub tokens: u64,
    /// Whether this section can be truncated (vs dropped entirely).
    pub truncatable: bool,
}
```

```rust
pub struct BootstrapResult {
    /// The assembled system prompt text.
    pub system_prompt: String,
    /// Section names that were included (in order).
    pub sections_included: Vec<String>,
    /// Section names that were truncated.
    pub sections_truncated: Vec<String>,
    /// Section names that were dropped entirely.
    pub sections_dropped: Vec<String>,
    /// Workspace file names filtered out by the task hint (never loaded).
    pub sections_filtered: Vec<String>,
    /// Total estimated tokens consumed by the system prompt.
    pub total_tokens: u64,
    /// The task hint used for conditional loading.
    pub task_hint: TaskHint,
}
```

> Assembles the bootstrap system prompt from oikos workspace files.
> 
> Resolves files through the three-tier cascade (`nous/{id}/` → `shared/` → `theke/`),
> reads contents, estimates tokens, and packs sections in priority order.
> 
> Workspace file reads are served from an optional [`BootstrapFileCache`]
> when one is attached via [`new_with_cache`](Self::new_with_cache). Without
> a cache, every call re-reads every file from disk (legacy behaviour).
```rust
pub struct BootstrapAssembler<'a> {
    oikos: &'a Oikos,
    estimator: CharEstimator,
    /// Minimum tokens remaining before attempting truncation (below this, just drop).
    /// Default read from [`taxis::config::AgentBehaviorDefaults::bootstrap_min_truncation_budget`].
    min_truncation_budget: u64,
    /// Shared file cache: `None` disables caching (legacy path, used by tests
    /// that want guaranteed fresh reads).
    cache: Option<&'a BootstrapFileCache>,
    /// Recipe for loading `_llm/` content. `None` skips _llm/ entirely.
    llm_recipe: LlmRecipe,
}
```

```rust
impl <'a> BootstrapAssembler<'a> {
    pub fn new (oikos: &'a Oikos) -> Self;
    pub fn new_with_chars_per_token (oikos: &'a Oikos, chars_per_token: u64) -> Self;
    pub fn with_cache (mut self, cache: &'a BootstrapFileCache) -> Self;
    pub fn with_llm_recipe (mut self, recipe: LlmRecipe) -> Self;
    pub async fn assemble (
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
    ) -> Result<BootstrapResult>;
    pub async fn assemble_with_extra (
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
    ) -> Result<BootstrapResult>;
    pub async fn assemble_conditional (
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
        hint: TaskHint,
    ) -> Result<BootstrapResult>;
    pub async fn assemble_conditional_with_recipe (
        &self,
        nous_id: &str,
        budget: &mut TokenBudget,
        extra_sections: Vec<BootstrapSection>,
        hint: TaskHint,
        recipe: LlmRecipe,
    ) -> Result<BootstrapResult>;
    pub fn estimate_tokens (&self, text: &str) -> u64;
}
```

```rust
pub fn classify_task_hint (content: &str) -> TaskHint
```

> Convert domain pack sections into bootstrap sections.
> 
> Maps thesauros [`PackSection`] values to [`BootstrapSection`] values,
> computing token estimates for each section's content. Section names
> are prefixed with the pack name for traceability.
```rust
pub fn pack_sections_to_bootstrap (
    sections: &[&PackSection],
    estimator: &CharEstimator,
) -> Vec<BootstrapSection>
```

## `src/bootstrap/tools.rs`

```rust
pub struct ToolSummary {
    /// Tool name.
    pub name: String,
    /// One-line description (max 80 chars).
    pub one_liner: String,
}
```

## `src/budget.rs`

> Character-based token estimator: 1 token ≈ N characters (ceiling division).
> 
> Conservative estimate suitable for budget planning. Actual token counts
> from the Anthropic API will be lower, giving natural headroom.
> `chars_per_token` is configurable via `agents.defaults.chars_per_token`
> in `aletheia.toml`; the default of 4 preserves prior behaviour.
```rust
pub struct CharEstimator {
    pub(crate) chars_per_token: u64,
}
```

```rust
impl CharEstimator {
    pub fn new (chars_per_token: u64) -> Self;
    pub fn chars_per_token (&self) -> u64;
    pub fn estimate (&self, text: &str) -> u64;
}
```

```rust
pub struct TokenBudget {
    context_window: u64,
    reserved_for_turn: u64,
    reserved_for_history: u64,
    system_budget: u64,
    consumed: u64,
}
```

```rust
impl TokenBudget {
    pub fn new (
        context_window: u64,
        history_ratio: f64,
        turn_reserve: u64,
        bootstrap_cap: u64,
    ) -> Self;
    pub fn remaining (&self) -> u64;
    pub fn consume (&mut self, tokens: u64) -> bool;
    pub fn can_fit (&self, tokens: u64) -> bool;
    pub fn consumed (&self) -> u64;
    pub fn system_budget (&self) -> u64;
    pub fn history_budget (&self) -> u64;
    pub fn context_window (&self) -> u64;
    pub fn turn_reserve (&self) -> u64;
}
```

```rust
pub struct CompactionMetrics {
    /// Token count before compaction.
    pub pre_compact_tokens: u64,
    /// Token count after compaction.
    pub post_compact_tokens: u64,
    /// Number of tool results cleared by microcompaction.
    pub results_cleared: u32,
    /// Number of tool results preserved (last-N or unexpired).
    pub results_preserved: u32,
    /// Whether full compaction was triggered.
    pub full_compaction_triggered: bool,
}
```

```rust
impl CompactionMetrics {
    pub fn tokens_reclaimed (&self) -> u64;
}
```

## `src/compact/mod.rs`

```rust
pub struct CompactConfig {
    /// Per-tool-type time-to-live durations.
    pub ttls: HashMap<ToolResultType, SignedDuration>,
    /// Number of most-recent results per tool type to preserve regardless of age.
    pub keep_last_n: usize,
    /// Token usage ratio (0.0--1.0) that triggers full compaction.
    pub full_compact_threshold: f64,
    /// Number of most-recent turns to preserve after full compaction.
    pub preserve_turns: usize,
    /// Maximum number of critical files to re-inject after full compaction.
    pub max_critical_files: usize,
    /// Number of recent turns to scan for critical file identification.
    pub critical_file_lookback: usize,
}
```

## `src/competence/mod.rs`

```rust
pub struct CompetenceConfig {
    /// Competence score penalty per correction. Default: 0.05.
    pub correction_penalty: f64,
    /// Competence score bonus per successful turn. Default: 0.02.
    pub success_bonus: f64,
    /// Competence score penalty per user disagreement. Default: 0.01.
    pub disagreement_penalty: f64,
    /// Competence score floor. Default: 0.1.
    pub min_score: f64,
    /// Competence score ceiling. Default: 0.95.
    pub max_score: f64,
    /// Initial competence score for a new agent. Default: 0.5.
    pub default_score: f64,
    /// Competence score below which escalation fires. Default: 0.30.
    pub escalation_failure_threshold: f64,
    /// Minimum samples before escalation threshold is evaluated. Default: 5.
    pub escalation_min_samples: u32,
}
```

```rust
impl CompetenceConfig {
    pub fn from_behavior (behavior: &taxis::config::AgentBehaviorDefaults) -> Self;
}
```

```rust
pub enum TaskOutcome {
    /// Task completed successfully.
    Success,
    /// Task partially completed.
    Partial,
    /// Task failed.
    Failure,
}
```

```rust
pub struct DomainScore {
    /// Domain name (e.g., "coding", "research").
    pub domain: String,
    /// Competence score (0.0--1.0), starts at 0.5.
    pub score: f64,
    /// Total successes recorded.
    pub successes: u32,
    /// Total partial completions recorded.
    pub partials: u32,
    /// Total failures recorded.
    pub failures: u32,
    /// Operator corrections (decreases score).
    pub corrections: u32,
    /// Cross-agent disagreements (decreases score).
    pub disagreements: u32,
    /// Last update timestamp.
    pub updated_at: String,
}
```

```rust
pub struct AgentCompetence {
    /// Agent identifier.
    pub nous_id: String,
    /// Per-domain scores.
    pub domains: Vec<DomainScore>,
    /// Weighted average of domain scores.
    pub overall_score: f64,
}
```

```rust
pub struct EscalationRecommendation {
    /// Domain triggering the recommendation.
    pub domain: String,
    /// Current failure rate.
    pub failure_rate: f64,
    /// Current agent score in this domain.
    pub current_score: f64,
    /// Whether escalation to a higher-tier model is recommended.
    pub should_escalate: bool,
}
```

> Tracks agent competence per domain with fjall persistence.
```rust
pub struct CompetenceTracker {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex — serializes writers.
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
    config: CompetenceConfig,
}
```

```rust
impl CompetenceTracker {
    pub fn open (path: &Path, config: CompetenceConfig) -> error::Result<Self>;
    pub fn open_in_memory () -> error::Result<Self>;
    pub fn record_outcome (
        &self,
        nous_id: &str,
        domain: &str,
        outcome: TaskOutcome,
    ) -> error::Result<()>;
    pub fn record_correction (&self, nous_id: &str, domain: &str) -> error::Result<()>;
    pub fn record_disagreement (&self, nous_id: &str, domain: &str) -> error::Result<()>;
    pub fn score (&self, nous_id: &str, domain: &str) -> error::Result<f64>;
    pub fn agent_competence (&self, nous_id: &str) -> error::Result<AgentCompetence>;
    pub fn rolling_stats (
        &self,
        nous_id: &str,
        domain: &str,
        window_size: u32,
    ) -> error::Result<RollingStats>;
    pub fn escalation_recommendation (
        &self,
        nous_id: &str,
        domain: &str,
    ) -> error::Result<EscalationRecommendation>;
}
```

```rust
pub struct RollingStats {
    /// Configured window size.
    pub window_size: u32,
    /// Actual number of outcomes in the window.
    pub total: u32,
    /// Successes within the window.
    pub successes: u32,
    /// Partial completions within the window.
    pub partials: u32,
    /// Failures within the window.
    pub failures: u32,
}
```

```rust
impl RollingStats {
    pub fn failure_rate (&self) -> f64;
    pub fn success_rate (&self) -> f64;
}
```

## `src/config.rs`

```rust
pub fn serialize <S> (value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer,
```

```rust
pub fn deserialize <'de, D> (deserializer: D) -> Result<Arc<str>, D::Error> where
        D: serde::Deserializer<'de>,
```

```rust
pub struct NousGenerationConfig {
    /// Default model for this agent.
    pub model: String,
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
}
```

```rust
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
    /// disable. Default: 3. Closes #1980.
    pub max_consecutive_tool_only_iterations: u32,
}
```

```rust
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
    /// Server-side tools to include in API requests (e.g., web search).
    #[serde(default)]
    pub server_tools: Vec<hermeneus::types::ServerToolDefinition>,
    /// Whether prompt caching is enabled for this agent.
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    /// Per-agent recall pipeline configuration.
    #[serde(default)]
    pub recall: RecallConfig,
    /// Tool allowlist for sub-agent role enforcement.
    ///
    /// When `Some`, only the listed tool names are available during execution.
    /// When `None`, all registered tools are available. Set by role templates
    /// during ephemeral sub-agent spawning.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Turn-level hook configuration.
    #[serde(default)]
    pub hooks: HookConfig,
    /// Resolved per-agent behavioral parameters (distillation, competence, drift, etc.).
    ///
    /// Populated at startup from taxis config cascade and passed through the
    /// pipeline for all behavioral threshold reads. Defaults match the
    /// constants they replace so behaviour is identical when unconfigured.
    #[serde(default)]
    pub behavior: AgentBehaviorDefaults,
}
```

```rust
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
}
```

```rust
pub struct PipelineConfig {
    /// Token budget for history (remaining after bootstrap).
    pub history_budget_ratio: f64,
    /// Knowledge extraction configuration (None = disabled).
    #[serde(default)]
    pub extraction: Option<mneme::extract::ExtractionConfig>,
    /// Per-stage time budgets.
    #[serde(default)]
    pub stage_budget: StageBudget,
    /// Training data capture configuration.
    #[serde(default)]
    pub training: crate::training::TrainingConfig,
}
```

```rust
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
    /// Hard cap on total pipeline wall-clock time.
    pub total_secs: u32,
}
```

## `src/cross/mod.rs`

```rust
pub enum DeliveryState {
    /// Message created but not yet sent.
    Pending,
    /// Message placed in the target actor's inbox.
    Delivered,
    /// Target acknowledged receipt (reserved for future use).
    Acknowledged,
    /// A reply was received for this message.
    Replied,
    /// Delivery failed with the given reason.
    Failed { reason: String },
    /// Reply was not received within the timeout window.
    TimedOut,
}
```

```rust
pub struct CrossNousMessage {
    /// Unique message identifier.
    pub id: Ulid,
    /// Sender nous ID.
    pub from: String,
    /// Target nous ID.
    pub to: String,
    /// Session key on the target nous to inject the message into.
    pub target_session: String,
    /// Message text payload.
    pub content: String,
    /// Whether the sender expects a [`CrossNousReply`].
    pub expects_reply: bool,
    /// How long to wait for a reply before timing out.
    pub reply_timeout: Option<Duration>,
    /// When the message was created.
    pub created_at: jiff::Timestamp,
    /// Current delivery lifecycle state.
    pub delivery: DeliveryState,
}
```

```rust
impl CrossNousMessage {
    pub fn new (from: impl Into<String>, to: impl Into<String>, content: impl Into<String>) -> Self;
    pub fn with_target_session (mut self, session: impl Into<String>) -> Self;
    pub fn with_reply (mut self, timeout: Duration) -> Self;
}
```

```rust
pub struct CrossNousReply {
    /// ID of the original [`CrossNousMessage`] this replies to.
    pub in_reply_to: Ulid,
    /// Responding nous ID.
    pub from: String,
    /// Reply text payload.
    pub content: String,
    /// When the reply was created.
    pub created_at: jiff::Timestamp,
}
```

> Envelope wrapping a message and optional reply channel.
```rust
pub struct CrossNousEnvelope {
    /// The cross-nous message.
    pub message: CrossNousMessage,
}
```

## `src/cross/router.rs`

> Routes messages between nous actors using their IDs as keys.
```rust
pub struct CrossNousRouter {
    /// Maps nous id to its inbox sender. Invariant: every spawned actor has
    /// exactly one entry; removed on unregister. Held briefly during
    /// send/register/unregister.
    pub(super) routes: Arc<RwLock<HashMap<String, mpsc::Sender<CrossNousEnvelope>>>>,
    /// Maps correlation id to the one-shot reply channel for an in-flight ask.
    /// Invariant: each ask inserts one entry; consumed exactly once on reply
    /// or removed on timeout.
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    /// Append-only audit log of delivered messages. Invariant: entries are
    /// never modified after insertion; the log is read for diagnostics only.
    pub(super) delivery_log: Arc<RwLock<DeliveryLog>>,
    /// Directed graph of in-flight ask chains used for cycle detection.
    /// Invariant: an edge exists iff a pending ask is outstanding between
    /// the two nodes; removed when the reply arrives or the ask times out.
    pub(super) ask_graph: Arc<RwLock<AskGraph>>,
}
```

```rust
impl CrossNousRouter {
    pub fn new (max_log_entries: usize) -> Self;
    pub async fn register (
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        sender: mpsc::Sender<CrossNousEnvelope>,
    );
    pub async fn unregister (&self, nous_id: &str);
    pub async fn send (&self, message: CrossNousMessage) -> error::Result<DeliveryState>;
    pub async fn ask (&self, mut message: CrossNousMessage) -> error::Result<CrossNousReply>;
    pub async fn reply (&self, reply: CrossNousReply) -> error::Result<()>;
    pub async fn registered (&self) -> Vec<String>;
}
```

## `src/degraded_mode.rs`

```rust
pub enum DegradedMode {
    /// A recent distillation summary was found and returned as the response.
    DistillationCache {
        /// Human-readable status shown alongside the response.
        status_banner: String,
    },
    /// No cache available; an honest "unavailable" message was returned.
    Unavailable {
        /// Human-readable status shown alongside the response.
        status_banner: String,
    },
}
```

```rust
impl DegradedMode {
    pub fn status_banner (&self) -> &str;
}
```

```rust
pub fn is_transient_llm_error (err: &error::Error) -> bool
```

> Attempt to build a degraded [`TurnResult`] when the LLM provider is down.
> 
> # Behaviour
> 
> 1. If `recent_distillation` is `Some`, prepend a status banner and return
>    the summary as the response content with a [`DegradedMode::DistillationCache`]
>    indicator.
> 2. If `recent_distillation` is `None`, return a clear "can't help right now"
>    message with a [`DegradedMode::Unavailable`] indicator.
> 
> Either way the original error is logged at `warn` level so it remains visible
> in traces without being surfaced to the caller as a hard error.
> 
> # Parameters
> 
> - `nous_id`  -  agent identifier used for log context.
> - `session_id`  -  session identifier used for log context.
> - `original_error`  -  the transient error that triggered degradation.
> - `recent_distillation`  -  most recent distillation summary for this session,
>   if any. Callers should pass `None` when no store is available or when the
>   session has never been distilled.
```rust
pub fn build_degraded_response (
    nous_id: &str,
    session_id: &str,
    original_error: &error::Error,
    recent_distillation: Option<&str>,
) -> TurnResult
```

## `src/distillation.rs`

```rust
pub struct DistillTriggerConfig {
    /// Fraction of context window that triggers legacy threshold. Default: 0.7.
    pub max_history_share: f64,
    /// Model to use for distillation.
    pub model: String,
    /// Messages to preserve verbatim at the tail. Default: 3.
    pub verbatim_tail: usize,
    /// Context token count that unconditionally triggers distillation. Default: `120_000`.
    pub context_token_trigger: u64,
    /// Message count that unconditionally triggers distillation. Default: 150.
    pub message_count_trigger: i64,
    /// Days idle before a session is considered stale for distillation. Default: 7.
    pub stale_session_days: i64,
    /// Minimum messages required for stale-session distillation. Default: 20.
    pub stale_session_min_messages: i64,
    /// Message count trigger for sessions never distilled. Default: 30.
    pub never_distilled_trigger: i64,
    /// Minimum messages for legacy distillation threshold. Default: 10.
    pub legacy_min_messages: i64,
}
```

```rust
impl DistillTriggerConfig {
    pub fn from_behavior (behavior: &taxis::config::AgentBehaviorDefaults) -> Self;
}
```

```rust
pub fn should_trigger_distillation (
    session: &Session,
    context_window: u64,
    config: &DistillTriggerConfig,
) -> Option<String>
```

```rust
pub async fn maybe_distill (
    session_store: &SessionStore,
    provider: &dyn LlmProvider,
    session_id: &str,
    nous_id: &str,
    context_window: u64,
    config: &DistillTriggerConfig,
) -> error::Result<Option<DistillResult>>
```

> Apply distillation result to the session store.
```rust
pub fn apply_distillation (
    store: &SessionStore,
    session_id: &str,
    result: &DistillResult,
    history: &[mneme::types::Message],
) -> error::Result<()>
```

```rust
pub fn convert_to_hermeneus_messages (history: &[mneme::types::Message]) -> Vec<HermeneusMessage>
```

## `src/drift.rs`

```rust
pub struct TurnMetrics {
    /// Output tokens produced by the model.
    pub response_tokens: u64,
    /// Fraction of tool calls that returned errors (0.0--1.0).
    pub tool_error_rate: f64,
    /// Whether the user's next message was classified as a correction.
    pub user_correction: bool,
    /// Total tool calls in this turn.
    pub tool_call_count: u32,
    /// Timestamp when the turn completed.
    pub timestamp: Timestamp,
}
```

```rust
pub enum DriftMetric {
    /// Response length (tokens) dropped significantly.
    ResponseLength,
    /// Tool error rate increased significantly.
    ToolErrorRate,
    /// User correction frequency increased significantly.
    UserCorrections,
}
```

```rust
pub struct DriftEvent {
    /// Which metric drifted.
    pub metric: DriftMetric,
    /// Current value (recent window average).
    pub current: f64,
    /// Historical baseline (full window average).
    pub baseline: f64,
    /// Standard deviation of the historical window.
    pub std_dev: f64,
    /// Z-score: how many standard deviations from the baseline.
    pub z_score: f64,
    /// When the drift was detected.
    pub detected_at: Timestamp,
}
```

```rust
pub struct DriftConfig {
    /// Number of turns in the rolling window. Default: 20.
    pub window_size: usize,
    /// Number of recent turns to compare against the window. Default: 5.
    pub recent_size: usize,
    /// Z-score threshold for triggering a drift event. Default: 2.0.
    pub deviation_threshold: f64,
    /// Minimum turns in the window before drift detection activates. Default: 8.
    ///
    /// WHY: with fewer samples the standard deviation is unreliable and would
    /// produce false positives.
    pub min_samples: usize,
}
```

```rust
impl DriftConfig {
    pub fn from_behavior (behavior: &taxis::config::AgentBehaviorDefaults) -> Self;
}
```

> Rolling-window quality drift detector.
> 
> Accumulates [`TurnMetrics`] and compares the most recent `recent_size`
> turns against the full window. When a metric's z-score exceeds the
> configured threshold, a [`DriftEvent`] is produced and logged at warn
> level.
```rust
pub struct DriftDetector {
    window: VecDeque<TurnMetrics>,
    config: DriftConfig,
}
```

```rust
impl DriftDetector {
    pub fn new (config: DriftConfig) -> Self;
    pub fn record (&mut self, metrics: TurnMetrics) -> Vec<DriftEvent>;
    pub fn turn_count (&self) -> usize;
    pub fn reset (&mut self);
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// Session store error.
    #[snafu(display("session store error: {source}"))]
    Store {
        source: mneme::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider error.
    #[snafu(display("LLM error: {source}"))]
    Llm {
        source: hermeneus::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Context assembly failed.
    #[snafu(display("context assembly failed: {message}"))]
    ContextAssembly {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Context assembly failed reading a required workspace file.
    ///
    /// Preserves the original [`std::io::Error`] source so callers can inspect
    /// the OS-level failure (permission denied, missing file, etc.) without it
    /// being erased into a string message.
    #[snafu(display("context assembly failed: required file '{file}' unreadable: {source}"))]
    ContextAssemblyIo {
        file: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Workspace validation failed on actor startup.
    #[snafu(display("workspace validation failed for '{nous_id}': {message}"))]
    WorkspaceValidation {
        nous_id: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pipeline stage failed.
    #[snafu(display("pipeline stage '{stage}' failed: {message}"))]
    PipelineStage {
        stage: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Guard rejected the request.
    #[snafu(display("guard rejected: {reason}"))]
    GuardRejected {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Loop detected in tool execution.
    #[snafu(display("loop detected after {iterations} iterations: {pattern}"))]
    LoopDetected {
        iterations: u32,
        pattern: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session configuration error.
    #[snafu(display("session config error: {message}"))]
    Config {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor inbox send failed (actor shut down).
    #[snafu(display("actor send failed: {message}"))]
    ActorSend {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor reply receive failed (actor dropped reply channel).
    #[snafu(display("actor recv failed: {message}"))]
    ActorRecv {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Recall stage embedding failed.
    #[snafu(display("recall embedding failed: {message}"))]
    RecallEmbedding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Recall stage search failed.
    #[snafu(display("recall search failed: {message}"))]
    RecallSearch {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Target nous not found in the router.
    #[snafu(display("nous not found: {nous_id}"))]
    NousNotFound {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cross-nous message delivery failed (channel closed).
    #[snafu(display("delivery to '{nous_id}' failed: channel closed"))]
    DeliveryFailed {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cross-nous ask timed out waiting for reply.
    #[snafu(display("ask to '{nous_id}' timed out after {timeout_secs}s"))]
    AskTimeout {
        nous_id: String,
        timeout_secs: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Reply channel not found (already timed out or consumed).
    #[snafu(display("reply channel not found for message {message_id}"))]
    ReplyNotFound {
        message_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Cycle detected in ask chain (would deadlock).
    #[snafu(display("ask cycle detected: {chain}"))]
    AskCycleDetected {
        chain: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Distillation failed.
    #[snafu(display("distillation failed: {source}"))]
    Distillation {
        source: melete::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A mutex or rwlock was poisoned by a prior panic.
    #[snafu(display("mutex poisoned: {what}"))]
    MutexPoisoned {
        what: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A pipeline stage exceeded its time budget.
    #[snafu(display("pipeline stage '{stage}' timed out after {timeout_secs}s"))]
    PipelineTimeout {
        stage: String,
        timeout_secs: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor inbox is full and the send timed out.
    #[snafu(display("actor '{nous_id}' inbox full after {timeout_secs}s"))]
    InboxFull {
        nous_id: String,
        timeout_secs: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Actor is in degraded state after repeated panics.
    #[snafu(display("actor '{nous_id}' is degraded after {panic_count} panics"))]
    ServiceDegraded {
        nous_id: String,
        panic_count: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pipeline stage panicked (caught by the panic boundary).
    #[snafu(display("pipeline panic in actor '{nous_id}': {message}"))]
    PipelinePanic {
        nous_id: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Self-audit error.
    #[snafu(display("self-audit failed: {message}"))]
    SelfAudit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Role contract loading failed.
    #[snafu(display("role contract error: {message}"))]
    RoleContract {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Recipe loading or resolution failed.
    #[snafu(display("recipe error: {message}"))]
    RecipeLoading {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Competence store error.
    #[snafu(display("competence store error: {message}"))]
    CompetenceStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Uncertainty store error.
    #[snafu(display("uncertainty store error: {message}"))]
    UncertaintyStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias for results with [`Error`].
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/execute/mod.rs`

```rust
pub async fn execute (
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    hooks: Option<&HookRegistry>,
) -> error::Result<TurnResult>
```

```rust
pub async fn execute_streaming (
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
    hooks: Option<&HookRegistry>,
) -> error::Result<TurnResult>
```

## `src/finalize.rs`

```rust
pub struct FinalizeConfig {
    /// Whether to persist messages to the session store.
    pub persist_messages: bool,
    /// Whether to record usage metrics.
    pub record_usage: bool,
}
```

```rust
pub struct FinalizeResult {
    /// Number of messages persisted.
    pub messages_persisted: usize,
    /// Whether usage was recorded.
    pub usage_recorded: bool,
}
```

## `src/handle.rs`

> Default timeout for sending messages to an actor's inbox.
```rust
pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(30);
```

```rust
pub struct NousHandle {
    id: String,
    sender: mpsc::Sender<NousMessage>,
}
```

```rust
impl NousHandle {
    pub fn id (&self) -> &str;
    pub async fn send_turn (
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
    ) -> error::Result<TurnResult>;
    pub async fn send_turn_with_session_id (
        &self,
        session_key: impl Into<String>,
        session_id: Option<String>,
        content: impl Into<String>,
        timeout: Duration,
    ) -> error::Result<TurnResult>;
    pub async fn send_turn_with_timeout (
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        timeout: Duration,
    ) -> error::Result<TurnResult>;
    pub async fn send_turn_streaming (
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
    ) -> error::Result<TurnResult>;
    pub async fn send_turn_streaming_with_session_id (
        &self,
        session_key: impl Into<String>,
        session_id: Option<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        timeout: Duration,
    ) -> error::Result<TurnResult>;
    pub async fn send_turn_streaming_with_timeout (
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        timeout: Duration,
    ) -> error::Result<TurnResult>;
    pub async fn ping (&self, timeout: Duration) -> error::Result<()>;
    pub async fn status (&self) -> error::Result<NousStatus>;
    pub async fn sleep (&self) -> error::Result<()>;
    pub async fn wake (&self) -> error::Result<()>;
    pub async fn recover (&self) -> error::Result<bool>;
    pub async fn shutdown (&self) -> error::Result<()>;
}
```

## `src/history.rs`

```rust
pub struct HistoryConfig {
    /// Maximum number of history messages to load.
    pub max_messages: usize,
    /// Reserve tokens for the user's current message.
    pub reserve_for_current: i64,
    /// Whether to include tool-result messages.
    pub include_tool_messages: bool,
}
```

```rust
pub struct HistoryResult {
    /// Number of messages loaded from store.
    pub messages_loaded: usize,
    /// Total tokens consumed by loaded history.
    pub tokens_consumed: i64,
    /// Whether history was truncated to fit budget.
    pub truncated: bool,
}
```

## `src/manager.rs`

```rust
pub struct NousManager {
    actors: HashMap<String, ActorEntry>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<TokioMutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    packs: Arc<Vec<LoadedPack>>,
    router: Option<Arc<crate::cross::CrossNousRouter>>,
    tool_services: Option<Arc<ToolServices>>,
    ready_tx: watch::Sender<bool>,
    ready_rx: watch::Receiver<bool>,
    /// Root cancellation token. Child tokens are given to each actor.
    /// Cancelling this stops all actors without needing `&mut self`.
    cancel: CancellationToken,
    /// Deployment-level behavioral configuration (health intervals, restart limits).
    nous_behavior: taxis::config::NousBehaviorConfig,
    /// Prompt audit log shared across all actors (#3411).
    audit_log: Option<Arc<crate::audit::PromptAuditLog>>,
}
```

```rust
impl NousManager {
    pub fn new (
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
        session_store: Option<Arc<TokioMutex<SessionStore>>>,
        #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
        packs: Arc<Vec<LoadedPack>>,
        router: Option<Arc<crate::cross::CrossNousRouter>>,
        tool_services: Option<Arc<ToolServices>>,
        nous_behavior: taxis::config::NousBehaviorConfig,
    ) -> Self;
    pub fn with_audit_log (mut self, audit_log: Arc<crate::audit::PromptAuditLog>) -> Self;
    pub fn ready (&self);
    pub fn ready_rx (&self) -> watch::Receiver<bool>;
    pub fn router (&self) -> Option<&Arc<crate::cross::CrossNousRouter>>;
    pub async fn spawn (
        &mut self,
        config: NousConfig,
        pipeline_config: PipelineConfig,
    ) -> crate::error::Result<NousHandle>;
    pub fn get (&self, nous_id: &str) -> Option<&NousHandle>;
    pub fn secret_vault (&self) -> Option<&hermeneus::secret::SecretVault>;
    pub fn get_config (&self, nous_id: &str) -> Option<&NousConfig>;
    pub fn configs (&self) -> Vec<&NousConfig>;
    pub async fn check_health (&self) -> BTreeMap<String, ActorHealth>;
    pub async fn health_cycle (&mut self);
    pub fn start_health_poller (
        manager: Arc<TokioMutex<Self>>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> JoinHandle<()>;
    pub async fn list (&self) -> Vec<NousStatus>;
    pub async fn shutdown_all (&mut self);
    pub async fn shutdown_all_with_timeout (&mut self, timeout: Duration);
    pub async fn drain (&self, timeout: Duration);
    pub async fn shutdown_readonly (&self);
    pub fn count (&self) -> usize;
    pub async fn register_agent (&mut self, config: NousConfig) -> crate::error::Result<NousHandle>;
    pub fn knowledge_store (&self) -> Option<&Arc<KnowledgeStore>>;
}
```

## `src/message.rs`

```rust
pub enum NousLifecycle {
    /// Processing a turn or background task.
    Active,
    /// Waiting for messages, no active work.
    Idle,
    /// Paused, inbox buffered. Wakes on message or schedule.
    Dormant,
    /// Too many panics: only accepts Status and Ping queries.
    Degraded,
}
```

```rust
pub struct NousStatus {
    /// Agent identifier.
    pub id: String,
    /// Current lifecycle state.
    pub lifecycle: NousLifecycle,
    /// Number of sessions tracked.
    pub session_count: usize,
    /// Currently active session key, if any.
    pub active_session: Option<String>,
    /// Number of panics caught by the panic boundary.
    pub panic_count: u32,
    /// How long the actor has been running.
    pub uptime: Duration,
}
```

```rust
pub struct ActorHealth {
    /// Whether the actor responded to a ping in time.
    pub alive: bool,
    /// Number of panics caught since (re)start.
    pub panic_count: u32,
    /// Uptime since last (re)start.
    pub uptime: Duration,
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/pipeline/mod.rs`

```rust
pub struct PipelineInput {
    /// The user's message content.
    pub content: String,
    /// Session state.
    pub session: SessionState,
    /// Pipeline configuration.
    pub config: PipelineConfig,
}
```

```rust
pub struct PipelineContext {
    /// The assembled system prompt.
    pub system_prompt: Option<String>,
    /// Conversation history (messages to send to the LLM).
    pub messages: Vec<PipelineMessage>,
    /// Available tools for this turn.
    pub tools: Vec<String>,
    /// Token budget remaining after bootstrap (system prompt space).
    pub remaining_tokens: i64,
    /// Token budget allocated for conversation history.
    pub history_budget: i64,
    /// Whether distillation is needed before this turn.
    pub needs_distillation: bool,
    /// Guard decision.
    pub guard_result: GuardResult,
    /// Recall stage output, if recall was run.
    pub recall_result: Option<crate::recall::RecallStageResult>,
    /// History stage output, if history was loaded.
    pub history_result: Option<HistoryResult>,
    /// Working state from the previous turn (loaded from persistence).
    pub working_state: Option<WorkingState>,
    /// Compaction metrics from the most recent compaction pass.
    pub compaction_metrics: Option<CompactionMetrics>,
}
```

```rust
pub struct PipelineMessage {
    /// Message role.
    pub role: String,
    /// Message content.
    pub content: String,
    /// Estimated tokens.
    pub token_estimate: i64,
}
```

```rust
pub enum GuardResult {
    /// Request is allowed.
    Allow,
    /// Request is rate-limited (retry after ms).
    RateLimited { retry_after_ms: u64 },
    /// Loop detected: abort.
    LoopDetected { pattern: String },
    /// Request rejected for safety.
    Rejected { reason: String },
}
```

```rust
pub enum LoopVerdict {
    /// No loop detected.
    Ok,
    /// Loop pattern detected; inject a warning and continue.
    Warn {
        /// Detected pattern description.
        pattern: String,
        /// Human-readable warning to inject into conversation.
        message: String,
    },
    /// Loop confirmed after repeated warnings; halt execution.
    Halt {
        /// Detected pattern description.
        pattern: String,
        /// Human-readable halt message.
        message: String,
    },
}
```

```rust
pub struct LoopDetector {
    /// Recent tool call records (ring buffer, capped at `window` entries).
    history: VecDeque<CallRecord>,
    /// Threshold for identical consecutive calls.
    threshold: u32,
    /// Threshold for consecutive error detection.
    error_threshold: u32,
    /// Maximum warnings before escalating to halt.
    max_warnings: u32,
    /// Number of warnings issued so far.
    warnings_issued: u32,
    /// Maximum history entries retained.
    window: usize,
    /// Maximum cycle length examined during cycle detection. Default: 10.
    cycle_detection_max_len: usize,
}
```

```rust
impl LoopDetector {
    pub fn new (threshold: u32) -> Self;
    pub fn with_limits (threshold: u32, error_threshold: u32, max_warnings: u32) -> Self;
    pub fn with_behavior (
        threshold: u32,
        error_threshold: u32,
        max_warnings: u32,
        behavior: &taxis::config::NousBehaviorConfig,
    ) -> Self;
    pub fn record (&mut self, tool_name: &str, input_hash: &str, is_error: bool) -> LoopVerdict;
    pub fn reset (&mut self);
    pub fn call_count (&self) -> usize;
    pub fn pattern_count (&self) -> usize;
    pub fn warnings_issued (&self) -> u32;
}
```

```rust
pub enum InteractionSignal {
    /// Direct conversation (no tools).
    Conversation,
    /// Tool execution occurred.
    ToolExecution,
    /// Code was written or modified.
    CodeGeneration,
    /// Research or web search.
    Research,
    /// Planning or architectural discussion.
    Planning,
    /// Error recovery.
    ErrorRecovery,
}
```

```rust
pub struct TurnResult {
    /// Assistant's response content.
    pub content: String,
    /// Tool calls made during this turn.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage.
    pub usage: TurnUsage,
    /// Interaction signals detected.
    pub signals: Vec<InteractionSignal>,
    /// Stop reason.
    pub stop_reason: String,
    /// Set when the pipeline is operating in degraded mode (LLM unavailable).
    ///
    /// `None` on all normal turns. `Some` only when the execute stage fell back
    /// to a cached distillation or an honest "unavailable" message.
    /// The TUI and API use this to render a warning banner instead of a normal
    /// response bubble.
    pub degraded: Option<crate::degraded_mode::DegradedMode>,
    /// Reasoning or thinking blocks generated by the model during this turn.
    pub reasoning: String,
}
```

```rust
pub struct ToolCall {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Input parameters (JSON).
    pub input: serde_json::Value,
    /// Result content.
    pub result: Option<String>,
    /// Whether the tool call errored.
    pub is_error: bool,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}
```

```rust
pub struct TurnUsage {
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cache read tokens.
    pub cache_read_tokens: u64,
    /// Cache write tokens.
    pub cache_write_tokens: u64,
    /// Number of LLM calls in this turn (1 + tool iterations).
    pub llm_calls: u32,
}
```

```rust
impl TurnUsage {
    pub fn total_tokens (&self) -> u64;
}
```

```rust
pub async fn assemble_context (
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
) -> crate::error::Result<()>
```

```rust
pub async fn assemble_context_with_extra (
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
) -> crate::error::Result<()>
```

```rust
pub async fn assemble_context_conditional (
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
    task_hint: TaskHint,
) -> crate::error::Result<()>
```

```rust
pub async fn assemble_context_conditional_with_cache (
    oikos: &Oikos,
    nous_config: &NousConfig,
    pipeline_config: &PipelineConfig,
    ctx: &mut PipelineContext,
    extra_sections: Vec<BootstrapSection>,
    task_hint: TaskHint,
    recipe: crate::bootstrap::LlmRecipe,
    cache: Option<&crate::bootstrap::BootstrapFileCache>,
) -> crate::error::Result<()>
```

```rust
pub fn check_guard (session: &SessionState, config: &NousConfig) -> GuardResult
```

## `src/recall/mod.rs`

```rust
pub struct RecallStageResult {
    /// Number of candidates retrieved from knowledge store.
    pub candidates_found: usize,
    /// Number that passed scoring threshold.
    pub results_injected: usize,
    /// Tokens consumed by injected knowledge.
    pub tokens_consumed: u64,
    /// The formatted recall section (appended to system prompt).
    pub recall_section: Option<String>,
    /// Source IDs of facts whose content was injected into the recall
    /// section. Used by the prompt audit log (#3411) so operators can see
    /// which stored facts were included in each outbound request.
    pub fact_ids: Vec<String>,
}
```

> Recall stage: scores and formats knowledge for injection into the system prompt.
> 
> # Examples
> 
> ```no_run
> use nous::recall::{RecallConfig, RecallStage};
> 
> let stage = RecallStage::new(RecallConfig::default());
> ```
```rust
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
    /// Optional side-query selected IDs for pre-filtering before 6-factor scoring.
    side_query_ids: Option<HashSet<String>>,
    /// Data-sovereignty target: gates which facts may leave the instance
    /// through this recall pass (#3404, #3413). Defaults to
    /// [`DeploymentTarget::Cloud`] — the safe assumption so callers who do
    /// not thread `with_deployment_target` never leak `Internal` or
    /// `Confidential` facts.
    deployment_target: DeploymentTarget,
}
```

```rust
impl RecallStage {
    pub fn new (config: RecallConfig) -> Self;
    pub fn with_side_query_ids (mut self, ids: HashSet<String>) -> Self;
    pub fn with_deployment_target (mut self, target: DeploymentTarget) -> Self;
    pub fn run (
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult>;
}
```

## `src/recall/scoring.rs`

```rust
pub struct RecallWeights {
    /// Temporal decay weight (0.0-1.0).
    pub decay: f64,
    /// Content relevance weight (0.0-1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0-1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0-1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0-1.0).
    pub access_frequency: f64,
    /// Graph `PageRank` importance weight (0.0-1.0).
    pub graph_importance: f64,
}
```

```rust
pub struct RecallConfig {
    /// Whether recall is enabled.
    pub enabled: bool,
    /// Maximum number of recalled items to inject.
    pub max_results: usize,
    /// Minimum score threshold to include a result.
    pub min_score: f64,
    /// Maximum tokens to allocate for recalled knowledge.
    pub max_recall_tokens: u64,
    /// Enable iterative 2-cycle retrieval with terminology discovery.
    pub iterative: bool,
    /// Maximum retrieval cycles (only used when `iterative` is true).
    pub max_cycles: usize,
    /// Per-factor scoring weights applied when building candidates.
    #[serde(default)]
    pub weights: RecallWeights,
    /// Inject factor metadata into recalled knowledge prompts.
    ///
    /// When enabled, each recalled fact includes its factor scores so the
    /// LLM can weight its reasoning by provenance quality.
    #[serde(default)]
    pub inject_metadata: bool,
    /// Characters per token for recall budget estimation.
    ///
    /// Wired from `agents.defaults.chars_per_token` at startup.
    /// Default: 4 (1 token ≈ 4 chars).
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: u64,
}
```

## `src/recall/search/search_impl.rs`

```rust
pub struct KnowledgeTextSearch {
    store: Arc<KnowledgeStore>,
}
```

```rust
pub struct KnowledgeVectorSearch {
    store: Arc<KnowledgeStore>,
}
```

```rust
impl KnowledgeVectorSearch {
    pub fn new (store: Arc<KnowledgeStore>) -> Self;
}
```

## `src/recall/search.rs`

> Abstracts vector knowledge search.
> 
> `KnowledgeStore` implements this when the `mneme-engine` feature is available.
> For tests, use `MockVectorSearch`.
```rust
pub trait VectorSearch : Send + Sync {
    fn search_vectors (
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>>;
}
```

## `src/recipes.rs`

```rust
pub struct RecipeFile {
    /// Resolution level (L1, L2, L3, L4, instructions, etc.).
    pub level: String,
    /// Path template relative to repo root. May contain `{param}` placeholders.
    pub path: String,
    /// Optional human-readable note explaining why this file loads.
    #[serde(default)]
    pub note: Option<String>,
}
```

```rust
pub struct RecipeValidation {
    /// Description of the real task (e.g. PR title).
    pub task: String,
    /// Tokens consumed by the naive grep-based baseline.
    pub baseline_tokens: u64,
    /// Tokens consumed by this recipe.
    pub recipe_tokens: u64,
    /// Whether the task completed successfully.
    pub success: bool,
    /// Optional note about the validation.
    #[serde(default)]
    pub note: Option<String>,
    /// Parameter values used for parameterized recipes.
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}
```

```rust
pub struct Recipe {
    /// Short identifier (e.g. `"cold_start"`, `"edit_crate"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// When to use this recipe.
    pub use_case: String,
    /// Conservative token budget for this recipe.
    pub token_budget: u64,
    /// Whether the recipe has `{crate}`-style parameter placeholders.
    #[serde(default)]
    pub parameterized: bool,
    /// Parameter names when `parameterized` is true.
    #[serde(default)]
    pub parameters: Vec<String>,
    /// Keywords for task-to-recipe matching.
    #[serde(default)]
    pub task_keywords: Vec<String>,
    /// Files to load.
    #[serde(default)]
    pub file: Vec<RecipeFile>,
    /// Validation records.
    #[serde(default)]
    pub validation: Vec<RecipeValidation>,
}
```

```rust
impl Recipe {
    pub fn resolve_files (&self, params: &HashMap<String, String>) -> Result<Vec<RecipeFile>>;
    pub fn avg_reduction_pct (&self) -> f64;
    pub fn success_rate (&self) -> f64;
}
```

```rust
pub struct RecipeRegistry {
    recipes: HashMap<String, Recipe>,
}
```

```rust
impl RecipeRegistry {
    pub fn empty () -> Self;
    pub fn load_from_file (path: &Path) -> Result<Self>;
    pub fn from_toml (content: &str) -> Result<Self>;
    pub fn get (&self, name: &str) -> Option<&Recipe>;
    pub fn all (&self) -> &HashMap<String, Recipe>;
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
    pub fn select_for_task (&self, task_description: &str) -> Option<&Recipe>;
    pub fn select (&self, hint: &str) -> Option<&Recipe>;
    pub fn resolve_files (
        &self,
        recipe_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<Vec<RecipeFile>>;
    pub fn all_reference_paths (&self) -> Vec<PathBuf>;
}
```

## `src/research.rs`

> Spawn parallel researchers for each domain and merge results.
> 
> Each researcher runs as an ephemeral sub-agent via [`SpawnService`]. All
> researchers run concurrently. Partial results are accepted if some researchers
> fail or timeout.
> 
> # Complexity
> 
> O(d) where d is the number of research domains. Each domain spawns a
> concurrent task, so wall-clock time is O(1) (bounded by the slowest domain),
> but total work scales linearly with domains.
> 
> # Errors
> 
> Returns `String` only if the spawn service itself is unavailable. Individual
> researcher failures are captured as [`FindingStatus::Failed`] or
> [`FindingStatus::TimedOut`] in the output.
> 
> # Cancel safety
> 
> Not cancel-safe. If cancelled while spawning researchers, some sub-agents
> may have been spawned but their results will never be collected. This leaks
> spawned tasks until they complete naturally. Do not use in `select!` branches.
```rust
pub async fn run_research (
    spawn_service: &Arc<dyn SpawnService>,
    parent_nous_id: &str,
    project_goal: &str,
    config: &ResearchConfig,
) -> Result<ResearchOutput, String>
```

## `src/roles/contract.rs`

```rust
pub struct RoleContract {
    /// Role name (e.g. "coder", "reviewer").
    pub role: String,
    /// Contract version. Increments when behaviors or constraints change.
    pub version: u32,
    /// Expected behaviors: what this role MUST do.
    pub behaviors: Vec<String>,
    /// Constraints: what this role MUST NOT do.
    pub constraints: Vec<String>,
}
```

```rust
impl RoleContract {
    pub fn to_prompt_section (&self) -> String;
}
```

```rust
pub struct ContractRegistry {
    contracts: HashMap<String, RoleContract>,
}
```

```rust
impl ContractRegistry {
    pub fn defaults () -> Self;
    pub fn load_from_file (path: &Path) -> Result<Self>;
    pub fn from_toml (content: &str) -> Result<Self>;
    pub fn get (&self, role: &str) -> Option<&RoleContract>;
    pub fn all (&self) -> &HashMap<String, RoleContract>;
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
}
```

## `src/roles/mod.rs`

```rust
pub enum Role {
    /// Implementation, testing, debugging. Full workspace access.
    Coder,
    /// Investigation, comparison, documentation. Read-only plus web access.
    Researcher,
    /// Code review, standards compliance, risk assessment. Read-only, no writes.
    Reviewer,
    /// Codebase exploration, architecture understanding. Read-only, no execution.
    Explorer,
    /// Task execution, command running, deployment. Execute plus read, no edits.
    Runner,
}
```

```rust
pub enum ToolPolicy {
    /// All registered tools available.
    Unrestricted,
    /// Only the listed tools are available. Everything else is denied.
    AllowOnly(Vec<String>),
}
```

```rust
pub struct RoleTemplate {
    /// Role identifier.
    pub role: Role,
    /// System prompt injected into the sub-agent's context.
    pub system_prompt: &'static str,
    /// Tool access restrictions.
    pub tool_policy: ToolPolicy,
    /// Preferred model identifier.
    pub model: &'static str,
}
```

## `src/self_audit/mod.rs`

```rust
pub enum CheckStatus {
    /// Check passed: metric is within acceptable bounds.
    Pass,
    /// Check produced a warning: metric is degraded but not critical.
    Warn,
    /// Check failed: metric is below acceptable threshold.
    Fail,
}
```

```rust
pub struct CheckResult {
    /// Overall status.
    pub status: CheckStatus,
    /// Numeric score between 0.0 (worst) and 1.0 (best).
    pub score: f64,
    /// Human-readable evidence describing the outcome.
    pub evidence: String,
}
```

```rust
pub struct ToolCallRecord {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Whether the call succeeded.
    pub success: bool,
}
```

```rust
pub struct CorrectionRecord {
    /// Session in which the correction occurred.
    pub session_id: String,
    /// Turn number where the operator corrected the nous.
    pub turn_number: u32,
}
```

```rust
pub struct MemoryRecallStats {
    /// Number of recent turns that triggered a knowledge-graph recall.
    pub recall_attempts: usize,
    /// Number of recalls that returned at least one relevant fact.
    pub recall_hits: usize,
}
```

```rust
pub struct SessionContinuityStats {
    /// Total turns in the session window being evaluated.
    pub total_turns: usize,
    /// Turns where the nous referenced prior context (back-references).
    pub context_carry_turns: usize,
    /// Number of times the operator had to re-state something already said.
    pub restatement_count: usize,
}
```

```rust
pub struct CheckContext {
    /// Which nous is being audited.
    pub nous_id: String,
    /// Recent tool call outcomes for this nous.
    pub recent_tool_calls: Vec<ToolCallRecord>,
    /// Recent assistant response lengths (in characters).
    pub recent_response_lengths: Vec<usize>,
    /// Total fact count in the knowledge graph.
    pub fact_count: usize,
    /// Count of facts with missing or invalid temporal bounds.
    pub temporal_violation_count: usize,
    /// Count of broken supersession chains (`superseded_by` points to nonexistent fact).
    pub broken_chain_count: usize,
    /// Operator corrections observed in recent sessions.
    pub recent_corrections: Vec<CorrectionRecord>,
    /// Total turns across the sessions covered by `recent_corrections`.
    pub total_turns_in_window: usize,
    /// Memory/knowledge-graph recall statistics.
    pub memory_recall: MemoryRecallStats,
    /// Session continuity statistics.
    pub session_continuity: SessionContinuityStats,
}
```

```rust
pub enum AuditTrigger {
    /// Time-based: periodic interval elapsed.
    Periodic {
        /// Configured interval in seconds.
        interval_secs: u64,
    },
    /// Event-based: agent performed N actions since last audit.
    EventBased {
        /// Number of actions that triggered this audit.
        after_n_actions: u32,
    },
    /// Manual trigger via CLI or API.
    Manual,
}
```

```rust
pub struct AuditCheckResult {
    /// Name of the check that ran.
    pub check_name: String,
    /// Description of what the check verifies.
    pub check_description: String,
    /// The check outcome.
    pub result: CheckResult,
}
```

```rust
pub struct AuditReport {
    /// Which nous was audited.
    pub nous_id: String,
    /// What triggered this audit.
    pub trigger: AuditTrigger,
    /// Individual check results.
    pub results: Vec<AuditCheckResult>,
    /// ISO 8601 timestamp when the audit completed.
    pub checked_at: String,
}
```

```rust
impl AuditReport {
    pub fn failed_checks (&self) -> impl Iterator<Item = &AuditCheckResult>;
    pub fn to_observations (&self) -> Vec<String>;
}
```

> A self-audit check that evaluates a specific aspect of agent behavior.
> 
> Implementations analyze the [`CheckContext`] and return a [`CheckResult`]
> indicating pass/warn/fail with a numeric score and evidence string.
```rust
pub trait ProsocheCheck : Send + Sync {
    fn name (&self) -> &'static str;
    fn description (&self) -> &'static str;
    fn run (&self, ctx: &CheckContext) -> CheckResult;
}
```

> Self-auditor: manages registered prosoche checks and trigger logic.
```rust
pub struct SelfAuditor {
    checks: Vec<Box<dyn ProsocheCheck>>,
    action_counter: AtomicU32,
    event_threshold: u32,
}
```

```rust
impl SelfAuditor {
    pub fn new () -> Self;
    pub fn with_event_threshold (mut self, n: u32) -> Self;
    pub fn register (&mut self, check: Box<dyn ProsocheCheck>);
    pub fn register_defaults (&mut self);
    pub fn record_action (&self) -> bool;
    pub fn run_audit (&self, ctx: &CheckContext, trigger: AuditTrigger) -> AuditReport;
    pub fn check_count (&self) -> usize;
}
```

```rust
pub fn store_audit_report (
    knowledge_store: &mneme::knowledge_store::KnowledgeStore,
    report: &AuditReport,
) -> crate::error::Result<()>
```

```rust
pub fn query_audit_history (
    knowledge_store: &mneme::knowledge_store::KnowledgeStore,
    nous_id: &str,
    limit: usize,
) -> crate::error::Result<Vec<mneme::knowledge::Fact>>
```

## `src/session.rs`

```rust
pub struct SessionState {
    pub id: String,
    pub nous_id: String,
    pub session_key: String, // kanon:ignore RUST/plain-string-secret

    pub model: String,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,

    pub turn: u64,
    /// Generated fresh on every [`next_turn`](Self::next_turn) call.
    /// Used by the finalize stage as a globally unique dedup key.
    pub turn_id: Ulid,
    pub token_estimate: i64,
    pub cumulative_tokens: u64,
    pub distillation_count: u32,
    pub bootstrap_hash: Option<String>,
    /// Last time the session was accessed. Used for LRU eviction.
    pub last_accessed: Instant,
}
```

```rust
impl SessionState {
    pub fn new (id: String, session_key: String, config: &NousConfig) -> Self;
    pub fn next_turn (&mut self) -> u64;
    pub fn needs_distillation (&self, threshold_ratio: f64, context_window: u32) -> bool;
}
```

```rust
pub struct SessionManager {
    config: NousConfig,
}
```

```rust
impl SessionManager {
    pub fn new (config: NousConfig) -> Self;
    pub fn create_session (&self, id: &str, session_key: &str) -> SessionState;
    pub fn config (&self) -> &NousConfig;
    pub fn is_ephemeral (session_key: &str) -> bool;
    pub fn is_background (session_key: &str) -> bool;
}
```

## `src/spawn_svc.rs`

> Concrete [`SpawnService`] that bridges to `actor::spawn`.
```rust
pub struct SpawnServiceImpl {
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
}
```

```rust
impl SpawnServiceImpl {
    pub fn new (
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
    ) -> Self;
}
```

## `src/stream.rs`

```rust
pub enum TurnStreamEvent {
    /// LLM streaming delta forwarded from the provider.
    LlmDelta(LlmStreamEvent),
    /// Tool execution started.
    ToolStart {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolResult {
        tool_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
        duration_ms: u64,
    },
}
```

## `src/tasks/gc.rs`

> Spawn a background GC task that periodically evicts stale entries.
> 
> The task runs until the `shutdown` token is cancelled. Output files for
> evicted tasks are cleaned up from disk. The sweep interval is read from
> [`taxis::config::NousBehaviorConfig`] defaults.
> 
> Returns a `JoinHandle` so the caller can await shutdown completion.
```rust
pub fn spawn_gc_task (
    registry: TaskRegistry,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()>
```

## `src/tasks/output.rs`

```rust
pub enum OutputError {
    /// Failed to create the output temp file.
    #[snafu(display("failed to create output file: {source}"))]
    Create {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write to the output file.
    #[snafu(display("failed to write output: {source}"))]
    Write {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to flush the output file.
    #[snafu(display("failed to flush output: {source}"))]
    Flush {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to open the output file for reading.
    #[snafu(display("failed to open output for reading: {source}"))]
    Open {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to remove the output file.
    #[snafu(display("failed to remove output file at {}: {source}", path.display()))]
    Remove {
        path: PathBuf,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Writes task output to a temp file as it arrives.
```rust
pub struct OutputWriter {
    file: fs::File,
    path: PathBuf,
}
```

```rust
impl OutputWriter {
    pub async fn new (dir: &Path) -> Result<Self, OutputError>;
    pub async fn write_chunk (&mut self, data: &[u8]) -> Result<(), OutputError>;
    pub fn path (&self) -> &Path;
}
```

> Streaming reader over a task's disk-backed output.
> 
> Implements `AsyncRead` so callers can page through output without loading
> the entire file into memory.
```rust
pub struct OutputReader {
    file: fs::File,
}
```

```rust
impl OutputReader {
    pub async fn open (path: &Path) -> Result<Self, OutputError>;
}
```

## `src/tasks/registry.rs`

```rust
pub enum RegistryError {
    /// Task not found in the registry.
    #[snafu(display("task {task_id} not found"))]
    NotFound {
        task_id: TaskId,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid status transition attempted.
    #[snafu(display("invalid transition for task {task_id}: {from} -> {to}"))]
    InvalidTransition {
        task_id: TaskId,
        from: TaskStatus,
        to: TaskStatus,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Registry lock poisoned.
    #[snafu(display("registry lock poisoned"))]
    LockPoisoned {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Output file operation failed.
    #[snafu(display("output error: {source}"))]
    Output {
        source: output::OutputError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub struct TaskSnapshot {
    /// Task identifier.
    pub id: TaskId,
    /// Task type with metadata.
    pub task_type: TaskType,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Human-readable description.
    pub description: String,
    /// When the task was registered.
    pub created_at: jiff::Timestamp,
    /// When the task reached a terminal status.
    pub completed_at: Option<jiff::Timestamp>,
    /// Recent tool call summaries (up to 5).
    pub recent_activity: Vec<ToolCallSummary>,
    /// Last error message, if any.
    pub error_snapshot: Option<String>,
}
```

```rust
pub struct TaskRegistry {
    tasks: Arc<RwLock<HashMap<TaskId, TaskEntry>>>,
    /// How long after completion before GC evicts a task.
    gc_deadline: Duration,
}
```

```rust
impl TaskRegistry {
    pub fn new (gc_deadline: Duration) -> Self;
    pub fn with_default_deadline () -> Self;
    pub fn gc_deadline (&self) -> Duration;
    pub fn register (
        &self,
        task_type: TaskType,
        description: String,
    ) -> Result<(TaskId, tokio_util::sync::CancellationToken), RegistryError>;
    pub fn update_status (
        &self,
        task_id: TaskId,
        new_status: TaskStatus,
    ) -> Result<(), RegistryError>;
    pub fn record_tool_call (
        &self,
        task_id: TaskId,
        summary: ToolCallSummary,
    ) -> Result<(), RegistryError>;
    pub fn record_error (&self, task_id: TaskId, error: String) -> Result<(), RegistryError>;
    pub fn set_output_path (
        &self,
        task_id: TaskId,
        path: std::path::PathBuf,
    ) -> Result<(), RegistryError>;
    pub fn broadcast_output_chunk (
        &self,
        task_id: TaskId,
        data: Vec<u8>,
    ) -> Result<(), RegistryError>;
    pub fn get (&self, task_id: TaskId) -> Result<TaskSnapshot, RegistryError>;
    pub fn list (
        &self,
        status_filter: Option<TaskStatus>,
    ) -> Result<Vec<TaskSnapshot>, RegistryError>;
    pub fn subscribe (
        &self,
        task_id: TaskId,
    ) -> Result<tokio::sync::broadcast::Receiver<ProgressEvent>, RegistryError>;
    pub fn kill (&self, task_id: TaskId) -> Result<(), RegistryError>;
    pub fn len (&self) -> Result<usize, RegistryError>;
    pub fn is_empty (&self) -> Result<bool, RegistryError>;
}
```

## `src/tasks/types.rs`

```rust
pub struct TaskId(koina::uuid::Uuid);
```

```rust
impl TaskId {
    pub fn new () -> Self;
}
```

```rust
pub enum TaskType {
    /// A shell command execution.
    Shell {
        /// The command being run.
        command: String,
    },
    /// A sub-agent running an autonomous loop.
    Agent {
        /// Identity of the spawned agent.
        agent_id: String,
        /// The prompt given to the agent.
        prompt: String,
    },
    /// A multi-step workflow execution.
    Workflow {
        /// Human-readable workflow name.
        name: String,
    },
    /// Memory consolidation (dream) task.
    Consolidation {
        /// Number of sessions being consolidated.
        sessions_count: usize,
    },
    /// Background monitoring task (e.g. MCP health).
    Monitor {
        /// What is being monitored.
        target: String,
    },
}
```

```rust
pub enum TaskStatus {
    /// Registered but not yet started.
    Pending,
    /// Actively executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Terminated due to an error.
    Failed,
    /// Explicitly cancelled via `kill()`.
    Killed,
}
```

```rust
impl TaskStatus {
    pub fn is_terminal (self) -> bool;
}
```

```rust
pub struct ToolCallSummary {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Wall-clock duration of the tool execution.
    pub elapsed: jiff::SignedDuration,
}
```

```rust
pub enum ProgressEvent {
    /// Task transitioned between statuses.
    StatusChanged {
        /// Previous status.
        from: TaskStatus,
        /// New status.
        to: TaskStatus,
    },
    /// A tool call completed.
    ToolActivity(ToolCallSummary),
    /// A chunk of output was produced.
    OutputChunk(Vec<u8>),
    /// An error snapshot for diagnostics.
    Error(String),
}
```

> A task entry in the registry.
> 
> Contains all state needed for status queries, progress streaming,
> cancellation, and GC eligibility.
```rust
pub struct TaskEntry {
    /// Unique task identifier.
    pub id: TaskId,
    /// What kind of task this is.
    pub task_type: TaskType,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Human-readable description.
    pub description: String,
    /// When the task was registered.
    pub created_at: jiff::Timestamp,
    /// When the task reached a terminal status.
    pub completed_at: Option<jiff::Timestamp>,
    /// Rolling window of recent tool call summaries (last 5).
    pub recent_activity: VecDeque<ToolCallSummary>,
    /// Token for cooperative cancellation.
    pub cancellation_token: CancellationToken,
    /// Broadcast sender for progress events.
    ///
    /// WHY: `broadcast::Sender` is kept alive as long as the entry exists so
    /// late-joining subscribers can still receive future events. Capacity is
    /// bounded at [`PROGRESS_CHANNEL_CAPACITY`].
    pub progress_tx: broadcast::Sender<ProgressEvent>,
    /// Path to the disk-backed output file, if created.
    pub output_path: Option<PathBuf>,
    /// Last error message, if any.
    pub error_snapshot: Option<String>,
}
```

## `src/training/dpo.rs`

```rust
pub enum DpoError {
    /// Failed to create the DPO output directory.
    #[snafu(display("failed to create DPO directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the DPO JSONL file for appending.
    #[snafu(display("failed to open DPO file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a DPO pair to JSON.
    #[snafu(display("failed to serialize DPO pair: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a DPO pair to the JSONL file.
    #[snafu(display("failed to write DPO pair to {}: {source}", path.display()))]
    WritePair {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to read file metadata.
    #[snafu(display("failed to read metadata for {}: {source}", path.display()))]
    ReadMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
}
```

> Result alias for DPO operations.
```rust
pub type Result<T> = std::result::Result<T, DpoError>;
```

```rust
pub struct DpoPair {
    /// The user prompt that both the rejected and chosen responses answer.
    pub prompt: String,
    /// The corrected assistant response (preferred).
    pub chosen: String,
    /// The original assistant response that was corrected (dispreferred).
    pub rejected: String,
    /// Session identifier linking the pair to its conversation.
    pub session_id: String,
    /// Turn number of the rejected response.
    pub rejected_turn: u64,
    /// Turn number of the chosen response.
    pub chosen_turn: u64,
}
```

> Extractor that detects correction→response sequences and produces
> [`DpoPair`]s.
> 
> Maintains a small per-session buffer of the most recent turn and
> at most one pending correction. State is bounded: old pending
> state is silently overwritten if a new correction arrives before
> the chosen response.
```rust
pub struct DpoExtractor {
    /// Most recent non-correction turn per session.
    last_turn: HashMap<String, TurnSnapshot>,
    /// Pending correction waiting for the chosen response.
    pending: HashMap<String, PendingCorrection>,
}
```

```rust
impl DpoExtractor {
    pub fn new () -> Self;
    pub fn process_turn (
        &mut self,
        session_id: &str,
        turn_number: u64,
        user_message: &str,
        assistant_response: &str,
        is_correction: bool,
    ) -> Option<DpoPair>;
}
```

> Writer for DPO preference pairs to a dated JSONL file.
> 
> File naming: `dpo-pairs-YYYYMMDD.jsonl` in the training directory.
> The file is opened in append mode for each write; no handle is
> held between calls.
```rust
pub struct DpoWriter {
    path: PathBuf,
}
```

```rust
impl DpoWriter {
    pub fn new (dir: &Path) -> Result<Self>;
    pub fn write_pair (&self, pair: &DpoPair) -> Result<()>;
    pub fn file_path (&self) -> &Path;
}
```

```rust
pub fn process_turn_global (
    session_id: &str,
    turn_number: u64,
    user_message: &str,
    assistant_response: &str,
    is_correction: bool,
) -> Option<DpoPair>
```

> Record a captured DPO pair in the metrics registry.
```rust
pub fn record_dpo_pair_captured (nous_id: &str)
```

## `src/training/mod.rs`

```rust
pub enum TrainingCaptureError {
    /// Failed to create the training data directory.
    #[snafu(display("failed to create training directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the JSONL output file for appending.
    #[snafu(display("failed to open training file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a training record to JSON.
    #[snafu(display("failed to serialize training record: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a training record to the JSONL file.
    #[snafu(display("failed to write training record to {}: {source}", path.display()))]
    WriteRecord {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to read file metadata.
    #[snafu(display("failed to read metadata for {}: {source}", path.display()))]
    ReadMetadata {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to persist the training manifest.
    #[snafu(display("failed to persist training manifest to {}: {source}", path.display()))]
    PersistManifest {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize the training manifest.
    #[snafu(display("failed to serialize training manifest: {source}"))]
    SerializeManifest { source: serde_json::Error },

    /// Failed to rename temporary manifest file.
    #[snafu(display("failed to rename {} to {}: {source}", from.display(), to.display()))]
    RenameManifest {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
}
```

> Result alias for training capture operations.
```rust
pub type Result<T> = std::result::Result<T, TrainingCaptureError>;
```

```rust
pub enum CaptureStopReason {
    /// Normal end of turn — safe to capture.
    EndTurn,
    /// Model requested tool use — may or may not have text content.
    ToolUse,
    /// Hit max tokens limit — response is likely truncated.
    MaxTokens,
    /// Hit a stop sequence — safe to capture.
    StopSequence,
    /// Degraded mode — LLM was unavailable, response is synthetic.
    Degraded,
    /// Any unrecognized stop reason.
    Unknown,
}
```

```rust
impl CaptureStopReason {
    pub fn parse (s: &str) -> Self;
}
```

```rust
pub struct CaptureInput<'a> {
    /// Session identifier the turn belongs to.
    pub session_id: &'a str,
    /// Nous identifier (agent name) handling the turn.
    pub nous_id: &'a str,
    /// Raw user message that started the turn.
    pub user_message: &'a str,
    /// Final assistant response produced by the model.
    pub assistant_response: &'a str,
    /// Model identifier used for this turn (e.g. `claude-sonnet-4-20250514`).
    pub model: &'a str,
    /// Total tokens consumed by the turn (prompt + completion).
    pub tokens: u64,
    /// Stop reason reported by the provider.
    pub stop_reason: CaptureStopReason,
    /// Whether the turn included any tool calls.
    ///
    /// WHY: tool-use-only turns (tool calls present but no text content)
    /// are not useful training data — they teach the model to produce
    /// empty text responses.
    pub has_tool_calls: bool,

    // ── Episteme labels ──────────────────────────────────────────────
    /// Classification of the conversation turn (e.g. "discussion", "correction").
    pub turn_type: Option<String>,
    /// Whether this turn corrects a previous response.
    pub is_correction: Option<bool>,
    /// Types of facts extracted from this turn.
    pub fact_types: Option<Vec<String>>,

    // ── Behavioural signals (v3) ──────────────────────────────────────
    /// Outcomes of tool calls made during the turn, in invocation order.
    ///
    /// `None` preserves "no tool calls were made" vs `Some(vec![])`
    /// which means "tool call outcome capture was configured but
    /// produced no entries" (should be unreachable in practice).
    pub tool_outcomes: Option<Vec<ToolOutcome>>,

    /// Recall stage signals (facts recalled, which were referenced).
    ///
    /// `None` means the recall stage was skipped or produced no result.
    pub recall_signals: Option<RecallSignals>,
}
```

```rust
impl CaptureInput<'_> {
    pub fn compute_quality_score (&self) -> Option<f32>;
}
```

```rust
pub struct TrainingManifest {
    /// Ordered list of shard file names (relative to the training directory).
    pub shards: Vec<ShardEntry>,
    /// Total records across all shards.
    pub total_records: u64,
    /// Minimum schema version seen across all records.
    pub schema_version_min: u32,
    /// Maximum schema version seen across all records.
    pub schema_version_max: u32,
}
```

```rust
pub struct ShardEntry {
    /// File name (relative to the training directory).
    pub file_name: String,
    /// Number of records in this shard.
    pub record_count: u64,
    /// Size in bytes (last known).
    pub size_bytes: u64,
}
```

> Sharded, append-only training data writer.
> 
> Writes [`TrainingRecord`]s as JSON Lines to shard files on disk. When the
> current shard exceeds [`TrainingConfig::max_shard_bytes`], the writer
> rotates to a new shard. A [`TrainingManifest`] is persisted after each
> write for crash recovery.
```rust
pub struct TrainingCapture {
    /// Training data directory.
    dir: PathBuf,
    /// Full path to the current shard file.
    current_shard: PathBuf,
    /// Path to the manifest file.
    manifest_path: PathBuf,
    /// In-memory manifest state.
    manifest: TrainingManifest,
    /// Maximum shard size before rotation.
    max_shard_bytes: u64,
    /// Whether to apply PII redaction before writing each record.
    pii_filter_enabled: bool,
}
```

```rust
impl TrainingCapture {
    pub fn new (instance_root: &Path, config: &TrainingConfig) -> Result<Self>;
    pub fn write_record (&mut self, record: &TrainingRecord) -> Result<()>;
    pub fn maybe_capture (&mut self, input: CaptureInput<'_>) -> bool;
    pub fn file_path (&self) -> &Path;
    pub fn dir (&self) -> &Path;
    pub fn manifest (&self) -> &TrainingManifest;
}
```

## `src/training/pii.rs`

```rust
pub fn marker (kind: &str) -> String
```

```rust
pub fn redact (input: &str) -> (String, bool)
```

## `src/tuning/evidence.rs`

```rust
pub struct EvidenceResult {
    /// Mean of the first half of observations (baseline).
    pub mean_before: f64,
    /// Mean of the second half of observations (treatment).
    pub mean_after: f64,
    /// Difference: `mean_after - mean_before`.
    pub delta: f64,
    /// Standard deviation of the full observation set.
    pub stddev: f64,
    /// Whether the delta exceeds the significance threshold.
    pub is_significant: bool,
}
```

```rust
pub fn validate_evidence (values: &[f64], significance_threshold: f64) -> Option<EvidenceResult>
```

## `src/tuning/mod.rs`

```rust
pub struct MetricSample {
    /// Name of the metric (matches a `ParameterSpec::outcome_signal`).
    pub metric_name: String,
    /// Observed value.
    pub value: f64,
    /// When the observation was taken.
    pub timestamp: jiff::Timestamp,
}
```

```rust
pub struct ProposalEvidence {
    /// Number of metric samples used to compute the proposal.
    pub sample_count: usize,
    /// Mean metric value before the observation window.
    pub metric_before: f64,
    /// Mean metric value during the observation window.
    pub metric_after: f64,
    /// Difference: `metric_after - metric_before`.
    pub delta: f64,
    /// Human-readable rationale for the change.
    pub rationale: String,
}
```

```rust
pub struct ParameterProposal {
    /// Dotted config key (e.g. `"distillation.contextTokenTrigger"`).
    pub key: String,
    /// Current live value.
    pub current_value: ParameterValue,
    /// Evidence-derived proposed value.
    pub proposed_value: ParameterValue,
    /// Evidence supporting the change.
    pub evidence: ProposalEvidence,
    /// Agent `nous_id` that proposed the change.
    pub proposed_by: String,
}
```

```rust
pub enum ProposalOutcome {
    /// The proposal was accepted and the parameter was changed.
    Applied {
        /// Config key that was changed.
        key: String,
        /// Previous value.
        old: ParameterValue,
        /// New value.
        new: ParameterValue,
    },
    /// The proposal was rejected (insufficient evidence, disabled, etc.).
    Rejected {
        /// Config key that was rejected.
        key: String,
        /// Reason for rejection.
        reason: String,
    },
    /// The proposed value was outside operator bounds.
    OutOfBounds {
        /// Config key.
        key: String,
        /// The value that was proposed.
        proposed: ParameterValue,
        /// `(min, max)` bounds from the parameter spec.
        bounds: (f64, f64),
    },
}
```

> Core tuning proposer: evaluates metric observations against the parameter
> registry and generates bounded proposals.
```rust
pub struct TuningProposer {
    config: TuningConfig,
}
```

```rust
impl TuningProposer {
    pub fn new (config: TuningConfig) -> Self;
    pub fn evaluate (&self, observations: &[MetricSample], nous_id: &str) -> Vec<ProposalOutcome>;
}
```

```rust
pub fn build_override_fact_content (
    key: &str,
    value: &ParameterValue,
    set_by: &str,
    evidence_summary: &str,
) -> String
```

## `src/tuning/signals.rs`

> An outcome signal that the self-tuning loop can observe and optimise.
```rust
pub struct OutcomeSignal {
    /// Signal name (matches `ParameterSpec::outcome_signal`).
    pub name: &'static str,
    /// Human-readable description of what this signal measures.
    pub description: &'static str,
    /// Computation function: takes raw samples, returns a summary value.
    ///
    /// Returns `None` when there are insufficient samples for a meaningful result.
    pub compute: fn(&[MetricSample]) -> Option<f64>,
}
```

```rust
pub fn all_signals () -> &'static [OutcomeSignal]
```

```rust
pub fn signal_by_name (name: &str) -> Option<&'static OutcomeSignal>
```

## `src/uncertainty.rs`

```rust
pub struct CalibrationBin {
    /// Lower and upper bounds of the confidence range.
    pub range: (f64, f64),
    /// Total predictions in this bin.
    pub total: u32,
    /// Correct predictions in this bin.
    pub correct: u32,
    /// Actual accuracy (correct / total, or 0.0 if empty).
    pub accuracy: f64,
}
```

```rust
pub struct OverconfidencePattern {
    /// Domain where overconfidence was detected.
    pub domain: String,
    /// Average stated confidence in this domain.
    pub avg_confidence: f64,
    /// Actual success rate in this domain.
    pub actual_rate: f64,
    /// Gap between stated confidence and actual success (positive = overconfident).
    pub overconfidence_gap: f64,
    /// Number of data points.
    pub sample_count: u32,
}
```

```rust
pub struct CalibrationSummary {
    /// Total calibration data points.
    pub total_points: u32,
    /// Brier score (0.0 = perfect, 1.0 = worst).
    pub brier_score: f64,
    /// Expected Calibration Error.
    pub ece: f64,
    /// Calibration curve bins.
    pub calibration_curve: Vec<CalibrationBin>,
    /// Domains where overconfidence was detected.
    pub overconfidence_patterns: Vec<OverconfidencePattern>,
}
```

## `src/working_state.rs`

```rust
pub enum WaitKind {
    /// Waiting for a tool to return its result.
    ToolResult,
    /// Waiting for the user to provide input.
    UserInput,
    /// Waiting for a sub-agent to complete work.
    SubAgent,
}
```

```rust
pub struct TaskEntry {
    /// Human-readable description of the task.
    pub description: String,
    /// ISO-8601 timestamp when the task was pushed.
    pub started_at: String,
}
```

```rust
pub struct FocusContext {
    /// File path the agent is working with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Function or method name within the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Abstract concept or topic the agent is exploring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
}
```

```rust
pub struct WaitState {
    /// Type of pending operation.
    pub kind: WaitKind,
    /// Human-readable description of what is being waited on.
    pub description: String,
    /// ISO-8601 timestamp when the wait began.
    pub since: String,
}
```

```rust
pub struct WorkingState {
    /// Stack of active tasks (most recent at the end).
    pub task_stack: Vec<TaskEntry>,
    /// Current focus context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<FocusContext>,
    /// Current wait state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitState>,
    /// ISO-8601 timestamp of the last update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Cache-safe parameters shared with forked agents.
    /// WHY: runtime-only field; not persisted because `Arc` references are session-scoped.
    #[serde(skip)]
    pub(crate) cache_params: Option<Arc<CacheSafeParams>>,
}
```
