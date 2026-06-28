//! Krites-backed knowledge store implementation.
//!
//! This module requires the `mneme-engine` feature flag.
//!
//! **Storage:** production knowledge stores use Krites over Fjall. In-memory
//! Krites storage is available for tests and short-lived tools. No external
//! database process or C++ runtime is required.
//!
//! # Schema
//!
//! ## Relations (Datalog)
//!
//! ```text
//! facts { id: String, valid_from: String => content: String, nous_id: String,
//!         confidence: Float, tier: String, valid_to: String, superseded_by: String?,
//!         source_session_id: String?, recorded_at: String,
//!         access_count: Int, last_accessed_at: String, stability_hours: Float,
//!         fact_type: String, is_forgotten: Bool, forgotten_at: String?,
//!         forget_reason: String?, scope: String?, project_id: String?,
//!         visibility: String, sensitivity: String }
//!
//! entities { id: String => name: String, entity_type: String, aliases: String,
//!            created_at: String, updated_at: String,
//!            name_embedding: <F32; DIM>? }
//!
//! relationships { src: String, dst: String => relation: String, weight: Float,
//!                 created_at: String }
//!
//! embeddings { id: String => content: String, source_type: String, source_id: String,
//!              nous_id: String, embedding: <F32; DIM>, created_at: String }
//!
//! embedding_meta { model: String => dim: Int }
//!
//! type_hierarchy { child_type: String, parent_type: String => created_at: String }
//!
//! derived_facts { entity_id: String, rule_id: String, derived_content: String =>
//!                 confidence: Float, materialized_at: String }
//!
//! defaults { entity_id: String, tag: String => default_content: String,
//!            confidence: Float, created_at: String }
//! ```
//!
//! ## HNSW Index
//!
//! ```text
//! ::hnsw create embeddings:semantic_idx {
//!     dim: DIM, m: 16, ef_construction: 200,
//!     dtype: F32, distance: Cosine, fields: [embedding]
//! }
//! ```

// WHY: This module is activated only with the mneme-engine feature; Datalog queries
// are validated by the mneme-bench crate.

#[cfg(feature = "mneme-engine")]
mod causal;
#[cfg(feature = "mneme-engine")]
pub(crate) mod derived_rules;
#[cfg(feature = "mneme-engine")]
mod entity;
#[cfg(feature = "mneme-engine")]
mod entity_dedup;
#[cfg(feature = "mneme-engine")]
mod facts;
#[cfg(feature = "mneme-engine")]
pub(crate) mod marshal;
#[cfg(feature = "mneme-engine")]
mod migration;
#[cfg(feature = "mneme-engine")]
mod search;
#[cfg(feature = "mneme-engine")]
mod skills;

#[cfg(feature = "mneme-engine")]
pub use derived_rules::DerivedFreshness;

#[cfg(test)]
mod tests;

use tracing::instrument;

/// Datalog DDL for the embedding metadata relation.
pub const EMBEDDING_META_DDL: &str = r":create embedding_meta {
    model: String =>
    dim: Int
}";

/// Datalog DDL for the facts relation.
pub const FACTS_DDL: &str = r":create facts {
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
    forget_reason: String?,
    scope: String?,
    project_id: String?,
    visibility: String default 'private',
    sensitivity: String default 'public'
}";

/// Datalog DDL for append-only fact access events.
pub const FACT_ACCESS_LOG_DDL: &str = r":create fact_access_log {
    event_id: String =>
    fact_id: String,
    accessed_at: String
}";

/// Datalog DDL for the relationships relation.
pub const RELATIONSHIPS_DDL: &str = r":create relationships {
    src: String, dst: String =>
    relation: String,
    weight: Float,
    created_at: String
}";

/// Datalog DDL for the `fact_entities` relation.
pub const FACT_ENTITIES_DDL: &str = r":create fact_entities {
    fact_id: String, entity_id: String =>
    created_at: String
}";

/// Datalog DDL for the `merge_audit` relation.
pub const MERGE_AUDIT_DDL: &str = r":create merge_audit {
    canonical_id: String, merged_id: String =>
    merged_name: String,
    merge_score: Float,
    facts_transferred: Int,
    relationships_redirected: Int,
    merged_at: String
}";

/// Datalog DDL for the `pending_merges` relation.
pub const PENDING_MERGES_DDL: &str = r":create pending_merges {
    entity_a: String, entity_b: String =>
    name_a: String,
    name_b: String,
    name_similarity: Float,
    embed_similarity: Float,
    type_match: Bool,
    alias_overlap: Bool,
    merge_score: Float,
    created_at: String
}";

/// Datalog DDL for the `causal_edges` relation.
pub const CAUSAL_EDGES_DDL: &str = r":create causal_edges {
    cause: String, effect: String =>
    id: String,
    ordering: String,
    relationship_type: String,
    confidence: Float,
    evidence_session_id: String?,
    created_at: String
}";

/// Datalog DDL for the `type_hierarchy` relation (added in schema v8).
pub const TYPE_HIERARCHY_DDL: &str = r":create type_hierarchy {
    child_type: String, parent_type: String =>
    created_at: String
}";

/// Datalog DDL for the `derived_facts` relation (added in schema v8).
pub const DERIVED_FACTS_DDL: &str = r":create derived_facts {
    entity_id: String, rule_id: String, derived_content: String =>
    confidence: Float,
    materialized_at: String
}";

/// Datalog DDL for the defaults relation (added in schema v8).
pub const DEFAULTS_DDL: &str = r":create defaults {
    entity_id: String, tag: String =>
    default_content: String,
    confidence: Float,
    created_at: String
}";

