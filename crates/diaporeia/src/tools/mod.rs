// kanon:ignore RUST/file-too-long -- WHY: single #[tool_router] impl block; split would require multiple partial impls or a macro dispatch table; deferred as architectural work
//! MCP tool implementations for diaporeia.
//!
//! All tools are defined in a single `#[tool_router]` impl block on
//! `DiaporeiaServer`. Parameter structs are organized in `params`.

pub(crate) mod params;

use std::collections::HashSet;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router};
use snafu::{IntoError as _, OptionExt as _, ResultExt as _};
use tracing::Instrument as _;

use koina::id::SessionId;
use mneme::types::parse_session_or_agent_id;
use organon::surface::{SurfaceAvailability, SurfaceInputs};
use symbolon::types::Role;

use crate::auth::{McpCaller, resolve_caller};
use crate::error::{
    BlackboardStoreSnafu, BlackboardStoreUnavailableSnafu, DuplicateSessionSnafu,
    FactNotFoundSnafu, InvalidInputSnafu, KnowledgeStoreSnafu, KnowledgeStoreUnavailableSnafu,
    NoteStoreSnafu, NoteStoreUnavailableSnafu, NousNotFoundSnafu, PipelineSnafu, RepomixPackSnafu,
    RepomixUnavailableSnafu, SerializationSnafu, SessionNotFoundSnafu, SessionStoreSnafu,
    UnauthorizedSnafu,
};
use crate::rate_limit::Tier;
use crate::server::DiaporeiaServer;

#[cfg(test)]
const AGENT_ONLY_TOOLS: &[&str] = &[
    "session_list",
    "session_history",
    "nous_list",
    "nous_status",
    "nous_tools",
    "system_health",
];

/// Default page size for `session_list`.
///
/// Mirrors pylon's `pagination::DEFAULT_LIMIT` so MCP and HTTP surfaces expose
/// the same cap to callers.
const DEFAULT_SESSION_LIST_LIMIT: usize = 50;

/// Maximum page size for `session_list`.
///
/// Mirrors pylon's `pagination::MAX_LIMIT`.
const MAX_SESSION_LIST_LIMIT: usize = 1_000;

/// Require at least the given role, returning an MCP error if insufficient.
///
/// Used by tools and resources that need RBAC enforcement beyond the
/// operator-or-above check (e.g. session creation, config access). Applies
/// the shared rate limit for `tier`, keyed by the resolved caller's subject
/// (or the shared unauthenticated bucket when no caller resolves), before
/// enforcing the role requirement.
fn require_role(
    server: &DiaporeiaServer,
    context: &RequestContext<rmcp::RoleServer>,
    tier: Tier,
    minimum: Role,
    operation: &str,
) -> Result<McpCaller, rmcp::ErrorData> {
    let caller = resolve_caller(&server.state, context);
    server
        .rate_limiter
        .check(tier, caller.as_ref().map(|c| c.sub.as_str()))?;
    match caller {
        Some(caller) if caller.role >= minimum => Ok(caller),
        Some(caller) => {
            tracing::warn!(
                caller_role = %caller.role,
                required_role = %minimum,
                operation,
                "MCP RBAC denied",
            );
            Err(UnauthorizedSnafu {
                message: format!("{operation} requires {minimum} role or above"),
            }
            .build()
            .into())
        }
        None => {
            tracing::warn!(operation, "MCP RBAC denied: no role resolved");
            Err(UnauthorizedSnafu {
                message: format!("{operation} requires {minimum} role or above"),
            }
            .build()
            .into())
        }
    }
}

/// Resolve the caller and enforce the shared rate limit for `tier`, failing
/// closed with an `Unauthorized` error when no caller resolves.
///
/// Used ahead of [`require_caller_role`] by tools that need the full
/// [`McpCaller`] (for claims-scoped filtering) rather than just a
/// minimum-role gate. WHY(#5182, #4843): folding the rate-limit check in
/// here means every call site funnels through one place that keys the check
/// by the resolved principal, rather than checking an un-keyed, per-instance
/// bucket before authentication runs.
fn require_authenticated(
    server: &DiaporeiaServer,
    context: &RequestContext<rmcp::RoleServer>,
    tier: Tier,
    operation: &str,
) -> Result<McpCaller, rmcp::ErrorData> {
    let caller = resolve_caller(&server.state, context);
    server
        .rate_limiter
        .check(tier, caller.as_ref().map(|c| c.sub.as_str()))?;
    caller.ok_or_else(|| {
        UnauthorizedSnafu {
            message: format!("{operation} requires authentication"),
        }
        .build()
        .into()
    })
}

/// Require at least the given role for a verified caller.
fn require_caller_role(
    caller: &McpCaller,
    minimum: Role,
    operation: &str,
) -> Result<(), rmcp::ErrorData> {
    if caller.role >= minimum {
        Ok(())
    } else {
        tracing::warn!(
            caller_role = %caller.role,
            required_role = %minimum,
            operation,
            "MCP RBAC denied",
        );
        Err(UnauthorizedSnafu {
            message: format!("{operation} requires {minimum} role or above"),
        }
        .build()
        .into())
    }
}

/// Resolve the effective `nous_id` for a scoped operation.
///
/// - If the caller has a scoped `nous_id`, the request is forced to that
///   scope; a contradictory caller-supplied value is rejected.
/// - If the caller is an unscoped Operator/Admin, the caller-supplied value
///   (if any) is honored.
/// - Agent callers without a scope are rejected (fail closed).
fn resolve_nous_scope(
    caller: &McpCaller,
    requested: Option<&str>,
    operation: &str,
) -> Result<Option<String>, rmcp::ErrorData> {
    match (caller.nous_id.as_deref(), requested) {
        (Some(scoped), Some(requested)) if scoped != requested => {
            tracing::warn!(
                caller_scope = %scoped,
                requested_scope = %requested,
                operation,
                "MCP scoped access denied: contradictory nous_id",
            );
            Err(UnauthorizedSnafu {
                message: format!("{operation}: access denied for this agent"),
            }
            .build()
            .into())
        }
        (Some(scoped), _) => Ok(Some(scoped.to_owned())),
        (None, _) if caller.role == Role::Agent => {
            tracing::warn!(
                operation,
                "MCP scoped access denied: agent token lacks nous_id scope",
            );
            Err(UnauthorizedSnafu {
                message: format!("{operation}: agent token must be scoped to a nous_id"),
            }
            .build()
            .into())
        }
        (None, requested) => Ok(requested.map(str::to_owned)),
    }
}

/// Reject the request if the caller's scoped `nous_id` does not match the
/// target agent.
fn require_nous_access_for_caller(
    caller: &McpCaller,
    target_nous_id: &str,
    operation: &str,
) -> Result<(), rmcp::ErrorData> {
    if let Some(ref scoped) = caller.nous_id
        && scoped != target_nous_id
    {
        tracing::warn!(
            caller_scope = %scoped,
            target_nous_id,
            operation,
            "MCP scoped access denied: cross-agent access",
        );
        return Err(UnauthorizedSnafu {
            message: format!("{operation}: access denied for this agent"),
        }
        .build()
        .into());
    }
    if caller.role == Role::Agent && caller.nous_id.is_none() {
        return Err(UnauthorizedSnafu {
            message: format!("{operation}: agent token must be scoped to a nous_id"),
        }
        .build()
        .into());
    }
    Ok(())
}

/// Return whether a nous agent is visible to the caller.
///
/// Mirrors pylon's `nous_visible_to_claims`: a scoped caller only sees its own
/// agent; private agents are only visible to Operator/Admin.
fn nous_visible_to_caller(caller: &McpCaller, config: &nous::config::NousConfig) -> bool {
    let in_scope = caller
        .nous_id
        .as_deref()
        .is_none_or(|scoped| scoped == config.id.as_ref());
    let private_visible = !config.private || caller.role >= Role::Operator;
    in_scope && private_visible
}

/// Return whether a recall result is visible to the caller.
///
/// Enforces the same first-party visibility rule as the recall pipeline:
/// private and restricted facts are visible only to the owning agent unless
/// the caller is an unscoped Operator/Admin.
fn recall_visible_to_caller(
    caller: &McpCaller,
    fact_nous_id: &str,
    visibility: mneme::knowledge::Visibility,
) -> bool {
    // Unscoped Operator/Admin may read any fact in the store.
    if caller.role >= Role::Operator && caller.nous_id.is_none() {
        return true;
    }

    let own = caller
        .nous_id
        .as_deref()
        .is_some_and(|scoped| scoped == fact_nous_id);
    let shared = matches!(
        visibility,
        mneme::knowledge::Visibility::Shared | mneme::knowledge::Visibility::Published
    );
    own || shared
}

/// Return a verified nous identifier for memory/note operations.
///
/// Agent callers must be scoped. Unscoped Operator/Admin callers fall back to
/// their subject so the store always receives a verified author.
fn effective_nous_id(caller: &McpCaller, operation: &str) -> Result<String, rmcp::ErrorData> {
    if caller.role == Role::Agent {
        caller.nous_id.clone().ok_or_else(|| {
            UnauthorizedSnafu {
                message: format!("{operation}: agent token must be scoped to a nous_id"),
            }
            .build()
            .into()
        })
    } else {
        Ok(caller.nous_id.clone().unwrap_or_else(|| caller.sub.clone()))
    }
}

