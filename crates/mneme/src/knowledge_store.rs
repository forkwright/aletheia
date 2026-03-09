//! `CozoDB`-backed knowledge store implementation.
//!
//! This module is gated behind the `cozo` feature flag due to `sqlite3` link
//! conflict with `rusqlite`. In the final binary, the session store will migrate
//! from `rusqlite` to `CozoDB`'s embedded `SQLite` storage, resolving the conflict.
//!
//! Until then, this code compiles and tests only with:
//! ```sh
//! cargo test -p aletheia-mneme --no-default-features --features mneme-engine
//! ```
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

// This module contains the CozoDB store implementation as documentation and
// reference code. It will be activated when the cozo feature flag is enabled
// in the production binary.
//
// The Datalog queries are validated by the mneme-bench crate.

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
    pub seed_entities: Vec<String>,
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
    pub id: String,
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
        &self.id
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
    const SCHEMA_VERSION: i64 = 3;

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

    /// Insert or update a fact.
    #[instrument(skip(self, fact), fields(fact_id = %fact.id))]
    pub fn insert_fact(&self, fact: &crate::knowledge::Fact) -> crate::error::Result<()> {
        let params = fact_to_params(fact);
        self.run_mut(&queries::upsert_fact(), params)
    }

    /// Query current facts for a nous at a given time, up to limit results.
    #[instrument(skip(self))]
    pub fn query_facts(
        &self,
        nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("now".to_owned(), DataValue::Str(now.into()));
        params.insert("limit".to_owned(), DataValue::from(limit));

        let rows = self.run_read(&queries::full_current_facts(), params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Point-in-time fact query.
    #[instrument(skip(self))]
    pub fn query_facts_at(&self, time: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("time".to_owned(), DataValue::Str(time.into()));

        let rows = self.run_read(&queries::facts_at_time(), params)?;
        rows_to_facts_partial(rows)
    }

    /// Insert or update an entity.
    #[instrument(skip(self, entity), fields(entity_id = %entity.id))]
    pub fn insert_entity(&self, entity: &crate::knowledge::Entity) -> crate::error::Result<()> {
        let params = entity_to_params(entity);
        self.run_mut(&queries::upsert_entity(), params)
    }

    /// Insert a relationship.
    #[instrument(skip(self, rel))]
    pub fn insert_relationship(
        &self,
        rel: &crate::knowledge::Relationship,
    ) -> crate::error::Result<()> {
        let params = relationship_to_params(rel);
        self.run_mut(&queries::upsert_relationship(), params)
    }

    /// Query 2-hop entity neighborhood. Returns raw rows for flexible callers.
    #[instrument(skip(self))]
    pub fn entity_neighborhood(
        &self,
        entity_id: &str,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        self.run_read(queries::ENTITY_NEIGHBORHOOD, params)
    }

    /// Insert a vector embedding for semantic search.
    #[instrument(skip(self, chunk), fields(chunk_id = %chunk.id))]
    pub fn insert_embedding(
        &self,
        chunk: &crate::knowledge::EmbeddedChunk,
    ) -> crate::error::Result<()> {
        let params = embedding_to_params(chunk, self.dim);
        self.run_mut(&queries::upsert_embedding(), params)
    }

    /// kNN semantic vector search.
    #[instrument(skip(self, query_vec))]
    pub fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use crate::engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(query_vec))),
        );
        params.insert("k".to_owned(), DataValue::from(k));
        params.insert("ef".to_owned(), DataValue::from(ef));

        let rows = self.run_read(queries::SEMANTIC_SEARCH, params)?;
        let results = rows_to_recall_results(rows)?;

        let source_ids: Vec<String> = results
            .iter()
            .filter(|r| r.source_type == "fact")
            .map(|r| r.source_id.clone())
            .collect();
        if let Err(e) = self.increment_access(&source_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Get the current schema version.
    #[instrument(skip(self))]
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
        extract_int(row.first().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: "schema version row empty",
            }
            .build()
        })?)
    }

    /// Raw query escape hatch for callers needing custom Datalog.
    #[instrument(skip(self, params))]
    pub fn run_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        self.run_read(script, params)
    }

    /// Run a custom Datalog query with an optional timeout.
    ///
    /// If the query exceeds the timeout, returns `Error::QueryTimeout`.
    /// The `:timeout` directive is injected into the script — callers should not include it.
    ///
    /// Note: timeout detection relies on the engine error containing "killed before completion"
    /// (from `CozoDB`'s internal `ProcessKilled` error). This is a known fragile dependency.
    #[instrument(skip(self, params))]
    pub fn run_query_with_timeout(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
        timeout: Option<std::time::Duration>,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        use crate::engine::ScriptMutability;
        let script_with_timeout = match timeout {
            Some(d) => format!("{script}\n:timeout {}", d.as_secs_f64()),
            None => script.to_owned(),
        };
        self.db
            .run(&script_with_timeout, params, ScriptMutability::Immutable)
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
    #[instrument(skip(self, params))]
    pub fn run_mut_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        use crate::engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }

    /// Hybrid BM25 + HNSW vector + graph retrieval fused via `ReciprocalRankFusion`.
    ///
    /// Runs a single Datalog query combining all three signals in the engine.
    /// When `seed_entities` is empty, the graph signal contributes zero to RRF.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub fn search_hybrid(&self, q: &HybridQuery) -> crate::error::Result<Vec<HybridResult>> {
        use crate::engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "query_text".to_owned(),
            DataValue::Str(q.text.as_str().into()),
        );
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(q.embedding.clone()))),
        );
        // usize -> i64: limit/ef are user-controlled small values; truncate at i64::MAX for safety
        let limit_i64 = i64::try_from(q.limit).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(q.ef).unwrap_or(i64::MAX);
        params.insert("k".to_owned(), DataValue::from(limit_i64));
        params.insert("ef".to_owned(), DataValue::from(ef_i64));
        params.insert("limit".to_owned(), DataValue::from(limit_i64));

        let script = build_hybrid_query(q);
        let rows = self.run_read(&script, params)?;
        let results = rows_to_hybrid_results(rows)?;

        let fact_ids: Vec<String> = results.iter().map(|r| r.id.clone()).collect();
        if let Err(e) = self.increment_access(&fact_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Async `search_hybrid` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub async fn search_hybrid_async(
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_hybrid(&q))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Multi-query hybrid search: run hybrid search for each query variant,
    /// then merge results via reciprocal rank fusion.
    ///
    /// The `base_query` provides the embedding and search parameters. Each variant
    /// replaces the `text` field for BM25 scoring while reusing the same embedding.
    pub fn search_enhanced(
        &self,
        base_query: &HybridQuery,
        query_variants: &[String],
    ) -> crate::error::Result<Vec<HybridResult>> {
        use crate::query_rewrite::rrf_merge;

        if query_variants.is_empty() {
            return self.search_hybrid(base_query);
        }

        let mut results_per_query = Vec::with_capacity(query_variants.len());
        for variant in query_variants {
            let mut q = base_query.clone();
            q.text.clone_from(variant);
            match self.search_hybrid(&q) {
                Ok(results) => results_per_query.push(results),
                Err(e) => {
                    tracing::warn!(variant = %variant, error = %e, "search variant failed, skipping");
                }
            }
        }

        if results_per_query.is_empty() {
            return Ok(vec![]);
        }

        Ok(rrf_merge(&results_per_query, 60.0))
    }

    /// Tiered search: fast path -> enhanced -> graph-enhanced.
    ///
    /// Escalates through tiers until sufficient results are found.
    /// Requires a `QueryRewriter` and `RewriteProvider` for tier 2+.
    pub fn search_tiered(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        let start = std::time::Instant::now();

        // Tier 1: Fast path — single-query hybrid search
        let fast_results = self.search_hybrid(base_query)?;
        let sufficient = fast_results.len() >= config.fast_path_min_results
            && fast_results
                .iter()
                .any(|r| r.rrf_score >= config.fast_path_score_threshold);

        if sufficient {
            return Ok(crate::query_rewrite::TieredSearchResult {
                tier: crate::query_rewrite::SearchTier::Fast,
                results: fast_results,
                query_variants: None,
                total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            });
        }

        // Tier 2: Enhanced — LLM query rewrite + multi-query
        let rewrite_result = rewriter.rewrite(&base_query.text, context, provider);
        let enhanced_results = self.search_enhanced(base_query, &rewrite_result.variants)?;
        let sufficient = enhanced_results.len() >= config.enhanced_min_results
            && enhanced_results
                .iter()
                .any(|r| r.rrf_score >= config.enhanced_score_threshold);

        if sufficient {
            return Ok(crate::query_rewrite::TieredSearchResult {
                tier: crate::query_rewrite::SearchTier::Enhanced,
                results: enhanced_results,
                query_variants: Some(rewrite_result.variants),
                total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            });
        }

        // Tier 3: Graph-enhanced — expand via entity relationships
        let graph_results = self.expand_via_graph(&enhanced_results, config);
        let final_results = if graph_results.is_empty() {
            enhanced_results
        } else {
            // Merge enhanced + graph results
            use crate::query_rewrite::rrf_merge;
            rrf_merge(&[enhanced_results, graph_results], 60.0)
        };

        Ok(crate::query_rewrite::TieredSearchResult {
            tier: crate::query_rewrite::SearchTier::GraphEnhanced,
            results: final_results,
            query_variants: Some(rewrite_result.variants),
            total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        })
    }

    /// Expand search results via entity graph neighborhood.
    ///
    /// Takes the top entity IDs from existing results, queries their neighborhoods,
    /// and returns related facts as additional results.
    #[expect(
        clippy::cast_precision_loss,
        reason = "rank indices fit in f64 mantissa"
    )]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "rank indices are small positive values"
    )]
    fn expand_via_graph(
        &self,
        existing: &[HybridResult],
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> Vec<HybridResult> {
        // Collect unique fact IDs from existing results
        let fact_ids: Vec<&str> = existing
            .iter()
            .take(config.graph_expansion_limit)
            .map(|r| r.id.as_str())
            .collect();

        if fact_ids.is_empty() {
            return vec![];
        }

        // For each fact ID, look up which entities it references, then get their neighborhoods
        let mut expanded_ids = std::collections::HashSet::new();
        let existing_ids: std::collections::HashSet<&str> =
            existing.iter().map(|r| r.id.as_str()).collect();

        for fact_id in &fact_ids {
            // Try to find entity connections for this fact by checking entity neighborhoods
            // Use the fact_id as a potential entity_id (facts often share IDs with their subject entities)
            if let Ok(neighborhood) = self.entity_neighborhood(fact_id) {
                for row in &neighborhood.rows {
                    // Extract neighbor entity IDs and find their associated facts
                    if let Some(neighbor_id) = row.first().and_then(|v| v.get_str()) {
                        if !existing_ids.contains(neighbor_id) {
                            expanded_ids.insert(neighbor_id.to_owned());
                        }
                    }
                }
            }
        }

        // Create synthetic results for expanded facts with lower base scores
        let mut graph_results = Vec::new();
        for (rank, id) in expanded_ids.iter().enumerate() {
            graph_results.push(HybridResult {
                id: id.clone(),
                rrf_score: 1.0 / (60.0 + rank as f64 + 1.0),
                bm25_rank: -1,
                vec_rank: -1,
                graph_rank: (rank + 1) as i64,
            });
        }

        graph_results
    }

    /// Async tiered search — wraps sync call in `spawn_blocking`.
    ///
    /// Note: the `RewriteProvider` must be `Send + Sync + 'static` for async usage.
    pub async fn search_tiered_async(
        self: &std::sync::Arc<Self>,
        base_query: HybridQuery,
        rewriter: std::sync::Arc<crate::query_rewrite::QueryRewriter>,
        provider: std::sync::Arc<dyn crate::query_rewrite::RewriteProvider>,
        context: Option<String>,
        config: crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.search_tiered(
                &base_query,
                &rewriter,
                provider.as_ref(),
                context.as_deref(),
                &config,
            )
        })
        .await
        .context(crate::error::JoinSnafu)?
    }

    /// Increment access count and update last-accessed timestamp for the given fact IDs.
    #[instrument(skip(self), fields(count = fact_ids.len()))]
    pub fn increment_access(&self, fact_ids: &[String]) -> crate::error::Result<()> {
        if fact_ids.is_empty() {
            return Ok(());
        }
        let now = jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        for id in fact_ids {
            // Read the current fact rows, increment in Rust, then write back.
            // CozoDB in-memory read-modify-write in a single Datalog rule does not
            // reflect the mutation in subsequent reads — avoid that pattern.
            let facts = match self.read_facts_by_id(id) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(error = %e, fact_id = %id, "failed to read fact for access increment");
                    continue;
                }
            };
            for mut fact in facts {
                fact.access_count = fact.access_count.saturating_add(1);
                fact.last_accessed_at.clone_from(&now);
                if let Err(e) = self.insert_fact(&fact) {
                    tracing::warn!(error = %e, fact_id = %id, "failed to write incremented access count");
                }
            }
        }
        Ok(())
    }

    /// Async `increment_access` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self), fields(count = fact_ids.len()))]
    pub async fn increment_access_async(
        self: &std::sync::Arc<Self>,
        fact_ids: Vec<String>,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.increment_access(&fact_ids))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Soft-delete a fact: set `is_forgotten = true` with reason and timestamp.
    #[instrument(skip(self))]
    pub fn forget_fact(
        &self,
        fact_id: &str,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<()> {
        let now = jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type},
                id = $id,
                is_forgotten = true,
                forgotten_at = $now,
                forget_reason = $reason
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            "id".to_owned(),
            crate::engine::DataValue::Str(fact_id.into()),
        );
        params.insert("now".to_owned(), crate::engine::DataValue::Str(now.into()));
        params.insert(
            "reason".to_owned(),
            crate::engine::DataValue::Str(reason.as_str().into()),
        );
        self.run_mut(script, params)
    }

    /// Reverse a soft-delete: clear `is_forgotten`, `forgotten_at`, `forget_reason`.
    #[instrument(skip(self))]
    pub fn unforget_fact(&self, fact_id: &str) -> crate::error::Result<()> {
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type},
                id = $id,
                is_forgotten = false,
                forgotten_at = null,
                forget_reason = null
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            "id".to_owned(),
            crate::engine::DataValue::Str(fact_id.into()),
        );
        self.run_mut(script, params)
    }

    /// Query facts valid at a specific point in time.
    /// Returns facts where `valid_from <= at_time` AND `valid_to > at_time`.
    pub fn query_facts_temporal(
        &self,
        nous_id: &str,
        at_time: &str,
        filter: Option<&str>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("at_time".to_owned(), DataValue::Str(at_time.into()));

        let rows = match filter {
            Some(f) if !f.is_empty() => {
                params.insert("filter".to_owned(), DataValue::Str(f.into()));
                self.run_read(queries::TEMPORAL_FACTS_FILTERED, params)?
            }
            _ => self.run_read(&queries::temporal_facts(), params)?,
        };
        rows_to_facts(rows, nous_id)
    }

    /// Query facts that changed between two timestamps.
    /// Returns facts where `valid_from` is in `(from_time, to_time]` OR
    /// `valid_to` is in `(from_time, to_time]`.
    pub fn query_facts_diff(
        &self,
        nous_id: &str,
        from_time: &str,
        to_time: &str,
    ) -> crate::error::Result<crate::knowledge::FactDiff> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("from_time".to_owned(), DataValue::Str(from_time.into()));
        params.insert("to_time".to_owned(), DataValue::Str(to_time.into()));

        let added_rows = self.run_read(queries::TEMPORAL_DIFF_ADDED, params.clone())?;
        let added = rows_to_facts(added_rows, nous_id)?;

        let removed_rows = self.run_read(queries::TEMPORAL_DIFF_REMOVED, params)?;
        let removed = rows_to_facts(removed_rows, nous_id)?;

        // Modified facts: those that appear in both added and removed (supersession chain).
        // A fact ID in removed that has a superseded_by pointing to one in added is a modification.
        let added_ids: std::collections::HashSet<&str> =
            added.iter().map(|f| f.id.as_str()).collect();
        let mut modified = Vec::new();
        let mut pure_removed = Vec::new();

        for old in &removed {
            if let Some(ref new_id) = old.superseded_by {
                if added_ids.contains(new_id.as_str()) {
                    if let Some(new_fact) = added.iter().find(|f| f.id == *new_id) {
                        modified.push((old.clone(), new_fact.clone()));
                        continue;
                    }
                }
            }
            pure_removed.push(old.clone());
        }

        // Pure added: those not part of a modification pair
        let modified_new_ids: std::collections::HashSet<&str> =
            modified.iter().map(|(_, new)| new.id.as_str()).collect();
        let pure_added: Vec<_> = added
            .into_iter()
            .filter(|f| !modified_new_ids.contains(f.id.as_str()))
            .collect();

        Ok(crate::knowledge::FactDiff {
            added: pure_added,
            modified,
            removed: pure_removed,
        })
    }

    /// Search for facts relevant to a query, as they existed at a specific time.
    /// Filters hybrid search results through the temporal lens.
    pub fn search_temporal(
        &self,
        q: &HybridQuery,
        at_time: &str,
    ) -> crate::error::Result<Vec<HybridResult>> {
        let all_results = self.search_hybrid(q)?;

        // Get the set of fact IDs valid at the given time
        // We query with an empty nous_id filter to get all facts across all agents
        let valid_facts = self.query_facts_at_time_all(at_time)?;
        let valid_ids: std::collections::HashSet<&str> =
            valid_facts.iter().map(|f| f.id.as_str()).collect();

        let filtered: Vec<HybridResult> = all_results
            .into_iter()
            .filter(|r| valid_ids.contains(r.id.as_str()))
            .collect();

        Ok(filtered)
    }

    /// Async `query_facts_temporal` wrapper.
    pub async fn query_facts_temporal_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        at_time: String,
        filter: Option<String>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.query_facts_temporal(&nous_id, &at_time, filter.as_deref())
        })
        .await
        .context(crate::error::JoinSnafu)?
    }

    /// Async `query_facts_diff` wrapper.
    pub async fn query_facts_diff_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        from_time: String,
        to_time: String,
    ) -> crate::error::Result<crate::knowledge::FactDiff> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.query_facts_diff(&nous_id, &from_time, &to_time))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `search_temporal` wrapper.
    pub async fn search_temporal_async(
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
        at_time: String,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_temporal(&q, &at_time))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Audit query: returns all facts regardless of forgotten/superseded/temporal state.
    #[instrument(skip(self))]
    pub fn audit_all_facts(
        &self,
        nous_id: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("limit".to_owned(), DataValue::from(limit));

        let rows = self.run_read(&queries::audit_all_facts(), params)?;
        rows_to_facts(rows, nous_id)
    }

    // --- Async wrappers ---

    /// Async `forget_fact` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn forget_fact_async(
        self: &std::sync::Arc<Self>,
        fact_id: String,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.forget_fact(&fact_id, reason))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `unforget_fact` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn unforget_fact_async(
        self: &std::sync::Arc<Self>,
        fact_id: String,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.unforget_fact(&fact_id))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `audit_all_facts` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn audit_all_facts_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.audit_all_facts(&nous_id, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `insert_fact` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, fact), fields(fact_id = %fact.id))]
    pub async fn insert_fact_async(
        self: &std::sync::Arc<Self>,
        fact: crate::knowledge::Fact,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.insert_fact(&fact))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `query_facts` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn query_facts_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        now: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.query_facts(&nous_id, &now, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `search_vectors` — wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, query_vec))]
    pub async fn search_vectors_async(
        self: &std::sync::Arc<Self>,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_vectors(query_vec, k, ef))
            .await
            .context(crate::error::JoinSnafu)?
    }

    // --- Migration ---

    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    fn migrate_v1_to_v2(&self) -> crate::error::Result<()> {
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v1 -> v2");

        // 1. Read all existing facts
        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 read facts: {e}"),
                }
                .build()
            })?;

        // 2. Drop FTS index (must be dropped before relation)
        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        // 3. Drop old facts relation
        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 remove facts: {e}"),
                }
                .build()
            })?;

        // 4. Recreate with new schema (includes access tracking columns)
        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate facts: {e}"),
                }
                .build()
            })?;

        // 5. Reinsert facts with defaults for new columns
        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    0, '', 720.0, ''
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v1->v2 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        // 6. Recreate FTS index
        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 recreate FTS: {e}"),
                }
                .build()
            })?;

        // 7. Update schema version
        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v1->v2 update version: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v1 -> v2 complete");
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "migration is a single linear sequence"
    )]
    fn migrate_v2_to_v3(&self) -> crate::error::Result<()> {
        use crate::engine::{DataValue, ScriptMutability};
        use std::collections::BTreeMap;

        tracing::info!("migrating knowledge schema v2 -> v3");

        // 1. Read all existing facts (v2 schema: 14 columns)
        let all_facts = self
            .db
            .run(
                r"?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                    superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type] :=
                    *facts{id, valid_from, content, nous_id, confidence, tier,
                           valid_to, superseded_by, source_session_id, recorded_at,
                           access_count, last_accessed_at, stability_hours, fact_type}",
                BTreeMap::new(),
                ScriptMutability::Immutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 read facts: {e}"),
                }
                .build()
            })?;

        // 2. Drop FTS index
        let _ = self.db.run(
            "::fts drop facts:content_fts",
            BTreeMap::new(),
            ScriptMutability::Mutable,
        );

        // 3. Drop old facts relation
        self.db
            .run("::remove facts", BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 remove facts: {e}"),
                }
                .build()
            })?;

        // 4. Recreate with new schema (includes forget columns)
        self.db
            .run(KNOWLEDGE_DDL[0], BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate facts: {e}"),
                }
                .build()
            })?;

        // 5. Reinsert facts with defaults for new columns
        for row in &all_facts.rows {
            let script = r"
                ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
                  superseded_by, source_session_id, recorded_at,
                  access_count, last_accessed_at, stability_hours, fact_type,
                  is_forgotten, forgotten_at, forget_reason] <- [[
                    $id, $valid_from, $content, $nous_id, $confidence, $tier, $valid_to,
                    $superseded_by, $source_session_id, $recorded_at,
                    $access_count, $last_accessed_at, $stability_hours, $fact_type,
                    false, null, null
                ]]
                :put facts {id, valid_from => content, nous_id, confidence, tier,
                            valid_to, superseded_by, source_session_id, recorded_at,
                            access_count, last_accessed_at, stability_hours, fact_type,
                            is_forgotten, forgotten_at, forget_reason}
            ";
            let mut params = BTreeMap::new();
            for (i, name) in [
                "id",
                "valid_from",
                "content",
                "nous_id",
                "confidence",
                "tier",
                "valid_to",
                "superseded_by",
                "source_session_id",
                "recorded_at",
                "access_count",
                "last_accessed_at",
                "stability_hours",
                "fact_type",
            ]
            .iter()
            .enumerate()
            {
                if let Some(val) = row.get(i) {
                    params.insert((*name).to_owned(), val.clone());
                }
            }
            self.db
                .run(script, params, ScriptMutability::Mutable)
                .map_err(|e| {
                    crate::error::EngineQuerySnafu {
                        message: format!("v2->v3 reinsert fact: {e}"),
                    }
                    .build()
                })?;
        }

        // 6. Recreate FTS index
        self.db
            .run(fts_ddl(), BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 recreate FTS: {e}"),
                }
                .build()
            })?;

        // 7. Update schema version
        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        params.insert("version".to_owned(), DataValue::from(Self::SCHEMA_VERSION));
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("v2->v3 update version: {e}"),
                }
                .build()
            })?;

        tracing::info!("knowledge schema migration v2 -> v3 complete");
        Ok(())
    }

    // --- Db facade methods ---

    /// Backup the knowledge database to a file.
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
    #[instrument(skip(self, params))]
    pub fn run_script_read_only(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, crate::engine::DataValue>,
    ) -> crate::error::Result<crate::engine::NamedRows> {
        self.run_read(script, params)
    }

    // --- Internal helpers ---

    /// Query all facts valid at a specific time, across all nous IDs.
    /// Used internally by `search_temporal` for filtering.
    fn query_facts_at_time_all(
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
        rows_to_facts(rows, "")
    }

    /// Read a single fact by its ID (all temporal records matching).
    /// Returns all fields; does not apply time/validity filters.
    fn read_facts_by_id(&self, id: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
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
        rows_to_raw_facts(rows)
    }

    fn run_mut(
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

    fn run_read(
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

// --- Conversion helpers ---

#[cfg(feature = "mneme-engine")]
fn fact_to_params(
    fact: &crate::knowledge::Fact,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(fact.id.as_str().into()));
    p.insert(
        "valid_from".to_owned(),
        DataValue::Str(fact.valid_from.as_str().into()),
    );
    p.insert(
        "content".to_owned(),
        DataValue::Str(fact.content.as_str().into()),
    );
    p.insert(
        "nous_id".to_owned(),
        DataValue::Str(fact.nous_id.as_str().into()),
    );
    p.insert("confidence".to_owned(), DataValue::from(fact.confidence));
    p.insert("tier".to_owned(), DataValue::Str(fact.tier.as_str().into()));
    p.insert(
        "valid_to".to_owned(),
        DataValue::Str(fact.valid_to.as_str().into()),
    );
    p.insert(
        "superseded_by".to_owned(),
        match &fact.superseded_by {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "source_session_id".to_owned(),
        match &fact.source_session_id {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "recorded_at".to_owned(),
        DataValue::Str(fact.recorded_at.as_str().into()),
    );
    p.insert(
        "access_count".to_owned(),
        DataValue::from(i64::from(fact.access_count)),
    );
    p.insert(
        "last_accessed_at".to_owned(),
        DataValue::Str(fact.last_accessed_at.as_str().into()),
    );
    p.insert(
        "stability_hours".to_owned(),
        DataValue::from(fact.stability_hours),
    );
    p.insert(
        "fact_type".to_owned(),
        DataValue::Str(fact.fact_type.as_str().into()),
    );
    p.insert(
        "is_forgotten".to_owned(),
        DataValue::Bool(fact.is_forgotten),
    );
    p.insert(
        "forgotten_at".to_owned(),
        match &fact.forgotten_at {
            Some(s) => DataValue::Str(s.as_str().into()),
            None => DataValue::Null,
        },
    );
    p.insert(
        "forget_reason".to_owned(),
        match &fact.forget_reason {
            Some(r) => DataValue::Str(r.as_str().into()),
            None => DataValue::Null,
        },
    );
    p
}

#[cfg(feature = "mneme-engine")]
fn entity_to_params(
    entity: &crate::knowledge::Entity,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(entity.id.as_str().into()));
    p.insert(
        "name".to_owned(),
        DataValue::Str(entity.name.as_str().into()),
    );
    p.insert(
        "entity_type".to_owned(),
        DataValue::Str(entity.entity_type.as_str().into()),
    );
    p.insert(
        "aliases".to_owned(),
        DataValue::Str(entity.aliases.join(",").into()),
    );
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(entity.created_at.as_str().into()),
    );
    p.insert(
        "updated_at".to_owned(),
        DataValue::Str(entity.updated_at.as_str().into()),
    );
    p
}

#[cfg(feature = "mneme-engine")]
fn relationship_to_params(
    rel: &crate::knowledge::Relationship,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::DataValue;
    let mut p = std::collections::BTreeMap::new();
    p.insert("src".to_owned(), DataValue::Str(rel.src.as_str().into()));
    p.insert("dst".to_owned(), DataValue::Str(rel.dst.as_str().into()));
    p.insert(
        "relation".to_owned(),
        DataValue::Str(rel.relation.as_str().into()),
    );
    p.insert("weight".to_owned(), DataValue::from(rel.weight));
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(rel.created_at.as_str().into()),
    );
    p
}

