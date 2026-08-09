//! `DiaporeiaServer`: MCP server implementation.
//!
//! Implements `rmcp::ServerHandler` using the `#[tool_handler]` macro.
//! Tools are registered via `#[tool_router]` on the server struct.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    Implementation, InitializeResult, ListResourceTemplatesResult, ListResourcesResult,
    RawResource, ReadResourceRequestParams, ReadResourceResult, Resource, ResourceTemplate,
    ServerCapabilities,
};
use rmcp::tool_handler;

use symbolon::types::Role;

use crate::auth::{McpCaller, resolve_caller};
use crate::error::UnauthorizedSnafu;
use crate::rate_limit::{RateLimiter, Tier};
use crate::resources;
use crate::state::DiaporeiaState;

/// The MCP server for Aletheia.
///
/// Holds shared state and a tool router. Implements `ServerHandler` to serve
/// MCP requests over stdio or streamable HTTP.
#[derive(Clone)]
pub struct DiaporeiaServer {
    pub(crate) state: Arc<DiaporeiaState>,
    pub(crate) rate_limiter: Arc<RateLimiter>,
    #[expect(
        dead_code,
        reason = "read by #[tool_handler] macro-generated code in ServerHandler impl"
    )]
    tool_router: ToolRouter<Self>,
}

impl DiaporeiaServer {
    /// Create a new server instance sharing the given rate limiter.
    ///
    /// WHY(#5182, #4843): the streamable HTTP transport constructs a fresh
    /// `DiaporeiaServer` per session; the rate limiter must NOT be built
    /// fresh alongside it or quota would reset every time a client opens a
    /// new session. Callers build one `RateLimiter` per transport bind (see
    /// `transport.rs`) and pass the shared `Arc` into every session's
    /// `with_state` call. A standalone caller (a single-session stdio
    /// connection, a test) builds its own with
    /// `Arc::new(RateLimiter::from_config(&rate_cfg))`.
    #[must_use]
    pub fn with_state(state: Arc<DiaporeiaState>, rate_limiter: Arc<RateLimiter>) -> Self {
        Self {
            state,
            rate_limiter,
            tool_router: Self::tool_router(),
        }
    }

    /// Check that the caller has at least `minimum` role for a resource operation.
    ///
    /// Resolves the caller from auth state: uses the configured `none_role` in
    /// auth-disabled mode, otherwise validates through the shared MCP resolver.
    /// Applies the shared rate limit for `tier`, keyed by the resolved
    /// caller's subject (or the shared unauthenticated bucket when no caller
    /// resolves), before enforcing the role requirement.
    fn require_resource_role(
        &self,
        context: &rmcp::service::RequestContext<rmcp::RoleServer>,
        tier: Tier,
        minimum: Role,
        operation: &str,
    ) -> Result<McpCaller, rmcp::ErrorData> {
        let caller = resolve_caller(&self.state, context);
        self.rate_limiter
            .check(tier, caller.as_ref().map(|c| c.sub.as_str()))?;
        match caller {
            Some(caller) if caller.role >= minimum => Ok(caller),
            Some(caller) => {
                tracing::warn!(
                    caller_role = %caller.role,
                    required_role = %minimum,
                    operation,
                    "MCP resource RBAC denied",
                );
                Err(UnauthorizedSnafu {
                    message: format!("{operation} requires {minimum} role or above"),
                }
                .build()
                .into())
            }
            None => {
                tracing::warn!(operation, "MCP resource RBAC denied: no role resolved");
                Err(UnauthorizedSnafu {
                    message: format!("{operation} requires {minimum} role or above"),
                }
                .build()
                .into())
            }
        }
    }

    /// Reject scoped resource access to a different target agent.
    fn require_resource_nous_access(
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
                "MCP resource scoped access denied",
            );
            return Err(UnauthorizedSnafu {
                message: "access denied for this agent".to_owned(),
            }
            .build()
            .into());
        }
        Ok(())
    }
}

// NOTE: type alias required by rmcp: get_info must return this exact name
type ServerInfo = InitializeResult;

#[tool_handler]
impl rmcp::handler::server::ServerHandler for DiaporeiaServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("aletheia", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Aletheia cognitive agent runtime. \
             Use session_message to talk to nous agents, \
             nous_list to discover available agents, \
             and system_health to check the system.",
        )
    }

    async fn list_resource_templates(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourceTemplatesResult, rmcp::ErrorData> {
        // WHY(#3337): resource templates reveal what internal state is
        // accessible. Restrict to Operator+ so Readonly users cannot
        // discover agent workspace files or config structure.
        self.require_resource_role(
            &context,
            Tier::Cheap,
            Role::Operator,
            "list_resource_templates",
        )?;

        let mut templates: Vec<ResourceTemplate> = resources::nous::resource_templates();
        templates.extend(resources::config::resource_templates());
        Ok(ListResourceTemplatesResult {
            resource_templates: templates,
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resources(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        let caller =
            self.require_resource_role(&context, Tier::Cheap, Role::Operator, "list_resources")?;

        let mut resources: Vec<Resource> = Vec::new();

        // WHY(#4635): Advertise the concrete config resource that is already
        // readable via `read_resource`.
        resources.push(Resource::new(
            RawResource::new("aletheia://config", "Aletheia Configuration")
                .with_description("Runtime configuration (sensitive fields redacted)")
                .with_mime_type("application/json"),
            None,
        ));

        // WHY(#4635): Enumerate per-agent workspace files, but only advertise
        // files that actually exist so clients do not discover unreadable URIs.
        let config = self.state.config.read().await;
        for agent in &config.agents.list {
            if caller
                .nous_id
                .as_deref()
                .is_some_and(|scoped| scoped != agent.id)
            {
                continue;
            }
            for (slug, name, description) in resources::nous::WORKSPACE_FILES {
                let uri = format!("aletheia://nous/{}/{slug}", agent.id);
                if resources::nous::resource_exists(self.state.oikos.as_ref(), &uri) {
                    resources.push(Resource::new(
                        RawResource::new(uri, *name)
                            .with_description(*description)
                            .with_mime_type("text/markdown"),
                        None,
                    ));
                }
            }
        }

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let uri = params.uri.as_str();

        // WHY(#3337): all MCP resources expose internal state (agent workspace
        // files, runtime config). Require Operator+ to prevent Readonly users
        // from enumerating agents, config, or knowledge.
        let caller = self.require_resource_role(&context, Tier::Cheap, Role::Operator, uri)?;

        let contents = if uri.starts_with("aletheia://nous/") {
            let (nous_id, _) = resources::nous::parse_resource_uri(uri)?;
            Self::require_resource_nous_access(&caller, nous_id.as_str(), uri)?;
            resources::nous::read_resource(&self.state, &params)?
        } else if uri.starts_with("aletheia://config") {
            resources::config::read_resource(&self.state, &params).await?
        } else {
            return Err(rmcp::ErrorData::invalid_params(
                format!("unknown resource URI: {uri}"),
                None,
            ));
        };

        Ok(ReadResourceResult::new(contents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<DiaporeiaServer>();
        };
    }
}
