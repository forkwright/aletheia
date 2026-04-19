# L3 API Index: eidos

Crate path: `crates/eidos`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/id.rs`

```rust
pub enum IdValidationError {
    /// The identifier was empty.
    Empty { kind: &'static str },
    /// The identifier exceeded the maximum length.
    TooLong {
        kind: &'static str,
        max: usize,
        actual: usize,
    },
}
```

## `src/knowledge/causal.rs`

```rust
pub enum TemporalOrdering {
    /// Cause precedes effect in time.
    Before,
    /// Effect precedes cause in time (retroactive causation).
    After,
    /// Cause and effect are concurrent.
    Concurrent,
}
```

```rust
impl TemporalOrdering {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub enum CausalRelationType {
    /// X directly caused Y ("the build failed because of the merge").
    Caused,
    /// X created the conditions for Y ("adding the feature flag enabled the rollout").
    Enabled,
    /// X blocked or stopped Y from occurring ("rate limiting prevented the cascade").
    Prevented,
    /// X and Y co-occur but the causal direction is uncertain or indirect.
    Correlated,
}
```

```rust
impl CausalRelationType {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub struct CausalEdge {
    /// Unique identifier for this edge.
    pub id: CausalEdgeId,
    /// Fact ID of the source (cause) node.
    pub source_id: FactId,
    /// Fact ID of the target (effect) node.
    pub target_id: FactId,
    /// Semantic type of the causal relationship.
    pub relationship_type: CausalRelationType,
    /// Temporal ordering between source and target.
    pub ordering: TemporalOrdering,
    /// Confidence that this causal relationship holds (0.0--1.0).
    ///
    /// Reflects both the strength of the evidence and the extraction heuristic
    /// quality. Heuristically extracted edges default to 0.5; user-confirmed
    /// edges may be raised toward 1.0.
    pub confidence: f64,
    /// Session ID where the causal evidence was observed, if known.
    pub evidence_session_id: Option<String>,
    /// When this edge was recorded.
    pub timestamp: jiff::Timestamp,
}
```

## `src/knowledge/entity.rs`

```rust
pub struct Entity {
    /// Unique identifier.
    pub id: EntityId,
    /// Display name.
    pub name: String,
    /// Entity type (person, project, tool, concept, etc.).
    pub entity_type: String,
    /// Known aliases.
    pub aliases: Vec<String>,
    /// When first observed.
    pub created_at: jiff::Timestamp,
    /// When last updated.
    pub updated_at: jiff::Timestamp,
}
```

```rust
pub struct Relationship {
    /// Source entity ID.
    pub src: EntityId,
    /// Target entity ID.
    pub dst: EntityId,
    /// Relationship type (e.g. `works_on`, `knows`, `depends_on`).
    pub relation: String,
    /// Relationship weight/strength (0.0--1.0).
    pub weight: f64,
    /// When first observed.
    pub created_at: jiff::Timestamp,
}
```

```rust
pub struct EmbeddedChunk {
    /// Unique identifier.
    pub id: EmbeddingId,
    /// The text that was embedded.
    pub content: String,
    /// Source type (fact, message, note, document).
    pub source_type: String,
    /// Source ID (fact ID, message `session_id:seq`, etc.).
    pub source_id: String,
    /// Which nous this belongs to (empty = shared).
    pub nous_id: String,
    /// The embedding vector (dimension depends on model).
    pub embedding: Vec<f32>,
    /// When embedded.
    pub created_at: jiff::Timestamp,
}
```

```rust
pub struct RecallResult {
    /// The matching fact or chunk content.
    pub content: String,
    /// Distance/similarity score (lower = more similar for L2/cosine).
    pub distance: f64,
    /// Source type.
    pub source_type: String,
    /// Source ID.
    pub source_id: String,
    /// Data-sovereignty classification for the underlying fact, carried
    /// from [`Fact::sensitivity`] so the recall pipeline can filter results
    /// by the active provider's deployment target (#3404, #3413). Defaults
    /// to [`FactSensitivity::Public`] for non-fact sources (messages,
    /// notes) and for facts persisted before sensitivity tracking was
    /// introduced.
    ///
    /// [`Fact::sensitivity`]: super::fact::Fact::sensitivity
    /// [`FactSensitivity::Public`]: super::fact::FactSensitivity::Public
    #[serde(default)]
    pub sensitivity: super::fact::FactSensitivity,
    /// Normalized `PageRank` importance of the entity associated with this
    /// result. Zero when no graph score is available. Carried from the
    /// `graph_scores` relation so the recall pipeline can boost hub
    /// entities directly (#3432).
    #[serde(default)]
    pub graph_importance: f64,
}
```

## `src/knowledge/fact.rs`

> Maximum byte length for fact content strings.
```rust
pub const MAX_CONTENT_LENGTH: usize = 102_400;
```

```rust
pub struct FactTemporal {
    /// When this fact became true in the world (domain validity time).
    pub valid_from: jiff::Timestamp,
    /// When this fact ceased to be true in the world (domain validity time).
    ///
    /// Use [`far_future`](crate::knowledge::far_future) for facts that are
    /// currently valid.
    pub valid_to: jiff::Timestamp,
    /// When the system learned about this fact (system recording time).
    ///
    /// This is distinct from `valid_from`/`valid_to`, which describe when the
    /// fact was true in the domain, not when we recorded it.
    pub recorded_at: jiff::Timestamp,
}
```

```rust
pub struct FactProvenance {
    /// Normalized confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Epistemic confidence tier — how the fact was established.
    ///
    /// Tier reflects the epistemic basis (e.g. verified against ground truth,
    /// inferred from context, assumed, or derived from training outcomes).
    pub tier: EpistemicTier,
    /// Session that extracted or produced this fact, if known.
    pub source_session_id: Option<String>,
    /// Base FSRS stability in hours before the tier multiplier is applied.
    pub stability_hours: f64,
}
```

```rust
pub struct FactLifecycle {
    /// ID of the fact that replaced this one, if any.
    pub superseded_by: Option<FactId>,
    /// Whether this fact has been intentionally forgotten.
    pub is_forgotten: bool,
    /// When the fact was forgotten, if it has been.
    pub forgotten_at: Option<jiff::Timestamp>,
    /// Why the fact was forgotten, if applicable.
    pub forget_reason: Option<ForgetReason>,
}
```

```rust
pub struct FactAccess {
    /// Number of times this fact has been recalled.
    pub access_count: u32,
    /// Timestamp of the most recent recall, if any.
    pub last_accessed_at: Option<jiff::Timestamp>,
}
```

```rust
pub struct Fact {
    /// Stable identifier for this fact.
    pub id: FactId,
    /// Agent (nous) that owns this fact.
    pub nous_id: String,
    /// Classification determining base decay behavior.
    pub fact_type: String,
    /// Human-readable fact statement.
    pub content: String,

