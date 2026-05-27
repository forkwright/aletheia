//! `CozoDB`-backed knowledge store implementation.
//!
//! This module requires the `mneme-engine` feature flag.
//!
//! **Storage:** The `mneme-engine` vendored CozoDB uses only mem/redb/fjall
//! storage backends: no C++ dependencies. The legacy session-store SQLite
//! feature is no longer part of the live session backend.
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
//!         visibility: String }
//!
//! entities { id: String => name: String, entity_type: String, aliases: String,
//!            created_at: String, updated_at: String }
//!
//! relationships { src: String, dst: String => relation: String, weight: Float,
//!                 created_at: String }
//!
//! embeddings { id: String => content: String, source_type: String, source_id: String,
//!              nous_id: String, embedding: <F32; DIM>, created_at: String }
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
mod facts;
#[cfg(feature = "mneme-engine")]
mod marshal;
#[cfg(feature = "mneme-engine")]
mod migration;
#[cfg(feature = "mneme-engine")]
mod search;
#[cfg(feature = "mneme-engine")]
mod skills;

#[cfg(test)]
mod tests;

use tracing::instrument;

/// Datalog DDL for initializing the knowledge schema.
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
        forget_reason: String?,
        scope: String?,
        project_id: String?,
        visibility: String default 'private'
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
    // Index 10 — published_facts (added in schema v10, R716 Phase 3)
    r":create published_facts {
        id: String =>
        original_fact_id: String,
        published_by: String,
        published_at: String,
        verification_count: Int default 0,
        contested_by: String,
        contest_reason: String?
    }",
    // Index 11 — provenance (added in schema v10, R716 Phase 3)
    r":create provenance {
        published_fact_id: String, contributor: String =>
        contribution_type: String,
        confidence: Float,
        contributed_at: String
    }",
];

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

/// Re-export query builder types and pre-built query scripts.
///
/// Builder-generated queries (field-safe): `queries::upsert_fact()`, etc.
/// Raw Datalog constants (multi-rule): `queries::ENTITY_NEIGHBORHOOD`, etc.
#[cfg(feature = "mneme-engine")]
use crate::query::queries;

/// Typed wrapper for raw Datalog query results.
///
/// Returned by [`KnowledgeStore::run_query`] and related escape-hatch methods.
/// Hides the `crate::engine::NamedRows` type from callers, keeping `CozoDB`
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

/// Configuration for `KnowledgeStore` initialization.
#[cfg(feature = "mneme-engine")]
pub struct KnowledgeConfig {
    /// Embedding dimension for the HNSW index.
    pub dim: usize,
    /// Admission policy for fact insertion. Default: [`DefaultAdmissionPolicy`](crate::admission::DefaultAdmissionPolicy).
    pub admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}

#[cfg(feature = "mneme-engine")]
impl std::fmt::Debug for KnowledgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeConfig")
            .field("dim", &self.dim)
            .field("admission_policy", &"<dyn AdmissionPolicy>")
            .finish()
    }
}

#[cfg(feature = "mneme-engine")]
impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            dim: 384,
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