/// Datalog DDL for the `published_facts` relation (added in schema v10).
pub const PUBLISHED_FACTS_DDL: &str = r":create published_facts {
    id: String =>
    original_fact_id: String,
    published_by: String,
    published_at: String,
    verification_count: Int default 0,
    contested_by: String,
    contest_reason: String?
}";

/// Datalog DDL for the provenance relation (added in schema v10).
pub const PROVENANCE_DDL: &str = r":create provenance {
    published_fact_id: String, contributor: String =>
    contribution_type: String,
    confidence: Float,
    contributed_at: String
}";

/// Datalog DDL for the `entity_flags` relation (added in schema v16).
pub const ENTITY_FLAGS_DDL: &str = r":create entity_flags {
    entity_id: String =>
    reason: String,
    severity: String,
    flagged_by: String,
    flagged_at: String
}";

/// Datalog DDL for the `derived_source_revision` relation (added in schema v19, #4662).
pub const DERIVED_SOURCE_REVISION_DDL: &str = r":create derived_source_revision {
    key: String =>
    revision: Int
}";

/// Datalog DDL for the `derived_rule_watermarks` relation (added in schema v19, #4662).
pub const DERIVED_RULE_WATERMARKS_DDL: &str = r":create derived_rule_watermarks {
    rule_id: String =>
    source_revision: Int,
    materialized_at: String,
    dirty: Bool
}";

/// Datalog DDL for the entities relation. Dimension is parameterized so the
/// `name_embedding` column can hold a fixed-size F32 vector matching the
/// configured embedding provider. Nullable: entities created before the
/// dedup-reachability fix (#4165) and entities inserted while no provider is
/// configured leave the column NULL; the dedup pipeline treats NULL as
/// `embed_sim = 0.0` for those pairs (degraded-mode behaviour).
#[instrument]
pub fn entities_ddl(dim: usize) -> String {
    format!(
        r":create entities {{
            id: String =>
            name: String,
            entity_type: String,
            aliases: String,
            created_at: String,
            updated_at: String,
            name_embedding: <F32; {dim}>?
        }}"
    )
}

/// Datalog DDL for the embeddings relation. Dimension is parameterized.
#[instrument]
pub fn embeddings_ddl(dim: usize) -> String {
    format!(
        r":create embeddings {{
            id: String =>
            content: String,
            source_type: String,
            source_id: String,
            nous_id: String,
            embedding: <F32; {dim}>,
            created_at: String
        }}"
    )
}

/// Datalog DDL for the HNSW index on embeddings.
#[instrument]
pub fn hnsw_ddl(dim: usize) -> String {
    format!(
        r"::hnsw create embeddings:semantic_idx {{
            dim: {dim},
            m: 16,
            ef_construction: 200,
            dtype: F32,
            distance: Cosine,
            fields: [embedding]
        }}"
    )
}

/// Datalog DDL for FTS index on facts.content.
#[instrument]
pub fn fts_ddl() -> &'static str {
    r"::fts create facts:content_fts {
        extractor: content,
        tokenizer: Simple,
        filters: [Lowercase, Stemmer('English'), Stopwords('en')]
    }"
}

#[cfg(feature = "mneme-engine")]
/// Persisted embedding metadata for the knowledge store vector schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingMeta {
    /// Embedding model identifier persisted with the vector schema.
    pub model: String,
    /// Embedding vector dimension persisted with the vector schema.
    pub dim: usize,
}

/// Re-export query builder types and pre-built query scripts.
///
/// Builder-generated queries (field-safe): `queries::upsert_fact()`, etc.
/// Raw Datalog constants (multi-rule): `queries::ENTITY_NEIGHBORHOOD`, etc.
#[cfg(feature = "mneme-engine")]
use crate::query::queries;

/// Typed wrapper for raw Datalog query results.
///
/// Returned by [`KnowledgeStore::run_query`] and related escape-hatch methods.
/// Hides the `crate::engine::NamedRows` type from callers, keeping engine
/// internals encapsulated within the knowledge layer.
///
/// Use the typed accessor methods ([`get_string`](Self::get_string),
/// [`get_f64`](Self::get_f64), [`get_i64`](Self::get_i64),
/// [`get_bool`](Self::get_bool)) to extract values by column name.
/// Type mismatches return `None` instead of silently producing defaults.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone)]
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

#[cfg(feature = "mneme-engine")]
impl QueryResult {
    /// Number of result rows.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Whether the result set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Read-only access to raw result rows.
    ///
    /// Prefer the typed accessors ([`get_string`](Self::get_string),
    /// [`get_f64`](Self::get_f64), [`get_i64`](Self::get_i64),
    /// [`get_bool`](Self::get_bool)) for ordinary usage. This accessor is
    /// provided for callers that need to match on raw [`DataValue`](crate::engine::DataValue)
    /// variants — for example, distinguishing `Null` from a missing column, or
    /// inspecting values whose type depends on runtime state.
    #[must_use]
    pub fn rows(&self) -> &[Vec<crate::engine::DataValue>] {
        &self.rows
    }

