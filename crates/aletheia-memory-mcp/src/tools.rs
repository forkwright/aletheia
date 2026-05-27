//! MCP tool implementations for the memory server.
//!
//! Read tools (`nous_search`, `nous_neighbors`, `nous_list_topics`, `nous_stats`)
//! are always available.
//!
//! Write tools (`nous_annotate`, `nous_supersede`, `nous_forget`) are only
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
use mneme::id::{EntityId, FactId};
use mneme::knowledge::{Entity, Fact, ForgetReason, Relationship};
use mneme::knowledge_store::KnowledgeStore;

use crate::error::{InvalidInputSnafu, KnowledgeStoreSnafu, SerializationSnafu};
use crate::server::MemoryServer;

/// Parameters for `nous_search`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousSearchParams {
    /// Free-text query string; matched via BM25 against current fact content.
    pub query: String,
    /// Maximum number of results to return. Defaults to 20 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters for `nous_neighbors`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousNeighborsParams {
    /// ID of the seed fact whose entity neighbors should be returned.
    pub fact_id: String,
}

/// Parameters for `nous_annotate`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousAnnotateParams {
    /// Session ID for the annotation (identifies the agent or source).
    pub session_id: Option<String>,
    /// Fact ID to annotate.
    pub fact_id: String,
    /// Annotation content — agent-authored note or observation.
    pub content: String,
    /// Capability token for write authorization.
    pub write_token: String,
}

/// Parameters for `nous_supersede`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousSupersedeParams {
    /// ID of the fact being superseded.
    pub old_fact_id: String,
    /// ID of the new fact that supersedes it.
    pub new_fact_id: String,
    /// Reason for supersession.
    pub reason: String,
    /// Capability token for write authorization.
    pub write_token: String,
}

/// Parameters for `nous_forget`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousForgetParams {
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
fn fact_entity_id(fact_id: &str) -> crate::error::Result<EntityId> {
    EntityId::new(format!("fact:{fact_id}")).map_err(|e| {
        InvalidInputSnafu {
            message: format!("failed to create fact entity id: {e}"),
        }
        .build()
    })
}

fn insert_fact_entity_link(
    store: &KnowledgeStore,
    fact_id: &FactId,
    entity_id: &EntityId,
    timestamp: jiff::Timestamp,
) -> crate::error::Result<()> {
    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        DataValue::Str(fact_id.as_str().into()),
    );
    params.insert(
        "entity_id".to_owned(),
        DataValue::Str(entity_id.as_str().into()),
    );
    params.insert(
        "created_at".to_owned(),
        DataValue::Str(mneme::knowledge::format_timestamp(&timestamp).into()),
    );

    let script = r"
        ?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
        :put fact_entities {fact_id, entity_id => created_at}
    ";
    store.run_mut_query(script, params).map_err(|e| {
        KnowledgeStoreSnafu {
            message: e.to_string(),
        }
        .build()
    })?;
    Ok(())
}

