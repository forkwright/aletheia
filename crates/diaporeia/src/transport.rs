//! Transport setup helpers for stdio and streamable HTTP.

use std::sync::Arc;

use axum::middleware;
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;

use crate::auth;
use crate::error::{self, Result};
use crate::server::DiaporeiaServer;
use crate::state::DiaporeiaState;

/// Build an Axum Router that serves MCP over streamable HTTP.
///
/// Mount this into the main application router to expose MCP at `/mcp`.
/// The auth middleware validates Bearer JWT tokens (or passes through
/// anonymous claims when `auth_mode == "none"`).
///
/// # Security warnings
///
/// Logs a `WARN` when `auth_mode == "none"` (all connections receive the
/// configured `none_role` without any credential check). Escalates to
/// `ERROR` when the bind address is not loopback, because the MCP server
/// is reachable from the network with no authentication.
pub fn streamable_http_router(state: Arc<DiaporeiaState>) -> axum::Router {
    if state.auth_mode == "none" {
        // WHY: silent auth bypass is a security anti-pattern. Make the
        // operator decision visible in logs so it's never accidental.
        //
        // NOTE: try_read() avoids panicking when called within a tokio
        // runtime (integration tests). If the lock is held, default to
        // "localhost" which is the safe default — the warning is about
        // network exposure, so false-safe is acceptable.
        let bind = state
            .config
            .try_read()
            .map_or_else(|_| "localhost".to_owned(), |cfg| cfg.gateway.bind.clone());
        let is_loopback = bind == "localhost" || bind == "127.0.0.1" || bind == "::1";

        if is_loopback {
            tracing::warn!(
                "MCP authentication disabled — all connections have {} access",
                state.none_role,
            );
        } else {
            tracing::error!(
                bind = %bind,
                "MCP exposed on network with no authentication — all connections have {} access",
                state.none_role,
            );
        }
    }

    let auth_state = Arc::clone(&state);
    let service = StreamableHttpService::new(
        move || Ok(DiaporeiaServer::with_state(Arc::clone(&state))),
        LocalSessionManager::default().into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );

    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(move |req, next| {
            auth::mcp_auth(Arc::clone(&auth_state), req, next)
        }))
}

/// Run MCP over stdio (blocking: intended for `aletheia mcp` subcommand).
///
/// Reads JSON-RPC from stdin, writes to stdout. Blocks until the connection
/// closes or the shutdown token fires.
#[tracing::instrument(skip_all)]
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

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: Full integration coverage requires a running DiaporeiaState (session store,
    // nous manager, etc.) and is provided at the integration level.

    #[test]
    fn streamable_http_router_signature_is_correct() {
        // Compile-time verification that the function has the expected signature.
        let _: fn(std::sync::Arc<crate::state::DiaporeiaState>) -> axum::Router =
            streamable_http_router;
    }
}
