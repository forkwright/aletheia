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
/// Delegates to [`streamable_http_router_with_config`] with the default
/// `StreamableHttpServerConfig`. See that function for security warning details.
pub fn streamable_http_router(state: Arc<DiaporeiaState>) -> axum::Router {
    streamable_http_router_with_config(
        state,
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    )
}

/// Build an Axum Router with a custom Streamable HTTP config.
///
/// Used by integration tests to enable stateless+json-response mode for
/// simpler request-response testing without SSE parsing.
///
/// # Security warnings
///
/// Logs a `WARN` when `auth_mode == "none"` (all connections receive the
/// configured `none_role` without any credential check). Escalates to
/// `ERROR` when the bind address is not loopback, because the MCP server
/// is reachable from the network with no authentication.
pub fn streamable_http_router_with_config(
    state: Arc<DiaporeiaState>,
    config: rmcp::transport::streamable_http_server::StreamableHttpServerConfig,
) -> axum::Router {
    // WHY: snapshot the rate-limit config (and bind) ONCE at setup using
    // try_read so the per-session factory below stays sync. The snapshot is
    // reused for every session in this transport's lifetime; a config reload
    // between sessions is rare and acceptable to miss until the next startup.
    let (bind, rate_cfg) = state.config.try_read().map_or_else(
        |_| {
            (
                "localhost".to_owned(),
                taxis::config::McpRateLimitConfig::default(),
            )
        },
        |cfg| (cfg.gateway.bind.clone(), cfg.mcp.rate_limit.clone()),
    );

    if state.auth_mode == "none" {
        let is_loopback = bind == "localhost" || bind == "127.0.0.1" || bind == "::1";

        if is_loopback {
            tracing::warn!(
                "MCP authentication disabled — all connections have {} access",
                state.none_role,
            );
        } else {
            // WHY(#5183): a non-loopback MCP bind with no authentication exposes the full
            // MCP surface — sessions, memory, knowledge — to unauthenticated network callers.
            // This is a hard misconfiguration; fail at startup rather than silently serving.
            // To use auth_mode = "none" on a non-loopback bind (e.g. for a dedicated trusted
            // LAN segment), set gateway.auth.allowUnauthenticatedNetworkMcp = true in config.
            let allow_override = state
                .config
                .try_read()
                .is_ok_and(|cfg| cfg.gateway.auth.allow_unauthenticated_network_mcp);
            assert!(
                allow_override,
                "FATAL: MCP server configured with auth_mode=\"none\" on non-loopback bind — exposes full MCP surface without authentication; set gateway.auth.allowUnauthenticatedNetworkMcp = true to allow this deployment mode"
            );
            tracing::error!(
                bind = %bind,
                "MCP exposed on network with no authentication (explicit override active) — all connections have {} access",
                state.none_role,
            );
        }
    }

    let auth_state = Arc::clone(&state);
    let service = StreamableHttpService::new(
        move || Ok(DiaporeiaServer::with_state(Arc::clone(&state), &rate_cfg)),
        LocalSessionManager::default().into(),
        config,
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
pub async fn serve_stdio(state: Arc<DiaporeiaState>) -> Result<()> {
    let rate_cfg = state.config.read().await.mcp.rate_limit.clone();
    let server = DiaporeiaServer::with_state(state, &rate_cfg);
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

// Compile-time check: the function signature matches the router mounting contract.
const _: fn(std::sync::Arc<crate::state::DiaporeiaState>) -> axum::Router = streamable_http_router;
