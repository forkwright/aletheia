//! `CozoDB`-backed knowledge store implementation.
//!
//! This module requires the `mneme-engine` feature flag.
//!
//! **Coexistence with `sqlite` feature:** No link conflict. The `mneme-engine`
//! vendored CozoDB uses only mem/redb/fjall storage backends — no C++
//! dependencies. `rusqlite` (used by the `sqlite` feature) compiles with
//! `features = ["bundled"]`, keeping its symbols isolated. Both features may
//! be enabled simultaneously.
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
//!         forget_reason: String? }
//!
//! entities { id: String => name: String, entity_type: String, aliases: String,
//!            created_at: String, updated_at: String }
//!
//! relationships { src: String, dst: String => relation: String, weight: Float,
//!                 created_at: String }
//!
//! embeddings { id: String => content: String, source_type: String, source_id: String,
//!              nous_id: String, embedding: <F32; DIM>, created_at: String }
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

// This module contains the `CozoDB` store implementation as documentation and
// reference code. It will be activated when the cozo feature flag is enabled
// in the production binary.
//
// The Datalog queries are validated by the mneme-bench crate.

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
    // Facts: bi-temporal, epistemic-tiered
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
    // Entities: typed nodes in the knowledge graph
    r":create entities {
        id: String =>
        name: String,
        entity_type: String,
        aliases: String,
        created_at: String,
        updated_at: String
    }",
    // Relationships: weighted edges
    r":create relationships {
        src: String, dst: String =>
        relation: String,
        weight: Float,
        created_at: String
    }",
    // Fact-entity mappings: which entities a fact mentions
    r":create fact_entities {
        fact_id: String, entity_id: String =>
        created_at: String
    }",
    // Entity merge audit trail
    r":create merge_audit {
        canonical_id: String, merged_id: String =>
        merged_name: String,
        merge_score: Float,
        facts_transferred: Int,
        relationships_redirected: Int,
        merged_at: String
    }",
    // Pending entity merges awaiting review (score 0.70–0.90)
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
/// Row values are [`crate::engine::DataValue`] — call `.get_str()`, `.get_float()`,
/// etc. to extract typed values.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Column names in the order they appear in each row.
    pub headers: Vec<String>,
    /// Result rows. Each row is a flat `Vec` matching `headers` by position.
    pub rows: Vec<Vec<crate::engine::DataValue>>,
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
#[derive(Clone, Copy, Debug)]
pub struct KnowledgeConfig {
    /// Embedding dimension for the HNSW index.
    pub dim: usize,
}

#[cfg(feature = "mneme-engine")]
impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self { dim: 384 }
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
    fn rrf_score(&self) -> f64 {
        self.rrf_score
    }
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
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    const SCHEMA_VERSION: i64 = 5;

    /// Open an in-memory knowledge store with default configuration.
    #[instrument]
    pub fn open_mem() -> crate::error::Result<std::sync::Arc<Self>> {
        Self::open_mem_with_config(KnowledgeConfig::default())
    }

    /// Open an in-memory knowledge store with custom configuration.
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
        };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    /// Open a persistent knowledge store backed by fjall at the given path.
    ///
    /// Primary production backend: pure Rust, LSM-tree, LZ4 compression,
    /// native read-your-own-writes.
    #[cfg(feature = "storage-fjall")]
    #[instrument(skip(path))]
    pub fn open_fjall(
        path: impl AsRef<std::path::Path>,
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>> {
        let db = crate::engine::Db::open_fjall(path).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        let store = Self {
            db: std::sync::Arc::new(db),
            dim: config.dim,
        };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    /// Open a persistent knowledge store backed by redb at the given path.
    #[cfg(feature = "storage-redb")]
    #[instrument(skip(path))]
    pub fn open_redb(
        path: impl AsRef<std::path::Path>,
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>> {
        let db = crate::engine::Db::open_redb(path).map_err(|e| {
            crate::error::EngineInitSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        let store = Self {
            db: std::sync::Arc::new(db),
            dim: config.dim,
        };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "schema init is a single linear sequence"
    )]
    fn init_schema(&self) -> crate::error::Result<()> {
        use crate::engine::ScriptMutability;
        use std::collections::BTreeMap;

        // Check if the database is already initialized (persistent reopen)
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

        // Graph scores relation (PageRank + Louvain cache)
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

        // Consolidation audit relation
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

        // Schema version tracking relation
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

    // --- Schema & query infrastructure ---

    pub fn schema_version(&self) -> crate::error::Result<i64> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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
    /// internals encapsulated. Access row values via `result.rows[i][j]`.
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
    /// The `:timeout` directive is injected into the script — callers should not include it.
    ///
    /// Note: timeout detection relies on the engine error containing "killed before completion"
    /// (from `CozoDB`'s internal `ProcessKilled` error). This is a known fragile dependency.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB` internals
    /// encapsulated.
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

    /// Raw mutable query escape hatch — runs script with `ScriptMutability::Mutable`.
    /// Required for `:rm` and `:put` operations from caller code.
    ///
    /// Returns a [`QueryResult`] rather than raw `NamedRows` to keep `CozoDB` internals
    /// encapsulated.
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

    // --- Backup & restore ---

    /// Create a backup of the knowledge database.
    ///
    /// Delegates to the inner engine's `backup_db`. Currently returns an error
    /// for in-memory and redb backends (`SQLite` storage support was removed).
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
    #[instrument(skip(self, params))]
    pub fn run_script_read_only(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<QueryResult> {
        self.run_read(script, params).map(QueryResult::from)
    }

    // --- Internal helpers ---

    /// Query all facts valid at a specific time, across all nous IDs.
    /// Used internally by `search_temporal` for filtering.
    pub(super) fn query_facts_at_time_all(
        &self,
        at_time: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let script = r"
            ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to,
              superseded_by, source_session_id,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
                       superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason},
                is_forgotten == false,
                valid_from <= $at_time,
                valid_to > $at_time
        ";
        let mut params = BTreeMap::new();
        params.insert("at_time".to_owned(), DataValue::Str(at_time.into()));
        let rows = self.run_read(script, params)?;
        marshal::rows_to_facts(rows, "")
    }

    /// Read a single fact by its ID (all temporal records matching).
    /// Returns all fields; does not apply time/validity filters.
    pub(super) fn read_facts_by_id(&self, id: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason},
                id = $id
        ";
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(id.into()));
        let rows = self.run_read(script, params)?;
        marshal::rows_to_raw_facts(rows)
    }

    // --- Low-level engine wrappers ---

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
        self.db
            .run(script, params, ScriptMutability::Immutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
}
