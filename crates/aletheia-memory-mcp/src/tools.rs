//! MCP tool implementations for the memory server.
//!
//! Read tools (`memory_search`, `memory_neighbors`, `memory_list_topics`, `memory_stats`)
//! are always available.
//!
//! Write tools (`memory_annotate`, `memory_supersede`, `memory_forget`) are only
//! registered if the `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` environment variable is
//! set at server startup. Each write call must include a `write_token` field
//! that matches the configured token (via constant-time comparison).

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

/// Parameters for `memory_annotate`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct MemoryAnnotateParams {
    /// Session ID for the annotation (identifies the agent or source).
    pub session_id: Option<String>,
    /// Fact ID to annotate.
    pub fact_id: String,
    /// Annotation content — agent-authored note or observation.
    pub content: String,
    /// Capability token for write authorization.
    pub write_token: String,
}

/// Parameters for `memory_supersede`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct MemorySupersedeParams {
    /// ID of the fact being superseded.
    pub old_fact_id: String,
    /// ID of the new fact that supersedes it.
    pub new_fact_id: String,
    /// Reason for supersession.
    pub reason: String,
    /// Capability token for write authorization.
    pub write_token: String,
}

/// Parameters for `memory_forget`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct MemoryForgetParams {
    /// ID of the fact to forget.
    pub fact_id: String,
    /// Reason for forgetting.
    pub reason: String,
    /// Capability token for write authorization.
    pub write_token: String,
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

    /// Create an agent-authored annotation on an existing fact.
    ///
    /// The annotation is recorded as a new fact with `fact_type` "annotation"
    /// and linked to the target fact. Requires a valid write capability token.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_annotate",
    ///   "arguments": {
    ///     "fact_id": "f-abc-123",
    ///     "content": "This fact was verified against external source X",
    ///     "session_id": "agent-uuid",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(description = "Create an annotation on an existing fact. \
                         Requires write capability token. Returns the created annotation ID.")]
    #[tracing::instrument(skip(self), fields(tool = "memory_annotate", fact_id = %params.fact_id))]
    async fn memory_annotate(
        &self,
        Parameters(params): Parameters<MemoryAnnotateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Validate write authorization first, before any side effects
        self.validate_write_token(&params.write_token)
            .map_err(rmcp::ErrorData::from)?;

        if params.fact_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "fact_id must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        if params.content.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "content must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let fact_id = params.fact_id.clone();
        let content = params.content.clone();
        let session_id = params.session_id.clone();

        let result = self
            .run_blocking(move |store| {
                // Verify the target fact exists
                let target_facts = store.read_facts_by_id(&fact_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                if target_facts.is_empty() {
                    return Err(crate::error::FactNotFoundSnafu {
                        id: fact_id.clone(),
                    }
                    .build());
                }

                // Create annotation fact with a generated ID
                // Use "f-mcp-" prefix to mark MCP-inserted facts for audit purposes.
                let annotation_id_str = format!("f-mcp-{}", koina::uuid::uuid_v4());
                let annotation_id = mneme::id::FactId::new(annotation_id_str).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("failed to create fact id: {e}"),
                    }
                    .build()
                })?;
                let annotation_id_str = annotation_id.to_string();
                let now = jiff::Timestamp::now();

                let annotation_fact = mneme::knowledge::Fact {
                    id: annotation_id,
                    nous_id: session_id
                        .clone()
                        .unwrap_or_else(|| "mcp-client".to_owned()),
                    fact_type: "annotation".to_owned(),
                    content,
                    scope: None,
                    sensitivity: mneme::knowledge::FactSensitivity::Public,
                    temporal: mneme::knowledge::FactTemporal {
                        valid_from: now,
                        valid_to: mneme::knowledge::far_future(),
                        recorded_at: now,
                    },
                    provenance: mneme::knowledge::FactProvenance {
                        confidence: 0.95,
                        tier: mneme::knowledge::EpistemicTier::Inferred,
                        source_session_id: session_id,
                        stability_hours: mneme::knowledge::default_stability_hours("annotation"),
                    },
                    lifecycle: mneme::knowledge::FactLifecycle {
                        superseded_by: None,
                        is_forgotten: false,
                        forgotten_at: None,
                        forget_reason: None,
                    },
                    access: mneme::knowledge::FactAccess {
                        access_count: 0,
                        last_accessed_at: None,
                    },
                };

                store.insert_fact(&annotation_fact).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                Ok(annotation_id_str)
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        // Audit log for successful write
        tracing::info!(
            tool = "memory_annotate",
            target_fact_id = %params.fact_id,
            annotation_id = %result,
            "memory-mcp write"
        );

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "annotation_id": result,
            "target_fact_id": params.fact_id,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Mark one fact as superseded by another.
    ///
    /// Records the supersession relationship and reason. The old fact remains
    /// in the graph but is marked as superseded. Requires a valid write capability token.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_supersede",
    ///   "arguments": {
    ///     "old_fact_id": "f-abc-123",
    ///     "new_fact_id": "f-abc-124",
    ///     "reason": "Updated with more recent information",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(description = "Mark one fact as superseded by another. \
                         Requires write capability token. Returns the supersession record ID.")]
    #[tracing::instrument(skip(self), fields(tool = "memory_supersede", old_id = %params.old_fact_id, new_id = %params.new_fact_id))]
    async fn memory_supersede(
        &self,
        Parameters(params): Parameters<MemorySupersedeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Validate write authorization first
        self.validate_write_token(&params.write_token)
            .map_err(rmcp::ErrorData::from)?;

        if params.old_fact_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "old_fact_id must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        if params.new_fact_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "new_fact_id must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        if params.reason.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "reason must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let old_id = params.old_fact_id.clone();
        let new_id = params.new_fact_id.clone();
        let reason = params.reason.clone();

        let record_id = self
            .run_blocking(move |store| {
                // Verify both facts exist
                let old_facts = store.read_facts_by_id(&old_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                if old_facts.is_empty() {
                    return Err(crate::error::FactNotFoundSnafu { id: old_id.clone() }.build());
                }

                let new_facts = store.read_facts_by_id(&new_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                if new_facts.is_empty() {
                    return Err(crate::error::FactNotFoundSnafu { id: new_id.clone() }.build());
                }

                // Mark the old fact as superseded
                let mut old_fact = old_facts.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu { id: old_id.clone() }.build()
                })?;
                let new_fact_id = mneme::id::FactId::new(new_id.clone()).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("failed to create fact id: {e}"),
                    }
                    .build()
                })?;
                old_fact.lifecycle.superseded_by = Some(new_fact_id);

                store.insert_fact(&old_fact).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                // Create a record fact documenting the supersession
                let record_id_str = format!("f-mcp-{}", koina::uuid::uuid_v4());
                let record_id = mneme::id::FactId::new(record_id_str).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("failed to create fact id: {e}"),
                    }
                    .build()
                })?;
                let now = jiff::Timestamp::now();
                let record_content =
                    format!("Supersession: {old_id} was superseded by {new_id} — {reason}");

                let record_fact = mneme::knowledge::Fact {
                    id: record_id.clone(),
                    nous_id: "mcp-server".to_owned(),
                    fact_type: "supersession".to_owned(),
                    content: record_content,
                    scope: None,
                    sensitivity: mneme::knowledge::FactSensitivity::Public,
                    temporal: mneme::knowledge::FactTemporal {
                        valid_from: now,
                        valid_to: mneme::knowledge::far_future(),
                        recorded_at: now,
                    },
                    provenance: mneme::knowledge::FactProvenance {
                        confidence: 1.0,
                        tier: mneme::knowledge::EpistemicTier::Verified,
                        source_session_id: None,
                        stability_hours: mneme::knowledge::default_stability_hours("supersession"),
                    },
                    lifecycle: mneme::knowledge::FactLifecycle {
                        superseded_by: None,
                        is_forgotten: false,
                        forgotten_at: None,
                        forget_reason: None,
                    },
                    access: mneme::knowledge::FactAccess {
                        access_count: 0,
                        last_accessed_at: None,
                    },
                };

                store.insert_fact(&record_fact).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                Ok(record_id.to_string())
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        // Audit log for successful write
        tracing::info!(
            tool = "memory_supersede",
            old_fact_id = %params.old_fact_id,
            new_fact_id = %params.new_fact_id,
            record_id = %record_id,
            "memory-mcp write"
        );

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "record_id": record_id,
            "old_fact_id": params.old_fact_id,
            "new_fact_id": params.new_fact_id,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Soft-delete a fact: mark it as forgotten with a reason.
    ///
    /// The fact remains in the graph but is marked as forgotten and excluded
    /// from recall results. Requires a valid write capability token.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "memory_forget",
    ///   "arguments": {
    ///     "fact_id": "f-abc-123",
    ///     "reason": "Fact is no longer valid",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(description = "Soft-delete a fact (mark as forgotten). \
                         Requires write capability token. Returns forgotten_at timestamp.")]
    #[tracing::instrument(skip(self), fields(tool = "memory_forget", fact_id = %params.fact_id))]
    async fn memory_forget(
        &self,
        Parameters(params): Parameters<MemoryForgetParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Validate write authorization first
        self.validate_write_token(&params.write_token)
            .map_err(rmcp::ErrorData::from)?;

        if params.fact_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "fact_id must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        if params.reason.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "reason must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let fact_id_str = params.fact_id.clone();
        let reason_str = params.reason.clone();

        let forgotten_at = self
            .run_blocking(move |store| {
                // Parse fact ID and call the store's forget_fact method
                let fact_id = mneme::id::FactId::new(&fact_id_str).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("invalid fact_id format: {e}"),
                    }
                    .build()
                })?;

                // Map the string reason to a ForgetReason enum
                let forget_reason = match reason_str.to_lowercase().as_str() {
                    "outdated" => mneme::knowledge::ForgetReason::Outdated,
                    "incorrect" => mneme::knowledge::ForgetReason::Incorrect,
                    "privacy" => mneme::knowledge::ForgetReason::Privacy,
                    "stale" => mneme::knowledge::ForgetReason::Stale,
                    _ => mneme::knowledge::ForgetReason::UserRequested,
                };

                let forgotten_fact = store.forget_fact(&fact_id, forget_reason).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                Ok(forgotten_fact.temporal.recorded_at.to_string())
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        // Audit log for successful write
        tracing::info!(
            tool = "memory_forget",
            fact_id = %params.fact_id,
            "memory-mcp write"
        );

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "fact_id": params.fact_id,
            "forgotten_at": forgotten_at,
        }))
        .context(SerializationSnafu)
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }
}