fn link_annotation_to_target(
    store: &KnowledgeStore,
    annotation_id: &FactId,
    target_id: &FactId,
    timestamp: jiff::Timestamp,
) -> crate::error::Result<()> {
    let annotation_entity_id = fact_entity_id(annotation_id.as_str())?;
    let target_entity_id = fact_entity_id(target_id.as_str())?;
    for (entity_id, fact_id) in [
        (&annotation_entity_id, annotation_id.as_str()),
        (&target_entity_id, target_id.as_str()),
    ] {
        store
            .insert_entity(&Entity {
                id: entity_id.clone(),
                name: fact_id.to_owned(),
                entity_type: "fact".to_owned(),
                aliases: Vec::new(),
                created_at: timestamp,
                updated_at: timestamp,
            })
            .map_err(|e| {
                KnowledgeStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
    }

    insert_fact_entity_link(store, annotation_id, &annotation_entity_id, timestamp)?;
    insert_fact_entity_link(store, target_id, &target_entity_id, timestamp)?;

    store
        .insert_relationship(&Relationship {
            src: annotation_entity_id,
            dst: target_entity_id,
            relation: "annotates".to_owned(),
            weight: 1.0,
            created_at: timestamp,
        })
        .map_err(|e| {
            KnowledgeStoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
    Ok(())
}

fn forget_fact_with_current_schema(
    store: &KnowledgeStore,
    fact_id: &FactId,
    reason: ForgetReason,
) -> crate::error::Result<Fact> {
    let existing = store.read_facts_by_id(fact_id.as_str()).map_err(|e| {
        KnowledgeStoreSnafu {
            message: e.to_string(),
        }
        .build()
    })?;
    if existing.is_empty() {
        return Err(KnowledgeStoreSnafu {
            message: format!("fact not found: {}", fact_id.as_str()),
        }
        .build());
    }

    let now = mneme::knowledge::format_timestamp(&jiff::Timestamp::now());
    let script = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
          superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type,
          scope, project_id, visibility, is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier,
                   valid_to, superseded_by, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   scope, project_id, visibility},
            id = $id,
            is_forgotten = true,
            forgotten_at = $now,
            forget_reason = $reason
        :put facts {id, valid_from => content, nous_id, confidence, tier,
                    valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    scope, project_id, visibility, is_forgotten, forgotten_at, forget_reason}
    ";
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(fact_id.as_str().into()));
    params.insert("now".to_owned(), DataValue::Str(now.into()));
    params.insert("reason".to_owned(), DataValue::Str(reason.as_str().into()));
    store.run_mut_query(script, params).map_err(|e| {
        KnowledgeStoreSnafu {
            message: e.to_string(),
        }
        .build()
    })?;

    let facts = store.read_facts_by_id(fact_id.as_str()).map_err(|e| {
        KnowledgeStoreSnafu {
            message: e.to_string(),
        }
        .build()
    })?;
    facts.into_iter().next().ok_or_else(|| {
        KnowledgeStoreSnafu {
            message: format!("fact not found after forget: {}", fact_id.as_str()),
        }
        .build()
    })
}

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
    ///   "name": "nous_search",
    ///   "arguments": { "query": "fleet dispatch config", "limit": 10 }
    /// }
    /// ```
    #[tool(
        name = "nous_search",
        description = "BM25 text search across active facts in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                       Returns ranked matches with fact ID, content, and score."
    )]
    async fn nous_search(
        &self,
        Parameters(params): Parameters<NousSearchParams>,
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
    /// relation, then returns their direct neighbors. Each neighbor row carries
    /// `src_id`, `dst_id`, `name`, `entity_type`, `relation`, and `weight`. Use
    /// this to walk outward from a known fact into the wider knowledge graph.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "nous_neighbors",
    ///   "arguments": { "fact_id": "f-abc-123" }
    /// }
    /// ```
    #[tool(
        name = "nous_neighbors",
        description = "Return one-hop graph neighbors for a fact in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                       Each row includes src_id, dst_id, name, entity_type, relation, and weight."
    )]
    async fn nous_neighbors(
        &self,
        Parameters(params): Parameters<NousNeighborsParams>,
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
    ///   "name": "nous_list_topics",
    ///   "arguments": {}
    /// }
    /// ```
    #[tool(
        name = "nous_list_topics",
        description = "List all topic buckets in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                       Returns fact_type values with active-fact counts. \
                       Use as a discovery starting point for nous_search."
    )]
    async fn nous_list_topics(&self) -> Result<CallToolResult, rmcp::ErrorData> {
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
    /// `recorded_at` timestamp across active facts — the "last updated" signal.
    ///
    /// # Example
    ///
    /// ```json
    /// {
    ///   "name": "nous_stats",
    ///   "arguments": {}
    /// }
    /// ```
    #[tool(
        name = "nous_stats",
        description = "Return health stats for the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus: fact count, topic count, \
                       schema version, store path, and last updated timestamp."
    )]
    async fn nous_stats(&self) -> Result<CallToolResult, rmcp::ErrorData> {
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
                        *facts{recorded_at, is_forgotten, superseded_by},
                        is_forgotten == false,
                        is_null(superseded_by)
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
    ///   "name": "nous_annotate",
    ///   "arguments": {
    ///     "fact_id": "f-abc-123",
    ///     "content": "This fact was verified against external source X",
    ///     "session_id": "agent-uuid",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_annotate",
        description = "Create an annotation on an existing fact in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                         Requires write capability token. Returns the created annotation ID."
    )]
    #[tracing::instrument(skip(self), fields(tool = "nous_annotate", fact_id = %params.fact_id))]
    async fn nous_annotate(
        &self,
        Parameters(params): Parameters<NousAnnotateParams>,
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
                    id: annotation_id.clone(),
                    nous_id: session_id
                        .clone()
                        .unwrap_or_else(|| "mcp-client".to_owned()),
                    fact_type: "annotation".to_owned(),
                    content,
                    scope: None,
                    project_id: None,
                    sensitivity: mneme::knowledge::FactSensitivity::Public,
                    visibility: mneme::knowledge::Visibility::Private,
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
                let target_id = mneme::id::FactId::new(fact_id.clone()).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("failed to create target fact id: {e}"),
                    }
                    .build()
                })?;
                link_annotation_to_target(&store, &annotation_id, &target_id, now)?;

                Ok(annotation_id_str)
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        // Audit log for successful write
        tracing::info!(
            tool = "nous_annotate",
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
    ///   "name": "nous_supersede",
    ///   "arguments": {
    ///     "old_fact_id": "f-abc-123",
    ///     "new_fact_id": "f-abc-124",
    ///     "reason": "Updated with more recent information",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_supersede",
        description = "Mark one fact as superseded by another in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                         Requires write capability token. Returns the supersession record ID."
    )]
    #[tracing::instrument(skip(self), fields(tool = "nous_supersede", old_id = %params.old_fact_id, new_id = %params.new_fact_id))]
    async fn nous_supersede(
        &self,
        Parameters(params): Parameters<NousSupersedeParams>,
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
                    project_id: None,
                    sensitivity: mneme::knowledge::FactSensitivity::Public,
                    visibility: mneme::knowledge::Visibility::Private,
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
            tool = "nous_supersede",
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
    ///   "name": "nous_forget",
    ///   "arguments": {
    ///     "fact_id": "f-abc-123",
    ///     "reason": "Fact is no longer valid",
    ///     "write_token": "..."
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_forget",
        description = "Soft-delete a fact in the aletheia nous local knowledge store, session-scoped; not kanon mnemosyne's durable corpus. \
                         Requires write capability token. Returns forgotten_at timestamp."
    )]
    #[tracing::instrument(skip(self), fields(tool = "nous_forget", fact_id = %params.fact_id))]
    async fn nous_forget(
        &self,
        Parameters(params): Parameters<NousForgetParams>,
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

                let forgotten_fact =
                    forget_fact_with_current_schema(&store, &fact_id, forget_reason)?;

                forgotten_fact
                    .lifecycle
                    .forgotten_at
                    .map(|ts| ts.to_string())
                    .ok_or_else(|| {
                        KnowledgeStoreSnafu {
                            message: "forgotten fact is missing forgotten_at timestamp".to_owned(),
                        }
                        .build()
                    })
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        // Audit log for successful write
        tracing::info!(
            tool = "nous_forget",
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn nous_search_params_round_trip() {
        let json = r#"{"query":"fleet dispatch config","limit":10}"#;
        let params: NousSearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "fleet dispatch config");
        assert_eq!(params.limit, Some(10));

        let out = serde_json::to_string(&params).unwrap();
        let back: NousSearchParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.query, "fleet dispatch config");
        assert_eq!(back.limit, Some(10));
    }

    #[test]
    fn nous_search_params_default_limit() {
        let json = r#"{"query":"foo"}"#;
        let params: NousSearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "foo");
        assert_eq!(params.limit, None);
    }

    #[test]
    fn search_limit_clamping() {
        // Default is applied when limit is None
        assert_eq!(DEFAULT_SEARCH_LIMIT.min(MAX_SEARCH_LIMIT), 20);
        // Within-range limit is preserved
        assert_eq!(5_usize.min(MAX_SEARCH_LIMIT), 5);
        // Over-max limit is clamped
        assert_eq!(500_usize.min(MAX_SEARCH_LIMIT), 200);
    }

    #[test]
    fn nous_neighbors_params_round_trip() {
        let json = r#"{"fact_id":"f-abc-123"}"#;
        let params: NousNeighborsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-abc-123");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousNeighborsParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.fact_id, "f-abc-123");
    }

    #[test]
    fn nous_annotate_params_requires_write_token() {
        let json = r#"{"fact_id":"f-abc-123","content":"note"}"#;
        let result = serde_json::from_str::<NousAnnotateParams>(json);
        assert!(result.is_err());
    }

    #[test]
    fn nous_annotate_params_round_trip() {
        let json = r#"{"session_id":"agent-uuid","fact_id":"f-abc-123","content":"verified","write_token":"sekrit"}"#;
        let params: NousAnnotateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.session_id, Some("agent-uuid".to_owned()));
        assert_eq!(params.fact_id, "f-abc-123");
        assert_eq!(params.content, "verified");
        assert_eq!(params.write_token, "sekrit");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousAnnotateParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.write_token, "sekrit");
    }

    #[test]
    fn nous_supersede_params_round_trip() {
        let json = r#"{"old_fact_id":"f-old","new_fact_id":"f-new","reason":"updated","write_token":"sekrit"}"#;
        let params: NousSupersedeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.old_fact_id, "f-old");
        assert_eq!(params.new_fact_id, "f-new");
        assert_eq!(params.reason, "updated");
        assert_eq!(params.write_token, "sekrit");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousSupersedeParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.new_fact_id, "f-new");
    }

    #[test]
    fn nous_forget_params_round_trip() {
        let json = r#"{"fact_id":"f-abc-123","reason":"stale","write_token":"sekrit"}"#;
        let params: NousForgetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-abc-123");
        assert_eq!(params.reason, "stale");
        assert_eq!(params.write_token, "sekrit");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousForgetParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.write_token, "sekrit");
    }
}
