# L3 API Index: melete

Crate path: `crates/melete`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/contradiction.rs`

```rust
pub struct Contradiction {
    /// 0-based index of the first conflicting chunk.
    pub chunk_a: usize,
    /// 0-based index of the second conflicting chunk.
    pub chunk_b: usize,
    /// Human-readable description of the contradiction.
    pub description: String,
}
```

```rust
pub enum ResolutionStrategy {
    /// Prefer the more recent fact (higher index).
    PreferNewer,
    /// Requires explicit user review.
    NeedsUserReview,
}
```

```rust
pub struct ContradictionLog {
    /// Detected contradictions.
    pub contradictions: Vec<Contradiction>,
    /// When detection was performed (ISO 8601).
    pub timestamp: String,
    /// Suggested resolution strategy.
    pub resolution_strategy: ResolutionStrategy,
}
```

```rust
impl ContradictionLog {
    pub fn empty () -> Self;
    pub fn is_empty (&self) -> bool;
}
```

## `src/distill.rs`

> Default maximum conversation turns to skip between distillation retry attempts.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::distillation_max_backoff_turns`.
```rust
pub const DEFAULT_MAX_BACKOFF_TURNS: u32 = 8;
```

```rust
pub enum DistillSection {
    /// One-sentence overview of the conversation topic.
    Summary,
    /// What was being worked on and why, including agent identity.
    TaskContext,
    /// Bullet list of concrete actions taken and their outcomes.
    CompletedWork,
    /// Decisions made with rationale that must survive distillation.
    KeyDecisions,
    /// Snapshot of where things stand: done, in-progress, half-finished.
    CurrentState,
    /// Unfinished items, pending questions, and deferred work.
    OpenThreads,
    /// Mistakes discovered and corrected to prevent repetition.
    Corrections,
    /// Custom section with a name and description.
    Custom {
        /// Section heading text.
        name: String,
        /// Prompt instruction for what to include in this section.
        description: String,
    },
}
```

```rust
impl DistillSection {
    pub fn heading (&self) -> String;
    pub fn description (&self) -> &str;
    pub fn all_standard () -> Vec<Self>;
}
```

```rust
pub struct DistillConfig {
    /// Model to use for distillation.
    pub model: String,
    /// Maximum output tokens for the summary.
    pub max_output_tokens: u32,
    /// Minimum messages before distillation is worthwhile.
    pub min_messages: usize,
    /// Whether to include tool call details in the summary.
    pub include_tool_calls: bool,
    /// If set, use this model for distillation instead of the primary model.
    /// Enables cost reduction (e.g., Opus primary -> Sonnet for distillation).
    pub distillation_model: Option<String>,
    /// Number of recent messages to preserve verbatim (not summarized).
    pub verbatim_tail: usize,
    /// Sections to include in the structured summary.
    pub sections: Vec<DistillSection>,
    /// Jaccard similarity threshold for deduplication before distillation.
    /// Range: 0.0 to 1.0. Default: 0.85.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,
    /// Whether to run LLM-based contradiction detection during distillation.
    #[serde(default)]
    pub detect_contradictions: bool,
    /// Maximum backoff turns between retry attempts. Default: 8.
    #[serde(default = "default_max_backoff_turns")]
    pub max_backoff_turns: u32,
}
```

```rust
pub struct DistillResult {
    /// The distilled summary text.
    pub summary: String,
    /// Number of messages that were distilled (excluding verbatim tail).
    pub messages_distilled: usize,
    /// Estimated tokens before distillation.
    pub tokens_before: u64,
    /// Estimated tokens after distillation.
    pub tokens_after: u64,
    /// Which distillation number this is for the session.
    pub distillation_number: u32,
    /// Timestamp of distillation (ISO 8601).
    pub timestamp: String,
    /// Messages preserved verbatim (not summarized).
    pub verbatim_messages: Vec<Message>,
    /// Structured memory items extracted from the summary for long-term persistence.
    pub memory_flush: MemoryFlush,
    /// Statistics from similarity pruning (if any messages were compared).
    pub pruning_stats: Option<PruningStats>,
    /// Contradictions detected across chunks during distillation.
    pub contradiction_log: ContradictionLog,
}
```

```rust
pub struct DistillEngine {
    config: DistillConfig,
    // WHY: std::sync::Mutex: retry counter check/increment is O(1), never crosses an await point.
    retry_state: std::sync::Mutex<RetryState>,
}
```

