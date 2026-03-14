//! Transport setup helpers for stdio and streamable HTTP.

use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;

use crate::error::{self, Result};
use crate::server::DiaporeiaServer;
use crate::state::DiaporeiaState;

/// Build an Axum Router that serves MCP over streamable HTTP.
///
/// Mount this into the main application router to expose MCP at `/mcp`.
pub fn streamable_http_router(state: Arc<DiaporeiaState>) -> axum::Router {
    let service = StreamableHttpService::new(
        move || Ok(DiaporeiaServer::with_state(Arc::clone(&state))),
        LocalSessionManager::default().into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );

    axum::Router::new().nest_service("/mcp", service)
}

/// Run MCP over stdio (blocking — intended for `aletheia mcp` subcommand).
///
/// Reads JSON-RPC from stdin, writes to stdout. Blocks until the connection
/// closes or the shutdown token fires.
#[expect(
    dead_code,
    reason = "intended for future aletheia mcp stdio subcommand"
)]
pub(crate) async fn serve_stdio(state: Arc<DiaporeiaState>) -> Result<()> {
    let server = DiaporeiaServer::with_state(state);
    let service = server
        .serve(rmcp::transport::io::stdio())
        .await
        .map_err(|e| {
            error::TransportSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

    service.waiting().await.map_err(|e| {
        error::TransportSnafu {
            message: e.to_string(),
        }
        .build()
    })?;

    Ok(())
}