    /// Look up column index by name.
    fn col_index(&self, col: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == col)
    }

    /// Extract a string value by row index and column name.
    ///
    /// Returns `None` if the row index is out of bounds, the column name
    /// is not present, or the value is not a string.
    #[must_use]
    pub fn get_string(&self, row: usize, col: &str) -> Option<String> {
        let ci = self.col_index(col)?;
        self.rows.get(row)?.get(ci)?.get_str().map(str::to_owned)
    }

    /// Extract an `f64` value by row index and column name.
    ///
    /// Returns `None` if the row index is out of bounds, the column name
    /// is not present, or the value is not numeric.
    #[must_use]
    pub fn get_f64(&self, row: usize, col: &str) -> Option<f64> {
        let ci = self.col_index(col)?;
        self.rows.get(row)?.get(ci)?.get_float()
    }

    /// Extract an `i64` value by row index and column name.
    ///
    /// Returns `None` if the row index is out of bounds, the column name
    /// is not present, or the value is not an integer.
    #[must_use]
    pub fn get_i64(&self, row: usize, col: &str) -> Option<i64> {
        let ci = self.col_index(col)?;
        self.rows.get(row)?.get(ci)?.get_int()
    }

    /// Extract a boolean value by row index and column name.
    ///
    /// Returns `None` if the row index is out of bounds, the column name
    /// is not present, or the value is not a boolean.
    #[must_use]
    pub fn get_bool(&self, row: usize, col: &str) -> Option<bool> {
        let ci = self.col_index(col)?;
        self.rows.get(row)?.get(ci)?.get_bool()
    }

    /// Format all rows as display strings, one `Vec<String>` per row.
    ///
    /// Each cell is converted to its display representation:
    /// - Null -> `"null"`
    /// - Strings -> the string value (unquoted)
    /// - Numbers -> decimal representation (integers without `.0`)
    /// - Booleans -> `"true"` / `"false"`
    /// - Other -> debug representation
    #[must_use]
    pub fn rows_as_strings(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| row.iter().map(Self::format_cell).collect())
            .collect()
    }

    /// Convert all row data to `serde_json::Value` for serialization.
    ///
    /// Each cell is mapped to the closest JSON type:
    /// - Null -> `Value::Null`
    /// - Bool -> `Value::Bool`
    /// - Int -> `Value::Number`
    /// - Float -> `Value::Number` (falls back to string for non-finite)
    /// - Str -> `Value::String`
    /// - Other -> `Value::String` via debug formatting
    #[must_use]
    pub fn rows_to_json(&self) -> Vec<Vec<serde_json::Value>> {
        self.rows
            .iter()
            .map(|row| row.iter().map(Self::cell_to_json).collect())
            .collect()
    }

    /// Format a single cell for display.
    fn format_cell(v: &crate::engine::DataValue) -> String {
        use crate::engine::DataValue;
        match v {
            DataValue::Null => "null".to_owned(),
            DataValue::Str(s) => s.to_string(),
            DataValue::Bool(b) => b.to_string(),
            dv @ DataValue::Num(_) => {
                if let Some(i) = dv.get_int() {
                    i.to_string()
                } else if let Some(f) = dv.get_float() {
                    f.to_string()
                } else {
                    format!("{dv:?}")
                }
            }
            other => format!("{other:?}"),
        }
    }

    /// Convert a single cell to a JSON value.
    fn cell_to_json(v: &crate::engine::DataValue) -> serde_json::Value {
        use crate::engine::DataValue;
        match v {
            DataValue::Null => serde_json::Value::Null,
            DataValue::Bool(b) => serde_json::Value::Bool(*b),
            DataValue::Str(s) => serde_json::Value::String(s.to_string()),
            dv => {
                if let Some(i) = dv.get_int() {
                    serde_json::Value::Number(serde_json::Number::from(i))
                } else if let Some(f) = dv.get_float() {
                    serde_json::Number::from_f64(f).map_or_else(
                        || serde_json::Value::String(f.to_string()),
                        serde_json::Value::Number,
                    )
                } else {
                    serde_json::Value::String(format!("{dv:?}"))
                }
            }
        }
    }
}

#[cfg(feature = "mneme-engine")]
impl From<crate::engine::NamedRows> for QueryResult {
    fn from(nr: crate::engine::NamedRows) -> Self {
        Self {
            headers: nr.headers,
            rows: nr.rows,
        }
    }
}

/// Outcome of a serendipity discovery pass.
#[derive(Debug, Clone, Default)]
pub struct SerendipityDiscoveryReport {
    /// Total facts examined during the pass.
    pub items_processed: u64,
    /// Number of discoveries surfaced in the report.
    pub items_modified: u64,
    /// Number of candidate discoveries evaluated.
    pub discovery_count: u64,
    /// Fact ID of the discovery selected for injection into the next turn, if any.
    pub selected_fact_id: Option<String>,
    /// Human-readable reason why the selected discovery was interesting.
    pub selected_connection_reason: Option<String>,
    /// Serendipity score of the selected discovery, if any.
    pub selected_surprise_score: Option<f64>,
    /// Optional human-readable detail string.
    pub detail: Option<String>,
}

/// Configuration for `KnowledgeStore` initialization.
#[cfg(feature = "mneme-engine")]
pub struct KnowledgeConfig {
    /// Embedding dimension for the HNSW index.
    pub dim: usize,
    /// Embedding model identifier expected by the persisted vector schema.
    pub embedding_model: String,
    /// Permit stores migrated with an unknown legacy embedding model.
    pub allow_assumed_embedding_meta: bool,
    /// Admission policy for fact insertion. Default: [`DefaultAdmissionPolicy`](crate::admission::DefaultAdmissionPolicy).
    pub admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}

#[cfg(feature = "mneme-engine")]
impl std::fmt::Debug for KnowledgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeConfig")
            .field("dim", &self.dim)
            .field("embedding_model", &self.embedding_model)
            .field(
                "allow_assumed_embedding_meta",
                &self.allow_assumed_embedding_meta,
            )
            .field("admission_policy", &"<dyn AdmissionPolicy>")
            .finish()
    }
}