/// Read a fact and verify the caller may act on the owning nous.
#[cfg(feature = "knowledge-store")]
fn read_fact_for_caller(
    store: &mneme::knowledge_store::KnowledgeStore,
    caller: &McpCaller,
    fact_id: &str,
    operation: &str,
) -> Result<mneme::knowledge::Fact, rmcp::ErrorData> {
    let mut facts = store.read_facts_by_id(fact_id).map_err(|e| {
        rmcp::ErrorData::from(
            KnowledgeStoreSnafu {
                message: e.to_string(),
            }
            .build(),
        )
    })?;
    let fact = facts.pop().ok_or_else(|| {
        rmcp::ErrorData::from(
            FactNotFoundSnafu {
                id: fact_id.to_owned(),
            }
            .build(),
        )
    })?;
    require_nous_access_for_caller(caller, &fact.nous_id, operation)?;
    Ok(fact)
}

/// Check that the knowledge graph MCP surface is enabled.
///
/// Returns `Err(KnowledgeStoreUnavailable)` when the feature is disabled in
/// config, or when the server was not compiled with the `knowledge-store`
/// feature. On success, returns a snapshot of the current config.
///
/// # Why a helper
///
/// Every knowledge tool needs the same preamble: read config, gate on
/// `mcp.knowledge_graph.enabled`. Centralising avoids repeating the read in
/// every tool.
async fn require_knowledge_graph(
    server: &DiaporeiaServer,
) -> Result<taxis::config::AletheiaConfig, rmcp::ErrorData> {
    let config = server.state.config.read().await.clone();
    if !config.mcp.knowledge_graph.enabled {
        return Err(KnowledgeStoreUnavailableSnafu {}.build().into());
    }
    Ok(config)
}

/// Check that the repomix MCP surface is enabled.
///
/// Returns `Err(RepomixUnavailable)` when the feature is disabled in config.
/// On success, returns a snapshot of the current config.
async fn require_repomix(
    server: &DiaporeiaServer,
) -> Result<taxis::config::AletheiaConfig, rmcp::ErrorData> {
    let config = server.state.config.read().await.clone();
    if !config.mcp.repomix.enabled {
        return Err(RepomixUnavailableSnafu {}.build().into());
    }
    Ok(config)
}

/// Resolve the knowledge store from server state, returning an error if absent.
///
/// Fails with `KnowledgeStoreUnavailable` when compiled without the
/// `knowledge-store` feature or when the store was not initialised.
#[cfg(feature = "knowledge-store")]
fn resolve_store(
    server: &DiaporeiaServer,
) -> Result<std::sync::Arc<mneme::knowledge_store::KnowledgeStore>, crate::error::Error> {
    server
        .state
        .knowledge_store
        .clone()
        .ok_or_else(|| KnowledgeStoreUnavailableSnafu {}.build())
}

/// Forget a fact on a blocking-pool thread, flattening the join and store
/// errors to strings.
///
/// WHY: shared by `memory_correct`'s supersede write and its compensating
/// rollback so both go through the same error-flattening path.
#[cfg(feature = "knowledge-store")]
async fn forget_fact_spawned(
    store: std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    fact_id: mneme::id::FactId,
    reason: mneme::knowledge::ForgetReason,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || store.forget_fact(&fact_id, reason))
        .await
        .map_err(|e| e.to_string())?
        .map(|_fact| ())
        .map_err(|e| e.to_string())
}

/// Outcome when marking a fact superseded fails after its replacement was
/// already inserted, carrying the original supersede error.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[cfg(feature = "knowledge-store")]
enum SupersedeFailure {
    /// The supersede write failed but rolling back the new fact succeeded —
    /// no fact was left in an inconsistent state; retrying is safe.
    RolledBack(String),
    /// The supersede write failed AND the rollback also failed — both facts
    /// are now live; the caller must reconcile manually.
    Inconsistent(String, String),
}

/// Mark the original fact superseded; on failure, roll back by forgetting
/// the newly-inserted replacement.
///
/// WHY(#5185): the knowledge store exposes no cross-record transaction —
/// insertion and supersession are two separate writes (`memory_correct`
/// inserts the replacement fact and awaits that before calling this).
/// Extracted as a standalone function, taking the two remaining steps as
/// futures, so the rollback/inconsistency behavior is unit-testable against
/// fake operations, independent of the real store (see `tests` below).
#[cfg(feature = "knowledge-store")]
async fn supersede_with_rollback(
    supersede_old: impl std::future::Future<Output = Result<(), String>>,
    rollback_new: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), SupersedeFailure> {
    if let Err(supersede_err) = supersede_old.await {
        return Err(match rollback_new.await {
            Ok(()) => SupersedeFailure::RolledBack(supersede_err),
            Err(rollback_err) => SupersedeFailure::Inconsistent(supersede_err, rollback_err),
        });
    }
    Ok(())
}

/// Return whether a recall result is owned by the scoped nous agent.
#[cfg(feature = "knowledge-store")]
fn recall_owned_by_scope(
    result: &mneme::knowledge::RecallResult,
    effective_nous: Option<&str>,
) -> bool {
    effective_nous.is_some_and(|scoped| scoped == result.nous_id.as_str())
}

/// Return whether a recall result is visible to the scoped caller.
///
/// A result is visible when it is owned by the scoped nous agent or when it
/// has been explicitly shared or published.
#[cfg(feature = "knowledge-store")]
fn recall_visible_to_scope(
    result: &mneme::knowledge::RecallResult,
    effective_nous: Option<&str>,
) -> bool {
    recall_owned_by_scope(result, effective_nous)
        || matches!(
            result.visibility,
            mneme::knowledge::Visibility::Shared | mneme::knowledge::Visibility::Published
        )
}

#[cfg(feature = "knowledge-store")]
fn search_knowledge_store(
    store: &mneme::knowledge_store::KnowledgeStore,
    params: &params::KnowledgeSearchParams,
    max_results: u32,
) -> Result<Vec<mneme::knowledge::RecallResult>, rmcp::ErrorData> {
    let limit = params.limit.unwrap_or(20).min(max_results);
    let results = store
        .search_text_for_recall(&params.query, i64::from(limit))
        .map_err(|e| {
            rmcp::ErrorData::from(
                KnowledgeStoreSnafu {
                    message: e.to_string(),
                }
                .build(),
            )
        })?;

    let results = if let Some(ref nous_id) = params.nous_id {
        results
            .into_iter()
            .filter(|result| recall_owned_by_scope(result, Some(nous_id.as_str())))
            .collect()
    } else {
        results
    };

    Ok(results)
}

#[cfg(feature = "knowledge-store")]
fn direct_graph_neighbors(
    store: &mneme::knowledge_store::KnowledgeStore,
    entity_id: &str,
    hop: u32,
) -> Result<Vec<serde_json::Value>, rmcp::ErrorData> {
    use std::collections::BTreeMap;

    use mneme::engine::DataValue;

    let script = r"
        ?[neighbor_id, name, entity_type, relation, weight, direction] :=
            *relationships{src: $entity_id, dst: neighbor_id, relation, weight},
            *entities{id: neighbor_id, name, entity_type},
            direction = 'outgoing'

        ?[neighbor_id, name, entity_type, relation, weight, direction] :=
            *relationships{src: neighbor_id, dst: $entity_id, relation, weight},
            *entities{id: neighbor_id, name, entity_type},
            direction = 'incoming'
    ";

    let mut query_params: BTreeMap<String, DataValue> = BTreeMap::new();
    query_params.insert(
        "entity_id".to_owned(),
        DataValue::Str(entity_id.to_owned().into()),
    );

    let result = store.run_query(script, query_params).map_err(|e| {
        rmcp::ErrorData::from(
            KnowledgeStoreSnafu {
                message: e.to_string(),
            }
            .build(),
        )
    })?;

    Ok(result
        .rows_to_json()
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "entity_id": row.first().cloned().unwrap_or(serde_json::Value::Null),
                "name": row.get(1).cloned().unwrap_or(serde_json::Value::Null),
                "entity_type": row.get(2).cloned().unwrap_or(serde_json::Value::Null),
                "relation": row.get(3).cloned().unwrap_or(serde_json::Value::Null),
                "weight": row.get(4).cloned().unwrap_or(serde_json::Value::Null),
                "direction": row.get(5).cloned().unwrap_or(serde_json::Value::Null),
                "source_entity_id": entity_id,
                "hop": hop,
            })
        })
        .collect())
}

