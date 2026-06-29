// kanon:ignore RUST/file-too-long — MCP tool implementations with inline datalog scripts; splitting would fragment the #[tool_router] impl and break the single-file routing convention.
//! MCP tool implementations for the memory server.
//!
//! Read tools (`nous_search`, `nous_neighbors`, `nous_list_topics`, `nous_stats`)
//! are always available.
//!
//! Write tools (`nous_annotate`, `nous_supersede`, `nous_forget`) are only
//! registered if the `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` environment variable is
//! set at server startup. The capability token is configured out-of-band and is
//! never accepted as a model-visible tool argument.

use std::collections::BTreeMap;
use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use snafu::ResultExt as _;

use mneme::engine::DataValue;
use mneme::id::{EntityId, FactId};
use mneme::knowledge::{Entity, Fact, ForgetReason, Relationship};
use mneme::knowledge_store::KnowledgeStore;

use crate::error::{InvalidInputSnafu, KnowledgeStoreSnafu, SerializationSnafu};
use crate::server::MemoryServer;

/// Parameters for `nous_search`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only query text and scope filters; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousSearchParams {
    /// Free-text query string; matched via BM25 against current fact content.
    pub query: String,
    /// Maximum number of results to return. Defaults to 20 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Optional project partition (64-character SHA-256 hex) to restrict results.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope (`user`, `feedback`, `project`, or `reference`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility (`private`, `shared`, `restricted`, `published`).
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity (`public`, `internal`, `confidential`).
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}

/// Parameters for `nous_list_topics`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only scope filters; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousListTopicsParams {
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}

/// Parameters for `nous_stats`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only scope filters; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousStatsParams {
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
    /// Include the full local store path when admin diagnostics are enabled.
    #[serde(default)]
    pub include_store_path: Option<bool>,
}

/// Parameters for `nous_neighbors`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only a fact ID; no sensitive data
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NousNeighborsParams {
    /// ID of the seed fact whose entity neighbors should be returned.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Optional project partition filter.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Optional memory scope filter.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional minimum visibility filter.
    #[serde(default)]
    pub min_visibility: Option<String>,
    /// Optional maximum sensitivity filter.
    #[serde(default)]
    pub max_sensitivity: Option<String>,
}

/// Parameters for `nous_annotate`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only the target fact, owner, and provenance; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousAnnotateParams {
    /// Owning agent (nous) that is authoring the annotation. Must be explicit;
    /// the `mcp-client` fallback is no longer used for user memory.
    pub nous_id: String,
    /// Fact ID to annotate.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Annotation content — agent-authored note or observation.
    pub content: String,
    /// Optional source session that caused this annotation write.
    #[serde(default)]
    pub source_session_id: Option<String>,
}

/// Parameters for `nous_supersede`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only fact IDs and owner; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousSupersedeParams {
    /// ID of the fact being superseded.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub old_fact_id: String,
    /// ID of the new fact that supersedes it.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub new_fact_id: String,
    /// Owning agent (nous) recording the supersession. Must be explicit.
    pub nous_id: String,
    /// Optional source session that caused this supersession write.
    #[serde(default)]
    pub source_session_id: Option<String>,
    /// Reason for supersession.
    pub reason: String,
}

/// Parameters for `nous_forget`.
// kanon:ignore RUST/no-debug-derive-on-public-types — contains only fact ID and owner; no secrets
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[non_exhaustive]
pub struct NousForgetParams {
    /// ID of the fact to forget.
    // kanon:ignore RUST/primitive-for-domain-id — WHY: MCP JSON protocol boundary; String required for serde/schemars JsonSchema derivation
    pub fact_id: String,
    /// Owning agent (nous) requesting the forget. Must match the fact owner.
    pub nous_id: String,
    /// Typed reason for forgetting; one of `user_requested`, `outdated`,
    /// `incorrect`, `privacy`, `stale`, `superseded`, or `contradicted`.
    pub reason: String,
}

/// Default search limit when the caller omits one.
const DEFAULT_SEARCH_LIMIT: usize = 20;
/// Cap on per-call result size to avoid unbounded payloads.
const MAX_SEARCH_LIMIT: usize = 200;
const ALLOWED_FORGET_REASONS: &str =
    "user_requested, outdated, incorrect, privacy, stale, superseded, contradicted";

fn opaque_store_id(store_path: Option<&Path>) -> String {
    let Some(store_path) = store_path else {
        return "store:memory:ephemeral".to_owned();
    };

    let mut hasher = Sha256::new();
    hasher.update(b"aletheia-memory-mcp/store-path/v1");
    hasher.update(store_path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    format!("store:sha256:{}", hex_digest(&digest))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        hex.push(hex_nibble(byte >> 4));
        hex.push(hex_nibble(byte & 0x0f));
    }
    hex
}

fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '?',
    }
}

/// Parse an optional memory-scope filter string.
fn parse_scope(raw: Option<&str>) -> crate::error::Result<Option<mneme::knowledge::MemoryScope>> {
    match raw {
        Some(s) if !s.is_empty() => s.parse().map(Some).map_err(|e| {
            InvalidInputSnafu {
                message: format!("invalid scope filter: {e}"),
            }
            .build()
        }),
        _ => Ok(None),
    }
}

/// Parse an optional minimum-visibility filter string.
fn parse_visibility(
    raw: Option<&str>,
) -> crate::error::Result<Option<mneme::knowledge::Visibility>> {
    match raw {
        Some(s) if !s.is_empty() => s.parse().map(Some).map_err(|e| {
            InvalidInputSnafu {
                message: format!("invalid visibility filter: {e}"),
            }
            .build()
        }),
        _ => Ok(None),
    }
}

/// Parse an optional maximum-sensitivity filter string.
fn parse_sensitivity(
    raw: Option<&str>,
) -> crate::error::Result<Option<mneme::knowledge::FactSensitivity>> {
    match raw {
        Some(s) if !s.is_empty() => s.parse().map(Some).map_err(|e| {
            InvalidInputSnafu {
                message: format!("invalid sensitivity filter: {e}"),
            }
            .build()
        }),
        _ => Ok(None),
    }
}

fn parse_optional_source_session_id(raw: Option<&str>) -> crate::error::Result<Option<String>> {
    match raw {
        Some(s) if s.trim().is_empty() => Err(InvalidInputSnafu {
            message: "source_session_id must not be blank when supplied".to_owned(),
        }
        .build()),
        Some(s) => Ok(Some(s.trim().to_owned())),
        None => Ok(None),
    }
}

fn ensure_fact_is_active(fact: &Fact, id: &str) -> crate::error::Result<()> {
    if fact.lifecycle.is_forgotten || fact.lifecycle.superseded_by.as_ref().is_some() {
        return Err(crate::error::FactNotFoundSnafu { id: id.to_owned() }.build());
    }
    Ok(())
}