#[cfg(feature = "mneme-engine")]
impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            dim: 384,
            embedding_model: crate::embedding::DEFAULT_CANDLE_MODEL.to_owned(),
            allow_assumed_embedding_meta: false,
            admission_policy: Box::new(crate::admission::DefaultAdmissionPolicy),
        }
    }
}

/// Parameters for a hybrid BM25 + HNSW + graph retrieval query.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone)]
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

/// A single result from a hybrid retrieval query.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone)]
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

#[cfg(feature = "mneme-engine")]
impl crate::query_rewrite::HasId for HybridResult {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

#[cfg(feature = "mneme-engine")]
impl crate::query_rewrite::HasRrfScore for HybridResult {
    fn set_rrf_score(&mut self, score: f64) {
        self.rrf_score = score;
    }
}

#[cfg(feature = "mneme-engine")]
const INSERT_LOCK_SHARDS: usize = 64;

#[cfg(feature = "mneme-engine")]
const INSERT_LOCK_SHARD_MASK: u64 = 63;

/// Typed wrapper around the Datalog engine providing domain-level knowledge operations.
///
/// Holds an `Arc<Db>` internally. Callers share via `Arc<KnowledgeStore>`.
/// All sync methods can be called directly; async wrappers use `spawn_blocking`.
#[cfg(feature = "mneme-engine")]
pub struct KnowledgeStore {
    db: std::sync::Arc<crate::engine::Db>,
    dim: usize,
    embedding_model: String,
    allow_assumed_embedding_meta: bool,
    access_event_sequence: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    read_query_count: std::sync::atomic::AtomicUsize,
    /// Sharded admission-check + insert locks.
    ///
    /// WHY (#5673): `should_admit` and the following write must remain atomic
    /// for facts in the same shard, but a global lock serialized unrelated
    /// nouses. Sharding by `nous_id` preserves the admission invariant while
    /// allowing different shards to ingest concurrently.
    insert_locks: Vec<parking_lot::Mutex<()>>,
    /// Admission policy gate: checked before every fact insertion.
    admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    pub(crate) const SCHEMA_VERSION: i64 = 22;
    const MIN_SCHEMA_VERSION: i64 = 1;
    pub(crate) const ASSUMED_EMBEDDING_MODEL: &'static str = "assumed";

    fn new_insert_locks() -> Vec<parking_lot::Mutex<()>> {
        (0..INSERT_LOCK_SHARDS)
            .map(|_| parking_lot::Mutex::new(()))
            .collect()
    }

    fn insert_lock_for_nous(&self, nous_id: &str) -> Option<&parking_lot::Mutex<()>> {
        let shard = Self::insert_lock_shard(nous_id);
        self.insert_locks
            .iter()
            .enumerate()
            .find_map(|(idx, lock)| (idx == shard).then_some(lock))
    }

    fn insert_lock_shard(nous_id: &str) -> usize {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in nous_id.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        usize::try_from(hash & INSERT_LOCK_SHARD_MASK).unwrap_or(0)
    }

    /// Return the insert-lock shard for tests that assert sharding behavior.
    #[cfg(test)]
    pub(crate) fn insert_lock_shard_for_test(nous_id: &str) -> usize {
        Self::insert_lock_shard(nous_id)
    }

    /// Allocate a unique access-log event ID for this store process.
    pub(super) fn next_access_event_id(&self) -> String {
        let sequence = self
            .access_event_sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{}-{sequence}", koina::ulid::Ulid::new())
    }

    /// Open an in-memory knowledge store with default configuration.
    ///
    /// # Complexity
    ///
    /// O(S) where S is schema initialization cost (creates relations and indices).
    #[instrument]
    pub fn open_mem() -> crate::error::Result<std::sync::Arc<Self>> {
        Self::open_mem_with_config(KnowledgeConfig::default())
    }

    /// Open an in-memory knowledge store with custom configuration.
    ///
    /// # Complexity
    ///
    /// O(S) where S is schema initialization cost (creates relations and indices).
    #[instrument]
    pub fn open_mem_with_config(
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>> {
        let db = crate::engine::Db::open_mem().map_err(|e| {
            crate::error::EngineInitSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        let store = Self {
            db: std::sync::Arc::new(db),
            dim: config.dim,
            embedding_model: config.embedding_model,
            allow_assumed_embedding_meta: config.allow_assumed_embedding_meta,
            access_event_sequence: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            read_query_count: std::sync::atomic::AtomicUsize::new(0),
            insert_locks: Self::new_insert_locks(),
            admission_policy: config.admission_policy,
        };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    /// Open a persistent knowledge store backed by fjall at the given path.
    ///
    /// Primary production backend: pure Rust, LSM-tree, LZ4 compression,
    /// native read-your-own-writes.
    ///
    /// # Complexity
    ///
    /// O(S + L) where S is schema init and L is LSM recovery cost (typically O(1)
    /// for fjall with existing data).
    #[cfg(feature = "storage-fjall")]
    #[instrument(skip(path))]
    pub fn open_fjall(
        path: impl AsRef<std::path::Path>,
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>> {
        let path = path.as_ref();
        Self::migrate_to_cohort_layout(path)?;
        let db = crate::engine::Db::open_fjall(path).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        let store = Self {
            db: std::sync::Arc::new(db),
            dim: config.dim,
            embedding_model: config.embedding_model,
            allow_assumed_embedding_meta: config.allow_assumed_embedding_meta,
            access_event_sequence: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            read_query_count: std::sync::atomic::AtomicUsize::new(0),
            insert_locks: Self::new_insert_locks(),
            admission_policy: config.admission_policy,
        };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    #[cfg(feature = "storage-fjall")]
    fn migrate_to_cohort_layout(path: &std::path::Path) -> crate::error::Result<()> {
        if path.file_name().and_then(std::ffi::OsStr::to_str) != Some("shared") {
            return Ok(());
        }

        let Some(base) = path.parent() else {
            return Ok(());
        };
        if !base.exists() || path.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(base).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: format!(
                    "failed to inspect legacy knowledge store at {}: {e}",
                    base.display()
                ),
            }
            .build()
        })?;
        std::fs::create_dir_all(path).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: format!("failed to create shared cohort at {}: {e}", path.display()),
            }
            .build()
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::error::EngineInitSnafu {
                    message: format!(
                        "failed to read legacy knowledge store entry at {}: {e}",
                        base.display()
                    ),
                }
                .build()
            })?;
            if entry.file_name() == std::ffi::OsStr::new("shared") {
                continue;
            }
            let source = entry.path();
            let target = path.join(entry.file_name());
            Self::copy_legacy_entry(&source, &target)?;
        }
        Ok(())
    }