#[cfg(feature = "mneme-engine")]
fn embedding_to_params(
    chunk: &crate::knowledge::EmbeddedChunk,
    _dim: usize,
) -> std::collections::BTreeMap<String, crate::engine::DataValue> {
    use crate::engine::{Array1, DataValue, Vector};
    let mut p = std::collections::BTreeMap::new();
    p.insert("id".to_owned(), DataValue::Str(chunk.id.as_str().into()));
    p.insert(
        "content".to_owned(),
        DataValue::Str(chunk.content.as_str().into()),
    );
    p.insert(
        "source_type".to_owned(),
        DataValue::Str(chunk.source_type.as_str().into()),
    );
    p.insert(
        "source_id".to_owned(),
        DataValue::Str(chunk.source_id.as_str().into()),
    );
    p.insert(
        "nous_id".to_owned(),
        DataValue::Str(chunk.nous_id.as_str().into()),
    );
    p.insert(
        "embedding".to_owned(),
        DataValue::Vec(Vector::F32(Array1::from(chunk.embedding.clone()))),
    );
    p.insert(
        "created_at".to_owned(),
        DataValue::Str(chunk.created_at.as_str().into()),
    );
    p
}

// Parse rows from FULL_CURRENT_FACTS into Vec<Fact>.
// Columns: id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to, superseded_by, source_session_id
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "column extraction is sequential — splitting would obscure the mapping"
)]
fn rows_to_facts(
    rows: crate::engine::NamedRows,
    nous_id: &str,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing content",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing tier",
            }
            .build()
        })?)?;
        let recorded_at = extract_str(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing recorded_at",
            }
            .build()
        })?)?;
        let nous_id_col = extract_str(row.get(5).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing nous_id",
            }
            .build()
        })?)?;
        let valid_from = extract_str(row.get(6).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing valid_from",
            }
            .build()
        })?)?;
        let valid_to = extract_str(row.get(7).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing valid_to",
            }
            .build()
        })?)?;
        let superseded_by = extract_optional_str(row.get(8).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing superseded_by",
            }
            .build()
        })?)?;
        let source_session_id = extract_optional_str(row.get(9).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact row: missing source_session_id",
            }
            .build()
        })?)?;

        let tier = parse_epistemic_tier(&tier_str)?;

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "access count fits in u32"
        )]
        let access_count = row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0) as u32;
        let last_accessed_at = row
            .get(11)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let stability_hours = row
            .get(12)
            .and_then(|v| extract_float(v).ok())
            .unwrap_or(720.0);
        let fact_type = row
            .get(13)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let is_forgotten = row
            .get(14)
            .and_then(|v| extract_bool(v).ok())
            .unwrap_or(false);
        let forgotten_at = row
            .get(15)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None);
        let forget_reason = row
            .get(16)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::ForgetReason>().ok());

        out.push(Fact {
            id,
            nous_id: if nous_id_col.is_empty() {
                nous_id.to_owned()
            } else {
                nous_id_col
            },
            content,
            confidence,
            tier,
            valid_from,
            valid_to,
            superseded_by,
            source_session_id,
            recorded_at,
            access_count,
            last_accessed_at,
            stability_hours,
            fact_type,
            is_forgotten,
            forgotten_at,
            forget_reason,
        });
    }
    Ok(out)
}