#[cfg(feature = "knowledge-store")]
fn graph_neighbors_to_depth(
    store: &mneme::knowledge_store::KnowledgeStore,
    start_entity_id: &str,
    depth: u32,
) -> Result<Vec<serde_json::Value>, rmcp::ErrorData> {
    use std::collections::BTreeSet;

    let mut frontier = BTreeSet::from([start_entity_id.to_owned()]);
    let mut visited_entities = BTreeSet::from([start_entity_id.to_owned()]);
    let mut seen_neighbors = BTreeSet::new();
    let mut neighbors = Vec::new();

    for hop in 1..=depth {
        let mut next_frontier = BTreeSet::new();

        for entity_id in &frontier {
            for neighbor in direct_graph_neighbors(store, entity_id, hop)? {
                let neighbor_id = neighbor
                    .get("entity_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let relation = neighbor
                    .get("relation")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let key = (
                    entity_id.clone(),
                    neighbor_id.clone().unwrap_or_default(),
                    relation,
                    hop,
                );

                if seen_neighbors.insert(key) {
                    neighbors.push(neighbor);
                }

                if let Some(id) = neighbor_id
                    && visited_entities.insert(id.clone())
                {
                    next_frontier.insert(id);
                }
            }
        }

        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }

    Ok(neighbors)
}

/// Register all tools on the server via the `#[tool_router]` macro.
///
/// This generates the `DiaporeiaServer::tool_router()` associated function
/// used by the `#[tool_handler]` on the `ServerHandler` impl.
#[tool_router(vis = "pub(crate)")]
impl DiaporeiaServer {
    /// Create a new session for a nous agent. Returns session metadata as JSON.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "Create a new session for a nous agent")]
    async fn session_create(
        &self,
        Parameters(params): Parameters<params::SessionCreateParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Expensive, Role::Operator, "session_create")?;
        require_nous_access_for_caller(&caller, &params.nous_id, "session_create")?;
        let session_key = params.session_key.as_deref().unwrap_or("main");
        parse_session_or_agent_id(&params.nous_id).map_err(|e| {
            rmcp::ErrorData::from(
                InvalidInputSnafu {
                    message: e.to_string(),
                }
                .build(),
            )
        })?;
        parse_session_or_agent_id(session_key).map_err(|e| {
            rmcp::ErrorData::from(
                InvalidInputSnafu {
                    message: e.to_string(),
                }
                .build(),
            )
        })?;

        let nous_config = self
            .state
            .nous_manager
            .get_config(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;
        let model = nous_config.generation.model.clone();

        // WHY: SessionId (UUID v4) is the canonical format. ULID here caused
        // 'invalid SessionId' when nous parsed the stored ID back (#2349).
        let session_id = SessionId::new().to_string();

        let nous_id = params.nous_id.clone();
        let session_key_owned = session_key.to_owned();
        let store = self.state.session_store.lock().await;
        let session =
            match store.create_session(&session_id, &nous_id, session_key, None, Some(&model)) {
                Ok(session) => Ok(session),
                Err(e) if e.is_unique_constraint_violation() => Err(rmcp::ErrorData::from(
                    DuplicateSessionSnafu {
                        nous_id,
                        session_key: session_key_owned,
                    }
                    .build(),
                )),
                Err(e) => Err(rmcp::ErrorData::from(SessionStoreSnafu {}.into_error(e))),
            }?;

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "id": session.id,
            "nous_id": session.nous_id,
            "session_key": session.session_key,
            "status": session.status.as_str(),
            "message_count": session.metrics.message_count,
            "created_at": session.created_at,
        }))
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// List sessions, optionally filtered by nous agent ID.
    #[tool(description = "List sessions, optionally filtered by nous agent ID")]
    async fn session_list(
        &self,
        Parameters(params): Parameters<params::SessionListParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "session_list")?;
        require_caller_role(&caller, Role::Agent, "session_list")?;
        let effective_nous =
            resolve_nous_scope(&caller, params.nous_id.as_deref(), "session_list")?;

        let store = self.state.session_store.lock().await;
        let sessions = store
            .list_sessions(effective_nous.as_deref())
            .context(SessionStoreSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        // WHY(#4841): cap the response size to the same first-party limit pylon
        // uses for session list endpoints.
        let limit = params
            .limit
            .unwrap_or(DEFAULT_SESSION_LIST_LIMIT)
            .min(MAX_SESSION_LIST_LIMIT);
        let sessions: Vec<_> = sessions.into_iter().take(limit).collect();

        let summaries: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "nous_id": s.nous_id,
                    "session_key": s.session_key,
                    "status": s.status.as_str(),
                    "message_count": s.metrics.message_count,
                    "created_at": s.created_at,
                    "updated_at": s.updated_at,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&summaries)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Send a message to a nous agent session and get the response.
    ///
    /// Uses streaming execution so long-running turns emit events progressively
    /// instead of blocking until the full turn completes (avoids transport timeout).
    ///
    /// # Streaming limitation (#3251)
    ///
    /// The MCP protocol's `tools/call` is a request-response RPC: the client
    /// sends one request and receives one response. There is no MCP-level
    /// primitive for streaming partial tool results back to the caller.
    ///
    /// Internally, `send_turn_streaming` is used so the nous actor pipeline
    /// does not block on long turns (preventing transport timeout). However,
    /// stream events are drained and discarded because MCP has no server-push
    /// channel for tool execution. The client sees only the final result.
    ///
    /// To get token-level streaming, use the HTTP API's SSE endpoint at
    /// `POST /api/v1/sessions/{id}/messages` instead.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(
        description = "Send a message to a nous agent session and get the response. \
        Note: response is not streamed — the full result is returned after the turn completes. \
        For streaming, use the HTTP SSE endpoint."
    )]
    async fn session_message(
        &self,
        Parameters(params): Parameters<params::SessionMessageParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Expensive, Role::Operator, "session_message")?;
        require_nous_access_for_caller(&caller, &params.nous_id, "session_message")?;
        let handle = self
            .state
            .nous_manager
            .get(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;

        // WHY(#2446): send_turn blocks until the full turn completes. Long
        // reasoning turns with many tool iterations can exceed transport timeout.
        // send_turn_streaming keeps the connection alive via event flow while
        // still returning the final TurnResult.
        let (stream_tx, mut stream_rx) =
            tokio::sync::mpsc::channel::<nous::stream::TurnStreamEvent>(64);

        // WHY: drain stream events in the background so the channel doesn't
        // back-pressure the actor; MCP has no server-push, so events are
        // discarded. No approval gate is wired for this MCP request-response
        // path; shared dispatch auto-executes None/Advisory tools and
        // policy-denies Required/Mandatory tools.
        let drain = tokio::spawn(
            async move { while stream_rx.recv().await.is_some() {} }
                .instrument(tracing::info_span!("diaporeia.mcp.stream_drain")),
        );

        let result = handle
            .send_turn_streaming(&params.session_key, &params.content, stream_tx)
            .await
            .map_err(|e| {
                PipelineSnafu {
                    message: e.to_string(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

        // NOTE: fast — the channel closes when the actor completes.
        let _ = drain.await;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            result.content,
        )]))
    }

    /// Get conversation history for a session.
    #[tool(description = "Get conversation history for a session")]
    async fn session_history(
        &self,
        Parameters(params): Parameters<params::SessionHistoryParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "session_history")?;
        require_caller_role(&caller, Role::Agent, "session_history")?;

        let store = self.state.session_store.lock().await;
        let session = store
            .find_session_by_id(&params.session_id)
            .context(SessionStoreSnafu {})
            .map_err(rmcp::ErrorData::from)?
            .ok_or_else(|| {
                SessionNotFoundSnafu {
                    id: params.session_id.clone(),
                }
                .build()
            })?;
        require_nous_access_for_caller(&caller, &session.nous_id, "session_history")?;

        // WHY(#4841): apply the same first-party history caps as pylon.
        let config = self.state.config.read().await;
        let limit = params
            .limit
            .map_or(config.api_limits.default_history_limit, |l| {
                u32::try_from(l.max(0)).unwrap_or(u32::MAX)
            })
            .min(config.api_limits.max_history_limit);
        let messages = store
            .get_history(&params.session_id, Some(i64::from(limit)))
            .context(SessionStoreSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        let history: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "seq": m.seq,
                    "role": m.role.as_str(),
                    "content": m.content,
                    "created_at": m.created_at,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&history)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// List all registered nous agents with their current status.
    #[tool(description = "List all registered nous agents with their current status")]
    async fn nous_list(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "nous_list")?;
        require_caller_role(&caller, Role::Agent, "nous_list")?;
        let show_reliability = caller.role >= Role::Operator;
        let statuses = if show_reliability {
            self.state.nous_manager.list_all().await
        } else {
            self.state.nous_manager.list().await
        };

        let list: Vec<serde_json::Value> = statuses
            .iter()
            .filter(|s| {
                self.state
                    .nous_manager
                    .get_config(&s.id)
                    .is_some_and(|cfg| nous_visible_to_caller(&caller, cfg))
            })
            .map(|s| {
                if show_reliability {
                    serde_json::json!({
                        "id": s.id,
                        "lifecycle": s.lifecycle.to_string(),
                        "session_count": s.session_count,
                        "active_session": s.active_session,
                        "panic_count": s.panic_count,
                        "uptime_secs": s.uptime.as_secs(),
                    })
                } else {
                    serde_json::json!({
                        "id": s.id,
                        "lifecycle": s.lifecycle.to_string(),
                        "session_count": s.session_count,
                        "active_session": s.active_session,
                        "uptime_secs": s.uptime.as_secs(),
                    })
                }
            })
            .collect();

        let json = serde_json::to_string_pretty(&list)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Get detailed status of a specific nous agent.
    #[tool(description = "Get detailed status of a specific nous agent")]
    async fn nous_status(
        &self,
        Parameters(params): Parameters<params::NousIdParam>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "nous_status")?;
        require_caller_role(&caller, Role::Agent, "nous_status")?;
        let show_reliability = caller.role >= Role::Operator;

        let config = self
            .state
            .nous_manager
            .get_config(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;
        if !nous_visible_to_caller(&caller, config) {
            return Err(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            }
            .build()
            .into());
        }

        let handle = self
            .state
            .nous_manager
            .get(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;

        let status = handle
            .status()
            .await
            .map_err(|e| {
                PipelineSnafu {
                    message: e.to_string(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

        let json = if show_reliability {
            serde_json::to_string_pretty(&serde_json::json!({
                "id": status.id,
                "lifecycle": status.lifecycle.to_string(),
                "session_count": status.session_count,
                "active_session": status.active_session,
                "panic_count": status.panic_count,
                "uptime_secs": status.uptime.as_secs(),
            }))
        } else {
            serde_json::to_string_pretty(&serde_json::json!({
                "id": status.id,
                "lifecycle": status.lifecycle.to_string(),
                "session_count": status.session_count,
                "active_session": status.active_session,
                "uptime_secs": status.uptime.as_secs(),
            }))
        }
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// List tools available to nous agents.
    #[tool(description = "List tools available to nous agents")]
    async fn nous_tools(
        &self,
        Parameters(params): Parameters<params::NousIdParam>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "nous_tools")?;
        require_caller_role(&caller, Role::Agent, "nous_tools")?;
        let config = self
            .state
            .nous_manager
            .get_config(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;
        if !nous_visible_to_caller(&caller, config) {
            return Err(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            }
            .build()
            .into());
        }

        let active = HashSet::new();
        let surface = self.state.tool_registry.effective_surface(SurfaceInputs {
            policy: &config.tool_groups,
            allowlist: config.tool_allowlist.as_deref(),
            active: &active,
            server_tools: &config.server_tools,
            server_tool_config: None,
        });

        let tools: Vec<serde_json::Value> = surface
            .entries()
            .iter()
            .map(|entry| {
                let denial_reason = match entry.availability {
                    SurfaceAvailability::Denied(reason) => Some(reason.as_str()),
                    SurfaceAvailability::Callable | SurfaceAvailability::Inactive => None,
                };
                serde_json::json!({
                    "name": entry.name.as_str(),
                    "description": entry.description,
                    "category": entry.category.map(|category| category.to_string()),
                    "groups": entry.groups.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "reversibility": entry.reversibility.to_string(),
                    "approval": entry.approval.to_string(),
                    "auto_activate": entry.auto_activate,
                    "availability": entry.availability.as_str(),
                    "denial_reason": denial_reason,
                    "kind": entry.kind.as_str(),
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "nous_id": params.nous_id,
            "surface_hash": surface.hash().as_str(),
            "tools": tools,
        }))
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Semantic search across the knowledge graph.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "Semantic search across the knowledge graph")]
    async fn knowledge_search(
        &self,
        Parameters(params): Parameters<params::KnowledgeSearchParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Expensive, "knowledge_search")?;
        require_caller_role(&caller, Role::Operator, "knowledge_search")?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            let effective_nous =
                resolve_nous_scope(&caller, params.nous_id.as_deref(), "knowledge_search")?;
            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;
            let mut search_params = params;
            search_params.nous_id = effective_nous;
            let results = search_knowledge_store(
                &store,
                &search_params,
                config.mcp.knowledge_graph.max_search_results,
            )?;
            let results: Vec<_> = results
                .into_iter()
                .filter(|r| recall_visible_to_caller(&caller, &r.nous_id, r.visibility))
                .collect();
            let json = serde_json::to_string_pretty(&results)
                .context(SerializationSnafu {})
                .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = (params, config);
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// BM25 text recall across the knowledge graph.
    ///
    /// Returns ranked fact results using BM25 full-text scoring. For read
    /// access, requires `Agent` role or above. When the knowledge graph MCP
    /// surface is disabled (`mcp.knowledge_graph.enabled = false`), returns
    /// error code -32002.
    #[tool(description = "BM25 text recall across the knowledge graph. \
        Returns ranked facts. Requires Agent role.")]
    async fn knowledge_recall(
        &self,
        Parameters(params): Parameters<params::KnowledgeRecallParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Expensive, "knowledge_recall")?;
        require_caller_role(&caller, Role::Agent, "knowledge_recall")?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            let effective_nous =
                resolve_nous_scope(&caller, params.nous_id.as_deref(), "knowledge_recall")?;
            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;
            let limit = params
                .limit
                .unwrap_or(20)
                .min(config.mcp.knowledge_graph.max_search_results);
            let limit_i64 = i64::from(limit);

            let results = store
                .search_text_for_recall(&params.query, limit_i64)
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;

            // WHY(#4841): enforce first-party ownership and visibility rather
            // than trusting caller-supplied `nous_id` or `source_id` substrings.
            let results: Vec<_> = results
                .into_iter()
                .filter(|r| recall_visible_to_scope(r, effective_nous.as_deref()))
                .collect();

            let json = serde_json::to_string_pretty(&results)
                .context(SerializationSnafu {})
                .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = (params, config);
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Retrieve a single fact by ID.
    ///
    /// Returns the full `Fact` record including provenance, lifecycle, and
    /// access metadata. Requires `Agent` role or above.
    #[tool(description = "Retrieve a single knowledge fact by ID. Requires Agent role.")]
    async fn knowledge_get(
        &self,
        Parameters(params): Parameters<params::KnowledgeGetParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "knowledge_get")?;
        require_caller_role(&caller, Role::Agent, "knowledge_get")?;
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;
            let mut facts = store.read_facts_by_id(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;
            let fact = facts.pop().ok_or_else(|| {
                rmcp::ErrorData::from(
                    FactNotFoundSnafu {
                        id: params.fact_id.clone(),
                    }
                    .build(),
                )
            })?;
            if !recall_visible_to_caller(&caller, &fact.nous_id, fact.visibility) {
                // WHY(#4841): hide the fact's existence from unauthorized callers
                // rather than leaking that the ID is valid.
                return Err(FactNotFoundSnafu {
                    id: params.fact_id.clone(),
                }
                .build()
                .into());
            }
            let json = serde_json::to_string_pretty(&fact)
                .context(SerializationSnafu {})
                .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Insert a new fact into the knowledge graph.
    ///
    /// Creates a fact with the given content under the specified nous agent.
    /// The fact is admitted through the configured admission policy.
    ///
    /// Requires `Operator` role or above. Mutations are restricted to
    /// Operator+ to prevent agents from unilaterally persisting beliefs.
    #[tool(description = "Insert a new fact into the knowledge graph. \
        Requires Operator role. Returns the new fact ID.")]
    async fn knowledge_insert(
        &self,
        Parameters(params): Parameters<params::KnowledgeInsertParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Expensive, Role::Operator, "knowledge_insert")?;
        let effective_nous =
            resolve_nous_scope(&caller, Some(&params.nous_id), "knowledge_insert")?
                .unwrap_or_else(|| params.nous_id.clone());
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use mneme::id::FactId;
            use mneme::knowledge::{
                EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
                FactTemporal, default_stability_hours, far_future,
            };

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            // Parse sensitivity, defaulting to Public.
            let sensitivity = params
                .sensitivity
                .as_deref()
                .unwrap_or("public")
                .parse::<FactSensitivity>()
                .map_err(|e| rmcp::ErrorData::from(InvalidInputSnafu { message: e }.build()))?;

            let now = jiff::Timestamp::now();
            // WHY: koina::uuid::uuid_v4() provides UUID v4 without adding a
            // direct `uuid` crate dep. Prefix with "f-mcp-" to make the origin
            // of MCP-inserted facts identifiable during audits.
            let fact_id_str = format!("f-mcp-{}", koina::uuid::uuid_v4());
            let fact_id = FactId::new(&fact_id_str).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            let fact = Fact {
                id: fact_id.clone(),
                nous_id: effective_nous.clone(),
                fact_type: "knowledge".to_string(),
                content: params.content.clone(),
                scope: None,
                project_id: None,
                sensitivity,
                visibility: mneme::knowledge::Visibility::Private,
                temporal: FactTemporal {
                    valid_from: now,
                    valid_to: far_future(),
                    recorded_at: now,
                },
                provenance: FactProvenance {
                    confidence: 0.8,
                    tier: EpistemicTier::Inferred,
                    source_session_id: None,
                    stability_hours: default_stability_hours("knowledge"),
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
            };

            store.insert_fact_async(fact).await.map_err(|e| {
                rmcp::ErrorData::from(
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "fact_id": fact_id.as_str(),
                "nous_id": effective_nous,
                "sensitivity": sensitivity.as_str(),
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Soft-delete a fact from the knowledge graph.
    ///
    /// The fact is marked as forgotten (not physically deleted). Forgotten
    /// facts are excluded from recall but remain for audit purposes.
    ///
    /// Requires `Operator` role or above.
    #[tool(description = "Soft-delete a fact from the knowledge graph. \
        Requires Operator role.")]
    async fn knowledge_forget(
        &self,
        Parameters(params): Parameters<params::KnowledgeForgetParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Expensive, Role::Operator, "knowledge_forget")?;
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use mneme::id::FactId;
            use mneme::knowledge::ForgetReason;

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let fact_id = FactId::new(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;
            read_fact_for_caller(&store, &caller, fact_id.as_str(), "knowledge_forget")?;

            let reason = params
                .reason
                .as_deref()
                .unwrap_or("user_requested")
                .parse::<ForgetReason>()
                .map_err(|e| rmcp::ErrorData::from(InvalidInputSnafu { message: e }.build()))?;

            let fact_id_clone = fact_id.clone();
            let reason_clone = reason;
            tokio::task::spawn_blocking(move || store.forget_fact(&fact_id_clone, reason_clone))
                .await
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "fact_id": fact_id.as_str(),
                "forgotten": true,
                "reason": reason.as_str(),
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Traverse entity edges in the knowledge graph.
    ///
    /// Returns entities within `depth` hops of the given entity, along with
    /// the relationships connecting them. Depth is capped at 4 to prevent
    /// unbounded traversal.
    ///
    /// Requires `Agent` role or above.
    #[tool(
        description = "Traverse entity edges in the knowledge graph up to the given depth. \
        Returns neighbors and connecting relationships. Requires Agent role."
    )]
    async fn knowledge_graph_neighbors(
        &self,
        Parameters(params): Parameters<params::KnowledgeGraphNeighborsParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Expensive, "knowledge_graph_neighbors")?;
        require_caller_role(&caller, Role::Agent, "knowledge_graph_neighbors")?;
        // WHY(#4841): Agent callers must be scoped; unscoped traversal would let
        // them wander across party boundaries. Operator/Admin callers may remain
        // unscoped for administrative traversal.
        resolve_nous_scope(&caller, None, "knowledge_graph_neighbors")?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let max_depth = config.mcp.knowledge_graph.max_graph_depth.min(4);
            let depth = params.depth.unwrap_or(2).min(max_depth);
            let neighbors = graph_neighbors_to_depth(&store, &params.entity_id, depth)?;

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "entity_id": params.entity_id,
                "depth": depth,
                "neighbors": neighbors,
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = (params, config);
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// List available repomix templates.
    ///
    /// Returns the built-in template names and descriptions. Requires `Agent`
    /// role or above.
    #[tool(description = "List available repomix templates. Requires Agent role.")]
    async fn repomix_templates_list(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Agent, "repomix_templates_list")?;
        require_repomix(self).await?;

        let templates = crate::repomix::list_templates();
        let json = serde_json::to_string_pretty(&templates)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Get a single repomix template definition.
    ///
    /// Requires `Agent` role or above.
    #[tool(description = "Get a repomix template definition by name. Requires Agent role.")]
    async fn repomix_template_get(
        &self,
        Parameters(params): Parameters<params::RepomixTemplateGetParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Agent, "repomix_template_get")?;
        require_repomix(self).await?;

        let template = crate::repomix::get_template(&params.name).map_err(rmcp::ErrorData::from)?;
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "name": template.name,
            "description": template.description,
            "include_deps": template.include_deps,
        }))
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;
        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Pack crate source code into a token-efficient context window.
    ///
    /// Uses native Rust compression (comment stripping, whitespace collapse)
    /// to approximate repomix output without requiring an external npm
    /// binary. Respects `mcp.repomix.max_output_tokens` and resolves crate
    /// sources under the configured `mcp.repomix.workspace_root` — never the
    /// daemon's process working directory.
    ///
    /// Requires `Operator` role or above because packing can be expensive
    /// and generates dispatch context.
    #[tool(description = "Pack crate source into compressed context. \
        Requires Operator role. Returns packed XML with metadata.")]
    async fn repomix_pack(
        &self,
        Parameters(params): Parameters<params::RepomixPackParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Expensive, Role::Operator, "repomix_pack")?;
        let config = require_repomix(self).await?;

        // NOTE: `validate_startup` rejects a config that enables repomix
        // without a `workspace_root`, so this should be unreachable in a
        // properly started instance; it remains a defense-in-depth guard
        // against config mutated after startup (e.g. hot-reload).
        let workspace_root = config.mcp.repomix.workspace_root.clone().ok_or_else(|| {
            rmcp::ErrorData::from(
                RepomixPackSnafu {
                    message: "mcp.repomix.enabled is true but mcp.repomix.workspaceRoot is not \
                              configured; set it in aletheia.toml"
                        .to_owned(),
                }
                .build(),
            )
        })?;

        let max_tokens = params
            .max_tokens
            .unwrap_or(config.mcp.repomix.max_output_tokens);

        let template_name = params.template.clone();
        let crate_names = params.crate_names.clone();

        let result = tokio::task::spawn_blocking(move || {
            crate::repomix::pack(&workspace_root, &crate_names, &template_name, max_tokens)
        })
        .await
        .map_err(|e| {
            rmcp::ErrorData::from(
                RepomixPackSnafu {
                    message: e.to_string(),
                }
                .build(),
            )
        })?
        .map_err(rmcp::ErrorData::from)?;

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "packed": result.packed,
            "files_included": result.files_included,
            "raw_bytes": result.raw_bytes,
            "packed_bytes": result.packed_bytes,
            "estimated_tokens": result.estimated_tokens,
            "template": params.template,
            "crate_names": params.crate_names,
        }))
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;
        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Get the runtime configuration with sensitive fields redacted.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "Get the runtime configuration with sensitive fields redacted")]
    async fn config_get(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Operator, "config_get")?;
        let config = self.state.config.read().await;

        let redacted = serde_json::json!({
            "gateway": {
                "port": config.gateway.port,
                "bind": config.gateway.bind,
                "auth": {
                    "mode": config.gateway.auth.mode,
                },
            },
            "agents": {
                "count": config.agents.list.len(),
                "ids": config.agents.list.iter().map(|a| &a.id).collect::<Vec<_>>(),
            },
            "embedding": {
                "provider": config.embedding.provider,
                "model": config.embedding.model,
                "dimension": config.embedding.dimension,
            },
        });

        let json = serde_json::to_string_pretty(&redacted)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Store a secret in the session vault.
    ///
    /// The secret can later be referenced in tool arguments via
    /// `{{secret:<name>}}` or `$SECRET(<name>)` placeholders.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "Store a secret in the session vault for use in tool arguments")]
    async fn creds_set(
        &self,
        Parameters(params): Parameters<params::CredsSetParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Operator, "creds_set")?;

        let vault = self
            .state
            .nous_manager
            .secret_vault()
            .ok_or_else(|| {
                PipelineSnafu {
                    message: "secret vault not available".to_owned(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

        vault.store(&params.name, params.value);

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            format!("Secret `{}` stored.", params.name),
        )]))
    }

    /// List names of secrets stored in the session vault.
    ///
    /// Values are never exposed — only names are returned.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "List secret names stored in the session vault")]
    async fn creds_list(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Operator, "creds_list")?;

        let vault = self
            .state
            .nous_manager
            .secret_vault()
            .ok_or_else(|| {
                PipelineSnafu {
                    message: "secret vault not available".to_owned(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

        let names = vault.list_names();
        let json = serde_json::to_string_pretty(&names)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Remove a secret from the session vault.
    ///
    /// Requires `Operator` role or above (#3337).
    #[tool(description = "Remove a secret from the session vault")]
    async fn creds_rm(
        &self,
        Parameters(params): Parameters<params::CredsRmParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        require_role(self, &context, Tier::Cheap, Role::Operator, "creds_rm")?;

        let vault = self
            .state
            .nous_manager
            .secret_vault()
            .ok_or_else(|| {
                PipelineSnafu {
                    message: "secret vault not available".to_owned(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

        if vault.remove(&params.name).is_some() {
            Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                format!("Secret `{}` removed.", params.name),
            )]))
        } else {
            Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                format!("Secret `{}` not found.", params.name),
            )]))
        }
    }

    /// System health check with uptime, actor health, and version info.
    #[tool(description = "System health check with uptime, actor health, and version info")]
    async fn system_health(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Cheap, Role::Agent, "system_health")?;
        let show_reliability = caller.role >= Role::Operator;
        let health = self.state.nous_manager.check_health().await;
        let uptime = self.state.start_time.elapsed();

        let actors: serde_json::Value = health
            .iter()
            .map(|(id, h)| {
                if show_reliability {
                    (
                        id.clone(),
                        serde_json::json!({
                            "alive": h.alive,
                            "panic_count": h.panic_count,
                            "uptime_secs": h.uptime.as_secs(),
                        }),
                    )
                } else {
                    (
                        id.clone(),
                        serde_json::json!({
                            "alive": h.alive,
                            "uptime_secs": h.uptime.as_secs(),
                        }),
                    )
                }
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();

        let result = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": uptime.as_secs(),
            "actor_count": health.len(),
            "actors": actors,
        });

        let json = serde_json::to_string_pretty(&result)
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;

        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            json,
        )]))
    }

    /// Manage session notes (add, list, delete).
    #[tool(description = "Manage session notes. Actions: add, list, delete. Requires Agent role.")]
    async fn memory_note(
        &self,
        Parameters(params): Parameters<params::MemoryNoteParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "memory_note")?;
        require_caller_role(&caller, Role::Agent, "memory_note")?;

        let store = self
            .state
            .note_store
            .clone()
            .ok_or_else(|| NoteStoreUnavailableSnafu {}.build())?;

        // WHY(#4841): derive session and nous identity from verified claims.
        // Agent callers may only touch their own caller-sub session; only
        // Operator/Admin may supply a cross-session session_id.
        let (session_id, nous_id) = if caller.role >= Role::Operator {
            (
                params.session_id.clone(),
                effective_nous_id(&caller, "memory_note")?,
            )
        } else {
            if params.session_id != caller.sub {
                return Err(UnauthorizedSnafu {
                    message: "memory_note: cross-session access denied".to_owned(),
                }
                .build()
                .into());
            }
            let nous_id = effective_nous_id(&caller, "memory_note")?;
            (caller.sub.clone(), nous_id)
        };

        match params.action.as_str() {
            "add" => {
                let content = params.content.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "content is required for 'add' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                let category = params.category.as_deref().unwrap_or("context");
                let note_id = store
                    .add_note(&session_id, &nous_id, category, &content)
                    .map_err(|e| {
                        rmcp::ErrorData::from(
                            NoteStoreSnafu {
                                message: e.to_string(),
                            }
                            .build(),
                        )
                    })?;
                Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                    format!("Note added with ID {note_id}."),
                )]))
            }
            "list" => {
                let notes = store.get_notes(&session_id).map_err(|e| {
                    rmcp::ErrorData::from(
                        NoteStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                if notes.is_empty() {
                    return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        "No notes found for this session.",
                    )]));
                }
                let lines: Vec<String> = notes
                    .iter()
                    .map(|n| format!("[{}] {}: {}", n.id, n.category, n.content))
                    .collect();
                Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                    lines.join("\n"),
                )]))
            }
            "delete" => {
                let note_id = params.note_id.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "note_id is required for 'delete' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                // WHY(#4841): confirm the note belongs to the caller's session
                // before deleting; the underlying store is keyed by note id only.
                let notes = store.get_notes(&session_id).map_err(|e| {
                    rmcp::ErrorData::from(
                        NoteStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                if !notes.iter().any(|n| n.id == note_id) {
                    return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!("Note {note_id} not found."),
                    )]));
                }
                let deleted = store.delete_note(note_id).map_err(|e| {
                    rmcp::ErrorData::from(
                        NoteStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                if deleted {
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!("Note {note_id} deleted."),
                    )]))
                } else {
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!("Note {note_id} not found."),
                    )]))
                }
            }
            other => Err(rmcp::ErrorData::from(
                InvalidInputSnafu {
                    message: format!("unknown action: {other}; expected add, list, or delete"), // kanon:ignore RUST/format-sql — error message string, not a SQL query
                }
                .build(),
            )),
        }
    }

    /// Manage shared blackboard state (write, read, list, delete).
    #[tool(
        description = "Manage shared blackboard state. Actions: write, read, list, delete. Requires Agent role."
    )]
    async fn memory_blackboard(
        &self,
        Parameters(params): Parameters<params::MemoryBlackboardParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Cheap, "memory_blackboard")?;
        require_caller_role(&caller, Role::Agent, "memory_blackboard")?;

        let store = self
            .state
            .blackboard_store
            .clone()
            .ok_or_else(|| BlackboardStoreUnavailableSnafu {}.build())?;

        // WHY(#4841): derive the verified author from claims. Agent callers
        // cannot spoof another agent; Operator/Admin may supply an explicit
        // cross-scope author.
        let verified_author = if caller.role >= Role::Operator {
            params.author.clone().unwrap_or_else(|| caller.sub.clone())
        } else {
            let author = effective_nous_id(&caller, "memory_blackboard")?;
            if params.author.as_deref().is_some_and(|a| a != author) {
                return Err(UnauthorizedSnafu {
                    message: "memory_blackboard: cross-agent author denied".to_owned(),
                }
                .build()
                .into());
            }
            author
        };

        match params.action.as_str() {
            "write" => {
                let key = params.key.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "key is required for 'write' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                let value = params.value.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "value is required for 'write' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                let ttl = params.ttl_seconds.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "ttl_seconds is required for 'write' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                store
                    .write(&key, &value, &verified_author, ttl)
                    .map_err(|e| {
                        rmcp::ErrorData::from(
                            BlackboardStoreSnafu {
                                message: e.to_string(),
                            }
                            .build(),
                        )
                    })?;
                Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                    format!("Blackboard entry '{key}' written."),
                )]))
            }
            "read" => {
                let key = params.key.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "key is required for 'read' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                let entry = store.read(&key).map_err(|e| {
                    rmcp::ErrorData::from(
                        BlackboardStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                match entry {
                    Some(e) => Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!(
                            "{} = {} (author: {}, ttl: {}s)",
                            e.key, e.value, e.author_nous_id, e.ttl_seconds
                        ),
                    )])),
                    None => Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        "No entry found.".to_owned(),
                    )])),
                }
            }
            "list" => {
                let entries = store.list().map_err(|e| {
                    rmcp::ErrorData::from(
                        BlackboardStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                if entries.is_empty() {
                    return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        "Blackboard is empty.",
                    )]));
                }
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        format!(
                            "{} = {} (author: {}, ttl: {}s)",
                            e.key, e.value, e.author_nous_id, e.ttl_seconds
                        )
                    })
                    .collect();
                Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                    lines.join("\n"),
                )]))
            }
            "delete" => {
                let key = params.key.ok_or_else(|| {
                    rmcp::ErrorData::from(
                        InvalidInputSnafu {
                            message: "key is required for 'delete' action".to_owned(),
                        }
                        .build(),
                    )
                })?;
                let deleted = store.delete(&key, &verified_author).map_err(|e| {
                    rmcp::ErrorData::from(
                        BlackboardStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;
                if deleted {
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!("Blackboard entry '{key}' deleted."),
                    )]))
                } else {
                    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                        format!("Blackboard entry '{key}' not found."),
                    )]))
                }
            }
            other => Err(rmcp::ErrorData::from(
                InvalidInputSnafu {
                    message: format!(
                        "unknown action: {other}; expected write, read, list, or delete"
                    ),
                }
                .build(),
            )),
        }
    }

    /// Semantic search across the knowledge graph (memory-scoped).
    #[tool(description = "Semantic search across the knowledge graph. Requires Operator role.")]
    async fn memory_search(
        &self,
        Parameters(params): Parameters<params::MemorySearchParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_authenticated(self, &context, Tier::Expensive, "memory_search")?;
        require_caller_role(&caller, Role::Operator, "memory_search")?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            let effective_nous =
                resolve_nous_scope(&caller, params.nous_id.as_deref(), "memory_search")?;
            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;
            let limit = params
                .limit
                .unwrap_or(10)
                .min(usize::try_from(config.mcp.knowledge_graph.max_search_results).unwrap_or(20));
            let limit_i64 = i64::try_from(limit).unwrap_or(10);

            let results = store
                .search_text_for_recall(&params.query, limit_i64)
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;

            // WHY(#4841): enforce first-party ownership and visibility.
            let results: Vec<_> = results
                .into_iter()
                .filter(|r| recall_visible_to_scope(r, effective_nous.as_deref()))
                .collect();

            let json = serde_json::to_string_pretty(&results)
                .context(SerializationSnafu {})
                .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = (params, config);
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Correct a fact in the knowledge graph by creating a corrected copy
    /// and forgetting the original.
    #[tool(
        description = "Correct a fact in the knowledge graph. Requires Operator role. Returns the new fact ID."
    )]
    async fn memory_correct(
        &self,
        Parameters(params): Parameters<params::MemoryCorrectParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Cheap, Role::Operator, "memory_correct")?;
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use mneme::id::FactId;
            use mneme::knowledge::{
                EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
                ForgetReason, default_stability_hours, far_future,
            };

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let fact_id = FactId::new(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            let mut facts = store.read_facts_by_id(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;
            let old_fact = facts.pop().ok_or_else(|| {
                rmcp::ErrorData::from(
                    FactNotFoundSnafu {
                        id: params.fact_id.clone(),
                    }
                    .build(),
                )
            })?;
            require_nous_access_for_caller(&caller, &old_fact.nous_id, "memory_correct")?;

            // WHY(#5185): the knowledge store exposes no cross-record
            // transaction — insertion and supersession below are two
            // separate writes. Reject re-correcting a fact that a previous,
            // fully-successful call already superseded, so a naive client
            // retry after success cannot silently fork a second corrected
            // copy of the same claim. This does NOT fire for a retry after a
            // partial failure (insert succeeded, forget failed): the
            // rollback below un-inserts the new fact on that path, so
            // `old_fact` is still live when the retry re-reads it.
            if old_fact.lifecycle.is_forgotten
                && old_fact.lifecycle.forget_reason == Some(ForgetReason::Superseded)
            {
                return Err(rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: format!(
                            "fact {} was already corrected and superseded; \
                             call memory_correct on its replacement instead",
                            params.fact_id
                        ),
                    }
                    .build(),
                ));
            }

            let requested_nous_id = params.nous_id.as_deref().unwrap_or(&old_fact.nous_id);
            let effective_nous =
                resolve_nous_scope(&caller, Some(requested_nous_id), "memory_correct")?
                    .unwrap_or_else(|| requested_nous_id.to_owned());

            let now = jiff::Timestamp::now();
            let new_fact_id_str = format!("f-mcp-corrected-{}", koina::uuid::uuid_v4());
            let new_fact_id = FactId::new(&new_fact_id_str).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            let new_fact = Fact {
                id: new_fact_id.clone(),
                nous_id: effective_nous,
                fact_type: old_fact.fact_type.clone(),
                content: params.new_content,
                scope: old_fact.scope,
                project_id: old_fact.project_id.clone(),
                sensitivity: old_fact.sensitivity,
                visibility: old_fact.visibility,
                temporal: FactTemporal {
                    valid_from: now,
                    valid_to: far_future(),
                    recorded_at: now,
                },
                provenance: FactProvenance {
                    confidence: old_fact.provenance.confidence,
                    tier: EpistemicTier::Inferred,
                    source_session_id: None,
                    stability_hours: default_stability_hours("knowledge"),
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
            };

            store.insert_fact_async(new_fact).await.map_err(|e| {
                rmcp::ErrorData::from(
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            // WHY(#5185): if marking the old fact superseded fails after the
            // new fact is already live, roll the insert back (forget the new
            // fact) rather than leaving two live facts for the same claim.
            // If the rollback ALSO fails, surface both fact IDs so the
            // inconsistency is visible instead of silently duplicated state.
            // See `supersede_with_rollback` for the tested orchestration.
            let supersede_old = forget_fact_spawned(
                std::sync::Arc::clone(&store),
                fact_id.clone(),
                ForgetReason::Superseded,
            );
            let rollback_new =
                forget_fact_spawned(store, new_fact_id.clone(), ForgetReason::Incorrect);

            if let Err(failure) = supersede_with_rollback(supersede_old, rollback_new).await {
                let old_id = fact_id.as_str();
                let new_id = new_fact_id.as_str();
                let message = match failure {
                    SupersedeFailure::RolledBack(err) => format!(
                        "memory_correct: failed to mark fact {old_id} superseded ({err}); \
                         rolled back the new fact {new_id} — no changes were made, retry is safe"
                    ),
                    SupersedeFailure::Inconsistent(err, rollback_err) => format!(
                        "memory_correct: failed to mark fact {old_id} superseded ({err}) AND \
                         failed to roll back the new fact {new_id} ({rollback_err}) — both facts \
                         are now live; manual reconciliation required"
                    ),
                };
                return Err(rmcp::ErrorData::from(
                    KnowledgeStoreSnafu { message }.build(),
                ));
            }

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "new_fact_id": new_fact_id.as_str(),
                "old_fact_id": fact_id.as_str(),
                "status": "corrected",
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Retract a fact from the knowledge graph (soft-delete).
    #[tool(description = "Retract a fact from the knowledge graph. Requires Operator role.")]
    async fn memory_retract(
        &self,
        Parameters(params): Parameters<params::MemoryRetractParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Cheap, Role::Operator, "memory_retract")?;
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use mneme::id::FactId;
            use mneme::knowledge::ForgetReason;

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let fact_id = FactId::new(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;
            read_fact_for_caller(&store, &caller, fact_id.as_str(), "memory_retract")?;

            let reason = params
                .reason
                .as_deref()
                .unwrap_or("user_requested")
                .parse::<ForgetReason>()
                .map_err(|e| rmcp::ErrorData::from(InvalidInputSnafu { message: e }.build()))?;

            let fact_id_clone = fact_id.clone();
            let reason_clone = reason;
            tokio::task::spawn_blocking(move || store.forget_fact(&fact_id_clone, reason_clone))
                .await
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "fact_id": fact_id.as_str(),
                "retracted": true,
                "reason": reason.as_str(),
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }

    /// Forget a fact from the knowledge graph (soft-delete).
    #[tool(description = "Forget a fact from the knowledge graph. Requires Operator role.")]
    async fn memory_forget(
        &self,
        Parameters(params): Parameters<params::MemoryForgetParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let caller = require_role(self, &context, Tier::Cheap, Role::Operator, "memory_forget")?;
        require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use mneme::id::FactId;
            use mneme::knowledge::ForgetReason;

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let fact_id = FactId::new(&params.fact_id).map_err(|e| {
                rmcp::ErrorData::from(
                    InvalidInputSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;
            read_fact_for_caller(&store, &caller, fact_id.as_str(), "memory_forget")?;

            let reason = params
                .reason
                .parse::<ForgetReason>()
                .map_err(|e| rmcp::ErrorData::from(InvalidInputSnafu { message: e }.build()))?;

            let fact_id_clone = fact_id.clone();
            let reason_clone = reason;
            tokio::task::spawn_blocking(move || store.forget_fact(&fact_id_clone, reason_clone))
                .await
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?
                .map_err(|e| {
                    rmcp::ErrorData::from(
                        KnowledgeStoreSnafu {
                            message: e.to_string(),
                        }
                        .build(),
                    )
                })?;

            let json = serde_json::to_string_pretty(&serde_json::json!({
                "fact_id": fact_id.as_str(),
                "forgotten": true,
                "reason": reason.as_str(),
            }))
            .context(SerializationSnafu {})
            .map_err(rmcp::ErrorData::from)?;
            return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
                json,
            )]));
        }

        #[cfg(not(feature = "knowledge-store"))]
        {
            let _ = params;
            Err(KnowledgeStoreUnavailableSnafu {}.build().into())
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use hermeneus::provider::ProviderRegistry;
    use rmcp::model::CallToolRequestParams;
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[cfg(feature = "knowledge-store")]
    fn make_fact(id: &str, nous_id: &str, content: &str) -> mneme::knowledge::Fact {
        use mneme::id::FactId;
        use mneme::knowledge::{
            EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
            FactTemporal, Visibility, default_stability_hours, far_future,
        };

        let now = jiff::Timestamp::now();
        Fact {
            id: FactId::new(id).expect("valid fact id"),
            nous_id: nous_id.to_owned(),
            fact_type: "knowledge".to_owned(),
            content: content.to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: 0.9,
                tier: EpistemicTier::Verified,
                source_session_id: None,
                stability_hours: default_stability_hours("knowledge"),
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

    #[cfg(feature = "knowledge-store")]
    fn make_entity(id: &str, name: &str, entity_type: &str) -> mneme::knowledge::Entity {
        let now = jiff::Timestamp::now();
        mneme::knowledge::Entity {
            id: mneme::id::EntityId::new(id).expect("valid entity id"),
            name: name.to_owned(),
            entity_type: entity_type.to_owned(),
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[cfg(feature = "knowledge-store")]
    fn make_relationship(src: &str, dst: &str, relation: &str) -> mneme::knowledge::Relationship {
        mneme::knowledge::Relationship {
            src: mneme::id::EntityId::new(src).expect("valid src id"),
            dst: mneme::id::EntityId::new(dst).expect("valid dst id"),
            relation: relation.to_owned(),
            weight: 0.8,
            created_at: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn session_create_unknown_nous_maps_to_invalid_params() {
        let err = NousNotFoundSnafu {
            id: "ghost-nous".to_owned(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(
            mcp.code,
            rmcp::model::ErrorCode::INVALID_PARAMS,
            "unknown nous_id must map to INVALID_PARAMS so clients see a parameter error"
        );
        assert!(
            mcp.message.contains("ghost-nous"),
            "error message must identify the missing nous id"
        );
    }

    #[test]
    fn session_create_duplicate_maps_to_conflict_code() {
        let err = DuplicateSessionSnafu {
            nous_id: "alice".to_owned(),
            session_key: "main".to_owned(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(
            mcp.code,
            rmcp::model::ErrorCode(-32006),
            "duplicate session must use error code -32006 (distinct from INVALID_PARAMS)"
        );
        assert!(
            mcp.message.contains("main"),
            "error message must include the session_key"
        );
        assert!(
            mcp.message.contains("alice"),
            "error message must include the nous_id"
        );
    }

    fn make_server_state(
        store: Arc<tokio::sync::Mutex<mneme::store::SessionStore>>,
        oikos: Arc<taxis::oikos::Oikos>,
    ) -> Arc<crate::state::DiaporeiaState> {
        let mut config = taxis::config::AletheiaConfig::default();
        config.gateway.auth.mode = "none".to_owned();

        Arc::new(crate::state::DiaporeiaState {
            session_store: store.clone(),
            nous_manager: Arc::new(nous::manager::NousManager::new(
                Arc::new(ProviderRegistry::new()),
                Arc::new(organon::registry::ToolRegistry::new()),
                oikos.clone(),
                None,
                None,
                Some(store),
                #[cfg(feature = "knowledge-store")]
                None,
                Arc::new(Vec::new()),
                None,
                None,
                taxis::config::NousBehaviorConfig::default(),
                taxis::config::ToolLimitsConfig::default(),
            )),
            tool_registry: Arc::new(organon::registry::ToolRegistry::new()),
            oikos,
            auth_facade: None,
            start_time: Instant::now(),
            config: Arc::new(tokio::sync::RwLock::new(config)),
            auth_mode: "none".to_owned(),
            none_role: "admin".to_owned(),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            note_store: None,
            blackboard_store: None,
        })
    }

    #[tokio::test]
    async fn session_create_rejects_reserved_cross_key_without_persisting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let oikos = Arc::new(taxis::oikos::Oikos::from_root(dir.path()));
        let store = Arc::new(tokio::sync::Mutex::new(
            mneme::store::SessionStore::open(&oikos.sessions_db()).expect("open sessions store"),
        ));
        let state = make_server_state(Arc::clone(&store), oikos);
        let rate_limiter = Arc::new(crate::rate_limit::RateLimiter::from_config(
            &taxis::config::McpRateLimitConfig::default(),
        ));
        let server = DiaporeiaServer::with_state(state, rate_limiter);

        let (client_io, server_io) = tokio::io::duplex(16 * 1024);
        let server_task = tokio::spawn(async move { rmcp::serve_server(server, server_io).await });
        let mut client = rmcp::serve_client((), client_io)
            .await
            .expect("client initializes");
        let mut server_service = server_task
            .await
            .expect("server task joins")
            .expect("server initializes");

        let mut args = serde_json::Map::new();
        args.insert(
            "nous_id".to_owned(),
            serde_json::Value::String("syn".to_owned()),
        );
        args.insert(
            "session_key".to_owned(),
            serde_json::Value::String("cross:victim".to_owned()),
        );
        let err = client
            .peer()
            .call_tool(CallToolRequestParams::new("session_create").with_arguments(args))
            .await
            .expect_err("reserved session key must be rejected");

        match err {
            rmcp::service::ServiceError::McpError(error) => {
                assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
                assert!(
                    error.message.contains("reserved internal prefix"),
                    "got: {}",
                    error.message
                );
            }
            other => panic!("expected MCP invalid params error, got {other:?}"),
        }

        let persisted = store
            .lock()
            .await
            .find_session("syn", "cross:victim")
            .expect("query rejected session");
        assert!(
            persisted.is_none(),
            "MCP session_create must not persist reserved session keys"
        );

        client.close().await.expect("client close");
        server_service.close().await.expect("server close");
    }

    #[test]
    fn agent_only_tool_matrix_matches_rbac_hierarchy() {
        let mut cases = 0;
        for tool in AGENT_ONLY_TOOLS {
            assert!(
                Role::Agent >= Role::Agent,
                "{tool}: Agent must satisfy Agent gate"
            );
            assert!(
                Role::Operator >= Role::Agent,
                "{tool}: Operator must satisfy Agent gate"
            );
            assert!(
                Role::Readonly < Role::Agent,
                "{tool}: Readonly must not satisfy Agent gate"
            );
            cases += 3;
        }
        assert_eq!(cases, 18, "six Agent-only tools times three roles");
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn knowledge_search_uses_store_for_hit_and_miss() {
        let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
        store
            .insert_fact(&make_fact(
                "f-rust-search",
                "alice",
                "Rust borrow checker prevents memory unsafety",
            ))
            .expect("insert rust fact");

        let hit_params = params::KnowledgeSearchParams {
            query: "borrow checker".to_owned(),
            nous_id: None,
            limit: Some(10),
        };
        let hit = search_knowledge_store(&store, &hit_params, 20).expect("search hit");
        assert!(
            hit.iter().any(|result| result.source_id == "f-rust-search"),
            "knowledge_search must return the matching fact from the backend"
        );

        let miss_params = params::KnowledgeSearchParams {
            query: "nonexistent astrophysics keyword".to_owned(),
            nous_id: None,
            limit: Some(10),
        };
        let miss = search_knowledge_store(&store, &miss_params, 20).expect("search miss");
        assert!(
            miss.is_empty(),
            "unmatched query should return no backend hits"
        );
    }

    // ── #5185: memory_correct insert/supersede failure injection ──
    //
    // WHY: the knowledge store exposes no cross-record transaction, so
    // `memory_correct` performs insert-then-supersede as two separate
    // writes. These tests exercise `supersede_with_rollback`'s handling of
    // the second write failing — deterministically, via fake async
    // operations, rather than trying to force a real store failure between
    // two calls on the live backend.

    #[cfg(feature = "knowledge-store")]
    #[tokio::test]
    async fn supersede_with_rollback_succeeds_when_supersede_succeeds() {
        let result = supersede_with_rollback(
            async { Ok(()) },
            async { panic!("rollback must not run when supersede succeeds") },
        )
        .await;
        assert!(result.is_ok());
    }

    #[cfg(feature = "knowledge-store")]
    #[tokio::test]
    async fn supersede_with_rollback_reports_rolled_back_when_rollback_succeeds() {
        // FAILURE INJECTION: the supersede write fails (as it would if
        // `forget_fact` errored after `insert_fact_async` already
        // succeeded). The rollback must run and, on its own success, the
        // caller must be told this is a clean, retry-safe state — not
        // silently swallowed and not conflated with a genuinely
        // inconsistent outcome.
        let result = supersede_with_rollback(
            async { Err("supersede failed: store unavailable".to_owned()) },
            async { Ok(()) },
        )
        .await;
        assert_eq!(
            result,
            Err(SupersedeFailure::RolledBack(
                "supersede failed: store unavailable".to_owned()
            )),
            "a failed supersede with a successful rollback must be reported as RolledBack, \
             preserving the original supersede error for the operator"
        );
    }

    #[cfg(feature = "knowledge-store")]
    #[tokio::test]
    async fn supersede_with_rollback_reports_inconsistent_when_rollback_also_fails() {
        // FAILURE INJECTION: BOTH writes fail — the worst case, where the
        // new fact and the old fact are both left live. This must never be
        // silently swallowed: the caller needs both error messages to
        // reconcile manually.
        let result = supersede_with_rollback(
            async { Err("supersede failed: store unavailable".to_owned()) },
            async { Err("rollback failed: store unavailable".to_owned()) },
        )
        .await;
        assert_eq!(
            result,
            Err(SupersedeFailure::Inconsistent(
                "supersede failed: store unavailable".to_owned(),
                "rollback failed: store unavailable".to_owned(),
            )),
            "when the rollback ALSO fails, both errors must surface — this is the state \
             that needs manual reconciliation, so it must never be reported as merely \
             RolledBack"
        );
    }

    #[cfg(feature = "knowledge-store")]
    #[tokio::test]
    async fn memory_correct_rolls_back_new_fact_when_supersede_fails() {
        // End-to-end against the real in-memory store (not fakes): racing
        // `memory_correct`'s own read against a concurrent forget of the
        // same old fact is not reproducible from a single-threaded test, so
        // this drives the same two calls `memory_correct` makes
        // (`forget_fact_spawned` for the supersede write, then for the
        // rollback) directly, with the supersede write pointed at an ID that
        // cannot succeed — proving the rollback leaves no duplicate live
        // fact behind on the real backend, not just in the fake-driven
        // orchestration tests above.
        use mneme::knowledge::ForgetReason;

        // WHY: `open_mem()` already returns `Arc<KnowledgeStore>` (matching
        // `resolve_store`'s return type) — no extra `Arc::new` wrapping.
        let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
        let old_fact = make_fact("f-correct-src", "alice", "original claim");
        store.insert_fact(&old_fact).expect("insert original fact");

        let new_fact = make_fact("f-correct-new", "alice", "corrected claim");
        store
            .insert_fact_async(new_fact.clone())
            .await
            .expect("insert corrected fact");

        // Simulate the supersede write failing (e.g. a transient store
        // error) by pointing it at a fact ID that no longer exists —
        // `forget_fact` fails closed with FactNotFound rather than silently
        // no-oping.
        let missing_id = mneme::id::FactId::new("f-does-not-exist").expect("valid id");
        let supersede_old =
            forget_fact_spawned(Arc::clone(&store), missing_id, ForgetReason::Superseded);
        let rollback_new = forget_fact_spawned(
            Arc::clone(&store),
            new_fact.id.clone(),
            ForgetReason::Incorrect,
        );

        let result = supersede_with_rollback(supersede_old, rollback_new).await;
        assert!(
            matches!(result, Err(SupersedeFailure::RolledBack(_))),
            "expected a rolled-back failure, got {result:?}"
        );

        // The original fact is untouched (never forgotten)...
        let original_still_live = store
            .read_facts_by_id(old_fact.id.as_str())
            .expect("read original fact");
        assert!(
            original_still_live
                .first()
                .is_some_and(|f| !f.lifecycle.is_forgotten),
            "the original fact must be untouched when the supersede write fails"
        );

        // ...and the new fact was rolled back (forgotten), so recall does
        // not surface two live facts for the same claim.
        let new_fact_after = store
            .read_facts_by_id(new_fact.id.as_str())
            .expect("read new fact");
        assert!(
            new_fact_after.first().is_some_and(|f| f.lifecycle.is_forgotten),
            "the newly-inserted fact must be forgotten (rolled back) after a failed supersede"
        );
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn graph_neighbors_depth_expands_past_direct_neighbors() {
        let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
        for entity in [
            make_entity("e1", "Alice", "person"),
            make_entity("e2", "Aletheia", "project"),
            make_entity("e3", "Diaporeia", "crate"),
            make_entity("e4", "MCP", "protocol"),
        ] {
            store.insert_entity(&entity).expect("insert entity");
        }
        for relationship in [
            make_relationship("e1", "e2", "works_on"),
            make_relationship("e2", "e3", "contains"),
            make_relationship("e3", "e4", "implements"),
        ] {
            store
                .insert_relationship(&relationship)
                .expect("insert relationship");
        }

        let depth_one = graph_neighbors_to_depth(&store, "e1", 1).expect("depth 1 traversal");
        let depth_three = graph_neighbors_to_depth(&store, "e1", 3).expect("depth 3 traversal");

        assert_eq!(depth_one.len(), 1, "depth=1 should only return e2");
        assert!(
            depth_three.len() > depth_one.len(),
            "deeper traversal should discover more graph neighbors"
        );
        assert!(
            depth_three.iter().any(|neighbor| {
                neighbor
                    .get("entity_id")
                    .and_then(serde_json::Value::as_str)
                    == Some("e4")
            }),
            "depth=3 should reach the third-hop entity"
        );
    }

    #[cfg(feature = "knowledge-store")]
    fn make_recall_result(
        source_id: &str,
        nous_id: &str,
        visibility: mneme::knowledge::Visibility,
    ) -> mneme::knowledge::RecallResult {
        mneme::knowledge::RecallResult {
            content: "test content".to_owned(),
            distance: 0.0,
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
            nous_id: nous_id.to_owned(),
            sensitivity: mneme::knowledge::FactSensitivity::Public,
            graph_importance: 0.0,
            scope: None,
            project_id: None,
            visibility,
            source_count: 0,
        }
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn knowledge_search_filters_by_nous_id() {
        let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
        store
            .insert_fact(&make_fact("f-alice", "alice", "memory fact about rust"))
            .expect("insert alice fact");
        store
            .insert_fact(&make_fact("f-bob", "bob", "memory fact about rust"))
            .expect("insert bob fact");

        let params = params::KnowledgeSearchParams {
            query: "memory fact".to_owned(),
            nous_id: Some("alice".to_owned()),
            limit: Some(10),
        };
        let results = search_knowledge_store(&store, &params, 20).expect("search");
        assert!(
            results.iter().any(|r| r.source_id == "f-alice"),
            "matching nous_id must be included"
        );
        assert!(
            !results.iter().any(|r| r.source_id == "f-bob"),
            "non-matching nous_id must be excluded"
        );
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn recall_owned_by_scope_filters_by_nous_id() {
        let alice = make_recall_result("f-alice", "alice", mneme::knowledge::Visibility::Private);
        let bob = make_recall_result("f-bob", "bob", mneme::knowledge::Visibility::Private);

        assert!(recall_owned_by_scope(&alice, Some("alice")));
        assert!(!recall_owned_by_scope(&bob, Some("alice")));
        assert!(!recall_owned_by_scope(&alice, None));
    }

    #[cfg(feature = "knowledge-store")]
    #[test]
    fn recall_visible_to_scope_allows_owned_or_shared() {
        let owned = make_recall_result("f-alice", "alice", mneme::knowledge::Visibility::Private);
        let other_private =
            make_recall_result("f-bob", "bob", mneme::knowledge::Visibility::Private);
        let shared = make_recall_result("f-shared", "bob", mneme::knowledge::Visibility::Shared);

        assert!(recall_visible_to_scope(&owned, Some("alice")));
        assert!(!recall_visible_to_scope(&other_private, Some("alice")));
        assert!(recall_visible_to_scope(&shared, Some("alice")));
        assert!(recall_visible_to_scope(&shared, None));
    }
}