    /// Memory sharing scope for team memory.
    ///
    /// `None` for facts created before the team memory model was introduced.
    /// New facts should always populate this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScope>,

    /// Data-sovereignty classification gating which provider deployment
    /// targets may receive this fact during recall (#3404, #3413).
    ///
    /// Defaults to [`FactSensitivity::Public`] via `#[serde(default)]` so
    /// facts persisted before sensitivity tracking deserialize unchanged.
    #[serde(default)]
    pub sensitivity: FactSensitivity,

    /// Bi-temporal validity and recording timestamps.
    #[serde(flatten)]
    pub temporal: FactTemporal,
    /// Provenance and confidence metadata.
    #[serde(flatten)]
    pub provenance: FactProvenance,
    /// Supersession and forgetting lifecycle.
    #[serde(flatten)]
    pub lifecycle: FactLifecycle,
    /// Access-tracking counters.
    #[serde(flatten)]
    pub access: FactAccess,
}
```

```rust
pub enum FactSensitivity {
    /// Safe for any provider, including cloud LLM providers.
    #[default]
    Public,
    /// Safe for local or self-hosted providers; must not leave the instance
    /// via cloud APIs.
    Internal,
    /// Never send to any external provider. Only embedded (in-process)
    /// providers may receive this fact.
    Confidential,
}
```

```rust
impl FactSensitivity {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub enum EpistemicTier {
    /// Checked against ground truth.
    Verified,
    /// Reasoned from context.
    Inferred,
    /// Unchecked assumption.
    Assumed,
    /// Derived from agent session outcomes for training signal.
    Training,
}
```

```rust
impl EpistemicTier {
    pub fn as_str (self) -> &'static str;
    pub fn stability_multiplier (self) -> f64;
}
```

```rust
pub enum KnowledgeStage {
    /// Fully active, included in standard recall. Decay score >= 0.7.
    Active,
    /// Recall score declining. Still retrievable but deprioritized. Decay in [0.3, 0.7).
    Fading,
    /// Low recall probability. Excluded from default recall, available on explicit query. Decay in [0.1, 0.3).
    Dormant,
    /// Below retention threshold. Candidate for permanent removal. Decay < 0.1.
    Archived,
}
```

> Default decay score threshold for transitioning from Active to Fading.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_active_threshold`.
```rust
pub const DEFAULT_STAGE_ACTIVE_THRESHOLD: f64 = 0.7;
```

> Default decay score threshold for transitioning from Fading to Dormant.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_fading_threshold`.
```rust
pub const DEFAULT_STAGE_FADING_THRESHOLD: f64 = 0.3;
```

> Default decay score threshold for transitioning from Dormant to Archived.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::fact_dormant_threshold`.
```rust
pub const DEFAULT_STAGE_DORMANT_THRESHOLD: f64 = 0.1;
```

```rust
impl KnowledgeStage {
    pub fn from_decay_score (decay_score: f64) -> Self;
    pub fn from_decay_score_with_thresholds (
        decay_score: f64,
        active_threshold: f64,
        fading_threshold: f64,
        dormant_threshold: f64,
    ) -> Self;
    pub fn as_str (self) -> &'static str;
    pub fn is_prunable (self) -> bool;
    pub fn in_default_recall (self) -> bool;
}
```

```rust
pub struct StageTransition {
    /// The fact that transitioned.
    pub fact_id: FactId,
    /// Previous stage.
    pub from: KnowledgeStage,
    /// New stage.
    pub to: KnowledgeStage,
    /// Decay score that triggered the transition.
    pub decay_score: f64,
    /// When the transition occurred.
    pub transitioned_at: jiff::Timestamp,
}
```

```rust
pub enum ForgetReason {
    /// User explicitly requested removal.
    UserRequested,
    /// Fact is outdated.
    Outdated,
    /// Fact is incorrect.
    Incorrect,
    /// Privacy concern.
    Privacy,
    /// Skill retired due to prolonged inactivity (decay score below threshold).
    Stale,
    /// Replaced by a newer or better skill during deduplication.
    Superseded,
    /// Contradicted by a newer extraction during auto-dream consolidation.
    Contradicted,
}
```

```rust
impl ForgetReason {
    pub fn as_str (self) -> &'static str;
}
```

```rust
pub enum FactType {
    /// "My name is X": very stable (2 years).
    Identity,
    /// "I prefer tabs": stable (1 year).
    Preference,
    /// "I know Rust": moderately stable (6 months).
    Skill,
    /// "X works at Y": moderate (3 months).
    Relationship,
    /// "We discussed X": short-lived (30 days).
    Event,
    /// "TODO: fix bug": ephemeral (7 days).
    Task,
    /// "Build was slow": very ephemeral (3 days).
    Observation,
    /// Self-audit result: short-lived (30 days).
    Audit,
    /// Claim-source provenance check: ephemeral (7 days).
    Verification,
    /// Operational metric snapshot: ephemeral (3 days).
    Operational,
}
```

```rust
impl FactType {
    pub fn base_stability_hours (self) -> f64;
    pub fn classify (content: &str) -> Self;
    pub fn as_str (self) -> &'static str;
    pub fn from_str_lossy (s: &str) -> Self;
}
```

```rust
pub enum VerificationSource {
    /// Shell command whose output is compared against the claim.
    Command,
    /// Database or API query returning structured data.
    Query,
    /// Arithmetic re-derivation (e.g. sum checks, percentage recalculation).
    Arithmetic,
    /// Cross-reference against an authoritative document or fact.
    Reference,
}
```

```rust
impl VerificationSource {
    pub fn as_str (self) -> &'static str;
    pub fn from_str_opt (s: &str) -> Option<Self>;
}
```

```rust
pub enum VerificationStatus {
    /// Actual value matches expected within tolerance.
    Pass,
    /// Actual value diverges from expected beyond tolerance.
    Fail,
    /// Verification result is older than the staleness threshold.
    Stale,
}
```

```rust
impl VerificationStatus {
    pub fn as_str (self) -> &'static str;
    pub fn from_str_opt (s: &str) -> Option<Self>;
}
```

```rust
pub struct VerificationRecord {
    /// The assertion being verified (e.g. "build succeeded", "total is 383").
    pub claim: String,
    /// How the claim was checked against ground truth.
    pub source: VerificationSource,
    /// The value the claim asserts.
    pub expected: serde_json::Value,
    /// The value observed from the source.
    pub actual: serde_json::Value,
    /// Acceptable relative deviation before marking as `Fail` (0.0 = exact match).
    pub tolerance: f64,
    /// Outcome of the comparison.
    pub status: VerificationStatus,
    /// When the verification was performed.
    pub verified_at: jiff::Timestamp,
}
```

```rust
pub fn default_stability_hours (fact_type: &str) -> f64
```

```rust
pub fn far_future () -> jiff::Timestamp
```

```rust
pub fn parse_timestamp (s: &str) -> Option<jiff::Timestamp>
```

```rust
pub fn format_timestamp (ts: &jiff::Timestamp) -> String
```

```rust
pub struct FactDiff {
    /// Facts that became valid in the interval.
    pub added: Vec<Fact>,
    /// Facts where `valid_from` is before the interval but content or metadata changed.
    /// Tuple: (old version, new version).
    pub modified: Vec<(Fact, Fact)>,
    /// Facts whose `valid_to` fell within the interval.
    pub removed: Vec<Fact>,
}
```

## `src/knowledge/path.rs`

```rust
pub enum PathValidationLayer {
    /// Null bytes truncate paths in C-based syscalls (libc, kernel).
    NullByte,
    /// Raw string checks miss `foo/../../../etc/passwd`; resolved via
    /// `std::path::Path::components()`.
    Canonicalization,
    /// Symlinks can escape directory jails; resolved via
    /// `std::fs::canonicalize()` with root containment check.
    SymlinkResolution,
    /// Dangling symlinks indicate filesystem manipulation; detected via
    /// `std::fs::symlink_metadata()` when canonicalize returns ENOENT.
    DanglingSymlink,
    /// Symlink loops cause infinite recursion; capped at 40 hops matching
    /// the Linux `ELOOP` limit.
    LoopDetection,
    /// URL-encoded traversals (`%2e%2e%2f` = `../`) bypass string-level
    /// checks; detected by percent-decoding then re-checking for `..` or
    /// separator characters.
    UrlEncodedTraversal,
    /// Fullwidth characters (U+FF0E `.`, U+FF0F `/`) normalize to ASCII
    /// separators under NFKC; detected by normalizing and comparing to
    /// the original.
    UnicodeNormalization,
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment,
}
```

> Total number of filesystem-level validation layers (excluding scope
> containment which is a logical check).
```rust
pub const PATH_VALIDATION_FS_LAYERS: usize = 7;
```

> Maximum symlink hops before declaring a loop, matching the Linux
> `ELOOP` kernel limit.
```rust
pub const SYMLINK_HOP_LIMIT: usize = 40;
```

```rust
impl PathValidationLayer {
    pub fn as_str (self) -> &'static str;
    pub fn requires_io (self) -> bool;
}
```

```rust
pub enum PathValidationError {
    /// Path contains null bytes that would truncate C-level syscalls.
    NullByte { path: String },
    /// Path contains `..` or backslash components enabling directory traversal.
    Canonicalization { path: String, component: String },
    /// Symlink resolves outside the allowed root directory.
    SymlinkResolution { path: PathBuf, root: PathBuf },
    /// Symlink target does not exist (filesystem manipulation indicator).
    DanglingSymlink { path: PathBuf },
    /// Symlink chain exceeds the hop limit (loop indicator).
    LoopDetection { path: PathBuf, hops: usize },
    /// URL-encoded traversal characters detected (`%2e`, `%2f`, `%5c`).
    UrlEncodedTraversal {
        path: String,
        decoded_fragment: String,
    },
    /// Fullwidth Unicode characters that normalize to path separators under NFKC.
    UnicodeNormalization { path: String, offending_char: char },
    /// Resolved path falls outside the expected scope subdirectory.
    ScopeContainment {
        path: PathBuf,
        scope: MemoryScope,
        expected_dir: PathBuf,
    },
}
```

```rust
impl PathValidationError {
    pub fn layer (&self) -> PathValidationLayer;
}
```

```rust
pub struct ValidatedPath {
    inner: PathBuf,
    scope: MemoryScope,
}
```

```rust
impl ValidatedPath {
    pub fn as_path (&self) -> &Path;
    pub fn scope (&self) -> MemoryScope;
    pub fn into_path_buf (self) -> PathBuf;
    pub fn read (&self) -> std::io::Result<Vec<u8>>;
    pub fn write (&self, data: &[u8]) -> std::io::Result<()>;
}
```

> Validate a memory path against all defense-in-depth security layers.
> 
> Applies each [`PathValidationLayer`] in order. The path must pass all
> layers to produce a [`ValidatedPath`]. Relative paths are resolved
> against `root/scope_dir/`; absolute paths are checked directly against
> the scope boundary.
> 
> # Layers (applied in order)
> 
> 1. **Null byte** - reject `\0` characters
> 2. **Canonicalization** - reject `..` and backslash components
> 3. **URL-encoded traversal** - detect `%2e`, `%2f`, `%5c`
> 4. **Unicode normalization** - detect fullwidth `.` `/` `\` characters
> 5. **Scope containment** - resolved path must be under `root/scope_dir/`
> 6. **Symlink resolution** - canonical path must stay within root (I/O)
> 7. **Dangling symlink / loop detection** - reject broken or looping
>    symlinks (I/O)
> 
> # Errors
> 
> Returns [`PathValidationError`] identifying the first layer that
> rejected the path, with structured context for logging and diagnostics.
```rust
pub fn validate_memory_path (
    path: &Path,
    root: &Path,
    scope: MemoryScope,
) -> std::result::Result<ValidatedPath, PathValidationError>
```

## `src/knowledge/scope.rs`

```rust
pub enum MemoryScope {
    /// Private to the user, never shared with other agents.
    ///
    /// WHY: User memories contain personal context (role, preferences,
    /// knowledge level) that should not leak across agent boundaries.
    User,
    /// Selectively shared corrections and preferences, write-gated to the user.
    ///
    /// WHY: Feedback memories encode behavioral guidance. Agents read them
    /// to avoid repeating mistakes, but only the user can write because
    /// agent-written feedback creates self-reinforcing loops.
    Feedback,
    /// Shared across all agents in a workspace, read-write.
    ///
    /// WHY: Project memories track ongoing work, deadlines, and decisions
    /// that every agent in the workspace needs visibility into.
    Project,
    /// Hybrid: agents read, user curates write access.
    ///
    /// WHY: Reference memories point to external systems (Linear, Grafana,
    /// Slack). Agents need to read them for context but the user controls
    /// what gets indexed because stale pointers are worse than no pointers.
    Reference,
}
```

```rust
impl MemoryScope {
    pub fn as_str (self) -> &'static str;
    pub fn as_dir_name (self) -> &'static str;
    pub fn access_policy (self) -> ScopeAccessPolicy;
    pub fn from_str_opt (s: &str) -> Option<Self>;
}
```

```rust
pub struct ScopeAccessPolicy {
    /// Whether agents can read memories in this scope.
    pub agent_read: bool,
    /// Whether agents can write memories in this scope.
    pub agent_write: bool,
    /// Whether only the user can write (agent writes are rejected).
    pub user_write_only: bool,
}
```

```rust
impl ScopeAccessPolicy {
    pub fn permits_agent_write (&self) -> bool;
    pub fn permits_agent_read (&self) -> bool;
}
```

## `src/test_fixtures.rs`

> Parse an ISO 8601 timestamp for test data. Panics on invalid input.
```rust
pub fn test_ts (s: &str) -> jiff::Timestamp
```

> Build a minimal `Fact` with sensible test defaults.
> 
> Fields can be mutated after construction for test-specific overrides:
> ```ignore
> let mut f = make_fact("f1", "syn", "Rust is fast");
> f.provenance.confidence = 0.5;
> ```
```rust
pub fn make_fact (id: &str, nous_id: &str, content: &str) -> Fact
```

> Build a minimal `Entity` with sensible test defaults.
```rust
pub fn make_entity (id: &str, name: &str, entity_type: &str) -> Entity
```

> Build a minimal `Relationship` with sensible test defaults.
```rust
pub fn make_relationship (src: &str, dst: &str, relation: &str, weight: f64) -> Relationship
```

## `src/training.rs`

```rust
pub struct TrainingConfig {
    /// Whether training data capture is enabled.
    pub enabled: bool,
    /// Directory path for training data output, relative to the instance root.
    ///
    /// The JSONL file `conversations.jsonl` is written inside this directory.
    pub path: String,
    /// Maximum size in bytes before rotating to a new shard file.
    ///
    /// When the current shard exceeds this limit, it is closed and a new
    /// shard is started. Default: 50 `MiB`.
    #[serde(default = "default_max_shard_bytes")]
    pub max_shard_bytes: u64,
    /// Whether to redact PII and secret patterns from `user_message` and
    /// `assistant_response` before writing a record to disk.
    ///
    /// WHY default = `true`: training corpora are persisted to the
    /// filesystem and may be shared with downstream training jobs.
    /// A conservative default prevents accidental leakage. Operators
    /// running a trusted local-only pipeline can disable explicitly.
    #[serde(default = "default_pii_filter_enabled")]
    pub pii_filter_enabled: bool,
}
```

> Current schema version for [`TrainingRecord`].
> 
> Bump this constant whenever fields are added, removed, or change
> semantics so that records from different epochs can be distinguished
> at read time.
> 
> # History
> 
> - v0: initial, no `schema_version` field persisted
> - v1: added `schema_version` field
> - v2: added episteme labels (`turn_type`, `is_correction`, `fact_types`, `quality_score`)
> - v3: added `tool_outcomes`, `recall_signals`, `pii_redacted`
```rust
pub const TRAINING_RECORD_SCHEMA_VERSION: u32 = 3;
```

```rust
pub struct ToolOutcome {
    /// Name of the tool invoked (e.g. `"file_read"`, `"shell"`).
    pub name: String,
    /// Whether the tool call returned a successful result.
    pub success: bool,
    /// Wall-clock execution duration in milliseconds.
    pub duration_ms: u64,
    /// Coarse error classification when `success = false`. `None` on success.
    ///
    /// Callers should use short, stable labels (e.g. `"timeout"`,
    /// `"not_found"`, `"permission_denied"`) so downstream training
    /// jobs can bucket errors without parsing free-form text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}
```

```rust
pub struct RecalledFact {
    /// Stable identifier of the recalled source (fact / note / document id).
    pub source_id: String,
    /// Source type label (e.g. `"fact"`, `"note"`, `"document"`).
    pub source_type: String,
    /// Final weighted recall score in `[0.0, 1.0]`.
    pub score: f64,
    /// Whether the assistant's response contained a reference to the
    /// recalled content (substring match on a content excerpt).
    pub was_referenced: bool,
}
```

```rust
pub struct RecallSignals {
    /// Total candidates returned by the recall engine before filtering.
    pub candidates_found: u32,
    /// Number of candidates that passed the recall threshold and were
    /// injected into the system prompt.
    pub results_injected: u32,
    /// Tokens spent on the injected recall section.
    pub tokens_consumed: u64,
    /// Per-fact recall records (source id, score, referenced flag).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<RecalledFact>,
}
```

```rust
pub struct TrainingRecord {
    /// Schema version that produced this record.
    ///
    /// Defaults to `0` when deserializing records written before the
    /// field existed, distinguishing them from version-1+ records.
    #[serde(default)]
    pub schema_version: u32,
    /// Session identifier (groups turns within a conversation).
    pub session_id: String,
    /// Nous agent identifier that handled the turn.
    pub nous_id: String,
    /// The user's input message.
    pub user_message: String,
    /// The assistant's response content.
    pub assistant_response: String,
    /// LLM model used for generation.
    pub model: String,
    /// Total tokens consumed (input + output).
    pub tokens: u64,
    /// When the turn was captured.
    pub timestamp: Timestamp,

    // ── Episteme labels (v2) ──────────────────────────────────────────
    /// Classification of the conversation turn (e.g. "discussion", "correction").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_type: Option<String>,
    /// Whether this turn corrects a previous response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_correction: Option<bool>,
    /// Types of facts extracted from this turn (e.g. "identity", "preference").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_types: Option<Vec<String>>,
    /// Quality score for DPO/ORPO signal (0.0--1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,

    // ── Behavioural signals (v3) ──────────────────────────────────────
    /// Outcomes of tool calls made during the turn, in invocation order.
    ///
    /// `None` when the turn had no tool calls. An empty vec is reserved
    /// for turns that were configured to capture outcomes but produced
    /// none (should be unreachable in practice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcomes: Option<Vec<ToolOutcome>>,

    /// Recall stage signals for this turn (facts recalled, whether they
    /// were referenced in the output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_signals: Option<RecallSignals>,

    /// Whether PII/secret redaction was applied to `user_message` and
    /// `assistant_response` before persistence.
    ///
    /// WHY persist as a field: downstream training jobs need to know
    /// whether a record has been scrubbed so they can refuse to
    /// re-process unredacted corpora if the redaction policy changes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pii_redacted: bool,
}
```