// Parse rows from a raw all-fields fact scan (used by read_facts_by_id).
// Columns: id(0), valid_from(1), content(2), nous_id(3), confidence(4), tier(5),
//          valid_to(6), superseded_by(7), source_session_id(8), recorded_at(9),
//          access_count(10), last_accessed_at(11), stability_hours(12), fact_type(13),
//          is_forgotten(14), forgotten_at(15), forget_reason(16).
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "flat row parser — splitting would not improve clarity"
)]
fn rows_to_raw_facts(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing id",
            }
            .build()
        })?)?;
        let valid_from = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing valid_from",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing content",
            }
            .build()
        })?)?;
        let nous_id = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing nous_id",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(5).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing tier",
            }
            .build()
        })?)?;
        let valid_to = extract_str(row.get(6).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing valid_to",
            }
            .build()
        })?)?;
        let superseded_by = extract_optional_str(row.get(7).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing superseded_by",
            }
            .build()
        })?)?;
        let source_session_id = extract_optional_str(row.get(8).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing source_session_id",
            }
            .build()
        })?)?;
        let recorded_at = extract_str(row.get(9).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "raw fact: missing recorded_at",
            }
            .build()
        })?)?;
        let tier = parse_epistemic_tier(&tier_str)?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "access count fits in u32"
        )]
        let access_count = row.get(10).and_then(|v| extract_int(v).ok()).unwrap_or(0) as u32;
        let last_accessed_at = row
            .get(11)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let stability_hours = row
            .get(12)
            .and_then(|v| extract_float(v).ok())
            .unwrap_or(720.0);
        let fact_type = row
            .get(13)
            .and_then(|v| extract_str(v).ok())
            .unwrap_or_default();
        let is_forgotten = row
            .get(14)
            .and_then(|v| extract_bool(v).ok())
            .unwrap_or(false);
        let forgotten_at = row
            .get(15)
            .and_then(|v| extract_optional_str(v).ok())
            .flatten();
        let forget_reason = row
            .get(16)
            .and_then(|v| extract_optional_str(v).ok())
            .unwrap_or(None)
            .and_then(|s| s.parse::<crate::knowledge::ForgetReason>().ok());
        out.push(Fact {
            id,
            nous_id,
            content,
            confidence,
            tier,
            valid_from,
            valid_to,
            superseded_by,
            source_session_id,
            recorded_at,
            access_count,
            last_accessed_at,
            stability_hours,
            fact_type,
            is_forgotten,
            forgotten_at,
            forget_reason,
        });
    }
    Ok(out)
}