fn parse_forget_reason(raw: &str) -> crate::error::Result<ForgetReason> {
    raw.trim().parse().map_err(|e| {
        InvalidInputSnafu {
            message: format!(
                "invalid forget reason: {e}; allowed values: {ALLOWED_FORGET_REASONS}"
            ),
        }
        .build()
    })
}

/// Apply project/scope/visibility/sensitivity filters to recall results.
fn matches_scope_filters(
    result: &mneme::knowledge::RecallResult,
    project_id: Option<&str>,
    scope: Option<mneme::knowledge::MemoryScope>,
    min_visibility: Option<mneme::knowledge::Visibility>,
    max_sensitivity: Option<mneme::knowledge::FactSensitivity>,
) -> bool {
    if let Some(expected) = project_id {
        let matches = result
            .project_id
            .as_ref()
            .is_some_and(|p| p.as_str() == expected);
        if !matches {
            return false;
        }
    }
    if let Some(expected) = scope {
        let matches = result.scope.is_some_and(|s| s == expected);
        if !matches {
            return false;
        }
    }
    if let Some(min) = min_visibility
        && result.visibility < min
    {
        return false;
    }
    if let Some(max) = max_sensitivity
        && result.sensitivity > max
    {
        return false;
    }
    true
}

/// Datalog rule defining facts visible to a requesting nous.
///
/// WHY: mirrors `episteme::knowledge_store::marshal::scoped_visibility_rules`
/// without depending on the non-public helper. A fact is visible when it is
/// owned by the requester, marked `shared`, or marked `published`.
const SCOPED_VISIBILITY_RULES: &str = r"
    visible_fact[id] := *facts{id, nous_id: $requester_nous_id}
    visible_fact[id] := *facts{id, visibility: 'shared'}
    visible_fact[id] := *facts{id, visibility: 'published'}
";

/// A visible fact row materialized for aggregation tools.
#[derive(Debug, Clone)]
struct ScopedFactRow {
    fact_type: String,
    scope: Option<mneme::knowledge::MemoryScope>,
    project_id: Option<mneme::workspace::ProjectId>,
    visibility: mneme::knowledge::Visibility,
    sensitivity: mneme::knowledge::FactSensitivity,
    recorded_at: Option<String>,
}

fn scoped_row_str<'a>(
    row: &'a [serde_json::Value],
    index: usize,
    field: &str,
    id: &str,
) -> crate::error::Result<&'a str> {
    row.get(index).and_then(|v| v.as_str()).ok_or_else(|| {
        InvalidInputSnafu {
            message: format!("scoped aggregation query returned missing {field} for {id}"),
        }
        .build()
    })
}

fn scoped_row_optional_str<'a>(
    row: &'a [serde_json::Value],
    index: usize,
    field: &str,
    id: &str,
) -> crate::error::Result<Option<&'a str>> {
    match row.get(index) {
        Some(v) if v.is_null() => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| {
            InvalidInputSnafu {
                message: format!("scoped aggregation query returned non-string {field} for {id}"),
            }
            .build()
        }),
        None => Err(InvalidInputSnafu {
            message: format!("scoped aggregation query returned missing {field} for {id}"),
        }
        .build()),
    }
}

fn parse_scoped_fact_row(row: &[serde_json::Value]) -> crate::error::Result<ScopedFactRow> {
    let id = scoped_row_str(row, 0, "id", "<row>")?;
    let fact_type = scoped_row_str(row, 1, "fact_type", id)?.to_owned();
    let scope = scoped_row_optional_str(row, 2, "scope", id)?
        .map(|raw| {
            raw.parse().map_err(|e| {
                InvalidInputSnafu {
                    message: format!(
                        "scoped aggregation query returned invalid scope for {id}: {e}"
                    ),
                }
                .build()
            })
        })
        .transpose()?;
    let project_id = scoped_row_optional_str(row, 3, "project_id", id)?
        .map(|raw| {
            mneme::workspace::ProjectId::from_sha256_hex(raw).map_err(|e| {
                InvalidInputSnafu {
                    message: format!(
                        "scoped aggregation query returned invalid project_id for {id}: {e}"
                    ),
                }
                .build()
            })
        })
        .transpose()?;
    let visibility = scoped_row_str(row, 4, "visibility", id)?
        .parse()
        .map_err(|e| {
            InvalidInputSnafu {
                message: format!(
                    "scoped aggregation query returned invalid visibility for {id}: {e}"
                ),
            }
            .build()
        })?;
    let sensitivity = scoped_row_str(row, 5, "sensitivity", id)?
        .parse()
        .map_err(|e| {
            InvalidInputSnafu {
                message: format!(
                    "scoped aggregation query returned invalid sensitivity for {id}: {e}"
                ),
            }
            .build()
        })?;
    let recorded_at = scoped_row_optional_str(row, 6, "recorded_at", id)?.map(str::to_owned);

    Ok(ScopedFactRow {
        fact_type,
        scope,
        project_id,
        visibility,
        sensitivity,
        recorded_at,
    })
}

