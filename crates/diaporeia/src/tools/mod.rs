//! MCP tool implementations for diaporeia.
//!
//! All tools are defined in a single `#[tool_router]` impl block on
//! `DiaporeiaServer`. Parameter structs are organized in `params`.

pub(crate) mod params;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use snafu::{OptionExt as _, ResultExt as _};

use crate::error::{NousNotFoundSnafu, PipelineSnafu, SerializationSnafu, SessionStoreSnafu};
use crate::rate_limit::Tier;
use crate::server::DiaporeiaServer;

/// Register all tools on the server via the `#[tool_router]` macro.
///
/// This generates the `DiaporeiaServer::tool_router()` associated function
/// used by the `#[tool_handler]` on the `ServerHandler` impl.
#[tool_router(vis = "pub(crate)")]
impl DiaporeiaServer {
    /// Create a new session for a nous agent. Returns session metadata as JSON.
    #[tool(description = "Create a new session for a nous agent")]
    async fn session_create(
        &self,
        Parameters(params): Parameters<params::SessionCreateParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Expensive)?;
        let session_key = params.session_key.as_deref().unwrap_or("main");
        let session_id = ulid::Ulid::new().to_string();

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
            "message_count": session.message_count,
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
                    "message_count": s.message_count,
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
    #[tool(description = "Send a message to a nous agent session and get the response")]
    async fn session_message(
        &self,
        Parameters(params): Parameters<params::SessionMessageParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Expensive)?;
        let handle = self
            .state
            .nous_manager
            .get(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;

        let result = handle
            .send_turn(&params.session_key, &params.content)
            .await
            .map_err(|e| {
                PipelineSnafu {
                    message: e.to_string(),
                }
                .build()
            })
            .map_err(rmcp::ErrorData::from)?;

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
    async fn nous_list(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        let statuses = self.state.nous_manager.list().await;

        let list: Vec<serde_json::Value> = statuses
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "lifecycle": s.lifecycle.to_string(),
                    "session_count": s.session_count,
                    "active_session": s.active_session,
                    "panic_count": s.panic_count,
                    "uptime_secs": s.uptime.as_secs(),
                })
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
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
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

        let json = serde_json::to_string_pretty(&serde_json::json!({
            "id": status.id,
            "lifecycle": status.lifecycle.to_string(),
            "session_count": status.session_count,
            "active_session": status.active_session,
            "panic_count": status.panic_count,
            "uptime_secs": status.uptime.as_secs(),
        }))
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
        let _handle = self
            .state
            .nous_manager
            .get(&params.nous_id)
            .context(NousNotFoundSnafu {
                id: params.nous_id.clone(),
            })
            .map_err(rmcp::ErrorData::from)?;

        let defs = self.state.tool_registry.definitions();
        let tools: Vec<serde_json::Value> = defs
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
    #[tool(description = "Semantic search across the knowledge graph")]
    async fn knowledge_search(
        &self,
        Parameters(_params): Parameters<params::KnowledgeSearchParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Expensive)?;
        // TODO(#1137): Wire to RecallEngine when knowledge store integration is complete.
        Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            "Knowledge search is not available in this configuration. \
             The server must be built with the 'recall' feature enabled \
             and a knowledge store must be configured."
                .to_string(),
        )]))
    }

    /// Get the runtime configuration with sensitive fields redacted.
    #[tool(description = "Get the runtime configuration with sensitive fields redacted")]
    async fn config_get(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
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

    /// System health check with uptime, actor health, and version info.
    #[tool(description = "System health check with uptime, actor health, and version info")]
    async fn system_health(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        let health = self.state.nous_manager.check_health().await;
        let uptime = self.state.start_time.elapsed();

        let actors: serde_json::Value = health
            .iter()
            .map(|(id, h)| {
                (
                    id.clone(),
                    serde_json::json!({
                        "alive": h.alive,
                        "panic_count": h.panic_count,
                        "uptime_secs": h.uptime.as_secs(),
                    }),
                )
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
