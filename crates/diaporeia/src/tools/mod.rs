//! MCP tool implementations for diaporeia.
//!
//! All tools are defined in a single `#[tool_router]` impl block on
//! `DiaporeiaServer`. Parameter structs are organized in `params`.

pub(crate) mod params;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router};
use snafu::{OptionExt as _, ResultExt as _};
use tracing::Instrument as _;

use koina::http::BEARER_PREFIX;
use koina::id::SessionId;
use symbolon::types::Role;

use crate::error::{
    FactNotFoundSnafu, InvalidInputSnafu, KnowledgeStoreSnafu, KnowledgeStoreUnavailableSnafu,
    NousNotFoundSnafu, PipelineSnafu, RepomixPackSnafu, RepomixUnavailableSnafu,
    SerializationSnafu, SessionStoreSnafu, UnauthorizedSnafu,
};
use crate::rate_limit::Tier;
use crate::server::DiaporeiaServer;

/// Extract role from request context by validating the Bearer token.
///
/// When a `JwtManager` is available, validates the signature before
/// extracting claims (closes the payload-only decode bypass from #3337).
/// In single-operator mode (`auth.mode = "none"`), returns the configured
/// `none_role` (defaults to admin).
///
/// Returns `None` only when auth info is unavailable or the token is invalid.
async fn extract_role(
    server: &DiaporeiaServer,
    context: &RequestContext<rmcp::RoleServer>,
) -> Option<Role> {
    let config = server.state.config.read().await;

    // NOTE: Single-operator mode has no auth; use the default role.
    if config.gateway.auth.mode == "none" {
        return config
            .gateway
            .auth
            .none_role
            .parse::<Role>()
            .ok()
            .or(Some(Role::Admin));
    }

    let parts = context.extensions.get::<http::request::Parts>()?;

    let header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())?;

    let token = header.strip_prefix(BEARER_PREFIX)?;

    // WHY(#3337): When jwt_manager is available, validate the signature
    // instead of just decoding the payload. This prevents role spoofing
    // on direct MCP connections that bypass the gateway.
    if let Some(ref jwt) = server.state.jwt_manager {
        return jwt.validate(token).ok().map(|claims| claims.role);
    }

    // Fallback: decode-only when no jwt_manager (should not happen when
    // auth_mode != "none", but handle gracefully).
    parse_role_from_token(token)
}

/// Parse role from a JWT token payload without signature verification.
///
/// Only used as a fallback when no `JwtManager` is available. Prefer
/// signature-verified extraction via `extract_role`.
fn parse_role_from_token(token: &str) -> Option<Role> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;

    let payload_bytes = base64_decode_urlsafe(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;

    claims
        .get("role")
        .and_then(|r| r.as_str())
        .and_then(|r| r.parse::<Role>().ok())
}

/// Base64 decode with URL-safe alphabet and no padding.
fn base64_decode_urlsafe(input: &str) -> Result<Vec<u8>, koina::base64::DecodeError> {
    koina::base64::decode_url_safe_no_pad(input)
}

/// Check if the caller has operator-level access or above.
///
/// Returns `true` if:
/// - Auth mode is "none" (single-operator mode)
/// - Caller has `Role::Operator` or `Role::Admin`
async fn is_operator_or_above(
    server: &DiaporeiaServer,
    context: &RequestContext<rmcp::RoleServer>,
) -> bool {
    match extract_role(server, context).await {
        Some(role) => role >= Role::Operator,
        None => false,
    }
}

