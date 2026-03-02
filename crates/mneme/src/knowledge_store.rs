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
//!         source_session_id: String?, recorded_at: String }
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
        recorded_at: String
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
pub fn fts_ddl() -> &'static str {
    r"::fts create facts:content_fts {
        extractor: content,
        tokenizer: Simple,
        filters: [Lowercase, Stemmer('English'), Stopwords('en')]
    }"
}

/// Query templates for common knowledge operations.
pub mod queries {
    /// Insert or update a fact.
    pub const UPSERT_FACT: &str = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at] <- [[$id, $valid_from,
          $content, $nous_id, $confidence, $tier, $valid_to, $superseded_by,
          $source_session_id, $recorded_at]]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at}
    ";

    /// Query current facts for a nous (not superseded, currently valid).
    pub const CURRENT_FACTS: &str = r"
        ?[id, content, confidence, tier, recorded_at] :=
            *facts{id, valid_from, content, nous_id, confidence, tier,
                   valid_to, superseded_by, recorded_at},
            nous_id = $nous_id,
            valid_from <= $now,
            valid_to > $now,
            is_null(superseded_by)
        :order -confidence
        :limit $limit
    ";

    /// Point-in-time fact query.
    pub const FACTS_AT_TIME: &str = r"
        ?[id, content, confidence, tier] :=
            *facts{id, valid_from, content, confidence, tier, valid_to},
            valid_from <= $time,
            valid_to > $time
    ";

    /// Supersede a fact (close old, insert new).
    #[allow(clippy::needless_raw_string_hashes)]  // contains inner quotes
    pub const SUPERSEDE_FACT: &str = r#"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at] <- [
            [$old_id, $old_valid_from, $old_content, $nous_id, $old_confidence,
             $old_tier, $now, $new_id, $old_source, $old_recorded],
            [$new_id, $now, $new_content, $nous_id, $new_confidence,
             $new_tier, "9999-12-31", null, $source_session_id, $now]
        ]
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at}
    "#;

    /// Insert or update an entity.
    pub const UPSERT_ENTITY: &str = r"
        ?[id, name, entity_type, aliases, created_at, updated_at] <- [
            [$id, $name, $entity_type, $aliases, $created_at, $updated_at]
        ]
        :put entities {id => name, entity_type, aliases, created_at, updated_at}
    ";

    /// Insert a relationship.
    pub const UPSERT_RELATIONSHIP: &str = r"
        ?[src, dst, relation, weight, created_at] <- [
            [$src, $dst, $relation, $weight, $created_at]
        ]
        :put relationships {src, dst => relation, weight, created_at}
    ";

    /// 2-hop entity neighborhood.
    pub const ENTITY_NEIGHBORHOOD: &str = r"
        hop1[dst, rel] := *relationships{src: $entity_id, dst, relation: rel}
        hop2[dst, rel] := hop1[mid, _], *relationships{src: mid, dst, relation: rel}
        ?[id, name, entity_type, relation, hop] :=
            hop1[id, relation], *entities{id, name, entity_type}, hop = 1
        ?[id, name, entity_type, relation, hop] :=
            hop2[id, relation], *entities{id, name, entity_type}, hop = 2
        :order hop, name
    ";

    /// KNN vector search.
    pub const SEMANTIC_SEARCH: &str = r"
        ?[id, content, source_type, source_id, dist] :=
            ~embeddings:semantic_idx {id, content, source_type, source_id |
                query: $query_vec, k: $k, ef: $ef, bind_distance: dist}
    ";

    /// Entity search by name or alias (prefix match).
    pub const SEARCH_ENTITIES: &str = r"
        ?[id, name, entity_type] :=
            *entities{id, name, entity_type},
            starts_with(name, $prefix)
        ?[id, name, entity_type] :=
            *entities{id, name, entity_type, aliases},
            contains(aliases, $prefix)
        :limit $limit
    ";

    /// Hybrid search: BM25 + HNSW vector + graph neighborhood fused via RRF.
    /// Graph sub-rules are injected dynamically by `build_hybrid_query`.
    pub const HYBRID_SEARCH_BASE: &str = r"
        bm25[id, score] := ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}

        vec[id, score] :=
            ~embeddings:semantic_idx{id | query: $query_vec, k: $k, ef: $ef, bind_distance: raw_dist},
            score = 1.0 - raw_dist

        {GRAPH_RULES}

        ?[id, rrf_score, bm25_rank, vec_rank, graph_rank] <~
            ReciprocalRankFusion(bm25[], vec[], graph[])

        :order -rrf_score
        :limit $limit
    ";
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
    /// Rank in BM25 signal (0 = not in BM25 results).
    pub bm25_rank: i64,
    /// Rank in vector search signal (0 = not in vector results).
    pub vec_rank: i64,
    /// Rank in graph neighborhood signal (0 = no graph contribution).
    pub graph_rank: i64,
}