/// Typed wrapper around the Datalog engine providing domain-level knowledge operations.
///
/// Holds an `Arc<Db>` internally. Callers share via `Arc<KnowledgeStore>`.
/// All sync methods can be called directly; async wrappers use `spawn_blocking`.
#[cfg(feature = "mneme-engine")]
pub struct KnowledgeStore {
    db: std::sync::Arc<crate::engine::Db>,
    dim: usize,
    /// Serializes read-modify-write access counter increments to prevent races.
    access_lock: std::sync::Mutex<()>,
    /// Admission policy gate: checked before every fact insertion.
    admission_policy: Box<dyn crate::admission::AdmissionPolicy>,
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    const SCHEMA_VERSION: i64 = 12;

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
            access_lock: std::sync::Mutex::new(()),
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
            access_lock: std::sync::Mutex::new(()),
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
        let already_initialized = self
            .db
            .run(
                "?[v] := *schema_version{version: v}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .is_ok();

        if already_initialized {
            let current_version = self.schema_version().unwrap_or(0);
            if current_version < 2 {
                self.migrate_v1_to_v2()?;
            }
            if current_version < 3 {
                self.migrate_v2_to_v3()?;
            }
            if current_version < 4 {
                self.migrate_v3_to_v4()?;
            }
            if current_version < 5 {
                self.migrate_v4_to_v5()?;
            }
            if current_version < 6 {
                self.migrate_v5_to_v6()?;
            }
            if current_version < 7 {
                self.migrate_v6_to_v7()?;
            }
            if current_version < 8 {
                self.migrate_v7_to_v8()?;
            }
            if current_version < 9 {
                self.migrate_v8_to_v9()?;
            }
            if current_version < 10 {
                self.migrate_v9_to_v10()?;
            }
            if current_version < 11 {
                self.migrate_v10_to_v11()?;
            }
            if current_version < 12 {
                self.migrate_v11_to_v12()?;
            }
            return Ok(());
        }

        for ddl in KNOWLEDGE_DDL {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
        }

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

        let mut params = BTreeMap::new();
        params.insert(
            "key".to_owned(),
            crate::engine::DataValue::Str("schema".into()),
        );
        params.insert(
            "version".to_owned(),
            crate::engine::DataValue::from(Self::SCHEMA_VERSION),
        );
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        Ok(())
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
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB`
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

    /// Run a custom Datalog query with an optional timeout.
    ///
    /// If the query exceeds the timeout, returns `Error::QueryTimeout`.
    /// The `:timeout` directive is injected into the script: callers should not include it.
    ///
    /// Note: timeout detection relies on the engine error containing "killed before completion"
    /// (from `CozoDB`'s internal `ProcessKilled` error). This is a known fragile dependency.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB` internals
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
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("killed before completion") {
                    crate::error::QueryTimeoutSnafu {
                        secs: timeout.map_or(0.0, |d| d.as_secs_f64()),
                    }
                    .build()
                } else {
                    crate::error::EngineQuerySnafu { message: msg }.build()
                }
            })
    }

    /// Raw mutable query escape hatch: runs script with `ScriptMutability::Mutable`.
    /// Required for `:rm` and `:put` operations from caller code.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB` internals
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

    /// Create a backup of the knowledge database.
    ///
    /// Delegates to the inner engine's `backup_db`. Currently returns an error
    /// for in-memory and redb backends (`SQLite` storage support was removed).
    ///
    /// # Complexity
    ///
    /// O(D) where D is database size. Copies all `SSTables` and logs.
    #[instrument(skip(self, out_file))]
    pub fn backup_db(&self, out_file: impl AsRef<std::path::Path>) -> crate::error::Result<()> {
        self.db.backup_db(out_file).map_err(|e| {
            crate::error::EngineQuerySnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Restore the knowledge database from a backup file.
    ///
    /// Delegates to the inner engine's `restore_backup`. Currently returns an error
    /// for in-memory and redb backends (`SQLite` storage support was removed).
    ///
    /// # Complexity
    ///
    /// O(D) where D is backup size. Replaces all `SSTables`.
    #[instrument(skip(self, in_file))]
    pub fn restore_backup(&self, in_file: impl AsRef<std::path::Path>) -> crate::error::Result<()> {
        self.db.restore_backup(in_file).map_err(|e| {
            crate::error::EngineQuerySnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Import specific relations from a backup file into the live database.
    ///
    /// Delegates to the inner engine's `import_from_backup`. Currently returns an error
    /// for in-memory and redb backends (`SQLite` storage support was removed).
    ///
    /// # Complexity
    ///
    /// O(R) where R is total size of relations being imported.
    #[instrument(skip(self, in_file))]
    pub fn import_from_backup(
        &self,
        in_file: impl AsRef<std::path::Path>,
        relations: &[String],
    ) -> crate::error::Result<()> {
        self.db.import_from_backup(in_file, relations).map_err(|e| {
            crate::error::EngineQuerySnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Run a Datalog script in read-only mode. Convenience wrapper around `run_query`.
    ///
    /// Equivalent to calling `run_query`, but makes the immutability contract explicit
    /// for callers who need a read-only guarantee (e.g., the `datalog_query` tool).
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB` internals
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

    /// Read a single fact by its ID (all temporal records matching).
    /// Returns all fields; does not apply time/validity filters.
    ///
    /// # Complexity
    ///
    /// O(T) where T is temporal versions of the fact. Typically O(1) for
    /// non-versioned facts, O(log V) for versioned with time-travel indices.
    pub fn read_facts_by_id(&self, id: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id, visibility},
                id = $id
        ";
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(id.into()));
        let rows = self.run_read(script, params)?;
        marshal::rows_to_raw_facts(rows)
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
}