    #[cfg(feature = "storage-fjall")]
    fn copy_legacy_entry(
        source: &std::path::Path,
        target: &std::path::Path,
    ) -> crate::error::Result<()> {
        let metadata = std::fs::metadata(source).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: format!(
                    "failed to stat legacy knowledge entry {}: {e}",
                    source.display()
                ),
            }
            .build()
        })?;
        if metadata.is_dir() {
            std::fs::create_dir_all(target).map_err(|e| {
                crate::error::EngineInitSnafu {
                    message: format!(
                        "failed to create migrated knowledge directory {}: {e}",
                        target.display()
                    ),
                }
                .build()
            })?;
            for child in std::fs::read_dir(source).map_err(|e| {
                crate::error::EngineInitSnafu {
                    message: format!(
                        "failed to read legacy knowledge directory {}: {e}",
                        source.display()
                    ),
                }
                .build()
            })? {
                let child = child.map_err(|e| {
                    crate::error::EngineInitSnafu {
                        message: format!(
                            "failed to read legacy knowledge child in {}: {e}",
                            source.display()
                        ),
                    }
                    .build()
                })?;
                Self::copy_legacy_entry(&child.path(), &target.join(child.file_name()))?;
            }
        } else {
            std::fs::copy(source, target).map_err(|e| {
                crate::error::EngineInitSnafu {
                    message: format!(
                        "failed to copy legacy knowledge entry {} to {}: {e}",
                        source.display(),
                        target.display()
                    ),
                }
                .build()
            })?;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "schema init is a single linear sequence"
    )]
    /// Initialize the knowledge schema (relations, indices, migrations).
    ///
    /// # Complexity
    ///
    /// O(R) where R is number of relations to create (constant ~10).
    /// Migrations run conditionally based on schema version.
    fn init_schema(&self) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::ScriptMutability;
        if self.schema_version_relation_exists()? {
            let current_version = self.schema_version_for_startup()?;
            self.verify_schema_integrity(current_version)?;
            self.apply_pending_migrations(current_version)?;
            self.verify_schema_integrity(Self::SCHEMA_VERSION)?;
            self.verify_embedding_meta()?;
            return Ok(());
        }

        let relations = self.relation_names()?;
        if !relations.is_empty() {
            return Err(Self::schema_integrity_error(format!(
                "schema version corruption: schema_version relation is missing but store has relations {}; repair by restoring from backup or re-stamping only after verifying the schema version",
                relations.join(", ")
            )));
        }

        // WHY: each relation has a named DDL constant so additions cannot
        // silently shift positional indices. The `entities` relation is omitted
        // here because it needs a dim-parameterized `name_embedding` column for
        // the dedup pipeline (#4165 Path A) and is created explicitly via
        // `entities_ddl(self.dim)` below.
        let run_ddl = |ddl: &str, ctx: &str| -> crate::error::Result<()> {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("{ctx}: {e}"),
                    }
                    .build()
                })
                .map(|_| ())
        };

        run_ddl(FACTS_DDL, "init_schema facts")?;
        run_ddl(FACT_ACCESS_LOG_DDL, "init_schema fact_access_log")?;
        run_ddl(RELATIONSHIPS_DDL, "init_schema relationships")?;
        run_ddl(FACT_ENTITIES_DDL, "init_schema fact_entities")?;
        run_ddl(MERGE_AUDIT_DDL, "init_schema merge_audit")?;
        run_ddl(PENDING_MERGES_DDL, "init_schema pending_merges")?;
        run_ddl(CAUSAL_EDGES_DDL, "init_schema causal_edges")?;
        run_ddl(TYPE_HIERARCHY_DDL, "init_schema type_hierarchy")?;
        run_ddl(DERIVED_FACTS_DDL, "init_schema derived_facts")?;
        run_ddl(DEFAULTS_DDL, "init_schema defaults")?;
        run_ddl(PUBLISHED_FACTS_DDL, "init_schema published_facts")?;
        run_ddl(PROVENANCE_DDL, "init_schema provenance")?;
        run_ddl(EMBEDDING_META_DDL, "init_schema embedding_meta")?;
        run_ddl(ENTITY_FLAGS_DDL, "init_schema entity_flags")?;
        run_ddl(
            DERIVED_SOURCE_REVISION_DDL,
            "init_schema derived_source_revision",
        )?;
        run_ddl(
            DERIVED_RULE_WATERMARKS_DDL,
            "init_schema derived_rule_watermarks",
        )?;

        let entities_script = entities_ddl(self.dim);
        self.db
            .run(&entities_script, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let emb_ddl = embeddings_ddl(self.dim);
        self.db
            .run(&emb_ddl, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let hnsw = hnsw_ddl(self.dim);
        self.db
            .run(&hnsw, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let fts = fts_ddl();
        self.db
            .run(fts, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        self.db
            .run(
                crate::graph_intelligence::GRAPH_SCORES_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        self.db
            .run(
                crate::consolidation::CONSOLIDATION_AUDIT_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.db
            .run(
                crate::consolidation::CONSOLIDATION_AUDIT_RECORDED_AT_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.db
            .run(
                crate::consolidation::CONSOLIDATION_AUDIT_NOUS_RECORDED_AT_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        // WHY (#3634): fact_multiplicity is a side-index for consolidated-fact
        // strength. Lives next to consolidation_audit so both arrive at v9.
        self.db
            .run(
                crate::consolidation::FACT_MULTIPLICITY_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        // WHY (#4660): consolidation_provenance stores the original source fact
        // IDs and source session IDs for each consolidated fact, keeping
        // provenance inspectable without parsing audit JSON.
        self.db
            .run(
                crate::consolidation::CONSOLIDATION_PROVENANCE_DDL,
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        // WHY (#4662): initialize the derived-rule source-revision counter so
        // fresh stores start at revision 0 and base writes can bump it.
        self.db
            .run(
                r"?[key, revision] <- [['global', 0]]
                  :put derived_source_revision { key => revision }",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("init derived_source_revision failed: {e}"),
                }
                .build()
            })?;

        self.db
            .run(
                r":create schema_version { key: String => version: Int }",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        self.stamp_fresh_schema_versions()?;
        self.write_configured_embedding_meta()?;

        Ok(())
    }

    fn schema_version_relation_exists(&self) -> crate::error::Result<bool> {
        self.relation_exists("schema_version")
    }

    fn relation_exists(&self, name: &str) -> crate::error::Result<bool> {
        self.relation_names()
            .map(|names| names.iter().any(|n| n == name))
    }

    fn relation_names(&self) -> crate::error::Result<Vec<String>> {
        let rows = self.run_read("::relations", std::collections::BTreeMap::new())?;
        rows.rows
            .into_iter()
            .map(|row| {
                row.first()
                    .and_then(crate::engine::DataValue::get_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        crate::error::EngineQuerySnafu {
                            message: "schema relation listing returned a non-string relation name"
                                .to_owned(),
                        }
                        .build()
                    })
            })
            .collect()
    }

    fn schema_version_for_startup(&self) -> crate::error::Result<i64> {
        self.schema_version().map_err(|err| {
            Self::schema_integrity_error(format!(
                "schema version corruption: schema_version relation is present but row 'schema' is missing; repair by restoring from backup or re-stamping only after verifying the applied schema version: {err}"
            ))
        })
    }

    fn verify_schema_integrity(&self, current_version: i64) -> crate::error::Result<()> {
        if current_version < Self::MIN_SCHEMA_VERSION {
            return Err(Self::schema_integrity_error(format!(
                "schema version corruption: stored version {current_version} is below minimum {}; repair by restoring from backup or re-stamping only after verifying the schema version",
                Self::MIN_SCHEMA_VERSION
            )));
        }
        if current_version > Self::SCHEMA_VERSION {
            return Err(crate::error::SchemaVersionSnafu {
                expected: Self::SCHEMA_VERSION,
                found: current_version,
            }
            .build());
        }

        for step in migration::MIGRATIONS {
            if step.target_version > current_version {
                break;
            }
            let stamp = self.migration_stamp_version(step.target_version)?;
            match stamp {
                Some(version) if version == step.target_version => {}
                Some(version) => {
                    return Err(Self::schema_integrity_error(format!(
                        "schema version integrity hole: migration stamp for version {} recorded version {version}; repair by restoring from backup or re-stamping only after verifying migration v{} to v{} was applied",
                        step.target_version,
                        step.target_version - 1,
                        step.target_version
                    )));
                }
                None => {
                    return Err(Self::schema_integrity_error(format!(
                        "schema version integrity hole: store version {current_version} is missing migration stamp for version {}; repair by restoring from backup or re-stamping only after verifying migration v{} to v{} was applied",
                        step.target_version,
                        step.target_version - 1,
                        step.target_version
                    )));
                }
            }
        }

        tracing::info!(
            current_version,
            expected_version = Self::SCHEMA_VERSION,
            "knowledge schema version integrity verified"
        );
        Ok(())
    }

    fn apply_pending_migrations(&self, current_version: i64) -> crate::error::Result<()> {
        for step in migration::MIGRATIONS {
            if step.target_version <= current_version {
                continue;
            }
            tracing::info!(
                current_version,
                target_version = step.target_version,
                expected_version = Self::SCHEMA_VERSION,
                "applying pending knowledge schema migration"
            );
            (step.run)(self)?;
            let stamped_version = self.schema_version()?;
            if stamped_version != step.target_version {
                return Err(crate::error::SchemaVersionSnafu {
                    expected: step.target_version,
                    found: stamped_version,
                }
                .build());
            }
        }
        Ok(())
    }

    fn migration_stamp_key(version: i64) -> String {
        format!("migration:{version}")
    }

    fn migration_stamp_version(&self, version: i64) -> crate::error::Result<Option<i64>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "key".to_owned(),
            DataValue::Str(Self::migration_stamp_key(version).into()),
        );
        let rows = self.run_read(r"?[version] := *schema_version{key: $key, version}", params)?;
        let Some(row) = rows.rows.into_iter().next() else {
            return Ok(None);
        };
        marshal::extract_int(row.first().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: format!("migration stamp for version {version} is empty"),
            }
            .build()
        })?)
        .map(Some)
    }

    fn stamp_schema_version(&self, version: i64, context: &str) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        let mut params = BTreeMap::new();
        params.insert("schema_key".to_owned(), DataValue::Str("schema".into()));
        params.insert(
            "stamp_key".to_owned(),
            DataValue::Str(Self::migration_stamp_key(version).into()),
        );
        params.insert("version".to_owned(), DataValue::from(version));
        self.db
            .run(
                r"?[key, version] <- [[$schema_key, $version], [$stamp_key, $version]]
                  :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("{context} version write failed: {e}"),
                }
                .build()
            })?;
        tracing::info!(
            target_version = version,
            "knowledge schema migration version stamped"
        );
        Ok(())
    }

    fn stamp_fresh_schema_versions(&self) -> crate::error::Result<()> {
        use crate::engine::ScriptMutability;

        let mut rows = vec![format!(r#"["schema", {}]"#, Self::SCHEMA_VERSION)];
        rows.extend(migration::MIGRATIONS.iter().map(|step| {
            format!(
                r#"["{}", {}]"#,
                Self::migration_stamp_key(step.target_version),
                step.target_version
            )
        }));
        let script = format!(
            "?[key, version] <- [{}] :put schema_version {{ key => version }}",
            rows.join(", ")
        );
        self.db
            .run(
                &script,
                std::collections::BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("fresh schema version stamp failed: {e}"),
                }
                .build()
            })?;
        Ok(())
    }

    fn write_configured_embedding_meta(&self) -> crate::error::Result<()> {
        self.replace_embedding_meta(&self.embedding_model, self.dim)
    }

    fn replace_embedding_meta(&self, model: &str, dim: usize) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{DataValue, ScriptMutability};
        let dim = i64::try_from(dim).map_err(|err| {
            crate::error::ConversionSnafu {
                message: format!("embedding dimension overflowed i64: {err}"),
            }
            .build()
        })?;
        let mut params = BTreeMap::new();
        params.insert("model".to_owned(), DataValue::Str(model.into()));
        params.insert("dim".to_owned(), DataValue::from(dim));
        self.db
            .run(
                r"?[model] := *embedding_meta{model}
                  :rm embedding_meta {model}",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("embedding metadata clear failed: {e}"),
                }
                .build()
            })?;
        self.db
            .run(
                r"?[model, dim] <- [[$model, $dim]]
                  :put embedding_meta {model => dim}",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("embedding metadata write failed: {e}"),
                }
                .build()
            })?;
        Ok(())
    }

    pub(crate) fn embedding_meta(&self) -> crate::error::Result<EmbeddingMeta> {
        let rows = self.run_read(
            r"?[model, dim] := *embedding_meta{model, dim}",
            std::collections::BTreeMap::new(),
        )?;
        let row = rows.rows.into_iter().next().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "embedding metadata row missing".to_owned(),
            }
            .build()
        })?;
        let model = marshal::extract_str(row.first().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "embedding metadata model cell missing".to_owned(),
            }
            .build()
        })?)?
        .clone();
        let dim_value = marshal::extract_int(row.get(1).ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "embedding metadata dimension cell missing".to_owned(),
            }
            .build()
        })?)?;
        let dim = usize::try_from(dim_value).map_err(|err| {
            crate::error::ConversionSnafu {
                message: format!("embedding metadata dimension was not usize: {err}"),
            }
            .build()
        })?;
        Ok(EmbeddingMeta { model, dim })
    }

    fn verify_embedding_meta(&self) -> crate::error::Result<()> {
        let meta = self.embedding_meta()?;
        if meta.model == Self::ASSUMED_EMBEDDING_MODEL
            && self.allow_assumed_embedding_meta
            && meta.dim == self.dim
        {
            return Ok(());
        }
        if meta.model == self.embedding_model && meta.dim == self.dim {
            return Ok(());
        }
        Err(crate::error::EmbeddingDriftSnafu {
            stored_model: meta.model,
            stored_dim: meta.dim,
            configured_model: self.embedding_model.clone(),
            configured_dim: self.dim,
        }
        .build())
    }

    fn schema_integrity_error(message: impl Into<String>) -> crate::error::Error {
        crate::error::EngineQuerySnafu {
            message: message.into(),
        }
        .build()
    }

    /// Query the stored schema version from the database.
    ///
    /// # Complexity
    ///
    /// O(1) - single row lookup in `schema_version` relation.
    pub fn schema_version(&self) -> crate::error::Result<i64> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        let rows = self.run_read(r"?[version] := *schema_version{key: $key, version}", params)?;
        let row = rows.rows.into_iter().next().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "schema version record missing",
            }
            .build()
        })?;
        marshal::extract_int(row.first().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "schema version row empty",
            }
            .build()
        })?)
    }

    /// Raw query escape hatch for callers needing custom Datalog.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep engine
    /// internals encapsulated. Use typed accessors (`get_string`, `get_f64`, etc.).
    ///
    /// # Complexity
    ///
    /// Depends on query complexity. Simple lookups O(1), joins O(N log M),
    /// recursive queries O(E * I) where E is edges, I is iterations to fixpoint.
    #[instrument(skip(self, params))]
    pub fn run_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult> {
        self.run_read(script, params).map(QueryResult::from)
    }

    /// Map a Krites engine error to an episteme knowledge error.
    ///
    /// WHY: `krites::Error::QueryKilled` is the typed signal for query
    /// cancellation (poison) and `:timeout` expiry. Match it by variant rather
    /// than by display text so wording changes cannot silently turn a timeout
    /// into a generic engine failure.
    fn map_engine_err(
        err: crate::engine::Error,
        timeout: Option<std::time::Duration>,
    ) -> crate::error::Error {
        use crate::engine::Error as EngineError;
        match err {
            EngineError::QueryKilled { .. } => crate::error::QueryTimeoutSnafu {
                secs: timeout.map_or(0.0, |d| d.as_secs_f64()),
            }
            .build(),
            other => crate::error::EngineQuerySnafu {
                message: other.to_string(),
            }
            .build(),
        }
    }

    /// Run a custom Datalog query with an optional timeout.
    ///
    /// If the query exceeds the timeout, returns `Error::QueryTimeout`.
    /// The `:timeout` directive is injected into the script: callers should not include it.
    ///
    /// Timeout detection maps the engine's typed [`QueryKilled`](crate::engine::Error::QueryKilled)
    /// error to [`Error::QueryTimeout`](crate::error::Error::QueryTimeout).
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep engine internals
    /// encapsulated.
    ///
    /// # Complexity
    ///
    /// Same as `run_query`. Timeout adds minimal overhead (O(1) polling).
    #[instrument(skip(self, params))]
    pub fn run_query_with_timeout(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
        timeout: Option<std::time::Duration>,
    ) -> crate::error::Result<QueryResult> {
        use crate::engine::ScriptMutability;
        let script_with_timeout = match timeout {
            Some(d) => format!("{script}\n:timeout {}", d.as_secs_f64()),
            None => script.to_owned(),
        };
        self.db
            .run(&script_with_timeout, params, ScriptMutability::Immutable)
            .map(QueryResult::from)
            .map_err(|e| Self::map_engine_err(e, timeout))
    }

    /// Raw mutable query escape hatch: runs script with `ScriptMutability::Mutable`.
    /// Required for `:rm` and `:put` operations from caller code.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep engine internals
    /// encapsulated.
    ///
    /// # Complexity
    ///
    /// Depends on mutation scope. Single row writes O(1), bulk operations O(N).
    #[instrument(skip(self, params))]
    pub fn run_mut_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult> {
        use crate::engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Mutable)
            .map(QueryResult::from)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }

    /// Discover serendipitous facts from recently active entities.
    #[instrument(skip(self))]
    pub fn discover_serendipitous_facts(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<SerendipityDiscoveryReport> {
        crate::serendipity::discover_serendipitous_facts(self, nous_id)
    }

    /// Run a Datalog script in read-only mode. Convenience wrapper around `run_query`.
    ///
    /// Equivalent to calling `run_query`, but makes the immutability contract explicit
    /// for callers who need a read-only guarantee (e.g., the `datalog_query` tool).
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep engine internals
    /// encapsulated.
    ///
    /// # Complexity
    ///
    /// Same as `run_query`.
    #[instrument(skip(self, params))]
    pub fn run_script_read_only(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult> {
        self.run_read(script, params).map(QueryResult::from)
    }

    /// Read fact rows by ID without overlaying append-only access events.
    pub(super) fn read_facts_by_id_raw(
        &self,
        id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity},
                id = $id
        ";
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(id.into()));
        let rows = self.run_read(script, params)?;
        marshal::rows_to_raw_facts(rows)
    }

    /// Read a single fact by its ID (all temporal records matching).
    /// Returns all fields; does not apply time/validity filters.
    ///
    /// # Complexity
    ///
    /// O(T + A) where T is temporal versions of the fact and A is access events
    /// for those fact IDs. Typically O(1) for non-versioned facts.
    pub fn read_facts_by_id(&self, id: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        let mut facts = self.read_facts_by_id_raw(id)?;
        self.apply_access_log_to_facts(&mut facts)?;
        Ok(facts)
    }

    pub(super) fn run_mut(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<()> {
        use crate::engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Mutable)
            .map(|_| ())
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }

    pub(super) fn run_read(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        use crate::engine::ScriptMutability;
        #[cfg(test)]
        self.read_query_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // WHY: A failing read query (e.g. a CozoScript parse error on the recall
        // path, #4156) is otherwise invisible — the error carries only the engine
        // message, not the script that produced it. Capture the script text and
        // the parameter key set so the exact failing query is recoverable from
        // logs at debug level. Parameter values are deliberately not logged
        // (they may carry user content); scripts use `$param` placeholders, so
        // the script text itself contains no user data.
        let param_keys: Vec<String> = params.keys().cloned().collect();
        self.db
            .run(script, params, ScriptMutability::Immutable)
            .map_err(|e| {
                tracing::debug!(
                    script,
                    param_keys = ?param_keys,
                    error = %e,
                    "read query failed against the knowledge engine"
                );
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }

    /// Reset the test-only read query counter used by search complexity tests.
    #[cfg(test)]
    pub(crate) fn reset_read_query_count_for_test(&self) {
        self.read_query_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Return the test-only read query count used by search complexity tests.
    #[cfg(test)]
    pub(crate) fn read_query_count_for_test(&self) -> usize {
        self.read_query_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