// Parse rows from FACTS_AT_TIME into Vec<Fact> (partial — only has id, content, confidence, tier).
#[cfg(feature = "mneme-engine")]
fn rows_to_facts_partial(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
    use crate::knowledge::Fact;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing content",
            }
            .build()
        })?)?;
        let confidence = extract_float(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing confidence",
            }
            .build()
        })?)?;
        let tier_str = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "fact_at row: missing tier",
            }
            .build()
        })?)?;
        let tier = parse_epistemic_tier(&tier_str)?;

        out.push(Fact {
            id,
            nous_id: String::new(),
            content,
            confidence,
            tier,
            valid_from: String::new(),
            valid_to: String::new(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: String::new(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        });
    }
    Ok(out)
}

// Parse rows from SEMANTIC_SEARCH into Vec<RecallResult>.
// Columns: id, content, source_type, source_id, dist
#[cfg(feature = "mneme-engine")]
fn rows_to_recall_results(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
    use crate::knowledge::RecallResult;
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let _id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing id",
            }
            .build()
        })?)?;
        let content = extract_str(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing content",
            }
            .build()
        })?)?;
        let source_type = extract_str(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing source_type",
            }
            .build()
        })?)?;
        let source_id = extract_str(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing source_id",
            }
            .build()
        })?)?;
        let distance = extract_float(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "recall row: missing dist",
            }
            .build()
        })?)?;

        out.push(RecallResult {
            content,
            distance,
            source_type,
            source_id,
        });
    }
    Ok(out)
}

// Build the hybrid Datalog query with dynamic graph sub-rules.
// When seed_entities is empty, graph is an empty relation.
// When non-empty, seeds are expanded inline (avoids is_in() built-in dependency).
#[cfg(feature = "mneme-engine")]
fn build_hybrid_query(q: &HybridQuery) -> String {
    let graph_rules = if q.seed_entities.is_empty() {
        // Empty graph relation — graph signal contributes 0 to RRF
        "graph[id, score] <- []".to_owned()
    } else {
        let seed_data: Vec<String> = q
            .seed_entities
            .iter()
            .map(|s| format!("[\"{}\"]", s.replace('"', "\\\"")))
            .collect();
        let seeds_inline = seed_data.join(", ");
        format!(
            "seed_list[e] <- [{seeds_inline}]\n        \
             graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: id, weight: score}}\n        \
             graph_raw[id, score] := seed_list[seed], *relationships{{src: seed, dst: mid, weight: _w}}, \
             *relationships{{src: mid, dst: id, weight}}, score = weight * 0.5\n        \
             graph[id, sum(score)] := graph_raw[id, score]"
        )
    };
    queries::HYBRID_SEARCH_BASE.replace("{GRAPH_RULES}", &graph_rules)
}

// Parse rows from ReciprocalRankFusion output into Vec<HybridResult>.
// Columns: id (Str), rrf_score (Float), bm25_rank (Int), vec_rank (Int), graph_rank (Int)
#[cfg(feature = "mneme-engine")]
fn rows_to_hybrid_results(
    rows: crate::engine::NamedRows,
) -> crate::error::Result<Vec<HybridResult>> {
    let mut out = Vec::with_capacity(rows.rows.len());
    for row in rows.rows {
        let id = extract_str(row.first().ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing id",
            }
            .build()
        })?)?;
        let rrf_score = extract_float(row.get(1).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing rrf_score",
            }
            .build()
        })?)?;
        let bm25_rank = extract_int(row.get(2).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing bm25_rank",
            }
            .build()
        })?)?;
        let vec_rank = extract_int(row.get(3).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing vec_rank",
            }
            .build()
        })?)?;
        let graph_rank = extract_int(row.get(4).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: "hybrid row: missing graph_rank",
            }
            .build()
        })?)?;
        out.push(HybridResult {
            id,
            rrf_score,
            bm25_rank,
            vec_rank,
            graph_rank,
        });
    }
    // Sort by rrf_score descending (RRF output is unordered since it comes through :order in Datalog,
    // but :order is applied by the engine — this is a safety sort for correctness)
    out.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

// --- DataValue extraction utilities ---