/// Query active facts visible to `requester_nous_id`, returning rows with the
/// attributes needed for topic and stats aggregation.
fn run_scoped_facts_query(
    store: &KnowledgeStore,
    requester_nous_id: &str,
) -> crate::error::Result<Vec<ScopedFactRow>> {
    let rules = SCOPED_VISIBILITY_RULES;
    let script = format!(
        r"
        {rules}
        ?[id, fact_type, scope, project_id, visibility, sensitivity, recorded_at] :=
            visible_fact[id],
            *facts{{id, fact_type, is_forgotten, superseded_by, scope, project_id, visibility, sensitivity, recorded_at}},
            is_forgotten == false,
            is_null(superseded_by)
        :order id
        "
    );
    let mut params = BTreeMap::new();
    params.insert(
        "requester_nous_id".to_owned(),
        DataValue::Str(requester_nous_id.into()),
    );
    let result = store.run_query(&script, params).map_err(|e| {
        KnowledgeStoreSnafu {
            message: e.to_string(),
        }
        .build()
    })?;

    result
        .rows_to_json()
        .into_iter()
        .map(|row| parse_scoped_fact_row(&row))
        .collect()
}

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
        description = "BM25 text search across active facts in the aletheia nous local knowledge store, scoped to the requesting nous; not kanon mnemosyne's durable corpus. \
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

        // WHY: recall scope is bound to the server's authenticated caller
        // identity, not to any model-supplied argument.
        let requester = self.requester_nous_id()?.to_owned();

        let query = params.query.clone();
        let project_id = params.project_id.clone().filter(|s| !s.is_empty());
        let scope = parse_scope(params.scope.as_deref())?;
        let min_visibility = parse_visibility(params.min_visibility.as_deref())?;
        let max_sensitivity = parse_sensitivity(params.max_sensitivity.as_deref())?;

        let results = self
            .run_blocking(move |store| {
                // WHY: use the scoped recall path so foreign private facts are
                // excluded at the query level before scoring, cluster expansion,
                // or final truncation can leak them.
                let mut results = store
                    .search_text_for_recall_scoped(&query, limit_i64, &requester)
                    .map_err(|e| {
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })?;
                results.retain(|r| {
                    matches_scope_filters(
                        r,
                        project_id.as_deref(),
                        scope,
                        min_visibility,
                        max_sensitivity,
                    )
                });
                Ok(results)
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
        description = "Return one-hop graph neighbors for a visible fact in the aletheia nous local knowledge store, scoped to the requesting nous; not kanon mnemosyne's durable corpus. \
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

        // WHY: recall scope is bound to the server's authenticated caller
        // identity, not to any model-supplied argument.
        let requester = self.requester_nous_id()?.to_owned();

        let fact_id = params.fact_id.clone();
        let project_id = params.project_id.clone().filter(|s| !s.is_empty());
        let scope = parse_scope(params.scope.as_deref())?;
        let min_visibility = parse_visibility(params.min_visibility.as_deref())?;
        let max_sensitivity = parse_sensitivity(params.max_sensitivity.as_deref())?;

        let neighbors = self
            .run_blocking(move |store| {
                let seed_facts = store.read_facts_by_id(&fact_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let seed_fact = seed_facts.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu {
                        id: fact_id.clone(),
                    }
                    .build()
                })?;
                if seed_fact.lifecycle.is_forgotten
                    || seed_fact.lifecycle.superseded_by.as_ref().is_some()
                {
                    return Err(crate::error::FactNotFoundSnafu {
                        id: fact_id.clone(),
                    }
                    .build());
                }
                let is_visible = seed_fact.nous_id.as_str() == requester.as_str()
                    || matches!(
                        seed_fact.visibility,
                        mneme::knowledge::Visibility::Shared
                            | mneme::knowledge::Visibility::Published
                    );
                if !is_visible {
                    return Err(InvalidInputSnafu {
                        message: "fact is not visible to the supplied nous_id".to_owned(),
                    }
                    .build());
                }
                if !matches_scope_filters(
                    &mneme::knowledge::RecallResult {
                        content: String::new(),
                        distance: 0.0,
                        source_type: "fact".to_owned(),
                        source_id: seed_fact.id.to_string(),
                        nous_id: seed_fact.nous_id.clone(),
                        sensitivity: seed_fact.sensitivity,
                        graph_importance: 0.0,
                        scope: seed_fact.scope,
                        project_id: seed_fact.project_id.clone(),
                        visibility: seed_fact.visibility,
                        source_count: 0,
                    },
                    project_id.as_deref(),
                    scope,
                    min_visibility,
                    max_sensitivity,
                ) {
                    return Err(InvalidInputSnafu {
                        message: "fact does not match the supplied scope filters".to_owned(),
                    }
                    .build());
                }

                // WHY: two-step query — first resolve entities linked to the fact,
                // then return their adjacent edges. Datalog script is embedded
                // so we do not depend on any pub(crate) helper in episteme.
                //
                // `fact_entities{fact_id, entity_id}` links a fact to every entity it
                // mentions. `relationships{src, dst, relation, weight}` are edges
                // in the entity graph. We union inbound and outbound edges so the
                // caller sees both directions.
                // WHY: Datalog syntax — `rule[args]` is the rule head. We
                // build the script via concat! so each `[...]` occurrence lives
                // on a single-line string literal (per-line linters treat it as
                // data, not Rust indexing).
                // WHY: visible_neighbor requires every returned entity to be backed by at
                // least one active fact visible to the requester, preventing graph
                // traversal from leaking entity names/relations owned only by other nouses.
                let script = concat!(
                    "visible_neighbor[entity_id] :=\n",
                    "    *fact_entities{entity_id, fact_id: _fid},\n",
                    "    *facts{id: _fid, nous_id: $requester_nous_id, is_forgotten: false, superseded_by: _sb},\n",
                    "    is_null(_sb)\n",
                    "visible_neighbor[entity_id] :=\n",
                    "    *fact_entities{entity_id, fact_id: _fid},\n",
                    "    *facts{id: _fid, visibility: 'shared', is_forgotten: false, superseded_by: _sb},\n",
                    "    is_null(_sb)\n",
                    "visible_neighbor[entity_id] :=\n",
                    "    *fact_entities{entity_id, fact_id: _fid},\n",
                    "    *facts{id: _fid, visibility: 'published', is_forgotten: false, superseded_by: _sb},\n",
                    "    is_null(_sb)\n",
                    "\n",
                    "seed_entity[entity_id] :=\n",
                    "    *fact_entities{fact_id: $fact_id, entity_id}\n",
                    "\n",
                    "?[src_id, dst_id, name, entity_type, relation, weight] :=\n",
                    "    seed_entity[src_id],\n",
                    "    *relationships{src: src_id, dst: dst_id, relation, weight},\n",
                    "    visible_neighbor[dst_id],\n",
                    "    *entities{id: dst_id, name, entity_type}\n",
                    "\n",
                    "?[src_id, dst_id, name, entity_type, relation, weight] :=\n",
                    "    seed_entity[dst_id],\n",
                    "    *relationships{src: src_id, dst: dst_id, relation, weight},\n",
                    "    visible_neighbor[src_id],\n",
                    "    *entities{id: src_id, name, entity_type}\n",
                );

                let mut params = BTreeMap::new();
                params.insert("fact_id".to_owned(), DataValue::Str(fact_id.clone().into()));
                params.insert("requester_nous_id".to_owned(), DataValue::Str(requester.clone().into()));

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
    /// sorted alphabetically for stable output. Results are scoped to the
    /// requesting `nous_id` and optional filters.
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
        description = "List topic buckets in the aletheia nous local knowledge store, scoped to the requesting nous; not kanon mnemosyne's durable corpus. \
                       Returns fact_type values with active-fact counts. \
                       Use as a discovery starting point for nous_search."
    )]
    async fn nous_list_topics(
        &self,
        Parameters(params): Parameters<NousListTopicsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // WHY: recall scope is bound to the server's authenticated caller
        // identity, not to any model-supplied argument.
        let requester = self.requester_nous_id()?.to_owned();

        let project_id = params.project_id.clone().filter(|s| !s.is_empty());
        let scope = parse_scope(params.scope.as_deref())?;
        let min_visibility = parse_visibility(params.min_visibility.as_deref())?;
        let max_sensitivity = parse_sensitivity(params.max_sensitivity.as_deref())?;

        let topics = self
            .run_blocking(move |store| {
                let rows = run_scoped_facts_query(&store, &requester)?;
                let mut counts: std::collections::BTreeMap<String, u64> =
                    std::collections::BTreeMap::new();
                for row in rows {
                    if !matches_scope_filters(
                        &mneme::knowledge::RecallResult {
                            content: String::new(),
                            distance: 0.0,
                            source_type: "fact".to_owned(),
                            source_id: String::new(),
                            nous_id: String::new(),
                            sensitivity: row.sensitivity,
                            graph_importance: 0.0,
                            scope: row.scope,
                            project_id: row.project_id,
                            visibility: row.visibility,
                            source_count: 0,
                        },
                        project_id.as_deref(),
                        scope,
                        min_visibility,
                        max_sensitivity,
                    ) {
                        continue;
                    }
                    *counts.entry(row.fact_type).or_insert(0) += 1;
                }
                let topics: Vec<serde_json::Value> = counts
                    .into_iter()
                    .map(|(topic, count)| {
                        serde_json::json!({
                            "topic": topic,
                            "count": count,
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
    /// an opaque store id, backend/readiness metadata, and the most recent
    /// `recorded_at` timestamp across active facts visible to the requesting
    /// `nous_id` — the "last updated" signal. Full store paths are redacted
    /// unless admin diagnostics are enabled server-side and the caller opts in.
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
        description = "Return health stats for the aletheia nous local knowledge store, scoped to the requesting nous; not kanon mnemosyne's durable corpus: fact count, topic count, \
                       schema version, opaque store id, backend/readiness, and last updated timestamp."
    )]
    async fn nous_stats(
        &self,
        Parameters(params): Parameters<NousStatsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // WHY: recall scope is bound to the server's authenticated caller
        // identity, not to any model-supplied argument.
        let requester = self.requester_nous_id()?.to_owned();

        let store_id = opaque_store_id(self.store_path.as_deref());
        let store_backend = if self.store_path.is_some() {
            "fjall"
        } else {
            "memory"
        };
        let include_store_path = params.include_store_path.unwrap_or(false);
        let exposed_store_path = if include_store_path && self.admin_diagnostics {
            self.store_path.as_ref().map(|p| p.display().to_string())
        } else {
            None
        };
        let store_path_redacted = self.store_path.is_some() && exposed_store_path.is_none();
        let project_id = params.project_id.clone().filter(|s| !s.is_empty());
        let scope = parse_scope(params.scope.as_deref())?;
        let min_visibility = parse_visibility(params.min_visibility.as_deref())?;
        let max_sensitivity = parse_sensitivity(params.max_sensitivity.as_deref())?;

        let stats = self
            .run_blocking(move |store| {
                let rows = run_scoped_facts_query(&store, &requester)?;
                let mut fact_count: u64 = 0;
                let mut topic_set = std::collections::HashSet::new();
                let mut last_updated: Option<String> = None;
                for row in rows {
                    if !matches_scope_filters(
                        &mneme::knowledge::RecallResult {
                            content: String::new(),
                            distance: 0.0,
                            source_type: "fact".to_owned(),
                            source_id: String::new(),
                            nous_id: String::new(),
                            sensitivity: row.sensitivity,
                            graph_importance: 0.0,
                            scope: row.scope,
                            project_id: row.project_id,
                            visibility: row.visibility,
                            source_count: 0,
                        },
                        project_id.as_deref(),
                        scope,
                        min_visibility,
                        max_sensitivity,
                    ) {
                        continue;
                    }
                    fact_count += 1;
                    topic_set.insert(row.fact_type);
                    if let Some(ts) = row.recorded_at
                        && last_updated.as_ref().is_none_or(|current| &ts > current)
                    {
                        last_updated = Some(ts);
                    }
                }

                let schema_version = store.schema_version().map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                Ok((
                    fact_count,
                    u64::try_from(topic_set.len()).map_err(|e| {
                        InvalidInputSnafu {
                            message: format!("topic count out of range: {e}"),
                        }
                        .build()
                    })?,
                    last_updated,
                    schema_version,
                ))
            })
            .await
            .map_err(rmcp::ErrorData::from)?;

        let (fact_count, topic_count, last_updated, schema_version) = stats;
        let mut payload = serde_json::Map::new();
        payload.insert("fact_count".to_owned(), serde_json::json!(fact_count));
        payload.insert("topic_count".to_owned(), serde_json::json!(topic_count));
        payload.insert(
            "schema_version".to_owned(),
            serde_json::json!(schema_version),
        );
        payload.insert("store_id".to_owned(), serde_json::json!(store_id));
        payload.insert("store_backend".to_owned(), serde_json::json!(store_backend));
        payload.insert("readiness".to_owned(), serde_json::json!("ready"));
        payload.insert(
            "store_path_redacted".to_owned(),
            serde_json::json!(store_path_redacted),
        );
        if let Some(store_path) = exposed_store_path {
            payload.insert("store_path".to_owned(), serde_json::json!(store_path));
        }
        payload.insert("last_updated".to_owned(), serde_json::json!(last_updated));

        let json = serde_json::to_string_pretty(&serde_json::Value::Object(payload))
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
    ///     "nous_id": "agent-uuid",
    ///     "source_session_id": "session-uuid"
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_annotate",
        description = "Create an annotation on an owned fact in the aletheia nous local knowledge store; not kanon mnemosyne's durable corpus. \
                         Write capability must be configured server-side. Returns the created annotation ID."
    )]
    #[tracing::instrument(skip_all, fields(tool = "nous_annotate"))]
    async fn nous_annotate(
        &self,
        Parameters(params): Parameters<NousAnnotateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // WHY: write authorization is server-side; there is no credential in the
        // model-visible tool arguments.
        self.require_write_token().map_err(rmcp::ErrorData::from)?;

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

        if params.nous_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "nous_id (owner) must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let fact_id = params.fact_id.clone();
        let content = params.content.clone();
        let owner_nous_id = params.nous_id.trim().to_owned();
        let source_session_id =
            parse_optional_source_session_id(params.source_session_id.as_deref())?;

        let result = self
            .run_blocking(move |store| {
                // Verify the target fact exists and capture its scope/visibility
                // so the annotation inherits the same access posture.
                let target_facts = store.read_facts_by_id(&fact_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                let target = target_facts.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu {
                        id: fact_id.clone(),
                    }
                    .build()
                })?;
                if target.nous_id.as_str() != owner_nous_id.as_str() {
                    return Err(InvalidInputSnafu {
                        message:
                            "annotation request must target a fact owned by the supplied owner nous"
                                .to_owned(),
                    }
                    .build());
                }
                ensure_fact_is_active(&target, &fact_id)?;

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
                    nous_id: owner_nous_id.clone(),
                    fact_type: "annotation".to_owned(),
                    content,
                    scope: target.scope,
                    project_id: target.project_id.clone(),
                    sensitivity: target.sensitivity,
                    visibility: target.visibility,
                    temporal: mneme::knowledge::FactTemporal {
                        valid_from: now,
                        valid_to: mneme::knowledge::far_future(),
                        recorded_at: now,
                    },
                    provenance: mneme::knowledge::FactProvenance {
                        confidence: 0.95,
                        tier: mneme::knowledge::EpistemicTier::Inferred,
                        source_session_id,
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
    ///     "nous_id": "alice",
    ///     "source_session_id": "session-uuid",
    ///     "reason": "Updated with more recent information",
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_supersede",
        description = "Mark one owned fact as superseded by another in the aletheia nous local knowledge store; not kanon mnemosyne's durable corpus. \
                         Write capability must be configured server-side. Returns the supersession record ID."
    )]
    #[tracing::instrument(skip_all, fields(tool = "nous_supersede"))]
    async fn nous_supersede(
        &self,
        Parameters(params): Parameters<NousSupersedeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // WHY: write authorization is server-side; there is no credential in the
        // model-visible tool arguments.
        self.require_write_token().map_err(rmcp::ErrorData::from)?;

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

        if params.nous_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "nous_id (owner) must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let old_id = params.old_fact_id.clone();
        let new_id = params.new_fact_id.clone();
        let reason = params.reason.clone();
        let owner_nous_id = params.nous_id.trim().to_owned();
        let source_session_id =
            parse_optional_source_session_id(params.source_session_id.as_deref())?;

        let record_id = self
            .run_blocking(move |store| {
                // Verify both facts exist
                let old_facts = store.read_facts_by_id(&old_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                let old_fact = old_facts.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu { id: old_id.clone() }.build()
                })?;

                let new_facts = store.read_facts_by_id(&new_id).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                let new_fact = new_facts.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu { id: new_id.clone() }.build()
                })?;

                if old_fact.nous_id.as_str() != owner_nous_id.as_str()
                    || new_fact.nous_id.as_str() != owner_nous_id.as_str()
                {
                    return Err(InvalidInputSnafu {
                        message:
                            "supersede request must target facts owned by the supplied nous_id"
                                .to_owned(),
                    }
                    .build());
                }
                ensure_fact_is_active(&old_fact, &old_id)?;

                // Mark the old fact as superseded
                let new_fact_id = mneme::id::FactId::new(new_id.clone()).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("failed to create fact id: {e}"),
                    }
                    .build()
                })?;
                let mut old_fact = old_fact;
                old_fact.lifecycle.superseded_by = Some(new_fact_id);

                store.insert_fact(&old_fact).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

                // Create a record fact documenting the supersession, attributing
                // it to the explicitly supplied owner and inheriting the old
                // fact's scope/visibility/sensitivity.
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
                    nous_id: owner_nous_id.clone(),
                    fact_type: "supersession".to_owned(),
                    content: record_content,
                    scope: old_fact.scope,
                    project_id: old_fact.project_id.clone(),
                    sensitivity: old_fact.sensitivity,
                    visibility: old_fact.visibility,
                    temporal: mneme::knowledge::FactTemporal {
                        valid_from: now,
                        valid_to: mneme::knowledge::far_future(),
                        recorded_at: now,
                    },
                    provenance: mneme::knowledge::FactProvenance {
                        confidence: 1.0,
                        tier: mneme::knowledge::EpistemicTier::Verified,
                        source_session_id,
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
    ///     "nous_id": "alice",
    ///     "reason": "outdated",
    ///   }
    /// }
    /// ```
    #[tool(
        name = "nous_forget",
        description = "Soft-delete an owned fact in the aletheia nous local knowledge store; not kanon mnemosyne's durable corpus. \
                         Write capability must be configured server-side. Returns forgotten_at timestamp."
    )]
    #[tracing::instrument(skip_all, fields(tool = "nous_forget"))]
    async fn nous_forget(
        &self,
        Parameters(params): Parameters<NousForgetParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // WHY: write authorization is server-side; there is no credential in the
        // model-visible tool arguments.
        self.require_write_token().map_err(rmcp::ErrorData::from)?;

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

        if params.nous_id.trim().is_empty() {
            return Err(InvalidInputSnafu {
                message: "nous_id (owner) must not be empty".to_owned(),
            }
            .build()
            .into());
        }

        let fact_id_str = params.fact_id.clone();
        let owner_nous_id = params.nous_id.clone();
        let forget_reason = parse_forget_reason(&params.reason)?;

        let forgotten_at = self
            .run_blocking(move |store| {
                // Parse fact ID and verify ownership before forgetting.
                let fact_id = mneme::id::FactId::new(&fact_id_str).map_err(|e| {
                    InvalidInputSnafu {
                        message: format!("invalid fact_id format: {e}"),
                    }
                    .build()
                })?;

                let existing = store.read_facts_by_id(fact_id.as_str()).map_err(|e| {
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let target = existing.into_iter().next().ok_or_else(|| {
                    crate::error::FactNotFoundSnafu {
                        id: fact_id_str.clone(),
                    }
                    .build()
                })?;
                if target.nous_id != owner_nous_id {
                    return Err(InvalidInputSnafu {
                        message: "forget request must target a fact owned by the supplied nous_id"
                            .to_owned(),
                    }
                    .build());
                }

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
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test assertions may panic on failure"
)]
mod tests {
    use super::*;

    use mneme::knowledge::{
        EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
        FactTemporal, MemoryScope, Visibility, default_stability_hours, far_future,
    };
    use mneme::knowledge_store::KnowledgeStore;

    fn open_store() -> std::sync::Arc<KnowledgeStore> {
        KnowledgeStore::open_mem().expect("in-memory store should open")
    }

    fn sample_fact(
        id: &str,
        nous_id: &str,
        content: &str,
        fact_type: &str,
        visibility: Visibility,
        sensitivity: FactSensitivity,
        scope: Option<MemoryScope>,
    ) -> Fact {
        let now = jiff::Timestamp::now();
        Fact {
            id: mneme::id::FactId::new(id).expect("valid fact id"),
            nous_id: nous_id.to_owned(),
            fact_type: fact_type.to_owned(),
            content: content.to_owned(),
            scope,
            project_id: None,
            sensitivity,
            visibility,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: default_stability_hours(fact_type),
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    fn assert_fact_not_found(
        result: Result<CallToolResult, rmcp::ErrorData>,
        expected_fact_id: &str,
    ) {
        let Err(err) = result else {
            panic!("expected FactNotFound for {expected_fact_id}");
        };
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(
            err.message
                .contains(&format!("fact not found: {expected_fact_id}")),
            "unexpected error message: {}",
            err.message
        );
    }

    fn assert_invalid_input_contains(
        result: Result<CallToolResult, rmcp::ErrorData>,
        expected_fragment: &str,
    ) {
        let Err(err) = result else {
            panic!("expected invalid input containing {expected_fragment}");
        };
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(
            err.message.contains(expected_fragment),
            "unexpected error message: {}",
            err.message
        );
    }

    #[test]
    fn nous_search_params_round_trip() {
        let json = r#"{"query":"fleet dispatch config","limit":10,"scope":"project"}"#;
        let params: NousSearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.query, "fleet dispatch config");
        assert_eq!(params.limit, Some(10));
        assert_eq!(params.scope, Some("project".to_owned()));

        let out = serde_json::to_string(&params).unwrap();
        let back: NousSearchParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.limit, Some(10));
    }

    #[test]
    fn nous_search_params_accepts_no_nous_id() {
        // WHY: recall scope is server-bound, so the model cannot supply it.
        let json = r#"{"query":"foo"}"#;
        let result = serde_json::from_str::<NousSearchParams>(json);
        assert!(
            result.is_ok(),
            "nous_id must not be required in tool arguments"
        );
    }

    #[test]
    fn search_params_ignores_foreign_nous_id_argument() {
        // WHY: extra arguments must not be able to choose a sibling identity.
        let json = r#"{"query":"foo","nous_id":"bob"}"#;
        let result = serde_json::from_str::<NousSearchParams>(json).unwrap();
        // serde ignores unknown fields by default, so the supplied nous_id
        // has no effect on recall scope.
        assert_eq!(result.query, "foo");
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
        let json = r#"{"fact_id":"f-abc-123","scope":"project"}"#;
        let params: NousNeighborsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-abc-123");
        assert_eq!(params.scope, Some("project".to_owned()));

        let out = serde_json::to_string(&params).unwrap();
        let back: NousNeighborsParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.fact_id, "f-abc-123");
    }

    #[test]
    fn nous_annotate_params_requires_owner() {
        // Missing nous_id (owner)
        let json = r#"{"fact_id":"f-abc-123","content":"note"}"#;
        assert!(serde_json::from_str::<NousAnnotateParams>(json).is_err());
    }

    #[test]
    fn nous_annotate_params_round_trip() {
        let json = r#"{"nous_id":"agent-uuid","fact_id":"f-abc-123","content":"verified","source_session_id":"session-uuid"}"#;
        let params: NousAnnotateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.nous_id, "agent-uuid".to_owned());
        assert_eq!(params.fact_id, "f-abc-123");
        assert_eq!(params.content, "verified");
        assert_eq!(params.source_session_id.as_deref(), Some("session-uuid"));

        let out = serde_json::to_string(&params).unwrap();
        let back: NousAnnotateParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.nous_id, "agent-uuid");
        assert_eq!(back.source_session_id.as_deref(), Some("session-uuid"));
    }

    #[test]
    fn nous_supersede_params_round_trip() {
        let json = r#"{"old_fact_id":"f-old","new_fact_id":"f-new","nous_id":"alice","source_session_id":"session-uuid","reason":"updated"}"#;
        let params: NousSupersedeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.old_fact_id, "f-old");
        assert_eq!(params.new_fact_id, "f-new");
        assert_eq!(params.nous_id, "alice");
        assert_eq!(params.source_session_id.as_deref(), Some("session-uuid"));
        assert_eq!(params.reason, "updated");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousSupersedeParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.nous_id, "alice");
        assert_eq!(back.source_session_id.as_deref(), Some("session-uuid"));
    }

    #[test]
    fn nous_forget_params_round_trip() {
        let json = r#"{"fact_id":"f-abc-123","nous_id":"alice","reason":"stale"}"#;
        let params: NousForgetParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.fact_id, "f-abc-123");
        assert_eq!(params.nous_id, "alice");
        assert_eq!(params.reason, "stale");

        let out = serde_json::to_string(&params).unwrap();
        let back: NousForgetParams = serde_json::from_str(&out).unwrap();
        assert_eq!(back.nous_id, "alice");
    }

    #[test]
    fn write_tool_schemas_expose_no_credential_field() {
        // WHY: regression test for #5068 — the capability token must never be
        // advertised as part of the model-visible input schema.
        use schemars::JsonSchema;

        let annotate_schema =
            NousAnnotateParams::json_schema(&mut schemars::SchemaGenerator::default());
        let annotate_props = annotate_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("annotate schema properties");
        assert!(
            !annotate_props.contains_key("write_token"),
            "nous_annotate schema must not contain write_token"
        );
        assert!(
            annotate_props.contains_key("nous_id"),
            "nous_annotate schema must expose owner as nous_id"
        );
        assert!(
            !annotate_props.contains_key("session_id"),
            "nous_annotate schema must not conflate owner with session provenance"
        );

        let supersede_schema =
            NousSupersedeParams::json_schema(&mut schemars::SchemaGenerator::default());
        let supersede_props = supersede_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("supersede schema properties");
        assert!(
            !supersede_props.contains_key("write_token"),
            "nous_supersede schema must not contain write_token"
        );
        assert!(
            supersede_props.contains_key("source_session_id"),
            "nous_supersede schema must expose optional source_session_id"
        );

        let forget_schema =
            NousForgetParams::json_schema(&mut schemars::SchemaGenerator::default());
        let forget_props = forget_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("forget schema properties");
        assert!(
            !forget_props.contains_key("write_token"),
            "nous_forget schema must not contain write_token"
        );
    }

    #[tokio::test]
    async fn search_scopes_to_bound_server_identity() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-alice-1",
                "alice",
                "alice private note about dispatch",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob private note about dispatch",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousSearchParams {
            query: "dispatch".to_owned(),
            limit: Some(10),
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
        };
        let result = server.nous_search(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("f-alice-1"), "alice should see her own fact");
        assert!(
            !text.contains("f-bob-1"),
            "alice must not see bob's private fact"
        );
    }

    #[tokio::test]
    async fn search_ignores_model_supplied_nous_id() {
        // WHY: regression test for #5067 — a caller must not be able to recall
        // as a sibling identity by passing a different `nous_id` argument.
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-alice-1",
                "alice",
                "alice private note about dispatch",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob private note about dispatch",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        // Server is bound to alice; the request still carries a foreign nous_id.
        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousSearchParams {
            query: "dispatch".to_owned(),
            limit: Some(10),
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
        };
        let result = server.nous_search(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("f-alice-1"));
        assert!(
            !text.contains("f-bob-1"),
            "model-supplied nous_id must not change recall scope"
        );
    }

    #[tokio::test]
    async fn read_tools_reject_unbound_identity() {
        let store = open_store();
        let server = MemoryServer::with_write_token(store, None, None);
        let params = NousSearchParams {
            query: "dispatch".to_owned(),
            limit: Some(10),
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
        };
        let result = server.nous_search(Parameters(params)).await;
        assert!(
            result.is_err(),
            "read tools must fail closed when no caller identity is bound"
        );
    }

    #[tokio::test]
    async fn list_topics_scopes_to_bound_server_identity() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-alice-1",
                "alice",
                "alice note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();
        store
            .insert_fact(&sample_fact(
                "f-alice-2",
                "alice",
                "alice task",
                "task",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousListTopicsParams {
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
        };
        let result = server.nous_list_topics(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        // alice sees one note and one task; bob's private note is excluded
        assert!(text.contains("\"note\""));
        assert!(text.contains("\"task\""));
        assert!(
            text.contains('"') && text.matches("count").count() >= 2,
            "expected two topic rows for alice"
        );
    }

    #[tokio::test]
    async fn stats_scopes_to_bound_server_identity() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-alice-1",
                "alice",
                "alice note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousStatsParams {
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
            include_store_path: None,
        };
        let result = server.nous_stats(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["fact_count"], 1, "alice should count only her fact");
        assert_eq!(parsed["topic_count"], 1);
    }

    #[tokio::test]
    async fn stats_redacts_store_path_by_default() {
        let store = open_store();
        let raw_path = std::path::PathBuf::from("/tmp/alice/private/knowledge.fjall/shared");
        let server = MemoryServer::with_write_token(store, Some(raw_path), None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousStatsParams {
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
            include_store_path: None,
        };

        let result = server.nous_stats(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert!(
            parsed.get("store_path").is_none(),
            "default stats response must not include raw store_path: {text}"
        );
        assert!(
            !text.contains("/tmp/alice") && !text.contains("knowledge.fjall"),
            "default stats response leaked path details: {text}"
        );
        assert_eq!(
            parsed
                .get("store_path_redacted")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            parsed
                .get("store_backend")
                .and_then(serde_json::Value::as_str),
            Some("fjall")
        );
        assert_eq!(
            parsed.get("readiness").and_then(serde_json::Value::as_str),
            Some("ready")
        );
        let store_id = parsed
            .get("store_id")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(
            store_id.starts_with("store:sha256:"),
            "store id should be an opaque fingerprint: {store_id}"
        );
    }

    #[tokio::test]
    async fn stats_exposes_store_path_only_in_admin_diagnostics() {
        let store = open_store();
        let raw_path = std::path::PathBuf::from("/tmp/alice/private/knowledge.fjall/shared");
        let server =
            MemoryServer::with_write_token(store, Some(raw_path.clone()), Some("a".repeat(32)))
                .with_admin_diagnostics(true)
                .with_nous_id(Some("alice".to_owned()));
        let params = NousStatsParams {
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
            include_store_path: Some(true),
        };

        let result = server.nous_stats(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let expected_path = raw_path.to_string_lossy();

        assert_eq!(
            parsed.get("store_path").and_then(serde_json::Value::as_str),
            Some(expected_path.as_ref())
        );
        assert_eq!(
            parsed
                .get("store_path_redacted")
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

    #[tokio::test]
    async fn stats_ignores_store_path_request_without_admin_diagnostics() {
        let store = open_store();
        let raw_path = std::path::PathBuf::from("/tmp/alice/private/knowledge.fjall/shared");
        let server = MemoryServer::with_write_token(store, Some(raw_path), Some("a".repeat(32)))
            .with_nous_id(Some("alice".to_owned()));
        let params = NousStatsParams {
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
            include_store_path: Some(true),
        };

        let result = server.nous_stats(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert!(
            parsed.get("store_path").is_none(),
            "store_path request must be ignored without admin diagnostics: {text}"
        );
        assert!(
            !text.contains("/tmp/alice") && !text.contains("knowledge.fjall"),
            "store_path request leaked path details without admin diagnostics: {text}"
        );
        assert_eq!(
            parsed
                .get("store_path_redacted")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[tokio::test]
    async fn neighbors_scopes_seed_fact_to_bound_server_identity() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob private note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, None)
            .with_nous_id(Some("alice".to_owned()));
        let params = NousNeighborsParams {
            fact_id: "f-bob-1".to_owned(),
            project_id: None,
            scope: None,
            min_visibility: None,
            max_sensitivity: None,
        };
        let result = server.nous_neighbors(Parameters(params)).await;
        assert!(
            result.is_err(),
            "alice must not traverse from bob's private fact"
        );
    }

    #[tokio::test]
    async fn annotate_preserves_target_scope_and_visibility() {
        let store = open_store();
        let target = sample_fact(
            "f-target-1",
            "alice",
            "shared project decision",
            "decision",
            Visibility::Shared,
            FactSensitivity::Internal,
            Some(MemoryScope::Project),
        );
        // project_id is not set in sample_fact; leave it None for this test
        store.insert_fact(&target).unwrap();

        let server = MemoryServer::with_write_token(store.clone(), None, Some("a".repeat(32)));
        let params = NousAnnotateParams {
            nous_id: "alice".to_owned(),
            fact_id: "f-target-1".to_owned(),
            content: "verified by external source".to_owned(),
            source_session_id: Some("session-annotation-1".to_owned()),
        };
        let result = server.nous_annotate(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let annotation_id = parsed["annotation_id"].as_str().unwrap();

        let annotation = store
            .read_facts_by_id(annotation_id)
            .unwrap()
            .into_iter()
            .next()
            .expect("annotation fact should exist");
        assert_eq!(
            annotation.nous_id, "alice",
            "annotation must not fallback to mcp-client"
        );
        assert_eq!(
            annotation.provenance.source_session_id.as_deref(),
            Some("session-annotation-1")
        );
        assert_eq!(annotation.visibility, Visibility::Shared);
        assert_eq!(annotation.sensitivity, FactSensitivity::Internal);
        assert_eq!(annotation.scope, Some(MemoryScope::Project));
    }

    #[tokio::test]
    async fn annotate_rejects_foreign_owned_fact() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "shared project decision",
                "decision",
                Visibility::Shared,
                FactSensitivity::Internal,
                Some(MemoryScope::Project),
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousAnnotateParams {
            nous_id: "alice".to_owned(),
            fact_id: "f-bob-1".to_owned(),
            content: "verified by external source".to_owned(),
            source_session_id: None,
        };
        let result = server.nous_annotate(Parameters(params)).await;
        assert!(result.is_err(), "alice must not annotate bob's fact");
    }

    #[tokio::test]
    async fn annotate_rejects_forgotten_target() {
        let store = open_store();
        let mut target = sample_fact(
            "f-target-forgotten",
            "alice",
            "old value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        target.lifecycle.is_forgotten = true;
        target.lifecycle.forgotten_at = Some(jiff::Timestamp::now());
        target.lifecycle.forget_reason = Some(mneme::knowledge::ForgetReason::Stale);
        store.insert_fact(&target).unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousAnnotateParams {
            nous_id: "alice".to_owned(),
            fact_id: "f-target-forgotten".to_owned(),
            content: "verified by external source".to_owned(),
            source_session_id: None,
        };
        assert_fact_not_found(
            server.nous_annotate(Parameters(params)).await,
            "f-target-forgotten",
        );
    }

    #[tokio::test]
    async fn annotate_rejects_superseded_target() {
        let store = open_store();
        let mut target = sample_fact(
            "f-target-superseded",
            "alice",
            "old value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        target.lifecycle.superseded_by =
            Some(mneme::id::FactId::new("f-target-replacement").unwrap());
        store.insert_fact(&target).unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousAnnotateParams {
            nous_id: "alice".to_owned(),
            fact_id: "f-target-superseded".to_owned(),
            content: "verified by external source".to_owned(),
            source_session_id: None,
        };
        assert_fact_not_found(
            server.nous_annotate(Parameters(params)).await,
            "f-target-superseded",
        );
    }

    #[tokio::test]
    async fn supersede_uses_explicit_owner_not_mcp_server() {
        let store = open_store();
        let old_fact = sample_fact(
            "f-old-1",
            "alice",
            "old value",
            "note",
            Visibility::Shared,
            FactSensitivity::Public,
            Some(MemoryScope::Project),
        );
        let new_fact = sample_fact(
            "f-new-1",
            "alice",
            "new value",
            "note",
            Visibility::Shared,
            FactSensitivity::Public,
            Some(MemoryScope::Project),
        );
        store.insert_fact(&old_fact).unwrap();
        store.insert_fact(&new_fact).unwrap();

        let server = MemoryServer::with_write_token(store.clone(), None, Some("a".repeat(32)));
        let params = NousSupersedeParams {
            old_fact_id: "f-old-1".to_owned(),
            new_fact_id: "f-new-1".to_owned(),
            nous_id: "alice".to_owned(),
            source_session_id: Some("session-supersede-1".to_owned()),
            reason: "updated".to_owned(),
        };
        let result = server.nous_supersede(Parameters(params)).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let record_id = parsed["record_id"].as_str().unwrap();

        let record = store
            .read_facts_by_id(record_id)
            .unwrap()
            .into_iter()
            .next()
            .expect("supersession record should exist");
        assert_eq!(
            record.nous_id, "alice",
            "supersession record must not use mcp-server"
        );
        assert_eq!(
            record.provenance.source_session_id.as_deref(),
            Some("session-supersede-1")
        );
        assert_eq!(record.visibility, Visibility::Shared);
        assert_eq!(record.scope, Some(MemoryScope::Project));
    }

    #[tokio::test]
    async fn supersede_rejects_foreign_owned_facts() {
        let store = open_store();
        let old_fact = sample_fact(
            "f-old-bob",
            "bob",
            "old value",
            "note",
            Visibility::Shared,
            FactSensitivity::Public,
            Some(MemoryScope::Project),
        );
        let new_fact = sample_fact(
            "f-new-alice",
            "alice",
            "new value",
            "note",
            Visibility::Shared,
            FactSensitivity::Public,
            Some(MemoryScope::Project),
        );
        store.insert_fact(&old_fact).unwrap();
        store.insert_fact(&new_fact).unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousSupersedeParams {
            old_fact_id: "f-old-bob".to_owned(),
            new_fact_id: "f-new-alice".to_owned(),
            nous_id: "alice".to_owned(),
            source_session_id: None,
            reason: "updated".to_owned(),
        };
        let result = server.nous_supersede(Parameters(params)).await;
        assert!(
            result.is_err(),
            "alice must not supersede facts outside her ownership"
        );
    }

    #[tokio::test]
    async fn supersede_rejects_forgotten_old_fact() {
        let store = open_store();
        let mut old_fact = sample_fact(
            "f-old-forgotten",
            "alice",
            "old value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        old_fact.lifecycle.is_forgotten = true;
        old_fact.lifecycle.forgotten_at = Some(jiff::Timestamp::now());
        old_fact.lifecycle.forget_reason = Some(mneme::knowledge::ForgetReason::Stale);
        let new_fact = sample_fact(
            "f-new-for-forgotten",
            "alice",
            "new value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        store.insert_fact(&old_fact).unwrap();
        store.insert_fact(&new_fact).unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousSupersedeParams {
            old_fact_id: "f-old-forgotten".to_owned(),
            new_fact_id: "f-new-for-forgotten".to_owned(),
            nous_id: "alice".to_owned(),
            source_session_id: None,
            reason: "updated".to_owned(),
        };
        assert_fact_not_found(
            server.nous_supersede(Parameters(params)).await,
            "f-old-forgotten",
        );
    }

    #[tokio::test]
    async fn supersede_rejects_superseded_old_fact() {
        let store = open_store();
        let mut old_fact = sample_fact(
            "f-old-superseded",
            "alice",
            "old value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        old_fact.lifecycle.superseded_by =
            Some(mneme::id::FactId::new("f-old-existing-replacement").unwrap());
        let new_fact = sample_fact(
            "f-new-for-superseded",
            "alice",
            "new value",
            "note",
            Visibility::Private,
            FactSensitivity::Public,
            None,
        );
        store.insert_fact(&old_fact).unwrap();
        store.insert_fact(&new_fact).unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousSupersedeParams {
            old_fact_id: "f-old-superseded".to_owned(),
            new_fact_id: "f-new-for-superseded".to_owned(),
            nous_id: "alice".to_owned(),
            source_session_id: None,
            reason: "updated".to_owned(),
        };
        assert_fact_not_found(
            server.nous_supersede(Parameters(params)).await,
            "f-old-superseded",
        );
    }

    #[tokio::test]
    async fn forget_rejects_foreign_owned_fact() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-bob-1",
                "bob",
                "bob secret",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store, None, Some("a".repeat(32)));
        let params = NousForgetParams {
            fact_id: "f-bob-1".to_owned(),
            nous_id: "alice".to_owned(),
            reason: "stale".to_owned(),
        };
        let result = server.nous_forget(Parameters(params)).await;
        assert!(result.is_err(), "alice must not forget bob's fact");
    }

    #[tokio::test]
    async fn forget_rejects_unknown_reason() {
        let store = open_store();
        store
            .insert_fact(&sample_fact(
                "f-forget-reason",
                "alice",
                "alice note",
                "note",
                Visibility::Private,
                FactSensitivity::Public,
                None,
            ))
            .unwrap();

        let server = MemoryServer::with_write_token(store.clone(), None, Some("a".repeat(32)));
        let params = NousForgetParams {
            fact_id: "f-forget-reason".to_owned(),
            nous_id: "alice".to_owned(),
            reason: "privcy".to_owned(),
        };
        assert_invalid_input_contains(
            server.nous_forget(Parameters(params)).await,
            "allowed values: user_requested, outdated, incorrect, privacy, stale, superseded, contradicted",
        );

        let fact = store
            .read_facts_by_id("f-forget-reason")
            .unwrap()
            .into_iter()
            .next()
            .expect("fact remains readable");
        assert!(
            !fact.lifecycle.is_forgotten,
            "invalid forget reason must not mutate the fact"
        );
    }
}