/// Typed wrapper around the Datalog engine providing domain-level knowledge operations.
///
/// Holds an `Arc<Db>` internally. Callers share via `Arc<KnowledgeStore>`.
/// All sync methods can be called directly; async wrappers use `spawn_blocking`.
#[cfg(feature = "mneme-engine")]
pub struct KnowledgeStore {
    db: std::sync::Arc<aletheia_mneme_engine::Db>,
    dim: usize,
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    const SCHEMA_VERSION: i64 = 1;

    /// Open an in-memory knowledge store with default configuration.
    pub fn open_mem() -> crate::error::Result<std::sync::Arc<Self>> {
        Self::open_mem_with_config(KnowledgeConfig::default())
    }

    /// Open an in-memory knowledge store with custom configuration.
    pub fn open_mem_with_config(
        config: KnowledgeConfig,
    ) -> crate::error::Result<std::sync::Arc<Self>> {
        let db = aletheia_mneme_engine::Db::open_mem()
            .map_err(|e| crate::error::EngineInitSnafu { message: e.to_string() }.build())?;
        let store = Self { db: std::sync::Arc::new(db), dim: config.dim };
        store.init_schema()?;
        Ok(std::sync::Arc::new(store))
    }

    fn init_schema(&self) -> crate::error::Result<()> {
        use aletheia_mneme_engine::ScriptMutability;
        use std::collections::BTreeMap;

        for ddl in KNOWLEDGE_DDL {
            self.db
                .run(ddl, BTreeMap::new(), ScriptMutability::Mutable)
                .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;
        }

        let emb_ddl = embeddings_ddl(self.dim);
        self.db
            .run(&emb_ddl, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;

        let hnsw = hnsw_ddl(self.dim);
        self.db
            .run(&hnsw, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;

        let fts = fts_ddl();
        self.db
            .run(fts, BTreeMap::new(), ScriptMutability::Mutable)
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;

        // Schema version tracking relation (no underscore prefix — CozoDB stores underscore
        // relations only in temp_store_tx which does not persist across run() calls).
        self.db
            .run(
                r":create schema_version { key: String => version: Int }",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;

        // Insert initial version
        let mut params = BTreeMap::new();
        params.insert(
            "key".to_owned(),
            aletheia_mneme_engine::DataValue::Str("schema".into()),
        );
        params.insert(
            "version".to_owned(),
            aletheia_mneme_engine::DataValue::from(Self::SCHEMA_VERSION),
        );
        self.db
            .run(
                r"?[key, version] <- [[$key, $version]] :put schema_version { key => version }",
                params,
                ScriptMutability::Mutable,
            )
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())?;

        Ok(())
    }

    /// Insert or update a fact.
    pub fn insert_fact(&self, fact: &crate::knowledge::Fact) -> crate::error::Result<()> {
        let params = fact_to_params(fact);
        self.run_mut(queries::UPSERT_FACT, params)
    }

    /// Query current facts for a nous at a given time, up to limit results.
    pub fn query_facts(
        &self,
        nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use aletheia_mneme_engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("now".to_owned(), DataValue::Str(now.into()));
        params.insert("limit".to_owned(), DataValue::from(limit));

        let rows = self.run_read(FULL_CURRENT_FACTS, params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Point-in-time fact query.
    pub fn query_facts_at(&self, time: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use aletheia_mneme_engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("time".to_owned(), DataValue::Str(time.into()));

        let rows = self.run_read(queries::FACTS_AT_TIME, params)?;
        rows_to_facts_partial(rows)
    }

    /// Insert or update an entity.
    pub fn insert_entity(&self, entity: &crate::knowledge::Entity) -> crate::error::Result<()> {
        let params = entity_to_params(entity);
        self.run_mut(queries::UPSERT_ENTITY, params)
    }

    /// Insert a relationship.
    pub fn insert_relationship(
        &self,
        rel: &crate::knowledge::Relationship,
    ) -> crate::error::Result<()> {
        let params = relationship_to_params(rel);
        self.run_mut(queries::UPSERT_RELATIONSHIP, params)
    }

    /// Query 2-hop entity neighborhood. Returns raw rows for flexible callers.
    pub fn entity_neighborhood(
        &self,
        entity_id: &str,
    ) -> crate::error::Result<aletheia_mneme_engine::NamedRows> {
        use aletheia_mneme_engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
        self.run_read(queries::ENTITY_NEIGHBORHOOD, params)
    }

    /// Insert a vector embedding for semantic search.
    pub fn insert_embedding(
        &self,
        chunk: &crate::knowledge::EmbeddedChunk,
    ) -> crate::error::Result<()> {
        let params = embedding_to_params(chunk, self.dim);
        self.run_mut(
            r"?[id, content, source_type, source_id, nous_id, embedding, created_at] <- [
                [$id, $content, $source_type, $source_id, $nous_id, $embedding, $created_at]
              ]
              :put embeddings { id => content, source_type, source_id, nous_id, embedding, created_at }",
            params,
        )
    }

    /// kNN semantic vector search.
    pub fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use aletheia_mneme_engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(query_vec))),
        );
        params.insert("k".to_owned(), DataValue::from(k));
        params.insert("ef".to_owned(), DataValue::from(ef));