```rust
impl DistillEngine {
    pub fn new (config: DistillConfig) -> Self;
    pub fn tick_turn (&self) -> bool;
    pub fn in_backoff (&self) -> bool;
    pub fn should_distill (
        &self,
        message_count: usize,
        token_estimate: u64,
        context_window: u64,
        threshold: f64,
    ) -> bool;
    pub async fn distill (
        &self,
        messages: &[Message],
        nous_id: &str,
        provider: &dyn LlmProvider,
        distillation_number: u32,
    ) -> Result<DistillResult>;
    pub fn config (&self) -> &DistillConfig;
}
```

## `src/dream/mod.rs`

> Default minimum hours between consolidation runs.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_min_hours`.
```rust
pub const DEFAULT_MIN_HOURS: u64 = 24;
```

> Default minimum sessions required to trigger consolidation.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_min_sessions`.
```rust
pub const DEFAULT_MIN_SESSIONS: usize = 5;
```

> Default session scan throttle interval (10 minutes in seconds).
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_scan_throttle_secs`.
```rust
pub const DEFAULT_SCAN_THROTTLE_SECS: i64 = 600;
```

> Default stale lock threshold (1 hour in seconds).
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_stale_threshold_secs`.
```rust
pub const DEFAULT_STALE_THRESHOLD_SECS: i64 = 3_600;
```

```rust
pub struct DreamConfig {
    /// Minimum hours between consolidation runs (default: 24).
    pub min_hours: u64,
    /// Minimum sessions since last consolidation to trigger (default: 5).
    pub min_sessions: usize,
    /// Path to the consolidation lock file.
    pub lock_path: PathBuf,
    /// Session scan throttle interval in seconds (default: 600 = 10 minutes).
    pub scan_interval_secs: i64,
    /// Stale lock threshold in seconds (default: 3600 = 1 hour).
    pub stale_threshold_secs: i64,
    /// Distillation engine configuration for fact extraction.
    pub distill_config: DistillConfig,
}
```

```rust
impl DreamConfig {
    pub fn new (lock_path: PathBuf) -> Self;
}
```

```rust
pub struct SessionTranscript {
    /// Session identifier.
    pub session_id: String,
    /// Nous (agent) identifier.
    pub nous_id: String,
    /// Conversation messages FROM this session.
    pub messages: Vec<Message>,
}
```

```rust
pub struct MergeReport {
    /// Facts newly added to the knowledge graph.
    pub facts_added: usize,
    /// Facts deduplicated against existing knowledge.
    pub facts_deduped: usize,
    /// Facts marked stale due to contradictions.
    pub facts_stale: usize,
}
```

> Trait for counting and loading session transcripts.
> 
> Implementors provide access to session storage (e.g. `SQLite` via graphe).
```rust
pub trait TranscriptSource : Send + Sync {
    fn count_sessions_since (
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<usize, std::io::Error>;
    fn load_transcripts_since (
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error>;
}
```

> Trait for persisting consolidation results to the knowledge graph.
> 
> Implementors provide the merge/dedup/stale-marking operations backed by
> the concrete knowledge store (e.g. episteme via mneme).
```rust
pub trait ConsolidationTarget : Send + Sync {
    fn merge_flush (
        &self,
        flush: &MemoryFlush,
        nous_id: &str,
    ) -> std::result::Result<MergeReport, std::io::Error>;
    fn mark_contradictions_stale (
        &self,
        log: &ContradictionLog,
        nous_id: &str,
    ) -> std::result::Result<usize, std::io::Error>;
}
```

> The auto-dream consolidation engine.
> 
> Manages the triple-gate system and spawns background consolidation tasks.
> Thread-safe: uses atomics for scan throttling, no mutex needed.
```rust
pub struct DreamEngine {
    config: DreamConfig,
    distill: DistillEngine,
    /// Unix timestamp of the last session scan (for 10-minute throttle).
    /// WHY: `AtomicI64` because we need lock-free reads FROM the gate check
    /// hot path. i64 holds Unix seconds until year ~292 billion.
    last_scan_at: AtomicI64,
}
```

```rust
impl DreamEngine {
    pub fn new (config: DreamConfig) -> Self;
    pub fn on_turn_complete (
        self: &Arc<Self>,
        source: &Arc<dyn TranscriptSource>,
        target: &Arc<dyn ConsolidationTarget>,
        provider: &Arc<dyn LlmProvider>,
    );
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// LLM call failed during distillation.
    #[snafu(display("LLM call failed during distillation: {source}"))]
    LlmCall {
        source: hermeneus::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Distillation produced an empty summary.
    #[snafu(display("distillation produced empty summary"))]
    EmptySummary {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session has no messages to distill.
    #[snafu(display("session has no messages to distill"))]
    NoMessages {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider panicked during distillation. (#2216)
    #[snafu(display("LLM call panicked during distillation: {message}"))]
    LlmPanic {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error during consolidation lock operations.
    #[snafu(display("consolidation lock I/O: {context}"))]
    DreamLockIo {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation lock is held by another active process.
    #[snafu(display("consolidation lock held by PID {pid}"))]
    DreamLockHeld {
        pid: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Transcript source failed during auto-dream consolidation.
    #[snafu(display("transcript source error: {context}"))]
    DreamTranscriptSource {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation target failed during fact merge.
    #[snafu(display("consolidation target error: {context}"))]
    DreamConsolidationTarget {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias for `Result` with melete's [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/flush.rs`

