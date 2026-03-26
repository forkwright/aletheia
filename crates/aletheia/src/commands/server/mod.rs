//! Default server startup: runs when no subcommand is given.

use std::path::PathBuf;
use std::sync::Arc;

use snafu::prelude::*;
use tracing::{info, warn};

use aletheia_pylon::router::build_router;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;

use crate::error::Result;
use crate::runtime::RuntimeBuilder;

/// Arguments forwarded from the top-level CLI to the server startup.
pub(crate) struct Args {
    pub instance_root: Option<PathBuf>,
    pub bind: Option<String>,
    pub port: Option<u16>,
    pub log_level: String,
    pub json_logs: bool,
}

#[expect(
    clippy::too_many_lines,
    reason = "server startup sequence: sequential subsystem init; splitting adds indirection without clarity"
)]
pub(crate) async fn run(args: Args) -> Result<()> {
    // Oikos is pure path resolution: no IO, safe before tracing is set up.
    let oikos = match &args.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    // Load config early to get [logging] settings before tracing is initialised.
    // Errors here surface to stderr before the subscriber is up.
    let config = load_config(&oikos).whatever_context("failed to load config")?;

    // Resolve and create the log directory.
    let log_dir = resolve_log_dir(&oikos, config.logging.log_dir.as_deref());
    std::fs::create_dir_all(&log_dir).whatever_context("failed to create log directory")?;

    // Initialise tracing: console at the CLI-specified level, JSON file at
    // the configured level (default WARN+). The returned guard must live for
    // the entire process lifetime to flush the non-blocking writer on exit.
    let _log_guard = init_tracing(
        &args.log_level,
        args.json_logs,
        &log_dir,
        &config.logging.level,
        &config.logging.redaction,
    )
    .whatever_context("failed to initialise file logging")?;

    info!("aletheia starting");

    let oikos_arc = Arc::new(oikos);

    // Build the full runtime via RuntimeBuilder
    let mut runtime = RuntimeBuilder::production(Arc::clone(&oikos_arc), config.clone())
        .build()
        .await?;

    // Log retention: runs immediately at startup then every 24 h.
    spawn_log_retention(
        log_dir.clone(),
        config.logging.retention_days,
        runtime.shutdown_token.child_token(),
    );

    // Pylon HTTP gateway
    let security = aletheia_pylon::security::SecurityConfig::from_gateway(&config.gateway);

    #[cfg(feature = "mcp")]
    let app = {
        let diaporeia_state = Arc::new(aletheia_diaporeia::state::DiaporeiaState {
            session_store: Arc::clone(&runtime.state.session_store),
            nous_manager: Arc::clone(&runtime.state.nous_manager),
            tool_registry: Arc::clone(&runtime.state.tool_registry),
            oikos: Arc::clone(&runtime.state.oikos),
            start_time: runtime.state.start_time,
            config: Arc::clone(&runtime.state.config),
            shutdown: runtime.shutdown_token.clone(),
        });
        let mcp_router = aletheia_diaporeia::transport::streamable_http_router(diaporeia_state);
        info!("diaporeia MCP server mounted at /mcp");
        build_router(runtime.state.clone(), &security).merge(mcp_router)
    };

    #[cfg(not(feature = "mcp"))]
    let app = build_router(runtime.state.clone(), &security);

    let port = args.port.unwrap_or(config.gateway.port);
    // Resolve bind address: CLI flag > config gateway.bind > default 127.0.0.1.
    // "lan" is a semantic alias for "0.0.0.0" (listen on all interfaces).
    let bind_host = args.bind.as_deref().unwrap_or(&config.gateway.bind);
    let bind_addr_str = match bind_host {
        "lan" => "0.0.0.0",
        "localhost" => "127.0.0.1",
        other => other,
    };

    // Warn unconditionally when auth is disabled: every request is granted Readonly role.
    if config.gateway.auth.mode == "none" {
        warn!("auth mode is 'none' -- all requests granted Readonly role");
    }
    // Additionally warn if auth is disabled on a non-localhost bind address.
    if config.gateway.auth.mode == "none"
        && bind_addr_str != "127.0.0.1"
        && bind_addr_str != "localhost"
        && bind_addr_str != "::1"
    {
        warn!(
            bind = %bind_addr_str,
            "authentication is disabled (auth.mode = \"none\") on a non-localhost address -- \
             the API is accessible without credentials"
        );
    }

    let bind_addr = format!("{bind_addr_str}:{port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                crate::error::Error::msg(format!(
                    "Port {port} is already in use.\n  \
                     Use --port to choose another port, or stop the process using port {port}."
                ))
            } else {
                crate::error::Error::msg(format!("failed to bind to {bind_addr}: {e}"))
            }
        })?;

    info!(addr = %bind_addr, "pylon listening");

    // Axum graceful shutdown: wait for OS signal, then cancel root token so
    // all subsystems observe shutdown simultaneously.
    let token_for_signal = runtime.shutdown_token.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("signal received -- cancelling shutdown token");
            token_for_signal.cancel();
        })
        .await
        .whatever_context("server error")?;

    // Drain ordering:
    // 1. HTTP server has stopped accepting new requests (axum graceful_shutdown).
    // 2. Root token is cancelled: daemon tasks observe it and exit their loops.
    // 3. Wait for system daemon to finish in-flight maintenance work.
    // 4. Drain nous actors with a timeout, flushing fjall WAL and other state.
    // 5. Drop AppState (session store, registries).

    info!("shutting down");

    let shutdown_timeout = std::time::Duration::from_secs(
        std::env::var("ALETHEIA_SHUTDOWN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10),
    );

    // Await system daemon handles
    for handle in runtime.daemon_handles.drain(..) {
        match tokio::time::timeout(shutdown_timeout, handle).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warn!(error = %e, "system daemon panicked during shutdown"),
            Err(_) => warn!(
                timeout_secs = shutdown_timeout.as_secs(),
                "system daemon did not exit within shutdown timeout"
            ),
        }
    }

    // Drain nous actors
    runtime.state.nous_manager.drain(shutdown_timeout).await;

    // Flush the SQLite session-store WAL explicitly (#1723).
    match runtime.state.session_store.try_lock() {
        Ok(store) => {
            if let Err(e) = store.checkpoint_wal() {
                warn!(error = %e, "SQLite WAL checkpoint on shutdown failed");
            } else {
                info!("SQLite WAL checkpoint complete");
            }
        }
        Err(_) => {
            warn!("session store lock held at shutdown, WAL checkpoint skipped");
        }
    }

    drop(runtime);

    info!("shutdown complete");

    Ok(())
}

mod tracing_setup;

use tracing_setup::{init_tracing, resolve_log_dir, shutdown_signal, spawn_log_retention};