        let rows = self.run_read(queries::SEMANTIC_SEARCH, params)?;
        rows_to_recall_results(rows)
    }

    /// Get the current schema version.
    pub fn schema_version(&self) -> crate::error::Result<i64> {
        use aletheia_mneme_engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("key".to_owned(), DataValue::Str("schema".into()));
        let rows = self.run_read(
            r"?[version] := *schema_version{key: $key, version}",
            params,
        )?;
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
    pub fn run_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue>,
    ) -> crate::error::Result<aletheia_mneme_engine::NamedRows> {
        self.run_read(script, params)
    }

    /// Run a custom Datalog query with an optional timeout.
    ///
    /// If the query exceeds the timeout, returns `Error::QueryTimeout`.
    /// The `:timeout` directive is injected into the script — callers should not include it.
    ///
    /// Note: timeout detection relies on the engine error containing "killed before completion"
    /// (from CozoDB's internal `ProcessKilled` error). This is a known fragile dependency.
    pub fn run_query_with_timeout(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue>,
        timeout: Option<std::time::Duration>,
    ) -> crate::error::Result<aletheia_mneme_engine::NamedRows> {
        use aletheia_mneme_engine::ScriptMutability;
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
                        secs: timeout.map(|d| d.as_secs_f64()).unwrap_or(0.0),
                    }
                    .build()
                } else {
                    crate::error::EngineQuerySnafu { message: msg }.build()
                }
            })
    }

    /// Raw mutable query escape hatch — runs script with `ScriptMutability::Mutable`.
    /// Required for `:rm` and `:put` operations from caller code.
    pub fn run_mut_query(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue>,
    ) -> crate::error::Result<aletheia_mneme_engine::NamedRows> {
        use aletheia_mneme_engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Mutable)
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())
    }

    /// Hybrid BM25 + HNSW vector + graph retrieval fused via `ReciprocalRankFusion`.
    ///
    /// Runs a single Datalog query combining all three signals in the engine.
    /// When `seed_entities` is empty, the graph signal contributes zero to RRF.
    pub fn search_hybrid(
        &self,
        q: &HybridQuery,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use aletheia_mneme_engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("query_text".to_owned(), DataValue::Str(q.text.as_str().into()));
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
        rows_to_hybrid_results(rows)
    }

    /// Async `search_hybrid` — wraps sync call in `spawn_blocking`.
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

    // --- Async wrappers ---

    /// Async `insert_fact` — wraps sync call in `spawn_blocking`.
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

    // --- Internal helpers ---

    fn run_mut(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue>,
    ) -> crate::error::Result<()> {
        use aletheia_mneme_engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Mutable)
            .map(|_| ())
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())
    }

    fn run_read(
        &self,
        script: &str,
        params: std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue>,
    ) -> crate::error::Result<aletheia_mneme_engine::NamedRows> {
        use aletheia_mneme_engine::ScriptMutability;
        self.db
            .run(script, params, ScriptMutability::Immutable)
            .map_err(|e| crate::error::EngineQuerySnafu { message: e.to_string() }.build())
    }
}

// Extended query that returns all Fact fields (used by query_facts).
#[cfg(feature = "mneme-engine")]
const FULL_CURRENT_FACTS: &str = r"
    ?[id, content, confidence, tier, recorded_at, nous_id, valid_from, valid_to, superseded_by, source_session_id] :=
        *facts{id, valid_from, content, nous_id, confidence, tier,
               valid_to, superseded_by, source_session_id, recorded_at},
        nous_id = $nous_id,
        valid_from <= $now,
        valid_to > $now,
        is_null(superseded_by)
    :order -confidence
    :limit $limit
";

// --- Conversion helpers ---

#[cfg(feature = "mneme-engine")]
fn fact_to_params(
    fact: &crate::knowledge::Fact,
) -> std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue> {
    use aletheia_mneme_engine::DataValue;
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
    p.insert(
        "confidence".to_owned(),
        DataValue::from(fact.confidence),
    );
    p.insert(
        "tier".to_owned(),
        DataValue::Str(fact.tier.as_str().into()),
    );
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
    p
}