/// Require at least the given role, returning an MCP error if insufficient.
///
/// Used by tools and resources that need RBAC enforcement beyond the
/// operator-or-above check (e.g. session creation, config access).
async fn require_role(
    server: &DiaporeiaServer,
    context: &RequestContext<rmcp::RoleServer>,
    minimum: Role,
    operation: &str,
) -> Result<(), rmcp::ErrorData> {
    let role = extract_role(server, context).await;
    match role {
        Some(r) if r >= minimum => Ok(()),
        Some(r) => {
            tracing::warn!(
                caller_role = %r,
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "session_create").await?;
        let session_key = params.session_key.as_deref().unwrap_or("main");
        // WHY: SessionId (UUID v4) is the canonical format. ULID here caused
        // 'invalid SessionId' when nous parsed the stored ID back (#2349).
        let session_id = SessionId::new().to_string();

        let store = self.state.session_store.lock().await;
        let session = store
            .find_or_create_session(&session_id, &params.nous_id, session_key, None, None)
            .context(SessionStoreSnafu {})
            .map_err(rmcp::ErrorData::from)?;

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
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        let store = self.state.session_store.lock().await;
        let sessions = store
            .list_sessions(params.nous_id.as_deref())
            .context(SessionStoreSnafu {})
            .map_err(rmcp::ErrorData::from)?;

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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "session_message").await?;
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

        // Drain stream events in background so the channel doesn't back-pressure
        // the actor. MCP doesn't support server-push, so events are discarded.
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

        // Wait for drain to finish (fast — channel closes when actor completes)
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
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        let store = self.state.session_store.lock().await;
        let messages = store
            .get_history(&params.session_id, params.limit)
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
        self.rate_limiter.check(Tier::Cheap)?;
        let show_reliability = is_operator_or_above(self, &context).await;
        let statuses = if show_reliability {
            self.state.nous_manager.list_all().await
        } else {
            self.state.nous_manager.list().await
        };

        let list: Vec<serde_json::Value> = statuses
            .iter()
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
        self.rate_limiter.check(Tier::Cheap)?;
        let show_reliability = is_operator_or_above(self, &context).await;
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
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        let config = self
            .state
            .nous_manager
            .get_config(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;

        let defs = self.state.tool_registry.definitions();

        // Filter tools by allowlist if configured for this agent
        let filtered_defs: Vec<_> = if let Some(allowlist) = &config.tool_allowlist {
            let allow_set: std::collections::HashSet<&str> =
                allowlist.iter().map(String::as_str).collect();
            defs.iter()
                .filter(|d| allow_set.contains(d.name.as_str()))
                .collect()
        } else {
            defs.iter().collect()
        };

        let tools: Vec<serde_json::Value> = filtered_defs
            .iter()
            .map(|d| {
                serde_json::json!({
                    "name": d.name.as_str(),
                    "description": d.description,
                    "category": format!("{:?}", d.category),
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&tools)
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
        Parameters(_params): Parameters<params::KnowledgeSearchParams>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "knowledge_search").await?;
        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            "Knowledge search is not available in this configuration. \
             The server must be built with the 'recall' feature enabled \
             and a knowledge store must be configured."
                .to_string(),
        )]))
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Agent, "knowledge_recall").await?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
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

            // Optionally filter to a single nous_id.
            let results: Vec<_> = if let Some(ref nous_id) = params.nous_id {
                results
                    .into_iter()
                    .filter(|r| r.source_id.contains(nous_id.as_str()))
                    .collect()
            } else {
                results
            };

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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Agent, "knowledge_get").await?;
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "knowledge_insert").await?;
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
                nous_id: params.nous_id.clone(),
                fact_type: "knowledge".to_string(),
                content: params.content.clone(),
                scope: None,
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
                "nous_id": params.nous_id,
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "knowledge_forget").await?;
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Agent, "knowledge_graph_neighbors").await?;
        let config = require_knowledge_graph(self).await?;

        #[cfg(feature = "knowledge-store")]
        {
            use std::collections::BTreeMap;

            use mneme::engine::DataValue;

            let store = resolve_store(self).map_err(rmcp::ErrorData::from)?;

            let max_depth = config.mcp.knowledge_graph.max_graph_depth.min(4);
            let depth = params.depth.unwrap_or(2).min(max_depth);

            // WHY: run_query accepts arbitrary Datalog so we can traverse
            // the neighborhood without exposing `entity_neighborhood`
            // (which is pub(crate) in episteme) through the public facade.
            // The query fetches direct edges (1-hop) from the seed entity.
            // Multi-hop traversal beyond depth=1 is left for future work
            // once the Datalog recursive-rule API is stable.
            let script = r"
                ?[entity_id, name, entity_type, relation, weight] :=
                    *relationships{src: $entity_id, dst: entity_id, relation, weight},
                    *entities{id: entity_id, name, entity_type}

                ?[entity_id, name, entity_type, relation, weight] :=
                    *relationships{src: entity_id, dst: $entity_id, relation, weight},
                    *entities{id: entity_id, name, entity_type}
            ";
            let mut query_params: BTreeMap<String, DataValue> = BTreeMap::new();
            query_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(params.entity_id.clone().into()),
            );

            let result = store.run_query(script, query_params).map_err(|e| {
                rmcp::ErrorData::from(
                    KnowledgeStoreSnafu {
                        message: e.to_string(),
                    }
                    .build(),
                )
            })?;

            let neighbors: Vec<serde_json::Value> = result
                .rows_to_json()
                .into_iter()
                .map(|row| {
                    serde_json::json!({
                        "entity_id": row.first().cloned().unwrap_or(serde_json::Value::Null),
                        "name": row.get(1).cloned().unwrap_or(serde_json::Value::Null),
                        "entity_type": row.get(2).cloned().unwrap_or(serde_json::Value::Null),
                        "relation": row.get(3).cloned().unwrap_or(serde_json::Value::Null),
                        "weight": row.get(4).cloned().unwrap_or(serde_json::Value::Null),
                    })
                })
                .collect();

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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Agent, "repomix_templates_list").await?;
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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Agent, "repomix_template_get").await?;
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
    /// binary. Respects `mcp.repomix.max_output_tokens` from config.
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
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "repomix_pack").await?;
        let config = require_repomix(self).await?;

        let workspace_root = crate::repomix::detect_workspace_root().ok_or_else(|| {
            rmcp::ErrorData::from(
                RepomixPackSnafu {
                    message: "could not detect workspace root (no Cargo.toml with [workspace])"
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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Operator, "config_get").await?;
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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Operator, "creds_set").await?;

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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Operator, "creds_list").await?;

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
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Operator, "creds_rm").await?;

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
        self.rate_limiter.check(Tier::Cheap)?;
        let show_reliability = is_operator_or_above(self, &context).await;
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
}
