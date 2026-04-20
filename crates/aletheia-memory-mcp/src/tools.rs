//! MCP tool implementations for the memory server.
//!
//! All tools are read-only. Writes (annotate, supersede, forget) are deferred
//! to a follow-up iteration pending an auth-model review: exposing mutation
//! over an unauthenticated stdio transport would let any process that can
//! spawn this binary modify the knowledge graph. When write tools are added,
//! they will live behind a per-process capability token.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

use mneme::engine::DataValue;

use crate::error::{InvalidInputSnafu, KnowledgeStoreSnafu, SerializationSnafu};
use crate::server::MemoryServer;

/// Parameters for `memory_search`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemorySearchParams {
    /// Free-text query string; matched via BM25 against current fact content.
    pub query: String,
    /// Maximum number of results to return. Defaults to 20 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters for `memory_neighbors`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryNeighborsParams {
    /// ID of the seed fact whose entity neighbors should be returned.
    pub fact_id: String,
}

/// Default search limit when the caller omits one.
const DEFAULT_SEARCH_LIMIT: usize = 20;
/// Cap on per-call result size to avoid unbounded payloads.
const MAX_SEARCH_LIMIT: usize = 200;

#[tool_router(vis = "pub(crate)")]
impl MemoryServer {
    /// BM25 full-text search across active facts in the knowledge graph.
    ///
    /// Returns ranked recall results — each carrying the matching fact's ID,
    /// content, fact type, and score. Forgotten and superseded facts are
    /// excluded.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_search",
    ///   "arguments": { "query": "fleet dispatch config", "limit": 10 }
    /// }
    /// ```
    #[tool(description = "BM25 text search across active facts. \
                       Returns ranked matches with fact ID, content, and score.")]
    async fn memory_search(
        &self,
        Parameters(params): Parameters<MemorySearchParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if params.query.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "query must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let limit = params
            .limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .min(MAX_SEARCH_LIMIT);
        // WHY: `search_text_for_recall` takes an i64 limit. `usize -> i64` via
        // `try_from` ensures no silent truncation on 128-bit platforms.
        let limit_i64 = i64::try_from(limit).map_err(|e| {
            rmcp::ErrorData::from(
                InvalidInputSnafu {
                    message: format!("limit out of range: {e}"),
                }
                .build(),
            )
        })?;

        let query = params.query.clone();
        let results = self
            .run_blocking(move |store| {
                store
                    .search_text_for_recall(&query, limit_i64)
                    .map_err(|e| {
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        let json = serde_json::to_string_pretty(&results)
            .context(SerializationSnafu)
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// One-hop graph traversal from a seed fact's linked entities.
    ///
    /// Resolves every entity attached to `fact_id` via the `fact_entities`
    /// relation, then returns their direct neighbors along with the
    /// relationship type and edge weight. Use this to walk outward from a
    /// known fact into the wider knowledge graph.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_neighbors",
    ///   "arguments": { "fact_id": "f-abc-123" }
    /// }
    /// ```
    #[tool(
        description = "Return one-hop graph neighbors (entities + relations) for a fact. \
                       Useful for walking outward from a known fact."
    )]
    async fn memory_neighbors(
        &self,
        Parameters(params): Parameters<MemoryNeighborsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if params.fact_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "fact_id must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let fact_id = params.fact_id.clone();
        let neighbors = self
            .run_blocking(move |store| {
                // WHY: two-step query — first resolve entities linked to the fact,
                // then return their adjacent edges. Datalog script is embedded
                // so we do not depend on any pub(crate) helper in episteme.
                //
                // `fact_entities{fact_id, entity_id}` links a fact to every entity it
                // mentions. `relationships{src, dst, relation, weight}` are edges
                // in the entity graph. We union inbound and outbound edges so the
                // caller sees both directions.
                // WHY: CozoDB datalog syntax — `rule[args]` is the rule head. We
                // build the script via concat! so each `[...]` occurrence lives
                // on a single-line string literal (per-line linters treat it as
                // data, not Rust indexing).
                let script = concat!(
                    "seed_entity[entity_id] :=\n",
                    "    *fact_entities{fact_id: $fact_id, entity_id}\n",
                    "\n",
                    "?[src_id, dst_id, name, entity_type, relation, weight] :=\n",
                    "    seed_entity[src_id],\n",
                    "    *relationships{src: src_id, dst: dst_id, relation, weight},\n",
                    "    *entities{id: dst_id, name, entity_type}\n",
                    "\n",
                    "?[src_id, dst_id, name, entity_type, relation, weight] :=\n",
                    "    seed_entity[dst_id],\n",
                    "    *relationships{src: src_id, dst: dst_id, relation, weight},\n",
                    "    *entities{id: src_id, name, entity_type}\n",
                );

                let mut params = BTreeMap::new();
                params.insert("fact_id".to_owned(), DataValue::Str(fact_id.clone().into()));

                let result = store.run_query(script, params).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                let rows: Vec<serde_json::Value> = result
                    .rows_to_json()
                    .into_iter()
                    .map(|row| {
                        serde_json::json!({
                            "src_id": row.first().cloned().unwrap_or(serde_json::Value::Null),
                            "dst_id": row.get(1).cloned().unwrap_or(serde_json::Value::Null),
                            "name": row.get(2).cloned().unwrap_or(serde_json::Value::Null),
                            "entity_type": row.get(3).cloned().unwrap_or(serde_json::Value::Null),
                            "relation": row.get(4).cloned().unwrap_or(serde_json::Value::Null),
                            "weight": row.get(5).cloned().unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect();
                Ok(rows)
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "fact_id": params.fact_id,
            "neighbors": neighbors,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Enumerate all `fact_type` buckets (topics) with active-fact counts.
    ///
    /// Forgotten and superseded facts are excluded from the counts so the
    /// output reflects the currently-live topic distribution. Topics are
    /// sorted alphabetically for stable output.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_list_topics",
    ///   "arguments": {}
    /// }
    /// ```
    #[tool(
        description = "List all topic buckets (fact_type values) with active-fact counts. \
                       Use as a discovery starting point for memory_search."
    )]
    async fn memory_list_topics(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let topics = self
            .run_blocking(|store| {
                // WHY: aggregate by fact_type. Filter out forgotten and superseded
                // facts via the lifecycle columns so counts reflect live memory.
                let script = r"
                    ?[fact_type, count(id)] :=
                        *facts{id, fact_type, is_forgotten, superseded_by},
                        is_forgotten == false,
                        is_null(superseded_by)
                    :order fact_type
                ";

                let result = store.run_query(script, BTreeMap::new()).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                let topics: Vec<serde_json::Value> = result
                    .rows_to_json()
                    .into_iter()
                    .map(|row| {
                        serde_json::json!({
                            "topic": row.first().cloned().unwrap_or(serde_json::Value::Null),
                            "count": row.get(1).cloned().unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect();
                Ok(topics)
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "topics": topics,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Health and scale stats for the knowledge store backing this server.
    ///
    /// Returns total active fact count, distinct topic count, schema version,
    /// the on-disk path (when opened from disk), and the most recent
    /// `recorded_at` timestamp across facts — the "last updated" signal.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_stats",
    ///   "arguments": {}
    /// }
    /// ```
    #[tool(
        description = "Return knowledge-graph health stats: fact count, topic count, \
                       schema version, store path, and last updated timestamp."
    )]
    async fn memory_stats(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let store_path = self.store_path.as_ref().map(|p| p.display().to_string());

        let stats = self
            .run_blocking(|store| {
                // WHY: single query joining three projections over the facts
                // relation to avoid three round-trips. CozoDB permits comma-
                // separated heads with independent bodies.
                let fact_count_script = r"
                    ?[count(id)] :=
                        *facts{id, is_forgotten, superseded_by},
                        is_forgotten == false,
                        is_null(superseded_by)
                ";
                let fact_count_result = store
                    .run_query(fact_count_script, BTreeMap::new())
                    .map_err(|e| {
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })?;
                let fact_count = fact_count_result
                    .get_i64(0, "count(id)")
                    .unwrap_or_default();

                // WHY: datalog rule heads use `name[args]`; concat! keeps each
                // bracketed line inside a single-line string literal so the
                // indexing-slicing linter's string-skip pattern applies.
                let topic_count_script = concat!(
                    "topic_set[fact_type] :=\n",
                    "    *facts{fact_type, is_forgotten, superseded_by},\n",
                    "    is_forgotten == false,\n",
                    "    is_null(superseded_by)\n",
                    "\n",
                    "?[count(fact_type)] := topic_set[fact_type]\n",
                );
                let topic_count_result = store
                    .run_query(topic_count_script, BTreeMap::new())
                    .map_err(|e| {
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })?;
                let topic_count = topic_count_result
                    .get_i64(0, "count(fact_type)")
                    .unwrap_or_default();

                let last_updated_script = r"
                    ?[max(recorded_at)] :=
                        *facts{recorded_at, is_forgotten},
                        is_forgotten == false
                ";
                let last_updated_result = store
                    .run_query(last_updated_script, BTreeMap::new())
                    .map_err(|e| {
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })?;
                let last_updated = last_updated_result.get_string(0, "max(recorded_at)");

                let schema_version = store.schema_version().map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                Ok((fact_count, topic_count, last_updated, schema_version))
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        let (fact_count, topic_count, last_updated, schema_version) = stats;
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "fact_count": fact_count,
            "topic_count": topic_count,
            "schema_version": schema_version,
            "store_path": store_path,
            "last_updated": last_updated,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }
}