#[cfg(feature = "mneme-engine")]
fn entity_to_params(
    entity: &crate::knowledge::Entity,
) -> std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue> {
    use aletheia_mneme_engine::DataValue;
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
) -> std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue> {
    use aletheia_mneme_engine::DataValue;
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
) -> std::collections::BTreeMap<String, aletheia_mneme_engine::DataValue> {
    use aletheia_mneme_engine::{Array1, DataValue, Vector};
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
fn rows_to_facts(
    rows: aletheia_mneme_engine::NamedRows,
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
        });
    }
    Ok(out)
}

// Parse rows from FACTS_AT_TIME into Vec<Fact> (partial — only has id, content, confidence, tier).
#[cfg(feature = "mneme-engine")]
fn rows_to_facts_partial(
    rows: aletheia_mneme_engine::NamedRows,
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
        });
    }
    Ok(out)
}

// Parse rows from SEMANTIC_SEARCH into Vec<RecallResult>.
// Columns: id, content, source_type, source_id, dist
#[cfg(feature = "mneme-engine")]
fn rows_to_recall_results(
    rows: aletheia_mneme_engine::NamedRows,
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
             graph[id, score] := seed_list[seed], *relationships{{src: seed, dst: id, weight: score}}\n        \
             graph[id, score] := seed_list[seed], *relationships{{src: seed, dst: mid, weight: _w}}, \
             *relationships{{src: mid, dst: id, weight}}, score = weight * 0.5"
        )
    };
    queries::HYBRID_SEARCH_BASE.replace("{GRAPH_RULES}", &graph_rules)
}

// Parse rows from ReciprocalRankFusion output into Vec<HybridResult>.
// Columns: id (Str), rrf_score (Float), bm25_rank (Int), vec_rank (Int), graph_rank (Int)
#[cfg(feature = "mneme-engine")]
fn rows_to_hybrid_results(
    rows: aletheia_mneme_engine::NamedRows,
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
        out.push(HybridResult { id, rrf_score, bm25_rank, vec_rank, graph_rank });
    }
    // Sort by rrf_score descending (RRF output is unordered since it comes through :order in Datalog,
    // but :order is applied by the engine — this is a safety sort for correctness)
    out.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

// --- DataValue extraction utilities ---

#[cfg(feature = "mneme-engine")]
fn extract_str(val: &aletheia_mneme_engine::DataValue) -> crate::error::Result<String> {
    match val {
        aletheia_mneme_engine::DataValue::Str(s) => Ok(s.to_string()),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
fn extract_optional_str(
    val: &aletheia_mneme_engine::DataValue,
) -> crate::error::Result<Option<String>> {
    match val {
        aletheia_mneme_engine::DataValue::Null => Ok(None),
        aletheia_mneme_engine::DataValue::Str(s) => Ok(Some(s.to_string())),
        other => Err(crate::error::ConversionSnafu {
            message: format!("expected Str or Null, got {other:?}"),
        }
        .build()),
    }
}

#[cfg(feature = "mneme-engine")]
fn extract_float(val: &aletheia_mneme_engine::DataValue) -> crate::error::Result<f64> {
    val.get_float().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Float), got {val:?}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn extract_int(val: &aletheia_mneme_engine::DataValue) -> crate::error::Result<i64> {
    val.get_int().ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("expected Num(Int), got {val:?}"),
        }
        .build()
    })
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
    use static_assertions::assert_impl_all;
    use super::KnowledgeStore;
    assert_impl_all!(KnowledgeStore: Send, Sync);
}

#[cfg(all(test, feature = "mneme-engine"))]
mod timeout_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn query_timeout_returns_typed_error() {
        let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
            .expect("open_mem");

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
        let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
            .expect("open_mem");

        let result = store.run_query_with_timeout(
            "?[x] := x = 42",
            BTreeMap::new(),
            None,
        );

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

    #[test]
    fn query_templates_contain_params() {
        assert!(queries::CURRENT_FACTS.contains("$nous_id"));
        assert!(queries::CURRENT_FACTS.contains("$now"));
        assert!(queries::SEMANTIC_SEARCH.contains("$query_vec"));
        assert!(queries::ENTITY_NEIGHBORHOOD.contains("$entity_id"));
        assert!(queries::SUPERSEDE_FACT.contains("$old_id"));
        assert!(queries::SUPERSEDE_FACT.contains("$new_id"));
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
        assert!(script.contains("graph[id, score] <- []"), "empty seeds must produce empty graph relation");
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
        assert!(script.contains("seed_list"), "non-empty seeds must produce seed_list relation");
        assert!(script.contains("e-rust"));
        assert!(script.contains("e-python"));
        assert!(script.contains("*relationships"));
    }
}