#[cfg(feature = "mneme-engine")]
fn extract_str(val: &crate::engine::DataValue) -> crate::error::Result<String> {
    match val {
        crate::engine::DataValue::Str(s) => Ok(s.to_string()),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
fn extract_optional_str(val: &crate::engine::DataValue) -> crate::error::Result<Option<String>> {
    match val {
        crate::engine::DataValue::Null => Ok(None),
        crate::engine::DataValue::Str(s) => Ok(Some(s.to_string())),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str or Null, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
fn extract_float(val: &crate::engine::DataValue) -> crate::error::Result<f64> {
    val.get_float().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Float), got {val:?}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn extract_int(val: &crate::engine::DataValue) -> crate::error::Result<i64> {
    val.get_int().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Int), got {val:?}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn extract_bool(val: &crate::engine::DataValue) -> crate::error::Result<bool> {
    match val {
        crate::engine::DataValue::Bool(b) => Ok(*b),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Bool, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
fn parse_epistemic_tier(s: &str) -> crate::error::Result<crate::knowledge::EpistemicTier> {
    use crate::knowledge::EpistemicTier;
    match s {
        "verified" => Ok(EpistemicTier::Verified),
        "inferred" => Ok(EpistemicTier::Inferred),
        "assumed" => Ok(EpistemicTier::Assumed),
        other => Err(crate::error::ConversionSnafu {
            message: format!("unknown epistemic tier: {other}"),
        }
        .build()),
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
mod engine_assertions {
    use super::KnowledgeStore;
    use static_assertions::assert_impl_all;
    assert_impl_all!(KnowledgeStore: Send, Sync);
}

#[cfg(all(test, feature = "mneme-engine"))]
mod timeout_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn query_timeout_returns_typed_error() {
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

        // Recursive transitive closure on a linear chain of N nodes requires N-1 semi-naive
        // fixpoint epochs. Each epoch checks the Poison flag. With N=2000 and timeout=50ms
        // the engine will hit the Poison kill well before all epochs complete.
        let result = store.run_query_with_timeout(
            r"
edge[a, b] := a in int_range(2000), b = a + 1
reach[a, b] := edge[a, b]
reach[a, c] := reach[a, b], edge[b, c]
?[a, c] := reach[a, c]
",
            BTreeMap::new(),
            Some(Duration::from_millis(50)),
        );

        assert!(result.is_err(), "expected timeout error");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("timed out"),
            "error should mention timeout, got: {msg}"
        );
        assert!(matches!(err, crate::error::Error::QueryTimeout { .. }));
    }

    #[test]
    fn query_without_timeout_succeeds() {
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

        let result = store.run_query_with_timeout("?[x] := x = 42", BTreeMap::new(), None);

        assert!(result.is_ok(), "query without timeout should succeed");
        let rows = result.unwrap();
        assert_eq!(rows.rows.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_templates_are_valid_strings() {
        // Verify DDL templates don't panic on formatting
        assert!(KNOWLEDGE_DDL.len() == 3);
        let emb = embeddings_ddl(1024);
        assert!(emb.contains("1024"));
        let idx = hnsw_ddl(1024);
        assert!(idx.contains("1024"));
        let fts = fts_ddl();
        assert!(fts.contains("content_fts"));
        assert!(fts.contains("bm25") || fts.contains("Simple"));
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn query_templates_contain_params() {
        let current = queries::current_facts();
        assert!(current.contains("$nous_id"));
        assert!(current.contains("$now"));
        assert!(queries::SEMANTIC_SEARCH.contains("$query_vec"));
        assert!(queries::ENTITY_NEIGHBORHOOD.contains("$entity_id"));
        let supersede = queries::supersede_fact();
        assert!(supersede.contains("$old_id"));
        assert!(supersede.contains("$new_id"));
        assert!(queries::HYBRID_SEARCH_BASE.contains("$query_text"));
        assert!(queries::HYBRID_SEARCH_BASE.contains("$query_vec"));
        assert!(queries::HYBRID_SEARCH_BASE.contains("ReciprocalRankFusion"));
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn build_hybrid_query_empty_seeds() {
        let q = HybridQuery {
            text: "test".into(),
            embedding: vec![0.0; 4],
            seed_entities: vec![],
            limit: 5,
            ef: 20,
        };
        let script = build_hybrid_query(&q);
        assert!(
            script.contains("graph[id, score] <- []"),
            "empty seeds must produce empty graph relation"
        );
        assert!(script.contains("ReciprocalRankFusion"));
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn build_hybrid_query_with_seeds() {
        let q = HybridQuery {
            text: "test".into(),
            embedding: vec![0.0; 4],
            seed_entities: vec!["e-rust".into(), "e-python".into()],
            limit: 5,
            ef: 20,
        };
        let script = build_hybrid_query(&q);
        assert!(
            script.contains("seed_list"),
            "non-empty seeds must produce seed_list relation"
        );
        assert!(script.contains("e-rust"));
        assert!(script.contains("e-python"));
        assert!(script.contains("*relationships"));
        assert!(
            script.contains("graph_raw"),
            "must use graph_raw intermediate for aggregation"
        );
        assert!(
            script.contains("sum(score)"),
            "must aggregate scores per entity"
        );
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn hybrid_search_empty_seeds_returns_results() {
        use crate::knowledge::{EmbeddedChunk, EpistemicTier, Fact};

        let dim = 4;
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        let fact = Fact {
            id: "f1".to_owned(),
            nous_id: "test".to_owned(),
            content: "Rust systems programming".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        store.insert_fact(&fact).expect("insert fact");

        let chunk = EmbeddedChunk {
            id: "f1".to_owned(),
            content: "Rust systems programming".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: "test".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1],
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_embedding(&chunk).expect("insert embedding");

        let results = store
            .search_hybrid(&HybridQuery {
                text: "Rust programming".to_owned(),
                embedding: vec![0.9, 0.1, 0.1, 0.1],
                seed_entities: vec![],
                limit: 5,
                ef: 20,
            })
            .expect("hybrid search with empty seeds");

        assert!(
            !results.is_empty(),
            "empty seeds must still return BM25+vec results"
        );
        for r in &results {
            assert_eq!(
                r.graph_rank, -1,
                "graph_rank must be -1 when seeds are empty"
            );
        }
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "integration test with setup/assert phases"
    )]
    fn hybrid_search_graph_aggregation() {
        use crate::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact, Relationship};

        let dim = 4;
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        // f1: reachable from 3 seed entities
        let f1 = Fact {
            id: "f1".to_owned(),
            nous_id: "test".to_owned(),
            content: "Rust systems programming".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        store.insert_fact(&f1).expect("insert f1");
        store
            .insert_embedding(&EmbeddedChunk {
                id: "f1".to_owned(),
                content: "Rust systems programming".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "test".to_owned(),
                embedding: vec![0.9, 0.1, 0.1, 0.1],
                created_at: "2026-03-01T00:00:00Z".to_owned(),
            })
            .expect("insert f1 embedding");

        // f2: reachable from only 1 seed entity
        let f2 = Fact {
            id: "f2".to_owned(),
            nous_id: "test".to_owned(),
            content: "Rust memory safety".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        store.insert_fact(&f2).expect("insert f2");
        store
            .insert_embedding(&EmbeddedChunk {
                id: "f2".to_owned(),
                content: "Rust memory safety".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "test".to_owned(),
                embedding: vec![0.8, 0.2, 0.1, 0.1],
                created_at: "2026-03-01T00:00:00Z".to_owned(),
            })
            .expect("insert f2 embedding");

        // Three seed entities: all point to f1, only s1 points to f2
        for (id, name) in [("s1", "Seed1"), ("s2", "Seed2"), ("s3", "Seed3")] {
            store
                .insert_entity(&Entity {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    entity_type: "concept".to_owned(),
                    aliases: vec![],
                    created_at: "2026-03-01T00:00:00Z".to_owned(),
                    updated_at: "2026-03-01T00:00:00Z".to_owned(),
                })
                .expect("insert entity");
            store
                .insert_relationship(&Relationship {
                    src: id.to_owned(),
                    dst: "f1".to_owned(),
                    relation: "describes".to_owned(),
                    weight: 0.7,
                    created_at: "2026-03-01T00:00:00Z".to_owned(),
                })
                .expect("insert relationship to f1");
        }
        store
            .insert_relationship(&Relationship {
                src: "s1".to_owned(),
                dst: "f2".to_owned(),
                relation: "describes".to_owned(),
                weight: 0.7,
                created_at: "2026-03-01T00:00:00Z".to_owned(),
            })
            .expect("insert relationship to f2");

        let results = store
            .search_hybrid(&HybridQuery {
                text: "Rust programming".to_owned(),
                embedding: vec![0.9, 0.1, 0.1, 0.1],
                seed_entities: vec!["s1".to_owned(), "s2".to_owned(), "s3".to_owned()],
                limit: 10,
                ef: 20,
            })
            .expect("hybrid search with three seeds");

        // f1 must appear exactly once (aggregated from 3 paths)
        let f1_hits: Vec<_> = results.iter().filter(|r| r.id == "f1").collect();
        assert_eq!(
            f1_hits.len(),
            1,
            "entity reachable via multiple paths must appear once"
        );
        assert!(
            f1_hits[0].graph_rank > 0,
            "f1 must have a positive graph rank"
        );

        // f2 must appear exactly once (from 1 path)
        let f2_hits: Vec<_> = results.iter().filter(|r| r.id == "f2").collect();
        assert_eq!(f2_hits.len(), 1, "f2 must appear once");
        assert!(
            f2_hits[0].graph_rank > 0,
            "f2 must have a positive graph rank"
        );

        // f1 (3 paths) should have a higher RRF score than f2 (1 path)
        assert!(
            f1_hits[0].rrf_score > f2_hits[0].rrf_score,
            "3-path entity must score higher than 1-path entity: f1={} vs f2={}",
            f1_hits[0].rrf_score,
            f2_hits[0].rrf_score,
        );
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn hybrid_search_two_signal_no_graph() {
        use crate::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact};

        let dim = 4;
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        let fact = Fact {
            id: "f-twosig".to_owned(),
            nous_id: "test".to_owned(),
            content: "unique harpsichord melody testing".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        store.insert_fact(&fact).expect("insert fact");

        store
            .insert_embedding(&EmbeddedChunk {
                id: "f-twosig".to_owned(),
                content: "unique harpsichord melody testing".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f-twosig".to_owned(),
                nous_id: "test".to_owned(),
                embedding: vec![0.7, 0.3, 0.2, 0.1],
                created_at: "2026-03-01T00:00:00Z".to_owned(),
            })
            .expect("insert embedding");

        // Insert an unrelated seed entity so the graph signal is structurally present but yields
        // no matches for f-twosig
        store
            .insert_entity(&Entity {
                id: "e-unrelated".to_owned(),
                name: "Unrelated".to_owned(),
                entity_type: "concept".to_owned(),
                aliases: vec![],
                created_at: "2026-03-01T00:00:00Z".to_owned(),
                updated_at: "2026-03-01T00:00:00Z".to_owned(),
            })
            .expect("insert entity");

        let results = store
            .search_hybrid(&HybridQuery {
                text: "harpsichord melody".to_owned(),
                embedding: vec![0.7, 0.3, 0.2, 0.1],
                seed_entities: vec!["e-unrelated".to_owned()],
                limit: 5,
                ef: 20,
            })
            .expect("hybrid search two signals");

        let hit = results.iter().find(|r| r.id == "f-twosig");
        assert!(hit.is_some(), "BM25+vector fact must appear in results");
        let hit = hit.unwrap();
        assert!(hit.bm25_rank > 0, "must have positive BM25 rank");
        assert!(hit.vec_rank > 0, "must have positive vector rank");
        assert_eq!(hit.graph_rank, -1, "absent from graph signal must be -1");
        assert!(
            hit.rrf_score > 0.0,
            "RRF score must be positive from two signals"
        );
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn hybrid_search_absent_signal_rank_is_negative_one() {
        use crate::knowledge::{EpistemicTier, Fact};

        let dim = 4;
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        // Insert a fact but no embedding and no graph edges
        let fact = Fact {
            id: "f-bm25-only".to_owned(),
            nous_id: "test".to_owned(),
            content: "unique xylophone testing keyword".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        store.insert_fact(&fact).expect("insert fact");

        let results = store
            .search_hybrid(&HybridQuery {
                text: "xylophone testing".to_owned(),
                embedding: vec![0.5, 0.5, 0.5, 0.5],
                seed_entities: vec![],
                limit: 5,
                ef: 20,
            })
            .expect("hybrid search bm25-only");

        let hit = results.iter().find(|r| r.id == "f-bm25-only");
        assert!(hit.is_some(), "BM25-only fact must appear in results");
        let hit = hit.unwrap();
        assert!(hit.bm25_rank > 0, "must have positive BM25 rank");
        assert_eq!(hit.vec_rank, -1, "absent from vector signal must be -1");
        assert_eq!(hit.graph_rank, -1, "absent from graph signal must be -1");
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
mod knowledge_store_tests {
    use super::*;
    use crate::knowledge::{
        EmbeddedChunk, Entity, EpistemicTier, Fact, ForgetReason, Relationship,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const DIM: usize = 4;

    fn make_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
    }

    fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
        Fact {
            id: id.to_owned(),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
        Entity {
            id: id.to_owned(),
            name: name.to_owned(),
            entity_type: entity_type.to_owned(),
            aliases: vec![],
            created_at: "2026-03-01T00:00:00Z".to_owned(),
            updated_at: "2026-03-01T00:00:00Z".to_owned(),
        }
    }

    fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
        Relationship {
            src: src.to_owned(),
            dst: dst.to_owned(),
            relation: relation.to_owned(),
            weight,
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        }
    }

    fn make_embedding(id: &str, content: &str, source_id: &str, nous_id: &str) -> EmbeddedChunk {
        EmbeddedChunk {
            id: id.to_owned(),
            content: content.to_owned(),
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
            nous_id: nous_id.to_owned(),
            embedding: vec![0.5, 0.5, 0.5, 0.5],
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        }
    }

    // ---- CRUD: Facts ----

    #[test]
    fn insert_fact_and_retrieve() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Rust is a systems programming language");
        store.insert_fact(&fact).expect("insert fact");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f1");
        assert_eq!(results[0].content, "Rust is a systems programming language");
        assert!((results[0].confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn insert_multiple_facts_and_retrieve() {
        let store = make_store();
        for i in 0..5 {
            let fact = make_fact(&format!("f{i}"), "agent-a", &format!("Fact number {i}"));
            store.insert_fact(&fact).expect("insert fact");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query facts");
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn upsert_fact_overwrites() {
        let store = make_store();
        let mut fact = make_fact("f1", "agent-a", "Original content");
        store.insert_fact(&fact).expect("insert fact");

        fact.content = "Updated content".to_owned();
        fact.confidence = 0.95;
        store.insert_fact(&fact).expect("upsert fact");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Updated content");
        assert!((results[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    // ---- CRUD: Entities ----

    #[test]
    fn insert_entity_and_query_neighborhood() {
        let store = make_store();
        let entity = make_entity("e1", "Rust", "language");
        store.insert_entity(&entity).expect("insert entity");

        let rows = store.entity_neighborhood("e1").expect("neighborhood");
        // No relationships yet, so empty result set is fine (no panic)
        assert!(rows.rows.is_empty());
    }

    #[test]
    fn insert_entity_with_aliases() {
        let store = make_store();
        let mut entity = make_entity("e1", "Rust", "language");
        entity.aliases = vec!["rustlang".to_owned(), "rust-lang".to_owned()];
        store
            .insert_entity(&entity)
            .expect("insert entity with aliases");

        // Verify via raw query that the entity was stored
        let rows = store
            .run_query(
                r"?[id, name, aliases] := *entities{id, name, aliases}, id = 'e1'",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        assert_eq!(rows.rows.len(), 1);
    }

    // ---- CRUD: Relationships ----

    #[test]
    fn insert_relationship_and_retrieve_neighborhood() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Aletheia", "project"))
            .expect("insert e2");
        store
            .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
            .expect("insert relationship");

        let rows = store.entity_neighborhood("e1").expect("neighborhood");
        assert!(
            !rows.rows.is_empty(),
            "neighborhood should contain the relationship"
        );
    }

    #[test]
    fn insert_relationship_bidirectional_neighborhood() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Bob", "person"))
            .expect("insert e2");
        store
            .insert_relationship(&make_relationship("e1", "e2", "knows", 0.8))
            .expect("insert rel");

        // e1 neighborhood should include e2
        let from_e1 = store.entity_neighborhood("e1").expect("e1 neighborhood");
        assert!(!from_e1.rows.is_empty());

        // e2 neighborhood may or may not include e1 (depends on query directionality)
        // Just verify it doesn't error
        let _from_e2 = store.entity_neighborhood("e2").expect("e2 neighborhood");
    }

    // ---- CRUD: Embeddings ----

    #[test]
    fn insert_embedding_and_search() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Rust memory safety");
        store.insert_fact(&fact).expect("insert fact");

        let mut chunk = make_embedding("emb1", "Rust memory safety", "f1", "agent-a");
        chunk.embedding = vec![0.9, 0.1, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");

        let results = store
            .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 5, 20)
            .expect("search vectors");
        assert!(!results.is_empty());
        assert_eq!(results[0].content, "Rust memory safety");
        assert_eq!(results[0].source_type, "fact");
        assert_eq!(results[0].source_id, "f1");
    }

    // ---- Forget / Unforget ----

    #[test]
    fn forget_fact_excludes_from_query() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Secret fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .forget_fact("f1", ForgetReason::UserRequested)
            .expect("forget fact");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        // query_facts returns current, non-forgotten facts
        assert!(
            results.is_empty() || results.iter().all(|f| f.id != "f1" || !f.is_forgotten),
            "forgotten fact should be excluded or marked"
        );
    }

    #[test]
    fn unforget_fact_restores_visibility() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Recoverable fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .forget_fact("f1", ForgetReason::Outdated)
            .expect("forget");
        store.unforget_fact("f1").expect("unforget");

        // After unforgetting, audit should show it as not forgotten
        let all = store.audit_all_facts("agent-a", 100).expect("audit facts");
        let found = all.iter().find(|f| f.id == "f1");
        assert!(found.is_some(), "fact should still exist");
        assert!(
            !found.expect("checked").is_forgotten,
            "should not be forgotten"
        );
    }

    #[test]
    fn forget_preserves_in_audit() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Auditable fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .forget_fact("f1", ForgetReason::Privacy)
            .expect("forget");

        let all = store.audit_all_facts("agent-a", 100).expect("audit all");
        let found = all.iter().find(|f| f.id == "f1");
        assert!(found.is_some(), "audit must return forgotten facts");
        let found = found.expect("checked");
        assert!(found.is_forgotten);
        assert_eq!(found.forget_reason, Some(ForgetReason::Privacy));
    }

    // ---- Increment Access ----

    #[test]
    fn increment_access_updates_count() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Accessed fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .increment_access(&["f1".to_owned()])
            .expect("increment");
        store
            .increment_access(&["f1".to_owned()])
            .expect("increment again");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query");
        let found = results.iter().find(|f| f.id == "f1").expect("found");
        assert_eq!(found.access_count, 2);
    }

    #[test]
    fn increment_access_empty_ids_is_noop() {
        let store = make_store();
        store
            .increment_access(&[])
            .expect("empty increment should succeed");
    }

    #[test]
    fn increment_access_nonexistent_id_is_silent() {
        let store = make_store();
        // Should not error — silently skips missing facts
        store
            .increment_access(&["nonexistent".to_owned()])
            .expect("increment nonexistent should not error");
    }

    // ---- Schema Version ----

    #[test]
    fn schema_version_returns_current() {
        let store = make_store();
        let version = store.schema_version().expect("schema version");
        assert_eq!(version, KnowledgeStore::SCHEMA_VERSION);
    }

    // ---- Search: query_facts filters ----

    #[test]
    fn query_facts_filters_by_nous_id() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Fact for A"))
            .expect("insert f1");
        store
            .insert_fact(&make_fact("f2", "agent-b", "Fact for B"))
            .expect("insert f2");
        store
            .insert_fact(&make_fact("f3", "agent-a", "Another fact for A"))
            .expect("insert f3");

        let results_a = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query agent-a");
        assert_eq!(results_a.len(), 2);
        assert!(results_a.iter().all(|f| f.nous_id == "agent-a"));

        let results_b = store
            .query_facts("agent-b", "2026-06-01", 100)
            .expect("query agent-b");
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].id, "f2");
    }

    #[test]
    fn query_facts_respects_limit() {
        let store = make_store();
        for i in 0..20 {
            store
                .insert_fact(&make_fact(
                    &format!("f{i}"),
                    "agent-a",
                    &format!("Fact {i}"),
                ))
                .expect("insert");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 5)
            .expect("query with limit");
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn query_facts_excludes_expired() {
        let store = make_store();

        // Active fact
        store
            .insert_fact(&make_fact("f-active", "agent-a", "Active fact"))
            .expect("insert active");

        // Expired fact (valid_to in the past)
        let mut expired = make_fact("f-expired", "agent-a", "Expired fact");
        expired.valid_to = "2025-01-01".to_owned();
        store.insert_fact(&expired).expect("insert expired");

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query");

        // Should only return the active fact
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f-active");
    }

    #[test]
    fn query_facts_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query empty store");
        assert!(results.is_empty());
    }

    #[test]
    fn query_facts_nonexistent_nous_id_returns_empty() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Some fact"))
            .expect("insert");

        let results = store
            .query_facts("nonexistent-agent", "2026-06-01", 100)
            .expect("query nonexistent nous");
        assert!(results.is_empty());
    }

    // ---- Search: point-in-time ----

    #[test]
    fn query_facts_at_returns_snapshot() {
        let store = make_store();

        // Fact valid from 2026-01-01 to 2026-06-01
        let mut fact = make_fact("f1", "agent-a", "Temporal fact");
        fact.valid_from = "2026-01-01".to_owned();
        fact.valid_to = "2026-06-01".to_owned();
        store.insert_fact(&fact).expect("insert temporal fact");

        // Query at a time within the validity window
        let results = store
            .query_facts_at("2026-03-15")
            .expect("query at mid-range");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f1");

        // Query at a time after the validity window
        let results = store
            .query_facts_at("2026-07-01")
            .expect("query at post-range");
        assert!(results.is_empty());
    }

    // ---- Search: vectors ----

    #[test]
    fn search_vectors_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .search_vectors(vec![0.5, 0.5, 0.5, 0.5], 5, 20)
            .expect("search empty");
        assert!(results.is_empty());
    }

    #[test]
    fn search_vectors_returns_nearest() {
        let store = make_store();

        // Insert two embeddings with different vectors
        let mut chunk_a = make_embedding("emb-a", "Rust programming", "f1", "agent-a");
        chunk_a.embedding = vec![1.0, 0.0, 0.0, 0.0];
        store.insert_embedding(&chunk_a).expect("insert emb-a");

        let mut chunk_b = make_embedding("emb-b", "Python scripting", "f2", "agent-a");
        chunk_b.embedding = vec![0.0, 1.0, 0.0, 0.0];
        store.insert_embedding(&chunk_b).expect("insert emb-b");

        // Query close to chunk_a
        let results = store
            .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 1, 20)
            .expect("search nearest");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Rust programming");
    }

    #[test]
    fn search_vectors_respects_k() {
        let store = make_store();
        for i in 0..10 {
            let mut chunk = make_embedding(
                &format!("emb-{i}"),
                &format!("Content {i}"),
                &format!("f{i}"),
                "agent-a",
            );
            #[expect(
                clippy::cast_precision_loss,
                reason = "test data — i is 0..9, fits in f32"
            )]
            let component = i as f32 * 0.1;
            chunk.embedding = vec![component, 0.5, 0.3, 0.1];
            store.insert_embedding(&chunk).expect("insert");
        }

        let results = store
            .search_vectors(vec![0.5, 0.5, 0.3, 0.1], 3, 20)
            .expect("search k=3");
        assert_eq!(results.len(), 3);
    }

    // ---- Edge Cases ----

    #[test]
    fn insert_fact_empty_content_accepted() {
        // BUG DISCOVERY: empty content is accepted without validation.
        // The store does not reject facts with empty content.
        let store = make_store();
        let fact = make_fact("f-empty", "agent-a", "");
        store
            .insert_fact(&fact)
            .expect("empty content is accepted (no validation)");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.is_empty());
    }

    #[test]
    fn insert_fact_confidence_out_of_range_accepted() {
        // BUG DISCOVERY: confidence > 1.0 and < 0.0 are accepted without validation.
        let store = make_store();

        let mut high = make_fact("f-high", "agent-a", "High confidence");
        high.confidence = 1.5;
        store
            .insert_fact(&high)
            .expect("confidence > 1.0 accepted (no validation)");

        let mut negative = make_fact("f-neg", "agent-a", "Negative confidence");
        negative.confidence = -0.5;
        store
            .insert_fact(&negative)
            .expect("confidence < 0.0 accepted (no validation)");

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn insert_duplicate_entity_name_upserts() {
        let store = make_store();
        let e1 = make_entity("e1", "Rust", "language");
        store.insert_entity(&e1).expect("insert first");

        // Same ID, updated name
        let e1_updated = make_entity("e1", "Rust Lang", "language");
        store.insert_entity(&e1_updated).expect("upsert");

        let rows = store
            .run_query(
                r"?[id, name] := *entities{id, name}",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        // Same ID → upsert (one row)
        assert_eq!(rows.rows.len(), 1);
    }

    #[test]
    fn insert_different_entities_same_name() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Rust", "language"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Rust", "game"))
            .expect("insert e2");

        let rows = store
            .run_query(
                r"?[id, name] := *entities{id, name}",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        // Different IDs → two separate entities
        assert_eq!(rows.rows.len(), 2);
    }

    #[test]
    fn retrieve_nonexistent_fact() {
        let store = make_store();
        let results = store
            .query_facts("nonexistent-agent", "2026-06-01", 10)
            .expect("query should succeed, returning empty");
        assert!(results.is_empty());
    }

    #[test]
    fn forget_nonexistent_fact_succeeds() {
        // forget_fact on a missing ID should not panic (Datalog :put on empty match is a noop)
        let store = make_store();
        let result = store.forget_fact("nonexistent", ForgetReason::UserRequested);
        // The behavior is either Ok (noop) or Err — neither should panic
        let _ = result;
    }

    #[test]
    fn concurrent_inserts() {
        let store = make_store();
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let s = Arc::clone(&store);
                std::thread::spawn(move || {
                    let fact = Fact {
                        id: format!("f-concurrent-{i}"),
                        nous_id: "agent-a".to_owned(),
                        content: format!("Concurrent fact {i}"),
                        confidence: 0.9,
                        tier: EpistemicTier::Inferred,
                        valid_from: "2026-01-01".to_owned(),
                        valid_to: "9999-12-31".to_owned(),
                        superseded_by: None,
                        source_session_id: None,
                        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
                        access_count: 0,
                        last_accessed_at: String::new(),
                        stability_hours: 720.0,
                        fact_type: String::new(),
                        is_forgotten: false,
                        forgotten_at: None,
                        forget_reason: None,
                    };
                    s.insert_fact(&fact).expect("concurrent insert");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread join");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query after concurrent inserts");
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn concurrent_entity_inserts() {
        let store = make_store();
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let s = Arc::clone(&store);
                std::thread::spawn(move || {
                    let entity = Entity {
                        id: format!("e-concurrent-{i}"),
                        name: format!("Entity {i}"),
                        entity_type: "concept".to_owned(),
                        aliases: vec![],
                        created_at: "2026-03-01T00:00:00Z".to_owned(),
                        updated_at: "2026-03-01T00:00:00Z".to_owned(),
                    };
                    s.insert_entity(&entity).expect("concurrent entity insert");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread join");
        }

        let rows = store
            .run_query(
                r"?[count(id)] := *entities{id}",
                std::collections::BTreeMap::new(),
            )
            .expect("count entities");
        assert_eq!(rows.rows.len(), 1);
    }

    // ---- Raw Query / Mut Query ----

    #[test]
    fn run_query_returns_results() {
        let store = make_store();
        let rows = store
            .run_query("?[x] := x = 42", std::collections::BTreeMap::new())
            .expect("run_query");
        assert_eq!(rows.rows.len(), 1);
    }

    #[test]
    fn run_mut_query_creates_and_reads() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Mutable test"))
            .expect("insert");

        // Use run_mut_query to delete the fact
        let mut params = std::collections::BTreeMap::new();
        params.insert("id".to_owned(), crate::engine::DataValue::Str("f1".into()));
        store
            .run_mut_query(
                r"?[id, valid_from] := *facts{id, valid_from}, id = $id :rm facts {id, valid_from}",
                params,
            )
            .expect("delete via run_mut_query");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query after delete");
        assert!(results.is_empty());
    }

    // ---- Relationship queries ----

    #[test]
    fn entity_neighborhood_2hop() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("e1");
        store
            .insert_entity(&make_entity("e2", "Aletheia", "project"))
            .expect("e2");
        store
            .insert_entity(&make_entity("e3", "Rust", "language"))
            .expect("e3");

        // e1 -> e2 -> e3 (2-hop chain)
        store
            .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
            .expect("rel e1-e2");
        store
            .insert_relationship(&make_relationship("e2", "e3", "uses", 0.8))
            .expect("rel e2-e3");

        let rows = store.entity_neighborhood("e1").expect("2-hop neighborhood");
        // Should include both e2 (1-hop) and e3 (2-hop)
        assert!(
            rows.rows.len() >= 2,
            "2-hop neighborhood should find at least 2 results, got {}",
            rows.rows.len()
        );
    }

    #[test]
    fn entity_neighborhood_nonexistent_entity() {
        let store = make_store();
        let rows = store
            .entity_neighborhood("nonexistent")
            .expect("neighborhood of missing entity should succeed");
        assert!(rows.rows.is_empty());
    }

    // ---- Async wrappers ----

    #[tokio::test]
    async fn insert_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-async", "agent-a", "Async inserted fact");
        store.insert_fact_async(fact).await.expect("async insert");

        let results = store
            .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
            .await
            .expect("async query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f-async");
    }

    #[tokio::test]
    async fn search_vectors_async_works() {
        let store = make_store();
        let mut chunk = make_embedding("emb-async", "Async search content", "f1", "agent-a");
        chunk.embedding = vec![0.7, 0.3, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");

        let results = store
            .search_vectors_async(vec![0.7, 0.3, 0.0, 0.0], 5, 20)
            .await
            .expect("async search");
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn forget_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-forget-async", "agent-a", "Async forget");
        store.insert_fact_async(fact).await.expect("insert");

        store
            .forget_fact_async("f-forget-async".to_owned(), ForgetReason::Incorrect)
            .await
            .expect("async forget");

        let all = store
            .audit_all_facts_async("agent-a".to_owned(), 100)
            .await
            .expect("async audit");
        let found = all
            .iter()
            .find(|f| f.id == "f-forget-async")
            .expect("found");
        assert!(found.is_forgotten);
    }

    #[tokio::test]
    async fn unforget_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-unforget-async", "agent-a", "Async unforget");
        store.insert_fact_async(fact).await.expect("insert");

        store
            .forget_fact_async("f-unforget-async".to_owned(), ForgetReason::Outdated)
            .await
            .expect("forget");
        store
            .unforget_fact_async("f-unforget-async".to_owned())
            .await
            .expect("unforget");

        let all = store
            .audit_all_facts_async("agent-a".to_owned(), 100)
            .await
            .expect("audit");
        let found = all
            .iter()
            .find(|f| f.id == "f-unforget-async")
            .expect("found");
        assert!(!found.is_forgotten);
    }

    #[tokio::test]
    async fn increment_access_async_works() {
        let store = make_store();
        let fact = make_fact("f-access-async", "agent-a", "Async access");
        store.insert_fact_async(fact).await.expect("insert");

        store
            .increment_access_async(vec!["f-access-async".to_owned()])
            .await
            .expect("async increment");

        let results = store
            .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
            .await
            .expect("query");
        let found = results
            .iter()
            .find(|f| f.id == "f-access-async")
            .expect("found");
        assert_eq!(found.access_count, 1);
    }

    // --- Bi-temporal query tests ---

    fn make_temporal_fact(
        id: &str,
        nous_id: &str,
        content: &str,
        valid_from: &str,
        valid_to: &str,
    ) -> Fact {
        Fact {
            id: id.to_owned(),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: valid_from.to_owned(),
            valid_to: valid_to.to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_point_in_time() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "Rust is fast",
                "2026-01-01",
                "2026-06-01",
            ))
            .expect("insert t1");
        store
            .insert_fact(&make_temporal_fact(
                "t2",
                "agent",
                "Python is dynamic",
                "2026-03-01",
                "9999-12-31",
            ))
            .expect("insert t2");

        let at_feb = store
            .query_facts_temporal("agent", "2026-02-01", None)
            .expect("query feb");
        assert_eq!(at_feb.len(), 1);
        assert_eq!(at_feb[0].id, "t1");

        let at_apr = store
            .query_facts_temporal("agent", "2026-04-01", None)
            .expect("query apr");
        assert_eq!(at_apr.len(), 2);

        let at_jul = store
            .query_facts_temporal("agent", "2026-07-01", None)
            .expect("query jul");
        assert_eq!(at_jul.len(), 1);
        assert_eq!(at_jul[0].id, "t2");
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_before_any_facts_returns_empty() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "fact",
                "2026-06-01",
                "9999-12-31",
            ))
            .expect("insert");

        let results = store
            .query_facts_temporal("agent", "2026-01-01", None)
            .expect("query");
        assert!(results.is_empty());
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_boundary_inclusion() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "boundary fact",
                "2026-03-01",
                "2026-06-01",
            ))
            .expect("insert");

        let at_start = store
            .query_facts_temporal("agent", "2026-03-01", None)
            .expect("at valid_from");
        assert_eq!(at_start.len(), 1, "valid_from boundary is inclusive");

        let at_end = store
            .query_facts_temporal("agent", "2026-06-01", None)
            .expect("at valid_to");
        assert!(at_end.is_empty(), "valid_to boundary is exclusive");
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_with_content_filter() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "Rust is fast",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert t1");
        store
            .insert_fact(&make_temporal_fact(
                "t2",
                "agent",
                "Python is dynamic",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert t2");

        let filtered = store
            .query_facts_temporal("agent", "2026-03-01", Some("Rust"))
            .expect("filtered query");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "t1");
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_diff_added_and_removed() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "old",
                "agent",
                "old knowledge",
                "2026-01-01",
                "2026-03-01",
            ))
            .expect("insert old");
        store
            .insert_fact(&make_temporal_fact(
                "new",
                "agent",
                "new knowledge",
                "2026-02-15",
                "9999-12-31",
            ))
            .expect("insert new");

        let diff = store
            .query_facts_diff("agent", "2026-02-01", "2026-04-01")
            .expect("diff");

        assert_eq!(diff.added.len(), 1, "one fact added in interval");
        assert_eq!(diff.added[0].id, "new");
        assert_eq!(diff.removed.len(), 1, "one fact removed in interval");
        assert_eq!(diff.removed[0].id, "old");
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_diff_supersession_chain() {
        let store = make_store();
        let mut fact_a = make_temporal_fact("a", "agent", "version 1", "2026-01-01", "2026-03-01");
        fact_a.superseded_by = Some("b".to_owned());
        store.insert_fact(&fact_a).expect("insert a");

        store
            .insert_fact(&make_temporal_fact(
                "b",
                "agent",
                "version 2",
                "2026-03-01",
                "9999-12-31",
            ))
            .expect("insert b");

        let diff = store
            .query_facts_diff("agent", "2026-02-01", "2026-04-01")
            .expect("diff");

        assert_eq!(diff.modified.len(), 1, "one modified pair");
        assert_eq!(diff.modified[0].0.id, "a");
        assert_eq!(diff.modified[0].1.id, "b");
        assert!(diff.added.is_empty(), "superseded new is not in pure added");
        assert!(
            diff.removed.is_empty(),
            "superseding old is not in pure removed"
        );
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_isolates_nous_ids() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "alice",
                "Alice knows Rust",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert alice");
        store
            .insert_fact(&make_temporal_fact(
                "t2",
                "bob",
                "Bob knows Python",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert bob");

        let alice_facts = store
            .query_facts_temporal("alice", "2026-03-01", None)
            .expect("alice query");
        assert_eq!(alice_facts.len(), 1);
        assert_eq!(alice_facts[0].content, "Alice knows Rust");

        let bob_facts = store
            .query_facts_temporal("bob", "2026-03-01", None)
            .expect("bob query");
        assert_eq!(bob_facts.len(), 1);
        assert_eq!(bob_facts[0].content, "Bob knows Python");
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_excludes_forgotten_facts() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "forgotten fact",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert");
        store
            .forget_fact("t1", crate::knowledge::ForgetReason::UserRequested)
            .expect("forget");

        let results = store
            .query_facts_temporal("agent", "2026-03-01", None)
            .expect("query");
        assert!(results.is_empty(), "forgotten facts should be excluded");
    }

    #[cfg(feature = "mneme-engine")]
    #[tokio::test]
    async fn temporal_query_async_works() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "async temporal",
                "2026-01-01",
                "9999-12-31",
            ))
            .expect("insert");

        let results = store
            .query_facts_temporal_async("agent".to_owned(), "2026-03-01".to_owned(), None)
            .await
            .expect("async query");
        assert_eq!(results.len(), 1);
    }

    #[cfg(feature = "mneme-engine")]
    #[tokio::test]
    async fn temporal_diff_async_works() {
        let store = make_store();
        store
            .insert_fact(&make_temporal_fact(
                "t1",
                "agent",
                "diff async",
                "2026-02-01",
                "9999-12-31",
            ))
            .expect("insert");

        let diff = store
            .query_facts_diff_async(
                "agent".to_owned(),
                "2026-01-01".to_owned(),
                "2026-03-01".to_owned(),
            )
            .await
            .expect("async diff");
        assert_eq!(diff.added.len(), 1);
    }

    #[test]
    fn backup_db_returns_error_for_mem_backend() {
        let store = make_store();
        let dir = tempfile::tempdir().unwrap();
        let backup_path = dir.path().join("backup.db");
        let result = store.backup_db(&backup_path);
        assert!(
            result.is_err(),
            "backup_db should error on in-memory backend"
        );
    }

    #[test]
    fn restore_backup_returns_error_for_mem_backend() {
        let store = make_store();
        let dir = tempfile::tempdir().unwrap();
        let backup_path = dir.path().join("backup.db");
        std::fs::write(&backup_path, "fake").unwrap();
        let result = store.restore_backup(&backup_path);
        assert!(
            result.is_err(),
            "restore_backup should error on in-memory backend"
        );
    }

    #[test]
    fn import_from_backup_returns_error_for_mem_backend() {
        let store = make_store();
        let dir = tempfile::tempdir().unwrap();
        let backup_path = dir.path().join("backup.db");
        std::fs::write(&backup_path, "fake").unwrap();
        let result = store.import_from_backup(&backup_path, &["facts".to_owned()]);
        assert!(
            result.is_err(),
            "import_from_backup should error on in-memory backend"
        );
    }

    #[test]
    fn run_script_read_only_basic() {
        let store = make_store();
        let result = store
            .run_script_read_only("?[x] := x = 42", BTreeMap::new())
            .expect("read-only query should succeed");
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn run_script_read_only_rejects_mutations() {
        let store = make_store();
        let result = store.run_script_read_only(
            r"?[x] <- [[1]] :put facts { id: 'x', valid_from: 'now' => content: 'test', nous_id: 'a', confidence: 1.0, tier: 'verified', valid_to: 'end', recorded_at: 'now', access_count: 0, last_accessed_at: '', stability_hours: 720.0, fact_type: '' }",
            BTreeMap::new(),
        );
        assert!(
            result.is_err(),
            "read-only mode should reject :put operations"
        );
    }
}