```rust
pub struct MemoryFlush {
    /// Key decisions that must survive distillation.
    pub decisions: Vec<FlushItem>,
    /// Corrections that prevent repeating mistakes.
    pub corrections: Vec<FlushItem>,
    /// Facts learned in this session.
    pub facts: Vec<FlushItem>,
    /// Current task state.
    pub task_state: Option<String>,
}
```

```rust
pub struct FlushItem {
    /// The text content to persist.
    pub content: String,
    /// When this item was identified (ISO 8601).
    pub timestamp: String,
    /// How this item was discovered.
    pub source: FlushSource,
}
```

```rust
pub enum FlushSource {
    /// Extracted from conversation by LLM.
    Extracted,
    /// Explicitly noted by the agent.
    AgentNote,
    /// Detected from tool usage patterns.
    ToolPattern,
}
```

```rust
impl MemoryFlush {
    pub fn empty () -> Self;
    pub fn is_empty (&self) -> bool;
    pub fn to_markdown (&self) -> String;
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/probe.rs`

```rust
pub struct Probe {
    /// The question that the fact should be able to answer.
    pub question: String,
    /// Index of the flush item this probe targets.
    pub source_index: usize,
    /// Which flush category the source belongs to.
    pub source_category: ProbeCategory,
}
```

```rust
pub enum ProbeCategory {
    /// A key decision.
    Decision,
    /// A correction that prevents repeating mistakes.
    Correction,
    /// A learned fact.
    Fact,
}
```

```rust
pub struct ProbeVerification {
    /// The probe that was verified.
    pub probe: Probe,
    /// Whether the probe passed verification.
    pub passed: bool,
    /// Token overlap score between the fact content and the transcript (0.0..=1.0).
    pub overlap_score: f64,
    /// Explanation of the verification result.
    pub explanation: String,
}
```

```rust
pub struct ProbeReport {
    /// Individual verification results.
    pub verifications: Vec<ProbeVerification>,
    /// Indices of flush items that failed verification, grouped by category.
    pub failed_decisions: Vec<usize>,
    /// Indices of flush items that failed verification.
    pub failed_corrections: Vec<usize>,
    /// Indices of flush items that failed verification.
    pub failed_facts: Vec<usize>,
}
```

```rust
impl ProbeReport {
    pub fn all_passed (&self) -> bool;
    pub fn failure_count (&self) -> usize;
    pub fn total_probes (&self) -> usize;
    pub fn pass_rate (&self) -> f64;
}
```

```rust
pub struct ProbeConfig {
    /// Minimum token overlap score to pass verification (0.0..=1.0). Default: 0.15.
    pub min_overlap: f64,
    /// Maximum number of probes to generate per flush item. Default: 3.
    pub max_probes_per_item: usize,
}
```

```rust
pub struct ProbeVerifier {
    config: ProbeConfig,
}
```

```rust
impl ProbeVerifier {
    pub fn new (config: ProbeConfig) -> Self;
    pub fn generate_probes (&self, flush: &MemoryFlush) -> Vec<Probe>;
    pub fn verify (&self, flush: &MemoryFlush, transcript: &str) -> ProbeReport;
    pub fn filter_passed (&self, flush: &MemoryFlush, report: &ProbeReport) -> MemoryFlush;
}
```

## `src/prompt.rs`

> Default maximum character length for truncated tool results in distillation prompts.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::distillation_max_tool_result_len`.
```rust
pub const DEFAULT_MAX_TOOL_RESULT_LEN: usize = 500;
```

## `src/similarity.rs`

> Default minimum token length to include in similarity comparison.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::similarity_min_token_len`.
```rust
pub const DEFAULT_MIN_TOKEN_LEN: usize = 3;
```

> Default Jaccard similarity threshold for near-duplicate detection.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::similarity_threshold`.
```rust
pub const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.85;
```

```rust
pub struct PruningStats {
    /// Total chunks evaluated.
    pub total_chunks: usize,
    /// Chunks removed as near-duplicates.
    pub pruned_count: usize,
}
```

```rust
impl PruningStats {
    pub fn reduction_percent (&self) -> f64;
}
```
