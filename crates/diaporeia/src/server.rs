//! `DiaporeiaServer`: MCP server implementation.
//!
//! Implements `rmcp::ServerHandler` using the `#[tool_handler]` macro.
//! Tools are registered via `#[tool_router]` on the server struct.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    Implementation, InitializeResult, ListResourceTemplatesResult, ListResourcesResult,
    ReadResourceRequestParams, ReadResourceResult, ResourceTemplate, ServerCapabilities,
};
use rmcp::tool_handler;

use crate::resources;
use crate::state::DiaporeiaState;

/// The MCP server for Aletheia.
///
/// Holds shared state and a tool router. Implements `ServerHandler` to serve
/// MCP requests over stdio or streamable HTTP.
#[derive(Clone)]
pub struct DiaporeiaServer {
    pub(crate) state: Arc<DiaporeiaState>,
    tool_router: ToolRouter<Self>,
}

impl DiaporeiaServer {
    /// Create a new server instance with shared state.
    pub fn with_state(state: Arc<DiaporeiaState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

// NOTE: type alias required by rmcp. Get_info must return this exact name
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
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourceTemplatesResult, rmcp::ErrorData> {
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
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        // NOTE: static resources only. Dynamic listing deferred
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let uri = params.uri.as_str();

        let contents = if uri.starts_with("aletheia://nous/") {
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
        static_assertions::assert_impl_all!(DiaporeiaServer: Send, Sync);
    }
}
