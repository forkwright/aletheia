# L3 API Index: episteme

Crate path: `crates/episteme`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/admission.rs`

```rust
pub enum AdmissionDecision {
    /// Fact should be admitted to the knowledge graph.
    Admit,
    /// Fact should be rejected. The reason is logged but not surfaced to the user.
    Reject(AdmissionRejection),
}
```

```rust
impl AdmissionDecision {
    pub fn is_admitted (&self) -> bool;
}
```

```rust
pub struct AdmissionRejection {
    /// Which factor(s) caused the rejection.
    pub factor: RejectionFactor,
    /// Human-readable explanation (logged at debug level).
    pub reason: String,
}
```

```rust
pub enum RejectionFactor {
    /// The fact's predicted future utility is too low.
    LowUtility,
    /// The source confidence is below the admission threshold.
    LowConfidence,
    /// The fact is semantically redundant with existing knowledge.
    LowNovelty,
    /// The fact is too old to be worth storing.
    Stale,
    /// The content type has a low prior for admission.
    LowTypePrior,
    /// Combined score across all factors falls below threshold.
    BelowThreshold,
}
```

```rust
pub struct AdmissionScores {
    /// Predicted future utility (will this fact be needed later?).
    pub utility: f64,
    /// Source reliability (LLM extraction < user statement < external source).
    pub confidence: f64,
    /// Semantic novelty (1.0 = completely new, 0.0 = exact duplicate).
    pub novelty: f64,
    /// Temporal recency (1.0 = just now, decays toward 0.0).
    pub recency: f64,
    /// Content type prior (identity facts > preferences > transient observations).
    pub type_prior: f64,
}
```

```rust
impl AdmissionScores {
    pub fn combined (&self) -> f64;
}
```

> Gate that decides whether a fact should enter the knowledge graph.
> 
> Implementations range from [`DefaultAdmissionPolicy`] (admit all  -  current
> behavior) to [`StructuredAdmissionPolicy`] (five-factor A-MAC decision).
```rust
pub trait AdmissionPolicy : Send + Sync {
    fn should_admit (&self, fact: &Fact) -> AdmissionDecision;
}
```

```rust
pub struct DefaultAdmissionPolicy;
```

```rust
pub struct StructuredAdmissionConfig {
    /// Minimum combined score to admit (0.0..=1.0). Default: 0.3.
    pub threshold: f64,
    /// Minimum confidence to admit without other factors. Default: 0.1.
    pub min_confidence: f64,
    /// Maximum age in hours before a fact is considered stale. Default: 2160 (90 days).
    pub max_age_hours: f64,
}
```

```rust
pub struct StructuredAdmissionPolicy {
    config: StructuredAdmissionConfig,
}
```

```rust
impl StructuredAdmissionPolicy {
    pub fn new (config: StructuredAdmissionConfig) -> Self;
    pub fn score (&self, fact: &Fact) -> AdmissionScores;
}
```

## `src/causal.rs`

```rust
pub enum CausalError {
    /// An edge with the same ID already exists in the store.
    #[snafu(display("causal edge already exists: {id}"))]
    DuplicateEdge {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The requested fact ID has no causal edges in the store.
    #[snafu(display("no causal edges found for fact: {fact_id}"))]
    FactNotFound {
        fact_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub struct CausalChainNode {
    /// The fact at this position in the chain.
    pub fact_id: FactId,
    /// The edge that connects this node to the previous one.
    ///
    /// `None` for the root node (the fact the traversal started from).
    pub via_edge: Option<CausalEdge>,
    /// Cumulative confidence along the path from the root to this node.
    ///
    /// Product of all edge confidences on the path. Root node has confidence 1.0.
    pub chain_confidence: f64,
    /// Depth from the root (0 = root).
    pub depth: usize,
}
```

```rust
pub struct CausalStore {
    edges: HashMap<CausalEdgeId, CausalEdge>,
    /// Maps an effect fact to the IDs of edges that caused it.
    causes_of: HashMap<FactId, Vec<CausalEdgeId>>,
    /// Maps a cause fact to the IDs of edges it produced.
    effects_of: HashMap<FactId, Vec<CausalEdgeId>>,
}
```

```rust
impl CausalStore {
    pub fn new () -> Self;
    pub fn add_edge (&mut self, edge: CausalEdge) -> Result<(), CausalError>;
    pub fn all_edges (&self) -> impl Iterator<Item = &CausalEdge>;
    pub fn get_edge (&self, id: &CausalEdgeId) -> Option<&CausalEdge>;
    pub fn direct_causes (&self, fact_id: &FactId) -> Vec<&CausalEdge>;
    pub fn direct_effects (&self, fact_id: &FactId) -> Vec<&CausalEdge>;
    pub fn trace_causes (&self, fact_id: &FactId) -> Vec<CausalChainNode>;
    pub fn trace_effects (&self, fact_id: &FactId) -> Vec<CausalChainNode>;
}
```

## `src/conflict.rs`

```rust
pub enum ConflictError {
    /// The LLM classification call failed.
    #[snafu(display("conflict classification failed: {message}"))]
    Classification {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge store query failed during candidate retrieval.
    #[snafu(display("candidate retrieval failed: {message}"))]
    CandidateRetrieval {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub enum ConflictClassification {
    /// The new fact directly contradicts the existing fact.
    Contradicts,
    /// The new fact is a more specific/updated version.
    Refines,
    /// The new fact adds information without contradicting.
    Supplements,
    /// Despite textual similarity, these are about different things.
    Unrelated,
}
```

```rust
pub struct ConflictCandidate {
    /// ID of the existing fact.
    pub existing_fact_id: FactId,
    /// Content of the existing fact.
    pub existing_content: String,
    /// Confidence of the existing fact.
    pub existing_confidence: f64,
    /// Epistemic tier of the existing fact.
    pub existing_tier: EpistemicTier,
    /// Cosine similarity between the new and existing fact.
    pub cosine_similarity: f64,
}
```

```rust
pub struct ConflictResolution {
    /// What to do with the new fact.
    pub action: ConflictAction,
    /// How the new fact relates to the best-matching candidate.
    pub classification: ConflictClassification,
    /// If superseding, the ID of the fact being replaced.
    pub superseded_fact_id: Option<FactId>,
}
```

```rust
pub enum ConflictAction {
    /// Insert the new fact (no conflict or supplements existing).
    Insert,
    /// Supersede the old fact with the new one.
    Supersede {
        /// ID of the fact being superseded.
        old_id: FactId,
    },
    /// Drop the new fact (duplicate or lower quality).
    Drop,
}
```

```rust
pub struct FactForConflictCheck {
    /// The content of the fact (subject + predicate + object).
    pub content: String,
    /// Confidence score (0.0--1.0).
    pub confidence: f64,
    /// Epistemic tier.
    pub tier: EpistemicTier,
    /// Subject entity name (for BM25 matching).
    pub subject: String,
    /// Whether this is a correction fact.
    pub is_correction: bool,
    /// Pre-computed embedding vector for similarity search.
    pub embedding: Vec<f32>,
}
```

```rust
pub struct BatchConflictResult {
    /// Facts that should be inserted (after dedup), with their resolved actions.
    pub resolved: Vec<(FactForConflictCheck, ConflictAction)>,
    /// Number of facts dropped during intra-batch dedup.
    pub batch_duplicates_dropped: usize,
}
```

```rust
pub const DEFAULT_MAX_LLM_CALLS_PER_FACT: usize = 3;
```

```rust
pub const DEFAULT_INTRA_BATCH_DEDUP_THRESHOLD: f64 = 0.95;
```

```rust
pub const DEFAULT_CANDIDATE_DISTANCE_THRESHOLD: f64 = 0.28;
```

```rust
pub const DEFAULT_MAX_CANDIDATES: usize = 5;
```

## `src/consolidation/engine.rs`

```rust
impl KnowledgeStore {
    pub fn find_entity_overflow_candidates (
        &self,
        nous_id: &str,
        config: &ConsolidationConfig,
    ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError>;
    pub fn find_community_overflow_candidates (
        &self,
        nous_id: &str,
        config: &ConsolidationConfig,
    ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError>;
    pub fn get_fact_multiplicity (
        &self,
        fact_id: &FactId,
    ) -> Result<Option<FactMultiplicity>, ConsolidationError>;
}
```

## `src/consolidation/mod.rs`

```rust
pub struct ConsolidationConfig {
    /// Minimum facts per entity before consolidation triggers (default: 10).
    pub entity_fact_threshold: usize,
    /// Minimum facts per community cluster before consolidation triggers (default: 20).
    pub community_fact_threshold: usize,
    /// Minimum age in days before a fact is eligible for consolidation (default: 7).
    pub min_age_days: u32,
    /// Maximum facts to send in a single LLM call (default: 50).
    pub batch_limit: usize,
    /// Minimum hours between consolidation cycles for the same nous (default: 1).
    pub rate_limit_hours: f64,
}
```

```rust
pub enum ConsolidationError {
    /// The LLM consolidation call failed.
    #[snafu(display("consolidation LLM call failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The LLM response could not be parsed as valid consolidation JSON.
    #[snafu(display("failed to parse consolidation response: {source}"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge store operation failed during consolidation.
    #[snafu(display("consolidation store error: {message}"))]
    Store {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Rate limit exceeded: too soon since the last consolidation cycle.
    #[snafu(display(
        "rate limited: last consolidation was {elapsed_hours:.1}h ago (min {min_hours:.1}h)"
    ))]
    RateLimited {
        elapsed_hours: f64,
        min_hours: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub enum ConsolidationTrigger {
    /// An entity accumulated more than the threshold of active facts.
    EntityOverflow {
        entity_id: EntityId,
        fact_count: usize,
    },
    /// A Louvain community cluster accumulated more than the threshold of active facts.
    CommunityOverflow { cluster_id: i64, fact_count: usize },
}
```

```rust
impl ConsolidationTrigger {
    pub fn trigger_type (&self) -> &'static str;
    pub fn trigger_id (&self) -> String;
}
```

```rust
pub struct ConsolidationCandidate {
    /// Why this cluster was selected.
    pub trigger: ConsolidationTrigger,
    /// IDs of the facts to consolidate.
    pub fact_ids: Vec<FactId>,
    /// Number of eligible facts.
    pub fact_count: usize,
    /// Entity that triggered consolidation (if entity-triggered).
    pub entity_id: Option<EntityId>,
    /// Community cluster that triggered consolidation (if community-triggered).
    pub cluster_id: Option<i64>,
}
```

```rust
pub struct ConsolidatedFact {
    /// The consolidated fact content.
    pub content: String,
    /// Confidence score (fixed at 0.95 for consolidation outputs).
    pub confidence: f64,
    /// Epistemic tier (always `inferred` for LLM consolidation outputs).
    pub tier: String,
    /// IDs of the original facts that were consolidated into this one.
    pub source_fact_ids: Vec<FactId>,
    /// `recorded_at` timestamps (ISO 8601) of the original facts, aligned to
    /// [`source_fact_ids`](Self::source_fact_ids) by index.
    ///
    /// Used to compute multiplicity time-spread metadata (#3634). Defaulted
    /// via `#[serde(default)]` so legacy serialized `ConsolidatedFact`
    /// records (which predate this field) still deserialize.
    #[serde(default)]
    pub source_recorded_ats: Vec<String>,
}
```

```rust
pub struct ConsolidationResult {
    /// The new consolidated facts.
    pub consolidated_facts: Vec<ConsolidatedFact>,
    /// IDs of facts that were superseded.
    pub superseded_fact_ids: Vec<FactId>,
    /// Number of input facts.
    pub original_count: usize,
    /// Number of output facts.
    pub consolidated_count: usize,
}
```

```rust
pub struct FactMultiplicity {
    /// The consolidated fact this multiplicity record describes.
    pub fact_id: FactId,
    /// Number of independent source observations that converged on this fact.
    #[serde(default)]
    pub source_count: u32,
    /// Earliest `recorded_at` timestamp among source facts (ISO 8601).
    pub first_observed: String,
    /// Latest `recorded_at` timestamp among source facts (ISO 8601).
    pub last_observed: String,
    /// Span between `first_observed` and `last_observed` in seconds.
    pub time_spread_seconds: i64,
    /// When this multiplicity record was written (ISO 8601).
    pub recorded_at: String,
}
```

```rust
pub struct ConsolidationAuditRecord {
    /// Unique audit ID.
    pub id: String,
    /// What triggered this consolidation.
    pub trigger_type: String,
    /// Entity or cluster ID that triggered it.
    pub trigger_id: String,
    /// Number of original facts.
    pub original_count: usize,
    /// Number of consolidated facts.
    pub consolidated_count: usize,
    /// JSON array of original fact IDs.
    pub original_fact_ids: String,
    /// JSON array of consolidated fact IDs.
    pub consolidated_fact_ids: String,
    /// When consolidation was performed.
    pub consolidated_at: String,
}
```

> Minimal LLM interface for fact consolidation.
> 
> Keeps mneme independent of hermeneus. The nous layer bridges this trait
> to the configured LLM provider.
```rust
pub trait ConsolidationProvider : Send + Sync {
    fn consolidate (&self, system: &str, user_message: &str) -> Result<String, ConsolidationError>;
}
```

```rust
pub fn consolidation_system_prompt () -> &'static str
```

```rust
pub fn consolidation_user_message (facts: &[(FactId, String, f64, String)]) -> String
```

> Parse the LLM response into consolidated fact entries.
> 
> Expects a JSON array of objects with at least a `content` field.
> 
> # Errors
> 
> Returns an error if the response cannot be parsed as valid JSON.
```rust
pub fn parse_consolidation_response (
    response: &str,
) -> Result<Vec<LlmConsolidatedEntry>, ConsolidationError>
```

```rust
pub struct LlmConsolidatedEntry {
    /// The consolidated fact content.
    pub content: String,
    /// Entity names mentioned in this fact.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Relationships mentioned in this fact.
    #[serde(default)]
    pub relationships: Vec<LlmRelationshipEntry>,
}
```

```rust
pub struct LlmRelationshipEntry {
    /// Source entity name.
    pub from: String,
    /// Target entity name.
    pub to: String,
    /// Relationship type.
    #[serde(rename = "type")]
    pub rel_type: String,
}
```

> Datalog query: find entities with more than N active facts older than the age gate.
> 
> Parameters: `$min_count` (Int), `$cutoff` (String: ISO 8601 timestamp),
>             `$nous_id` (String).
> 
> Returns: `[entity_id, fact_count]` sorted by `fact_count` descending.
```rust
pub const ENTITY_OVERFLOW_CANDIDATES: &str = r"
candidates[entity_id, count(fact_id)] :=
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, valid_from, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

?[entity_id, fact_count] :=
    candidates[entity_id, fact_count],
    fact_count >= $min_count

:sort -fact_count
";
```

> Datalog query: find community clusters with more than N active facts older than the age gate.
> 
> Parameters: `$min_count` (Int), `$cutoff` (String: ISO 8601 timestamp),
>             `$nous_id` (String).
> 
> Returns: `[cluster_id, fact_count]` sorted by `fact_count` descending.
```rust
pub const COMMUNITY_OVERFLOW_CANDIDATES: &str = r"
candidates[cluster_id, count(fact_id)] :=
    *graph_scores{entity_id, score_type: 'louvain', cluster_id},
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, valid_from, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

?[cluster_id, fact_count] :=
    candidates[cluster_id, fact_count],
    fact_count >= $min_count

:sort -fact_count
";
```

> Datalog query: gather eligible fact IDs for an entity.
> 
> Parameters: `$entity_id` (String), `$cutoff` (String), `$nous_id` (String).
> Returns: `[fact_id, content, confidence, recorded_at]`.
```rust
pub const ENTITY_FACTS_FOR_CONSOLIDATION: &str = r"
?[fact_id, content, confidence, recorded_at] :=
    *fact_entities{fact_id, entity_id: $entity_id},
    *facts{id: fact_id, content, confidence, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

:sort -confidence
";
```

> Datalog query: gather eligible fact IDs for a community cluster.
> 
> Parameters: `$cluster_id` (Int), `$cutoff` (String), `$nous_id` (String).
> Returns: `[fact_id, content, confidence, recorded_at]`.
```rust
pub const CLUSTER_FACTS_FOR_CONSOLIDATION: &str = r"
?[fact_id, content, confidence, recorded_at] :=
    *graph_scores{entity_id, score_type: 'louvain', cluster_id: $cluster_id},
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, content, confidence, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

:sort -confidence
";
```

> Datalog DDL for the `consolidation_audit` relation.
```rust
pub const CONSOLIDATION_AUDIT_DDL: &str = r":create consolidation_audit {
    id: String =>
    trigger_type: String,
    trigger_id: String,
    original_count: Int,
    consolidated_count: Int,
    original_fact_ids: String,
    consolidated_fact_ids: String,
    consolidated_at: String
}";
```

> Datalog DDL for the `fact_multiplicity` side-index (#3634).
> 
> Side-indexed rather than folded into the `facts` relation so that the
> fact schema stays stable and legacy records without multiplicity
> metadata remain valid. Consumers (recall, conflict resolution) look
> up multiplicity by fact ID on demand.
```rust
pub const FACT_MULTIPLICITY_DDL: &str = r":create fact_multiplicity {
    fact_id: String =>
    source_count: Int,
    first_observed: String,
    last_observed: String,
    time_spread_seconds: Int,
    recorded_at: String
}";
```

## `src/decay.rs`

> Default reinforcement boost per explicit reinforcement event.
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_reinforcement_boost`.
```rust
pub const DEFAULT_REINFORCEMENT_BOOST: f64 = 0.02;
```

> Default maximum cumulative reinforcement bonus (caps at 50 reinforcements).
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_max_reinforcement_bonus`.
```rust
pub const DEFAULT_MAX_REINFORCEMENT_BONUS: f64 = 1.0;
```

> Default multiplier bonus per distinct agent that accessed a fact.
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_cross_agent_bonus_per_agent`.
```rust
pub const DEFAULT_CROSS_AGENT_BONUS_PER_AGENT: f64 = 0.15;
```

> Default maximum cross-agent multiplier (caps at 5 distinct agents → 1.75×).
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_max_cross_agent_multiplier`.
```rust
pub const DEFAULT_MAX_CROSS_AGENT_MULTIPLIER: f64 = 1.75;
```

## `src/dedup.rs`

```rust
pub struct EntityMergeCandidate {
    /// First entity in the pair.
    pub entity_a: EntityId,
    /// Second entity in the pair.
    pub entity_b: EntityId,
    /// Display name of entity A.
    pub name_a: String,
    /// Display name of entity B.
    pub name_b: String,
    /// Jaro-Winkler similarity between names (0.0--1.0).
    pub name_similarity: f64,
    /// Cosine similarity between name embeddings (0.0--1.0).
    pub embed_similarity: f64,
    /// Whether both entities share the same `entity_type`.
    pub type_match: bool,
    /// Whether the entities share any alias.
    pub alias_overlap: bool,
    /// Weighted composite merge score.
    pub merge_score: f64,
}
```

```rust
pub enum MergeDecision {
    /// Score ≥ 0.90: merge automatically.
    AutoMerge,
    /// 0.70 ≤ score < 0.90: queue for human review.
    Review,
    /// Score < 0.70: skip.
    Skip,
}
```

```rust
pub struct MergeRecord {
    /// The surviving entity.
    pub canonical_entity_id: EntityId,
    /// The entity that was merged and removed.
    pub merged_entity_id: EntityId,
    /// Display name of the merged entity (preserved for audit).
    pub merged_entity_name: String,
    /// The composite score that triggered the merge.
    pub merge_score: f64,
    /// Number of `fact_entities` mappings transferred.
    pub facts_transferred: u32,
    /// Number of relationship edges redirected.
    pub relationships_redirected: u32,
    /// When the merge was executed.
    pub merged_at: jiff::Timestamp,
}
```

```rust
pub const DEFAULT_WEIGHT_NAME: f64 = 0.4;
```

```rust
pub const DEFAULT_WEIGHT_EMBED: f64 = 0.3;
```

```rust
pub const DEFAULT_WEIGHT_TYPE: f64 = 0.2;
```

```rust
pub const DEFAULT_WEIGHT_ALIAS: f64 = 0.1;
```

```rust
pub const DEFAULT_JW_THRESHOLD: f64 = 0.85;
```

```rust
pub const DEFAULT_EMBED_THRESHOLD: f64 = 0.80;
```

## `src/derived_rules.rs`

> All rule IDs emitted by the derived-rule engine.
> 
> Used to filter and inspect `derived_facts` rows by provenance.
```rust
pub const RULE_IDS: &[&str] = &[
    "ontological:is_a",
    "causal:transitive_chain",
    "defeasible:default",
];
```

## `src/embedding.rs`

```rust
pub enum EmbeddingError {
    /// The embedding model failed to initialize.
    #[snafu(display("embedding model init failed: {message}"))]
    InitFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding a text chunk failed.
    #[snafu(display("embedding failed: {message}"))]
    EmbedFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The embedding model `RwLock` was poisoned by a prior panic.
    #[snafu(display("embedding model lock poisoned"))]
    LockPoisoned {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Trait for text→vector embedding providers.
> 
> Implementations must be `Send + Sync` for use across async boundaries.
```rust
pub trait EmbeddingProvider : Send + Sync {
    fn embed (&self, text: &str) -> EmbeddingResult<Vec<f32>>;
    fn embed_batch (&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>>; // default impl
    fn dimension (&self) -> usize;
    fn model_name (&self) -> &str;
}
```

```rust
pub struct MockEmbeddingProvider {
    dim: usize,
}
```

```rust
impl MockEmbeddingProvider {
    pub fn new (dim: usize) -> Self;
}
```

> Local embedding provider using candle (pure Rust).
> 
> Downloads and caches models from `HuggingFace` Hub on first use.
> Default model is `BAAI/bge-small-en-v1.5` (384 dimensions).
> 
> Thread-safe via `RwLock`: multiple concurrent reads (embedding requests)
> proceed in parallel. Write locks are only needed for model reload.
```rust
pub struct CandelProvider {
        model: std::sync::RwLock<BertModel>,
        tokenizer: std::sync::RwLock<Tokenizer>,
        model_name: String,
        dimension: usize,
        device: Device,
    }
```

```rust
pub struct EmbeddingConfig {
    /// Provider type: `mock`, `candle`, `voyage`.
    pub provider: String,
    /// Model name (provider-specific).
    pub model: Option<String>,
    /// Output dimension (auto-detected from model if not set).
    pub dimension: Option<usize>,
    /// API key (for cloud providers).
    pub api_key: Option<koina::secret::SecretString>,
}
```

```rust
pub struct DegradedEmbeddingProvider {
    dim: usize,
}
```

```rust
impl DegradedEmbeddingProvider {
    pub fn new (dim: usize) -> Self;
}
```

```rust
pub fn is_degraded_provider (provider: &dyn EmbeddingProvider) -> bool
```

```rust
pub fn create_provider (config: &EmbeddingConfig) -> EmbeddingResult<Box<dyn EmbeddingProvider>>
```

## `src/embedding_eval.rs`

```rust
pub enum EvalError {
    /// A JSONL line could not be parsed as an [`EvalQuery`].
    #[snafu(display("failed to parse eval dataset line {line}: {message}"))]
    ParseFailed {
        line: usize,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The corpus is empty — nothing to rank against.
    #[snafu(display("eval corpus is empty: provide at least one (id, text) pair"))]
    EmptyCorpus {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The dataset contains no queries.
    #[snafu(display("eval dataset is empty: provide at least one query"))]
    EmptyDataset {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding a text failed.
    #[snafu(display("embedding failed during eval: {message}"))]
    EmbedFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Result type for eval operations.
```rust
pub type EvalResult<T> = std::result::Result<T, EvalError>;
```

```rust
pub struct EvalQuery {
    /// The natural-language query text.
    pub query: String,
    /// Ground-truth corpus IDs that should rank in the top K for this query.
    pub relevant_ids: Vec<String>,
    /// Optional human-readable description (ignored during evaluation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
```

```rust
pub struct EvalDataset {
    /// The labelled queries.
    pub queries: Vec<EvalQuery>,
}
```

```rust
impl EvalDataset {
    pub fn from_jsonl_file (path: &std::path::Path) -> EvalResult<Self>;
    pub fn is_empty (&self) -> bool;
}
```

```rust
pub struct QueryResult {
    /// The query text.
    pub query: String,
    /// Whether any ground-truth ID appeared in the top-K results.
    pub hit: bool,
    /// 1/rank of the first hit, or 0.0 if no hit found.
    pub reciprocal_rank: f64,
    /// IDs returned in top-K order by the model.
    pub top_k_ids: Vec<String>,
}
```

```rust
pub struct ModelMetrics {
    /// The embedding model name as reported by the provider.
    pub model_name: String,
    /// K used during this evaluation run.
    pub k: usize,
    /// Recall@K: fraction of queries with at least one ground-truth hit in top K.
    pub recall_at_k: f64,
    /// Recall@5 (re-computed at K=5 regardless of the run K, or the run K if K<5).
    pub recall_at_5: f64,
    /// Recall@10 (re-computed at K=10 regardless of the run K, or the run K if K<10).
    pub recall_at_10: f64,
    /// Mean Reciprocal Rank across all queries.
    pub mrr: f64,
    /// Per-query detail.
    pub per_query: Vec<QueryResult>,
}
```

```rust
pub struct EvalRunResult {
    /// Metrics for the baseline (current) model.
    pub baseline: ModelMetrics,
    /// Metrics for the candidate model, if one was evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<ModelMetrics>,
    /// `true` if the candidate is at least as good as baseline (or no candidate).
    pub passed: bool,
}
```

```rust
pub fn compare_models (
    baseline: &dyn EmbeddingProvider,
    candidate: Option<&dyn EmbeddingProvider>,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
) -> EvalResult<EvalRunResult>
```

## `src/evidence_gap.rs`

```rust
pub struct AnsweredQuestion {
    /// The sub-question text.
    pub question: String,
    /// Fact IDs from the knowledge store that support the answer.
    pub evidence_ids: Vec<String>,
    /// How well the evidence covers this question (0.0 = no coverage, 1.0 = fully answered).
    pub confidence: f64,
}
```

```rust
pub struct EvidenceQuery {
    /// The user's original information need.
    pub original_query: String,
    /// Heuristically decomposed sub-questions.
    pub sub_questions: Vec<String>,
    /// Sub-questions with supporting evidence.
    pub answered: Vec<AnsweredQuestion>,
    /// Sub-questions still unanswered.
    pub gaps: Vec<String>,
}
```

```rust
pub struct EvidenceGapTracker {
    query: EvidenceQuery,
}
```

```rust
impl EvidenceGapTracker {
    pub fn new (query: &str) -> Self;
    pub fn record_evidence (&mut self, question_idx: usize, fact_id: &str, confidence: f64);
    pub fn remaining_gaps (&self) -> &[String];
    pub fn coverage_ratio (&self) -> f64;
    pub fn is_satisfied (&self, min_coverage: f64) -> bool;
    pub fn suggest_refinement (&self) -> Option<String>;
    pub fn query (&self) -> &EvidenceQuery;
}
```

## `src/extract/diff.rs`

```rust
pub struct ParsedDiff {
    /// Individual file diffs.
    pub files: Vec<DiffFile>,
}
```

```rust
pub struct DiffFile {
    /// Path of the file before the change (may be `/dev/null` for new files).
    pub old_path: String,
    /// Path of the file after the change (may be `/dev/null` for deleted files).
    pub new_path: String,
    /// Whether this is a new file.
    pub is_new: bool,
    /// Whether this file was deleted.
    pub is_deleted: bool,
    /// Individual change hunks within the file.
    pub hunks: Vec<DiffHunk>,
}
```

```rust
pub struct DiffHunk {
    /// Starting line number in the old file.
    pub old_start: u32,
    /// Number of lines in the old file.
    pub old_count: u32,
    /// Starting line number in the new file.
    pub new_start: u32,
    /// Number of lines in the new file.
    pub new_count: u32,
    /// Optional hunk header context (function/class name).
    pub context: String,
    /// Lines added in this hunk.
    pub additions: Vec<String>,
    /// Lines removed in this hunk.
    pub deletions: Vec<String>,
}
```

## `src/extract/dispatch.rs`

```rust
pub struct DispatchPattern {
    /// What kind of pattern was detected.
    pub pattern_type: PatternType,
    /// Human-readable description of the pattern.
    pub description: String,
    /// How severe/important this pattern is.
    pub severity: PatternSeverity,
    /// Number of occurrences that triggered detection.
    pub occurrence_count: u32,
    /// Project this pattern was detected in.
    pub project: String,
    /// Optional crate or module scope.
    pub scope: Option<String>,
}
```

```rust
pub enum PatternType {
    /// Same CI failure recurring across multiple dispatches.
    RecurringCiFailure,
    /// A prompt consistently needs multiple resumes.
    HighResumeRate,
    /// A crate consistently produces lint/format issues.
    CrateQualityDrift,
    /// Blast radius violations in a specific area.
    BlastRadiusHotspot,
    /// Cost anomaly (significantly above average).
    CostAnomaly,
    /// Merge conflicts recurring in the same files.
    ConflictHotspot,
}
```

```rust
pub enum PatternSeverity {
    /// Informational — worth tracking but no action needed.
    Info,
    /// Warning — may indicate a developing problem.
    Warning,
    /// Critical — requires attention or intervention.
    Critical,
}
```

```rust
pub struct PromptScore {
    /// The prompt number that was scored.
    pub prompt_number: u32,
    /// Whether the prompt completed without any resumes.
    pub one_shot: bool,
    /// Number of resumes needed.
    pub resume_count: u32,
    /// Whether CI passed on first push.
    pub ci_first_try: bool,
    /// Whether QA passed without corrective prompts.
    pub qa_pass: bool,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
    /// Overall quality grade.
    pub quality_grade: Grade,
}
```

```rust
pub enum Grade {
    /// One-shot success with CI pass on first try.
    A,
    /// One resume or one CI fix needed.
    B,
    /// Multiple resumes or fixes needed.
    C,
    /// Stuck or failed entirely.
    F,
}
```

```rust
pub struct ProjectScores {
    /// Total number of prompts scored.
    pub total_prompts: usize,
    /// Percentage of one-shot completions (0.0 to 1.0).
    pub one_shot_rate: f64,
    /// Percentage of CI first-try passes (0.0 to 1.0).
    pub ci_first_try_rate: f64,
    /// Percentage of QA passes (0.0 to 1.0).
    pub qa_pass_rate: f64,
    /// Average cost in USD per prompt.
    pub avg_cost_usd: f64,
    /// Average duration in milliseconds per prompt.
    pub avg_duration_ms: u64,
    /// Distribution of quality grades.
    pub grade_distribution: HashMap<Grade, usize>,
}
```

```rust
pub struct GradeInputs {
    /// Session completed in one shot, no resume needed.
    pub one_shot: bool,
    /// CI passed on the first attempt, no fix needed.
    pub ci_first_try: bool,
    /// QA gate passed.
    pub qa_pass: bool,
    /// Number of resume attempts during the session.
    pub resume_count: u32,
    /// Session was aborted, errored, or rolled back.
    pub has_failure: bool,
}
```

## `src/extract/engine.rs`

> Drives the extraction pipeline: prompt building, LLM calling, response parsing.
> 
> # Examples
> 
> ```no_run
> use episteme::extract::{ExtractionConfig, ExtractionEngine};
> 
> let config = ExtractionConfig::default();
> let engine = ExtractionEngine::new(config);
> ```
```rust
pub struct ExtractionEngine {
    config: ExtractionConfig,
}
```

```rust
impl ExtractionEngine {
    pub fn new (config: ExtractionConfig) -> Self;
    pub async fn extract (
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<Extraction, ExtractionError>;
    pub async fn extract_refined (
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<RefinedExtraction, ExtractionError>;
    pub fn persist (
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
    ) -> Result<PersistResult, ExtractionError>;
}
```

## `src/extract/error.rs`

```rust
pub enum ExtractionError {
    /// The LLM response could not be parsed as valid extraction JSON.
    ///
    /// Includes a truncated snippet of the raw response for debugging.
    #[snafu(display("failed to parse extraction response: {response_snippet}"))]
    ParseResponse {
        source: serde_json::Error,
        /// First 500 characters of the raw LLM response that failed to parse.
        response_snippet: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The LLM provider returned an error during extraction.
    #[snafu(display("LLM extraction failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Persisting extracted knowledge to the store failed.
    #[snafu(display("failed to persist extraction: {message}"))]
    Persist {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/extract/lesson.rs`

```rust
pub struct ChangeRecord {
    /// File path that was changed.
    pub file_path: String,
    /// Type of change.
    pub change_type: ChangeType,
    /// Summary of what changed (human-readable).
    pub summary: String,
    /// Lines added across all hunks.
    pub lines_added: u32,
    /// Lines removed across all hunks.
    pub lines_removed: u32,
    /// Function/context names from hunk headers, if available.
    pub contexts: Vec<String>,
}
```

```rust
pub enum ChangeType {
    /// New file added.
    Added,
    /// Existing file modified.
    Modified,
    /// File deleted.
    Deleted,
    /// File renamed (detected by `old_path` != `new_path` and both non-null).
    Renamed,
}
```

```rust
pub struct ExtractedLesson {
    /// Entities discovered in the diff (files, modules, functions).
    pub entities: Vec<super::types::ExtractedEntity>,
    /// Relationships between entities.
    pub relationships: Vec<super::types::ExtractedRelationship>,
    /// Facts about what changed and why.
    pub facts: Vec<super::types::ExtractedFact>,
    /// Causal edges between facts (e.g., "bug fix caused by code change").
    pub causal_edges: Vec<CausalFactPair>,
}
```

```rust
pub struct CausalFactPair {
    /// Index of the cause fact in the lesson's fact list.
    pub cause_index: usize,
    /// Index of the effect fact in the lesson's fact list.
    pub effect_index: usize,
    /// Confidence in this causal link.
    pub confidence: f64,
}
```

```rust
pub struct LessonConfig {
    /// PR identifier or title for provenance.
    pub pr_title: String,
    /// PR number for linking.
    pub pr_number: Option<u32>,
    /// The nous agent this lesson belongs to.
    pub nous_id: String,
    /// Source identifier (e.g., "pr-merge:123").
    pub source: String,
}
```

```rust
pub struct LessonPersistResult {
    /// Number of entities written.
    pub entities_inserted: usize,
    /// Number of relationships written.
    pub relationships_inserted: usize,
    /// Number of facts written.
    pub facts_inserted: usize,
    /// Number of causal edges written.
    pub causal_edges_inserted: usize,
}
```

## `src/extract/observation.rs`

```rust
pub enum ObservationType {
    /// A defect in existing code: crash, panic, error, wrong behavior.
    Bug,
    /// Technical debt: refactoring opportunity, cleanup, code smell.
    Debt,
    /// A new idea or improvement suggestion.
    Idea,
    /// Missing or inadequate test coverage.
    MissingTest,
    /// Missing or outdated documentation.
    DocGap,
}
```

```rust
impl ObservationType {
    pub fn classify (text: &str) -> Self;
    pub fn as_str (&self) -> &'static str;
    pub fn from_str_lossy (s: &str) -> Self;
}
```

```rust
pub struct RawObservation {
    /// The observation text, trimmed of leading bullet markers.
    pub text: String,
    /// Tags extracted from the text (crate names, file paths).
    pub tags: Vec<String>,
    /// Classified observation type.
    pub observation_type: ObservationType,
}
```

```rust
pub fn parse_observations (pr_body: &str) -> Vec<RawObservation>
```

```rust
pub fn extract_tags (text: &str) -> Vec<String>
```

## `src/extract/provider.rs`

> Minimal LLM completion interface for extraction.
> 
> Keeps mneme independent of hermeneus. The nous layer bridges this trait
> to the full `LlmProvider` + `CompletionRequest` API.
> 
> Uses a boxed future return type to remain dyn-compatible (object-safe).
```rust
pub trait ExtractionProvider : Send + Sync {
    fn complete <'a> (
        &'a self,
        system: &'a str,
        user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    >;
}
```

## `src/extract/refinement.rs`

```rust
pub enum TurnType {
    /// General conversation: extract facts, entities, relationships.
    Discussion,
    /// Code/tool output dominant: extract decisions, skip noise.
    ToolHeavy,
    /// Architecture/design: extract decisions and rationale.
    Planning,
    /// Error investigation: extract resolution, skip stack traces.
    Debugging,
    /// Explicit corrections: high priority extraction.
    Correction,
    /// How-to/instructions: extract steps and dependencies.
    Procedural,
}
```

```rust
pub fn classify_turn (content: &str) -> TurnType
```

```rust
pub enum FactType {
    /// Personal identity information (name, role, background).
    Identity,
    /// User preferences and opinions.
    Preference,
    /// Skills, tools, and expertise.
    Skill,
    /// Relationships between entities.
    Relationship,
    /// Time-bound events.
    Event,
    /// Tasks, todos, and action items.
    Task,
    /// General observations and inferences.
    Observation,
}
```

```rust
pub fn classify_fact (content: &str) -> FactType
```

```rust
pub struct CorrectionSignal {
    /// Whether the content contains a correction.
    pub is_correction: bool,
    /// Confidence boost to apply (0.2 for corrections, 0.0 otherwise).
    pub confidence_boost: f64,
}
```

```rust
pub fn detect_correction (content: &str) -> CorrectionSignal
```

```rust
pub enum FilterReason {
    /// Confidence score below threshold.
    LowConfidence,
    /// Content too short (< 10 chars).
    TooShort,
    /// Content too long (> 500 chars).
    TooLong,
    /// Content is trivial metadata.
    Trivial,
    /// Duplicate of an earlier fact in the same batch.
    Duplicate,
    /// One or more triple fields (subject, predicate, object) are empty or whitespace-only.
    EmptyField,
}
```

```rust
pub struct FilterResult {
    /// Whether the fact passed all filters.
    pub passed: bool,
    /// If rejected, the reason.
    pub reason: Option<FilterReason>,
}
```

```rust
pub struct RejectedFact {
    /// The fact content.
    pub content: String,
    /// The original confidence.
    pub confidence: f64,
    /// Why it was rejected.
    pub reason: FilterReason,
}
```

```rust
pub struct BatchFilterResult {
    /// Facts that passed all filters.
    pub passed: Vec<(String, f64)>,
    /// Facts that were rejected, with reasons.
    pub rejected: Vec<RejectedFact>,
}
```

## `src/extract/training.rs`

```rust
pub struct TrainingLesson {
    /// The lint rule this lesson is about.
    pub rule: String,
    /// Classification of the lesson outcome.
    pub outcome: LessonOutcome,
    /// Human-readable description of the lesson.
    pub description: String,
    /// Confidence in this lesson (0.0--1.0).
    pub confidence: f64,
    /// Files where this pattern was observed.
    pub affected_files: Vec<String>,
    /// Number of occurrences that contributed to this lesson.
    pub occurrence_count: u32,
    /// Source PR number, if from a merged PR.
    pub pr_number: Option<u32>,
}
```

```rust
pub enum LessonOutcome {
    /// A violation was fixed in a merged PR.
    FixedInPr,
    /// A violation pattern recurs across multiple scans (not yet fixed).
    RecurringViolation,
    /// A rule's violation count decreased over time (improving trend).
    ImprovingTrend,
    /// A rule's violation count increased over time (degrading trend).
    DegradingTrend,
}
```

```rust
pub struct ExtractionResult {
    /// Lessons extracted from the training data.
    pub lessons: Vec<TrainingLesson>,
    /// Number of violation records read.
    pub violations_read: usize,
    /// Number of lint summary records read.
    pub lint_summaries_read: usize,
    /// Number of records skipped (parse errors, quality gate failures).
    pub records_skipped: usize,
}
```

> Extract lessons from training data JSONL files.
> 
> Reads violations and lint summaries, applies quality gates, and produces
> deduplicated lessons grouped by rule.
> 
> # Quality gates
> 
> - Violations with `pr_number` and `sha` are treated as fixed (merged PR).
> - Violations without PR context are treated as unfixed (recurring).
> - Duplicate rule+file pairs are collapsed into a single lesson with
>   an occurrence count.
> 
> # Errors
> 
> Returns `std::io::Error` if the training data files cannot be read.
```rust
pub fn extract_from_training_data (training_dir: &Path) -> std::io::Result<ExtractionResult>
```

```rust
pub fn lessons_to_facts (lessons: &[TrainingLesson]) -> Vec<super::types::ExtractedFact>
```

## `src/extract/types.rs`

```rust
pub struct Extraction {
    /// Named entities found in the conversation.
    pub entities: Vec<ExtractedEntity>,
    /// Relationships between entities.
    pub relationships: Vec<ExtractedRelationship>,
    /// Factual claims as subject-predicate-object triples.
    pub facts: Vec<ExtractedFact>,
}
```

```rust
pub struct ExtractedEntity {
    /// Normalized entity name (proper noun form).
    pub name: String,
    /// Category: person, project, concept, tool, or location.
    pub entity_type: String,
    /// Brief description of the entity from context.
    pub description: String,
}
```

```rust
pub struct ExtractedRelationship {
    /// Entity name (source).
    pub source: String,
    /// Verb phrase: "works on", "depends on", "created by".
    pub relation: String,
    /// Entity name (target).
    pub target: String,
    /// 0.0--1.0.
    pub confidence: f64,
}
```

```rust
pub struct ExtractedFact {
    /// The entity or concept the fact is about.
    pub subject: String,
    /// The relationship verb phrase.
    pub predicate: String,
    /// The object of the claim.
    pub object: String,
    /// Confidence score (0.0--1.0).
    pub confidence: f64,
    /// Whether this fact is a correction of prior information.
    ///
    /// Detected by heuristic patterns (e.g. "actually, it's X not Y").
    /// Corrections get a +0.2 confidence boost (capped at 1.0) and
    /// skip the SUPPLEMENTS path in conflict detection.
    #[serde(default)]
    pub is_correction: bool,
    /// Classified fact type for FSRS decay tuning.
    #[serde(default)]
    pub fact_type: Option<String>,
}
```

```rust
pub struct ExtractionConfig {
    /// LLM model to use for extraction.
    pub model: String,
    /// Minimum total message length (chars) before extraction triggers.
    pub min_message_length: usize,
    /// Maximum entities to extract per conversation segment.
    pub max_entities: usize,
    /// Maximum relationships to extract per conversation segment.
    pub max_relationships: usize,
    /// Maximum facts to extract per conversation segment.
    pub max_facts: usize,
    /// Whether extraction is active.
    pub enabled: bool,
}
```

```rust
pub struct ExtractedToolCall {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Input parameters (JSON).
    pub input: serde_json::Value,
    /// Result content, if available.
    pub result: Option<String>,
    /// Whether the tool call errored.
    pub is_error: bool,
}
```

```rust
pub struct ConversationMessage {
    /// Message role (e.g. "user", "assistant").
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool calls made during this turn, if any.
    pub tool_calls: Option<Vec<ExtractedToolCall>>,
    /// Reasoning or thinking blocks generated by the model, if any.
    pub reasoning: Option<String>,
}
```

```rust
pub struct ExtractionPrompt {
    /// System prompt with JSON schema and extraction rules.
    pub system: String,
    /// Concatenated conversation text for the user message.
    pub user_message: String,
}
```

```rust
pub struct RefinedExtraction {
    /// The extraction after quality filters and confidence boosts.
    pub extraction: Extraction,
    /// The classified turn type.
    pub turn_type: refinement::TurnType,
    /// Number of facts filtered out by quality checks.
    pub facts_filtered: usize,
    /// Causal signal detected in the session text, if any.
    ///
    /// `Some((relation_type, confidence))` when the combined message text
    /// contains causal language ("because", "therefore", "caused by", etc.).
    /// Consumers can use this to drive the crate-private `extract_causal_edges`
    /// helper with the relevant fact IDs.
    pub causal_signal: Option<(CausalRelationType, f64)>,
}
```

```rust
pub struct PersistResult {
    /// Number of entities written.
    pub entities_inserted: usize,
    /// Number of relationships written.
    pub relationships_inserted: usize,
    /// Number of relationships skipped due to validation.
    pub relationships_skipped: usize,
    /// Number of facts written.
    pub facts_inserted: usize,
    /// Number of causal edges extracted and recorded.
    pub causal_edges_inserted: usize,
}
```

## `src/graph_intelligence.rs`

```rust
impl crate::knowledge_store::KnowledgeStore {
    pub fn recompute_graph_scores (&self) -> crate::error::Result<()>;
    pub fn compute_and_store_volatility (&self) -> crate::error::Result<()>;
    pub fn compute_centrality (&self) -> BTreeMap<crate::id::EntityId, f64>;
    pub fn shortest_path (
        &self,
        from: &crate::id::EntityId,
        to: &crate::id::EntityId,
    ) -> Option<Vec<crate::id::EntityId>>;
    pub fn connected_components (&self) -> Vec<Vec<crate::id::EntityId>>;
    pub fn compute_bfs_proximity_decay (
        &self,
        seeds: &[crate::id::EntityId],
        decay: f64,
    ) -> BTreeMap<crate::id::EntityId, f64>;
}
```

## `src/ingest.rs`

```rust
pub enum IngestFormat {
    /// Markdown with optional YAML frontmatter.
    Markdown,
    /// Plain text.
    PlainText,
    /// JSON array of facts or single fact object.
    Json,
    /// JSON Lines — one fact per line.
    Jsonl,
}
```

```rust
pub fn parse_format (s: &str) -> Option<IngestFormat>
```

```rust
pub struct IngestChunk {
    /// The chunk text.
    pub content: String,
    /// Optional source identifier (file name, URL, etc.).
    pub source_hint: Option<String>,
}
```

```rust
pub struct IngestConfig {
    /// Maximum characters per chunk before splitting.
    pub max_chunk_size: usize,
    /// Overlap between consecutive chunks.
    pub chunk_overlap: usize,
    /// Default confidence for heuristic-extracted facts.
    pub default_confidence: f64,
}
```

> Ingest raw content and produce facts.
> 
> For [`IngestFormat::Json`] and [`IngestFormat::Jsonl`], facts are parsed
> directly from the input. For [`IngestFormat::Markdown`] and
> [`IngestFormat::PlainText`], content is chunked and each chunk becomes a
> heuristic fact.
> 
> # Errors
> 
> Returns an error if JSON parsing fails or if a generated fact ID is
> invalid.
```rust
pub fn ingest_content (
    content: &str,
    format: IngestFormat,
    config: &IngestConfig,
    nous_id: &str,
) -> crate::error::Result<Vec<Fact>>
```

## `src/instinct.rs`

> Default maximum length for parameter values before truncation.
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::instinct_max_param_value_len`.
```rust
pub const DEFAULT_MAX_PARAM_VALUE_LEN: usize = 200;
```

> Default maximum length for context summaries.
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::instinct_max_context_summary_len`.
```rust
pub const DEFAULT_MAX_CONTEXT_SUMMARY_LEN: usize = 100;
```

> Default minimum observations before a behavioral pattern is created.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_instinct_min_observations`.
```rust
pub const DEFAULT_MIN_OBSERVATIONS: u32 = 5;
```

> Default minimum success rate (0.0--1.0) before a behavioral pattern is created.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_instinct_min_success_rate`.
```rust
pub const DEFAULT_MIN_SUCCESS_RATE: f64 = 0.80;
```

```rust
pub struct ToolObservation {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Sanitized parameters (secrets stripped, values truncated).
    pub parameters: serde_json::Value,
    /// Outcome of the tool call.
    pub outcome: ToolOutcome,
    /// Brief summary of the context that prompted this tool call (≤100 chars).
    pub context_summary: String,
    /// Which nous made the observation.
    pub nous_id: String,
    /// When the observation was recorded.
    pub observed_at: jiff::Timestamp,
}
```

```rust
pub enum ToolOutcome {
    /// Tool completed successfully.
    Success,
    /// Tool failed with an error.
    Failure {
        /// Error description.
        error: String,
    },
    /// Tool partially succeeded.
    Partial {
        /// Description of partial result.
        note: String,
    },
}
```

```rust
impl ToolOutcome {
    pub fn is_success (&self) -> bool;
    pub fn as_stored_string (&self) -> String;
}
```

```rust
pub fn sanitize_parameters (
    params: &serde_json::Value,
    max_param_value_len: usize,
) -> serde_json::Value
```

```rust
pub fn truncate_context_summary (summary: &str, max_context_summary_len: usize) -> String
```

## `src/knowledge_portability.rs`

```rust
pub struct KnowledgeImportResult {
    /// Number of facts successfully imported.
    pub facts_imported: usize,
    /// Number of entities successfully imported.
    pub entities_imported: usize,
    /// Number of relationships successfully imported.
    pub relationships_imported: usize,
}
```

## `src/knowledge_store/derived_rules.rs`

```rust
pub struct DerivedFact {
    /// The entity this derived fact is about.
    pub entity_id: String,
    /// The rule that produced this fact. One of [`crate::derived_rules::RULE_IDS`].
    pub rule_id: String,
    /// The inferred content string (format depends on rule family).
    pub derived_content: String,
    /// Confidence score in `[0.0, 1.0]`.
    pub confidence: f64,
}
```

```rust
impl KnowledgeStore {
    pub fn insert_type_hierarchy (
        &self,
        child_type: &str,
        parent_type: &str,
    ) -> crate::error::Result<()>;
    pub fn insert_default (
        &self,
        entity_id: &str,
        tag: &str,
        default_content: &str,
        confidence: f64,
    ) -> crate::error::Result<()>;
    pub fn materialize_derived_facts (&self) -> crate::error::Result<usize>;
    pub fn query_derived_facts (&self, entity_id: &str) -> crate::error::Result<Vec<DerivedFact>>;
    pub fn query_derived_facts_by_rule (
        &self,
        entity_id: &str,
        rule_prefix: &str,
    ) -> crate::error::Result<Vec<DerivedFact>>;
}
```

## `src/knowledge_store/entity.rs`

```rust
impl KnowledgeStore {
    pub fn insert_entity (&self, entity: &crate::knowledge::Entity) -> crate::error::Result<()>;
    pub fn insert_relationship (
        &self,
        rel: &crate::knowledge::Relationship,
    ) -> crate::error::Result<()>;
    pub fn list_entities (&self) -> crate::error::Result<Vec<crate::knowledge::Entity>>;
    pub fn find_duplicate_entities (
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>>;
    pub fn get_pending_merges (
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>>;
    pub fn run_entity_dedup (
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>>;
}
```

## `src/knowledge_store/facts.rs`

```rust
impl KnowledgeStore {
    pub fn insert_fact (&self, fact: &crate::knowledge::Fact) -> crate::error::Result<()>;
    pub fn query_facts (
        &self,
        nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn increment_access_async (
        self: &std::sync::Arc<Self>,
        fact_ids: Vec<crate::id::FactId>,
    ) -> crate::error::Result<()>;
    pub fn forget_fact (
        &self,
        fact_id: &crate::id::FactId,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<crate::knowledge::Fact>;
    pub async fn list_forgotten_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn query_facts_temporal_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        at_time: String,
        filter: Option<String>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn query_facts_diff_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        from_time: String,
        to_time: String,
    ) -> crate::error::Result<crate::knowledge::FactDiff>;
    pub fn list_all_facts (&self, limit: i64) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn list_all_facts_async (
        self: &std::sync::Arc<Self>,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub fn audit_all_facts (
        &self,
        nous_id: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn forget_fact_async (
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<crate::knowledge::Fact>;
    pub async fn unforget_fact_async (
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
    ) -> crate::error::Result<crate::knowledge::Fact>;
    pub async fn audit_all_facts_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn update_confidence_async (
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        confidence: f64,
    ) -> crate::error::Result<crate::knowledge::Fact>;
    pub async fn update_sensitivity_async (
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        sensitivity: crate::knowledge::FactSensitivity,
    ) -> crate::error::Result<crate::knowledge::Fact>;
    pub async fn insert_fact_async (
        self: &std::sync::Arc<Self>,
        fact: crate::knowledge::Fact,
    ) -> crate::error::Result<()>;
    pub fn query_facts_by_type (
        &self,
        nous_id: &str,
        fact_type: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn query_facts_by_type_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        fact_type: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub async fn query_facts_async (
        self: &std::sync::Arc<Self>,
        nous_id: String,
        now: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
}
```

## `src/knowledge_store/mod.rs`

> Datalog DDL for initializing the knowledge schema.
```rust
pub const KNOWLEDGE_DDL: &[&str] = &[
    r":create facts {
        id: String, valid_from: String =>
        content: String,
        nous_id: String,
        confidence: Float,
        tier: String,
        valid_to: String,
        superseded_by: String?,
        source_session_id: String?,
        recorded_at: String,
        access_count: Int,
        last_accessed_at: String,
        stability_hours: Float,
        fact_type: String,
        is_forgotten: Bool default false,
        forgotten_at: String?,
        forget_reason: String?
    }",
    r":create entities {
        id: String =>
        name: String,
        entity_type: String,
        aliases: String,
        created_at: String,
        updated_at: String
    }",
    r":create relationships {
        src: String, dst: String =>
        relation: String,
        weight: Float,
        created_at: String
    }",
    r":create fact_entities {
        fact_id: String, entity_id: String =>
        created_at: String
    }",
    r":create merge_audit {
        canonical_id: String, merged_id: String =>
        merged_name: String,
        merge_score: Float,
        facts_transferred: Int,
        relationships_redirected: Int,
        merged_at: String
    }",
    r":create pending_merges {
        entity_a: String, entity_b: String =>
        name_a: String,
        name_b: String,
        name_similarity: Float,
        embed_similarity: Float,
        type_match: Bool,
        alias_overlap: Bool,
        merge_score: Float,
        created_at: String
    }",
    r":create causal_edges {
        cause: String, effect: String =>
        ordering: String,
        relationship_type: String,
        confidence: Float,
        created_at: String
    }",
    // Index 7 — type_hierarchy (added in schema v8)
    r":create type_hierarchy {
        child_type: String, parent_type: String =>
        created_at: String
    }",
    // Index 8 — derived_facts (added in schema v8)
    r":create derived_facts {
        entity_id: String, rule_id: String, derived_content: String =>
        confidence: Float,
        materialized_at: String
    }",
    // Index 9 — defaults (added in schema v8)
    r":create defaults {
        entity_id: String, tag: String =>
        default_content: String,
        confidence: Float,
        created_at: String
    }",
];
```

```rust
pub fn embeddings_ddl (dim: usize) -> String
```

```rust
pub fn hnsw_ddl (dim: usize) -> String
```

```rust
pub fn fts_ddl () -> &'static str
```

```rust
pub struct QueryResult {
    /// Column names in the order they appear in each row.
    pub headers: Vec<String>,
    /// Result rows. Each row is a flat `Vec` matching `headers` by position.
    ///
    /// Crate-internal: external callers should use the typed accessor methods
    /// ([`get_string`](Self::get_string), [`get_f64`](Self::get_f64), etc.)
    /// instead of indexing into rows directly.
    pub(crate) rows: Vec<Vec<crate::engine::DataValue>>,
}
```

```rust
impl QueryResult {
    pub fn row_count (&self) -> usize;
    pub fn is_empty (&self) -> bool;
    pub fn rows (&self) -> &[Vec<crate::engine::DataValue>];
    pub fn get_string (&self, row: usize, col: &str) -> Option<String>;
    pub fn get_f64 (&self, row: usize, col: &str) -> Option<f64>;
    pub fn get_i64 (&self, row: usize, col: &str) -> Option<i64>;
    pub fn get_bool (&self, row: usize, col: &str) -> Option<bool>;
    pub fn rows_as_strings (&self) -> Vec<Vec<String>>;
    pub fn rows_to_json (&self) -> Vec<Vec<serde_json::Value>>;
}
```

```rust
pub struct KnowledgeConfig {
    /// Embedding dimension for the HNSW index.
    pub dim: usize,
    /// Admission policy for fact insertion. Default: [`DefaultAdmissionPolicy`](crate::admission::DefaultAdmissionPolicy).
    pub admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}
```

```rust
pub struct HybridQuery {
    /// Full-text search query string (BM25 signal).
    pub text: String,
    /// Query embedding vector (HNSW signal).
    pub embedding: Vec<f32>,
    /// Seed entity IDs for graph neighborhood expansion (graph signal).
    /// Empty slice disables the graph signal.
    pub seed_entities: Vec<crate::id::EntityId>,
    /// Maximum number of results to return.
    pub limit: usize,
    /// ef parameter for HNSW search (controls recall/speed tradeoff).
    pub ef: usize,
}
```

```rust
pub struct HybridResult {
    /// Document ID (from facts or embeddings relation).
    pub id: crate::id::FactId,
    /// Fused RRF score (higher = more relevant).
    pub rrf_score: f64,
    /// Rank in BM25 signal (-1 = absent, 1+ = rank where 1 is best).
    pub bm25_rank: i64,
    /// Rank in vector search signal (-1 = absent, 1+ = rank).
    pub vec_rank: i64,
    /// Rank in graph neighborhood signal (-1 = absent, 1+ = rank).
    pub graph_rank: i64,
}
```

```rust
pub struct KnowledgeStore {
    db: std::sync::Arc<crate::engine::Db>,
    dim: usize,
    /// Serializes read-modify-write access counter increments to prevent races.
    access_lock: std::sync::Mutex<()>,
    /// Admission policy gate: checked before every fact insertion.
    admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}
```

```rust
impl KnowledgeStore {
    pub fn open_mem () -> crate::error::Result<std::sync::Arc<Self>>;
    pub fn open_mem_with_config (
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>>;
    pub fn open_fjall (
        path: impl AsRef<std::path::Path>,
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>>;
    pub fn schema_version (&self) -> crate::error::Result<i64>;
    pub fn run_query (
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult>;
    pub fn run_query_with_timeout (
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
        timeout: Option<std::time::Duration>,
    ) -> crate::error::Result<QueryResult>;
    pub fn run_mut_query (
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult>;
    pub fn backup_db (&self, out_file: impl AsRef<std::path::Path>) -> crate::error::Result<()>;
    pub fn restore_backup (&self, in_file: impl AsRef<std::path::Path>) -> crate::error::Result<()>;
    pub fn import_from_backup (
        &self,
        in_file: impl AsRef<std::path::Path>,
        relations: &[String],
    ) -> crate::error::Result<()>;
    pub fn run_script_read_only (
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult>;
    pub fn read_facts_by_id (&self, id: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
}
```

## `src/knowledge_store/search.rs`

```rust
impl KnowledgeStore {
    pub fn insert_embedding (
        &self,
        chunk: &crate::knowledge::EmbeddedChunk,
    ) -> crate::error::Result<()>;
    pub fn search_vectors (
        &self,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>>;
    pub async fn search_vectors_async (
        self: &std::sync::Arc<Self>,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>>;
    pub fn search_text_for_recall (
        &self,
        query_text: &str,
        k: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>>;
    pub async fn search_hybrid_async (
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
    ) -> crate::error::Result<Vec<HybridResult>>;
    pub async fn search_temporal_async (
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
        at_time: String,
    ) -> crate::error::Result<Vec<HybridResult>>;
}
```

## `src/knowledge_store/skills.rs`

```rust
impl KnowledgeStore {
    pub fn find_skills_for_nous (
        &self,
        nous_id: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub fn search_skills (
        &self,
        nous_id: &str,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub fn find_skill_by_name (
        &self,
        nous_id: &str,
        skill_name: &str,
    ) -> crate::error::Result<Option<String>>;
    pub fn find_pending_skills (
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>>;
    pub fn approve_pending_skill (
        &self,
        pending_fact_id: &crate::id::FactId,
        nous_id: &str,
    ) -> crate::error::Result<crate::id::FactId>;
    pub fn reject_pending_skill (
        &self,
        pending_fact_id: &crate::id::FactId,
    ) -> crate::error::Result<()>;
    pub fn run_skill_decay (&self, nous_id: &str) -> crate::error::Result<(usize, usize, usize)>;
    pub fn find_duplicate_skill (
        &self,
        nous_id: &str,
        skill_content: &crate::skill::SkillContent,
    ) -> crate::error::Result<Option<crate::id::FactId>>;
}
```

## `src/manifest.rs`

```rust
pub struct MemoryHeader {
    /// Source identifier (fact ID, document reference, or path).
    pub source_id: String,
    /// Short name or title for this memory entry.
    pub name: String,
    /// Brief description extracted from entry metadata.
    pub description: Option<String>,
    /// Modification time in milliseconds since epoch.
    pub mtime_ms: i64,
}
```

```rust
pub struct MemoryManifest {
    headers: Vec<MemoryHeader>,
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/ops_facts.rs`

```rust
pub struct OpsSnapshot {
    /// Which nous these metrics belong to.
    pub nous_id: String,
    /// Total active sessions at snapshot time.
    pub active_session_count: u64,
    /// Total tool calls observed in the current window.
    pub tool_call_total: u64,
    /// Successful tool calls in the current window.
    pub tool_call_successes: u64,
    /// Total errors observed in the current window.
    pub error_count: u64,
    /// Average task execution latency in milliseconds (0 if no tasks ran).
    pub avg_task_latency_ms: u64,
    /// Number of tasks that contributed to the latency average.
    pub task_sample_count: u64,
}
```

```rust
pub struct OpsFact {
    /// The knowledge graph fact.
    pub fact: Fact,
}
```

> Extracts knowledge graph facts from operational metric snapshots.
> 
> Each extraction produces up to 4 facts:
> - `ops.sessions`: active session count
> - `ops.tool_success_rate`: tool call success rate percentage
> - `ops.error_count`: error count in the observation window
> - `ops.task_latency`: average task execution latency
```rust
pub struct OpsFactExtractor;
```

> Default minimum tool calls before success rate is meaningful.
> 
> Callers should prefer the value from `taxis::config::KnowledgeConfig::instinct_min_tool_calls`.
```rust
pub const DEFAULT_MIN_TOOL_CALLS: u64 = 5;
```

```rust
impl OpsFactExtractor {
    pub fn extract (
        snapshot: &OpsSnapshot,
        min_tool_calls: u64,
    ) -> Result<Vec<OpsFact>, ExtractError>;
}
```

```rust
pub enum ExtractError {
    /// Failed to create a fact ID.
    #[snafu(display("failed to create operational fact ID: {source}"))]
    FactId {
        /// The underlying ID validation error.
        source: crate::id::IdValidationError,
    },
}
```

## `src/query/builders.rs`

```rust
pub struct QueryBuilder {
    lines: Vec<String>,
    params: BTreeMap<String, DataValue>,
}
```

```rust
pub struct PutBuilder {
    parent: QueryBuilder,
    relation: Relation,
    all_fields: Vec<&'static str>,
    key_count: usize,
    rows: Vec<Vec<String>>,
}
```

```rust
pub struct ScanBuilder {
    parent: QueryBuilder,
    relation: Relation,
    select: Vec<&'static str>,
    bindings: Vec<String>,
    filters: Vec<String>,
    order: Option<String>,
    limit: Option<String>,
}
```

```rust
pub struct RmBuilder {
    parent: QueryBuilder,
    relation: Relation,
    key_fields: Vec<&'static str>,
}
```

## `src/query/schema.rs`

> Datalog field reference. Implemented by per-relation field enums.
```rust
pub trait Field : Copy {
    fn name (self) -> &'static str;
}
```

```rust
pub enum Relation {
    /// Temporal facts with validity windows and confidence scores.
    Facts,
    /// Named entities (people, places, concepts).
    Entities,
    /// Directed edges between entities with typed relations.
    Relationships,
    /// Vector embeddings for semantic search.
    Embeddings,
    /// Fact-to-entity membership mapping.
    FactEntities,
    /// Audit log of completed entity merges.
    MergeAudit,
    /// Queue of candidate entity merges awaiting review.
    PendingMerges,
    /// Directed causal edges between fact nodes.
    CausalEdges,
}
```

```rust
pub enum FactsField {
    Id,
    ValidFrom,
    Content,
    NousId,
    Confidence,
    Tier,
    ValidTo,
    SupersededBy,
    SourceSessionId,
    RecordedAt,
    AccessCount,
    LastAccessedAt,
    StabilityHours,
    FactType,
    IsForgotten,
    ForgottenAt,
    ForgetReason,
}
```

```rust
pub enum EntitiesField {
    Id,
    Name,
    EntityType,
    Aliases,
    CreatedAt,
    UpdatedAt,
}
```

```rust
pub enum RelationshipsField {
    Src,
    Dst,
    Relation,
    Weight,
    CreatedAt,
}
```

```rust
pub enum EmbeddingsField {
    Id,
    Content,
    SourceType,
    SourceId,
    NousId,
    Embedding,
    CreatedAt,
}
```

```rust
pub enum FactEntitiesField {
    FactId,
    EntityId,
    CreatedAt,
}
```

```rust
pub enum MergeAuditField {
    CanonicalId,
    MergedId,
    MergedName,
    MergeScore,
    FactsTransferred,
    RelationshipsRedirected,
    MergedAt,
}
```

```rust
pub enum PendingMergesField {
    EntityA,
    EntityB,
    NameA,
    NameB,
    NameSimilarity,
    EmbedSimilarity,
    TypeMatch,
    AliasOverlap,
    MergeScore,
    CreatedAt,
}
```

```rust
pub enum CausalEdgesField {
    Cause,
    Effect,
    Ordering,
    RelationshipType,
    Confidence,
    CreatedAt,
}
```

## `src/query_rewrite.rs`

```rust
pub enum RewriteError {
    /// The LLM provider returned an error.
    LlmCall(String),
    /// The LLM response could not be parsed.
    ParseResponse(String),
}
```

```rust
pub struct RewriteConfig {
    /// Maximum number of variant queries to generate (2-4).
    pub max_variants: usize,
    /// Whether to always include the original query in the variant set.
    pub include_original: bool,
}
```

```rust
pub struct RewriteResult {
    /// The original query string.
    pub original: String,
    /// Generated search variant queries (may include the original).
    pub variants: Vec<String>,
    /// Time spent on the rewrite operation in milliseconds.
    pub latency_ms: u64,
}
```

```rust
pub struct TieredSearchConfig {
    /// Minimum results from fast path before escalating to enhanced search.
    pub fast_path_min_results: usize,
    /// Minimum RRF score threshold for fast path results to be considered sufficient.
    pub fast_path_score_threshold: f64,
    /// Minimum results from enhanced search before escalating to graph-enhanced.
    pub enhanced_min_results: usize,
    /// Minimum RRF score threshold for enhanced results.
    pub enhanced_score_threshold: f64,
    /// Maximum entities to expand via graph neighborhood in tier 3.
    pub graph_expansion_limit: usize,
}
```

```rust
pub enum SearchTier {
    /// Single-query hybrid search (BM25 + vector).
    Fast,
    /// LLM query rewrite + multi-query hybrid search.
    Enhanced,
    /// Graph neighborhood expansion on top of enhanced results.
    GraphEnhanced,
}
```

```rust
pub struct TieredSearchResult<T> {
    /// Which tier produced the final results.
    pub tier: SearchTier,
    /// The merged, deduplicated results.
    pub results: Vec<T>,
    /// Query variants used (if enhanced tier was reached).
    pub query_variants: Option<Vec<String>>,
    /// Total latency across all tiers in milliseconds.
    pub total_latency_ms: u64,
}
```

## `src/recall.rs`

```rust
pub struct RecallWeights {
    /// Weight for vector similarity (cosine distance). Default: 0.30
    pub vector_similarity: f64,
    /// Weight for FSRS power-law decay. Default: 0.20
    pub decay: f64,
    /// Weight for nous-relevance (own memories boosted). Default: 0.15
    pub relevance: f64,
    /// Weight for epistemic tier (verified > inferred > assumed). Default: 0.10
    pub epistemic_tier: f64,
    /// Weight for graph relationship proximity. Default: 0.10
    pub relationship_proximity: f64,
    /// Weight for access frequency. Default: 0.05
    pub access_frequency: f64,
    /// Weight for graph `PageRank` importance (hub entities boosted).
    /// Default: 0.10
    pub graph_importance: f64,
}
```

```rust
pub struct FactorScores {
    /// Cosine similarity score [0.0, 1.0] (1.0 = identical).
    pub vector_similarity: f64,
    /// FSRS decay score [0.0, 1.0] (1.0 = just accessed).
    pub decay: f64,
    /// Relevance score [0.0, 1.0] (1.0 = same nous).
    pub relevance: f64,
    /// Epistemic tier score [0.0, 1.0] (1.0 = verified).
    pub epistemic_tier: f64,
    /// Relationship proximity score [0.0, 1.0] (1.0 = direct neighbor).
    pub relationship_proximity: f64,
    /// Access frequency score [0.0, 1.0] (1.0 = most accessed).
    pub access_frequency: f64,
    /// `PageRank` graph importance score [0.0, 1.0] (1.0 = highest hub).
    pub graph_importance: f64,
}
```

```rust
pub struct ScoredResult {
    /// Content of the recalled memory.
    pub content: String,
    /// Source type (fact, message, note, document).
    pub source_type: String,
    /// Source ID.
    pub source_id: String,
    /// Which nous this belongs to.
    pub nous_id: String,
    /// Raw factor scores.
    pub factors: FactorScores,
    /// Final weighted score [0.0, 1.0].
    pub score: f64,
    /// Data-sovereignty classification carried from the store so the recall
    /// pipeline can filter by the active provider's deployment target
    /// (#3404, #3413).
    pub sensitivity: crate::knowledge::FactSensitivity,
}
```

```rust
pub struct RecallEngine {
    weights: RecallWeights,
    /// Maximum access count for frequency normalization.
    max_access_count: f64,
}
```

```rust
impl RecallEngine {
    pub fn new () -> Self;
    pub fn with_weights (weights: RecallWeights) -> Self;
    pub fn score_vector_similarity (&self, cosine_distance: f64) -> f64;
    pub fn score_decay (
        &self,
        age_hours: f64,
        fact_type: FactType,
        tier: EpistemicTier,
        access_count: u32,
    ) -> f64;
    pub fn score_relevance (&self, memory_nous_id: &str, query_nous_id: &str) -> f64;
    pub fn score_epistemic_tier (&self, tier: &str) -> f64;
    pub fn rank_with_prefilter <S: BuildHasher> (
        &self,
        candidates: Vec<ScoredResult>,
        selected_ids: &HashSet<String, S>,
    ) -> Vec<ScoredResult>;
    pub fn rank (&self, mut candidates: Vec<ScoredResult>) -> Vec<ScoredResult>;
    pub fn score_graph_importance (&self, importance: f64) -> f64;
}
```

```rust
pub fn pre_filter_by_side_query <S: BuildHasher> (
    candidates: Vec<ScoredResult>,
    selected_ids: &HashSet<String, S>,
) -> Vec<ScoredResult>
```

## `src/rule_proposals.rs`

> Default minimum observations before a pattern can generate a proposal.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_rule_min_observations`.
```rust
pub const DEFAULT_MIN_OBSERVATIONS: u32 = 5;
```

> Default minimum confidence score (0.0--1.0) for a proposal to be emitted.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_rule_min_confidence`.
```rust
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.60;
```

```rust
pub enum RuleProposalError {
    /// Failed to serialize proposals to TOML.
    #[snafu(display("failed to serialize rule proposals to TOML: {source}"))]
    Serialize {
        source: toml::ser::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write proposals to disk.
    #[snafu(display("failed to write rule proposals to {path}: {source}"))]
    Write {
        path: String,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to create parent directory for proposals file.
    #[snafu(display("failed to create data directory {path}: {source}"))]
    CreateDir {
        path: String,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Result alias for rule proposal operations.
```rust
pub type Result<T, E = RuleProposalError> = std::result::Result<T, E>;
```

```rust
pub struct RuleProposal {
    /// Short `snake_case` rule name, suitable for use as a basanos rule key.
    pub rule_name: String,

    /// Human-readable description of the pattern that was observed.
    pub pattern_observed: String,

    /// Why this pattern warrants a lint rule.
    pub rationale: String,

    /// Confidence that this pattern is a real problem (0.0--1.0).
    ///
    /// Computed from failure rate and observation count.
    /// Only proposals with `confidence >= 0.60` are emitted.
    pub confidence: f64,

    /// Number of times the pattern was observed.
    pub observation_count: u32,

    /// When this proposal was generated.
    pub generated_at: String,
}
```

```rust
pub struct ProposalFile {
    /// Metadata about this proposal run.
    pub generated_at: String,
    /// Observations analyzed to generate these proposals.
    pub observations_analyzed: usize,
    /// All proposals meeting the confidence threshold.
    pub proposals: Vec<RuleProposal>,
}
```

```rust
pub fn propose_rules (
    observations: &[ToolObservation],
    min_observations: u32,
    min_confidence: f64,
) -> Vec<RuleProposal>
```

> Write proposals to `<data_dir>/rule_proposals.toml`.
> 
> Creates the directory if it does not exist. Overwrites any previous output.
> This is an append-on-success design: if serialization fails, the old file
> is preserved.
> 
> WHY: Proposals are for operator review, not runtime consumption. A flat
> TOML file is the least-friction format for a human to open and annotate.
> 
> # Errors
> 
> Returns an error if the directory cannot be created, if serialization fails,
> or if writing to the file fails.
```rust
pub fn write_proposals (
    proposals: &[RuleProposal],
    observations_analyzed: usize,
    data_dir: &Path,
) -> Result<()>
```

## `src/side_query.rs`

```rust
pub struct SideQueryConfig {
    /// Maximum number of results to return per query.
    pub max_results: usize,
    /// Cache entry time-to-live in seconds.
    pub cache_ttl_secs: u64,
    /// Maximum number of cached entries.
    pub cache_capacity: usize,
    /// Whether side-query pre-filtering is enabled.
    pub enabled: bool,
}
```

```rust
pub struct SideQueryResult {
    /// Source IDs selected by the side-query, in relevance order.
    pub selected_ids: Vec<String>,
    /// Whether this result was served from cache.
    pub from_cache: bool,
}
```

## `src/skill.rs`

```rust
pub struct SkillHealthMetrics {
    /// Total active (non-forgotten) skills.
    pub total_active: usize,
    /// Total retired (forgotten with reason "stale") skills.
    pub total_retired: usize,
    /// Total skills flagged as needing review.
    pub total_needs_review: usize,
    /// Average usage count across active skills.
    pub avg_usage_count: f64,
    /// Median days since last use across active skills.
    pub median_days_since_use: f64,
    /// Top skills by usage count (name, `usage_count`).
    pub top_skills: Vec<(String, u32)>,
    /// Bottom skills by usage count (name, `usage_count`).
    pub bottom_skills: Vec<(String, u32)>,
    /// Dedup rate: candidates discarded / total candidates processed.
    pub dedup_discard_count: u64,
    /// Total candidates processed through the dedup pipeline.
    pub dedup_total_count: u64,
}
```

```rust
pub struct SkillContent {
    /// Short identifier (slug), e.g. `"rust-error-handling"`.
    pub name: String,
    /// Human-readable description of what this skill does.
    pub description: String,
    /// Ordered steps to execute the skill.
    pub steps: Vec<String>,
    /// Tools referenced by the skill.
    pub tools_used: Vec<String>,
    /// Domain classification tags (e.g. `["rust", "error-handling"]`).
    pub domain_tags: Vec<String>,
    /// How this skill was created: `"manual"`, `"seeded"`, or `"extracted"`.
    pub origin: String,
}
```

```rust
pub struct SkillParseError {
    /// Path to the SKILL.md file that failed to parse.
    pub path: String,
    /// Human-readable description of the parse failure.
    pub reason: String,
}
```

> Parse a SKILL.md file into structured skill content.
> 
> Supports optional YAML frontmatter (delimited by `---`) with `tools` and
> `domains` fields. Falls back to extracting from markdown sections.
> 
> # Errors
> 
> Returns an error if the document is empty, missing a top-level heading,
> or has no description.
```rust
pub fn parse_skill_md (source: &str, slug: &str) -> Result<SkillContent, SkillParseError>
```

> Scan a directory for subdirectories containing SKILL.md files.
> 
> Returns `(slug, content_string)` pairs for each found skill.
> 
> # Errors
> 
> Returns an error if the directory cannot be read or if a skill file
> cannot be read.
```rust
pub fn scan_skill_dir (dir: &std::path::Path) -> Result<Vec<(String, String)>, std::io::Error>
```

```rust
pub struct ExportedSkill {
    /// Path to the written SKILL.md file.
    pub path: std::path::PathBuf,
    /// The slug used for the directory name.
    pub slug: String,
    /// The skill name (from content).
    pub name: String,
}
```

> Export a collection of skills to Claude Code's `.claude/skills/<slug>/SKILL.md` format.
> 
> Creates the directory structure and writes each skill as a SKILL.md file
> with YAML frontmatter. Existing files are overwritten.
> 
> This is a pure library function: no knowledge store dependency. Pass in
> already-resolved `SkillContent` values. The CLI and energeia bridge both
> use this same function.
> 
> # Errors
> 
> Returns `std::io::Error` if directory creation or file writing fails.
```rust
pub fn export_skills_to_cc (
    skills: &[SkillContent],
    output_dir: &std::path::Path,
    domain_filter: Option<&[&str]>,
) -> Result<Vec<ExportedSkill>, std::io::Error>
```

## `src/skills/candidate.rs`

```rust
pub struct SkillCandidate {
    /// Unique identifier (ULID as string).
    pub id: String,
    /// Which nous this candidate belongs to.
    pub nous_id: String,
    /// Normalised signature of the representative tool call sequence.
    pub signature: SequenceSignature,
    /// Number of sessions where this pattern appeared.
    pub recurrence_count: u32,
    /// Session IDs where the pattern appeared.
    pub session_refs: Vec<String>,
    /// Timestamp of first observation.
    pub first_seen: jiff::Timestamp,
    /// Timestamp of most recent observation.
    pub last_seen: jiff::Timestamp,
    /// Heuristic score from the first observation.
    pub heuristic_score: f64,
    /// Detected pattern type from the first observation.
    pub pattern_type: Option<crate::skills::PatternType>,
}
```

```rust
pub enum TrackResult {
    /// Sequence failed the heuristic gates: not tracked.
    Rejected,
    /// New candidate created (first occurrence).
    New,
    /// Existing candidate updated.  Contains the new recurrence count.
    Tracked(u32),
    /// Candidate promoted (`recurrence_count` reached [`PROMOTION_THRESHOLD`]).
    /// Contains the candidate ID.
    Promoted(String),
}
```

> In-memory store for skill candidates with Rule-of-Three promotion.
> 
> Thread-safe via an internal [`std::sync::Mutex`].
> Serialize each [`SkillCandidate`] to JSON and persist as a fact with
> `fact_type = "skill_candidate"` for durable storage.
```rust
pub struct CandidateTracker {
    candidates: std::sync::Mutex<Vec<SkillCandidate>>,
}
```

```rust
impl CandidateTracker {
    pub fn new () -> Self;
    pub fn track_sequence (
        &self,
        tool_calls: &[ToolCallRecord],
        session_id: &str,
        nous_id: &str,
    ) -> TrackResult;
    pub fn candidates_for (&self, nous_id: &str) -> Vec<SkillCandidate>;
}
```

## `src/skills/extract.rs`

```rust
pub enum SkillExtractionError {
    /// The LLM provider returned an error.
    #[snafu(display("LLM extraction failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The LLM response could not be parsed as valid skill JSON.
    #[snafu(display("failed to parse skill extraction response: {message}"))]
    ParseResponse {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Minimal LLM completion interface for skill extraction.
> 
> Keeps mneme independent of hermeneus. The nous layer bridges this trait
> to the full provider API, just like [`crate::extract::ExtractionProvider`].
> 
> Uses a boxed future return type to remain dyn-compatible (object-safe).
```rust
pub trait SkillExtractionProvider : Send + Sync {
    fn complete <'a> (
        &'a self,
        system: &'a str,
        user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, SkillExtractionError>> + Send + 'a>,
    >;
}
```

```rust
pub struct ExtractedSkill {
    /// Human-readable skill name.
    pub name: String,
    /// Description of what this skill does and when to use it.
    pub description: String,
    /// Ordered steps to execute the skill.
    pub steps: Vec<String>,
    /// Tools referenced by the skill.
    pub tools_used: Vec<String>,
    /// Domain classification tags.
    pub domain_tags: Vec<String>,
    /// When this skill should be applied (situational guidance).
    pub when_to_use: String,
}
```

```rust
impl ExtractedSkill {
    pub fn to_skill_content (&self) -> SkillContent;
}
```

> Extracts structured skill definitions from promoted candidates via LLM.
```rust
pub struct SkillExtractor<P> {
    provider: P,
}
```

```rust
impl <P: SkillExtractionProvider> SkillExtractor<P> {
    pub fn new (provider: P) -> Self;
    pub async fn extract_skill (
        &self,
        candidate: &SkillCandidate,
        tool_call_sequences: &[Vec<ToolCallRecord>],
    ) -> Result<ExtractedSkill, SkillExtractionError>;
}
```

```rust
pub enum DedupOutcome {
    /// No duplicate found: promote normally.
    Unique,
    /// Existing skill is better: discard candidate.
    DiscardCandidate {
        /// The ID of the existing skill that wins.
        existing_id: String,
    },
    /// Candidate is better: supersede the existing skill.
    SupersedeExisting {
        /// The ID of the existing skill to supersede.
        existing_id: String,
    },
}
```

> Parameters for a dedup comparison between a candidate and an existing skill.
```rust
pub struct DedupInput<'a> {
    /// The candidate skill content.
    pub candidate: &'a SkillContent,
    /// Candidate confidence score.
    pub candidate_confidence: f64,
    /// Candidate usage count.
    pub candidate_usage: u32,
    /// The existing skill content.
    pub existing: &'a SkillContent,
    /// Existing skill confidence score.
    pub existing_confidence: f64,
    /// Existing skill usage count.
    pub existing_usage: u32,
    /// Existing skill fact ID.
    pub existing_id: &'a str,
    /// Optional embedding for the candidate.
    pub candidate_embedding: Option<&'a [f32]>,
    /// Optional embedding for the existing skill.
    pub existing_embedding: Option<&'a [f32]>,
}
```

```rust
pub fn check_dedup (input: &DedupInput<'_>) -> DedupOutcome
```

```rust
pub struct PendingSkill {
    /// The extracted skill content.
    pub skill: SkillContent,
    /// The candidate that was promoted to trigger extraction.
    pub candidate_id: String,
    /// Review status: `"pending_review"`, `"approved"`, `"rejected"`.
    pub status: String,
    /// When the skill was extracted.
    pub extracted_at: jiff::Timestamp,
}
```

```rust
impl PendingSkill {
    pub fn new (extracted: &ExtractedSkill, candidate_id: &str) -> Self;
    pub fn to_json (&self) -> Result<String, serde_json::Error>;
    pub fn from_json (json: &str) -> Result<Self, serde_json::Error>;
}
```

## `src/skills/heuristics.rs`

```rust
pub struct HeuristicScore {
    /// Overall quality score (0.0--1.0). Meaningful only when `passed_gates` is true.
    pub total: f64,
    /// Whether all must-pass gates were cleared.
    pub passed_gates: bool,
    /// Detected pattern type (if any).
    pub pattern_type: Option<PatternType>,
    /// Human-readable scoring breakdown for debugging.
    pub details: Vec<String>,
}
```

```rust
pub enum PatternType {
    /// Read → analyze → fix cycle (debugging → verification).
    Diagnostic,
    /// Read → understand → transform → verify (code restructuring).
    Refactor,
    /// Search → read → synthesize (information gathering).
    Research,
    /// Create → test → iterate (constructive work).
    Build,
    /// Read → analyze → report (assessment without transformation).
    Review,
}
```

```rust
pub fn score_sequence (tool_calls: &[ToolCallRecord]) -> HeuristicScore
```

## `src/skills/mod.rs`

```rust
pub struct ToolCallRecord {
    /// Tool name (e.g. `"Read"`, `"Edit"`, `"Bash"`).
    pub tool_name: String,
    /// Whether the tool call resulted in an error.
    pub is_error: bool,
    /// How long the tool call took in milliseconds.
    pub duration_ms: u64,
}
```

```rust
impl ToolCallRecord {
    pub fn new (tool_name: impl Into<String>, duration_ms: u64) -> Self;
    pub fn errored (tool_name: impl Into<String>, duration_ms: u64) -> Self;
}
```

## `src/skills/signature.rs`

```rust
pub struct SequenceSignature {
    /// Ordered, deduplicated (consecutive) tool names.
    pub normalized: Vec<String>,
    /// Fast pre-filter hash of `normalized`.
    pub hash: u64,
}
```

```rust
pub fn sequence_signature (tool_calls: &[ToolCallRecord]) -> SequenceSignature
```

```rust
pub fn signature_similarity (a: &SequenceSignature, b: &SequenceSignature) -> f64
```

## `src/staleness.rs`

```rust
pub struct SourceLinkedFact {
    /// Fact identifier.
    pub fact_id: String,
    /// The stored fact content.
    pub content: String,
    /// URI of the original source (URL, file path, API endpoint).
    pub source_uri: String,
    /// When the fact was last validated against its source (ISO 8601).
    pub last_validated: Option<String>,
}
```

```rust
pub struct StalenessResult {
    /// The fact that was checked.
    pub fact_id: String,
    /// Whether the fact is still consistent with its source.
    pub status: StalenessStatus,
    /// Token overlap between fact content and source content (0.0..=1.0).
    pub similarity: f64,
    /// Explanation of the result.
    pub explanation: String,
}
```

```rust
pub enum StalenessStatus {
    /// Fact content is still grounded in the source.
    Fresh,
    /// Fact content has partial overlap — source may have changed.
    Drifted,
    /// Fact content has no overlap with the current source — likely stale.
    Stale,
    /// Source could not be re-fetched (unavailable, 404, etc.).
    Unreachable,
}
```

```rust
pub struct StalenessConfig {
    /// Minimum similarity score to consider a fact "fresh" (0.0..=1.0). Default: 0.5.
    pub fresh_threshold: f64,
    /// Minimum similarity score to consider a fact "drifted" (below this = stale). Default: 0.15.
    pub stale_threshold: f64,
}
```

```rust
pub struct StalenessChecker {
    config: StalenessConfig,
}
```

```rust
impl StalenessChecker {
    pub fn new (config: StalenessConfig) -> Self;
    pub fn check (&self, fact: &SourceLinkedFact, source_content: Option<&str>) -> StalenessResult;
    pub fn check_batch (&self, checks: &[(SourceLinkedFact, Option<String>)]) -> BatchResult;
}
```

```rust
pub struct BatchResult {
    /// Individual check results.
    pub results: Vec<StalenessResult>,
    /// Number of facts still fresh.
    pub fresh: usize,
    /// Number of facts that have drifted.
    pub drifted: usize,
    /// Number of facts that are stale.
    pub stale: usize,
    /// Number of facts whose sources were unreachable.
    pub unreachable: usize,
}
```

```rust
impl BatchResult {
    pub fn total (&self) -> usize;
    pub fn freshness_rate (&self) -> f64;
}
```

## `src/surprise.rs`

> Default surprise threshold (in nats) above which a turn is classified as an
> episode boundary. Empirically, bigram KL divergence on conversational text
> clusters around 0.5-1.5 for same-topic turns and 2.0+ for topic shifts.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_surprise_threshold`.
```rust
pub const DEFAULT_THRESHOLD: f64 = 2.0;
```

> Default exponential moving average decay factor. Controls how quickly the running
> distribution forgets old observations. 0.3 = new observation gets 30% weight.
> 
> Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::knowledge_surprise_ema_alpha`.
```rust
pub const DEFAULT_EMA_ALPHA: f64 = 0.3;
```

```rust
pub struct EpisodeBoundary {
    /// Zero-based turn index within the sequence.
    pub position: usize,
    /// KL divergence (surprise) measured at this turn, in nats.
    pub surprise_score: f64,
    /// Whether the surprise exceeded the threshold.
    pub is_boundary: bool,
}
```

```rust
pub struct SurpriseCalculator {
    /// Running (prior) bigram distribution, values are normalized frequencies.
    prior: HashMap<[u8; NGRAM_SIZE], f64>,
    /// Total observation mass in the prior (for re-normalization after EMA).
    total_mass: f64,
    /// EMA decay factor (configurable via taxis).
    ema_alpha: f64,
}
```

```rust
impl SurpriseCalculator {
    pub fn new () -> Self;
    pub fn with_alpha (ema_alpha: f64) -> Self;
    pub fn compute_surprise (&mut self, text: &str) -> f64;
}
```

```rust
pub fn detect_boundaries (turns: &[&str], threshold: f64) -> Vec<EpisodeBoundary>
```

```rust
pub fn detect_boundaries_default (turns: &[&str]) -> Vec<EpisodeBoundary>
```

## `src/trace_ingest.rs`

```rust
pub enum TraceEvent {
    /// A conversation turn completed.
    TurnCompleted {
        /// Owning session identifier.
        session_id: String,
        /// Agent identifier.
        nous_id: String,
        /// Model name used for this turn.
        model: String,
        /// Total tokens consumed (prompt + completion).
        tokens: i64,
        /// Wall-clock duration of the turn in milliseconds.
        duration_ms: i64,
        /// ISO-8601 timestamp when the turn completed.
        timestamp: String,
    },
    /// A tool call completed.
    ToolExecuted {
        /// Owning session identifier.
        session_id: String,
        /// Name of the tool that was called.
        tool_name: String,
        /// Whether the tool call returned successfully.
        success: bool,
        /// Wall-clock duration of the tool call in milliseconds.
        duration_ms: i64,
        /// ISO-8601 timestamp when the call completed.
        timestamp: String,
    },
    /// An error was recorded.
    ErrorOccurred {
        /// Owning session identifier.
        session_id: String,
        /// Classifier / category for the error (e.g. `"rate_limit"`).
        error_class: String,
        /// Human-readable error message.
        message: String,
        /// ISO-8601 timestamp when the error occurred.
        timestamp: String,
    },
}
```

## `src/vocab.rs`

```rust
pub enum RelationType {
    /// Matched a known vocabulary type (canonical uppercase form).
    Known(&'static str),
    /// Novel LLM-generated type not in the vocabulary, normalized to `UPPER_SNAKE_CASE`.
    Novel(String),
    /// Matched a rejected type: must not be persisted.
    Rejected,
    /// Empty, whitespace-only, or invalid format after normalization.
    Malformed,
}
```
