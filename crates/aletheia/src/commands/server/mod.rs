//! Default server startup: runs when no subcommand is given.

use std::path::PathBuf;
use std::sync::Arc;

use snafu::prelude::*;
use tracing::{info, warn};

use koina::system::{Environment, RealSystem};
#[cfg(not(feature = "mcp"))]
use pylon::router::build_router;
#[cfg(feature = "mcp")]
use pylon::router::build_router_with;
use taxis::loader::load_config;
use taxis::oikos::Oikos;

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
    // WHY: Oikos is pure path resolution: no IO, safe before tracing is set up.
    let oikos = match &args.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    // WHY: Load config early to get [logging] settings before tracing is
    // initialised. Errors here surface to stderr before the subscriber is up.
    let config = load_config(&oikos).whatever_context("failed to load config")?;

    let log_dir = resolve_log_dir(&oikos, config.logging.log_dir.as_deref());
    std::fs::create_dir_all(&log_dir).whatever_context("failed to create log directory")?;

    let trace_ingest_layer = config
        .observability
        .trace_ingest
        .then(mneme::trace_ingest::TraceIngestLayer::new);

    // WARNING: The returned guard must live for the entire process lifetime
    // to flush the non-blocking writer on exit.
    let _log_guard = init_tracing(
        &args.log_level,
        args.json_logs,
        &log_dir,
        &config.logging.level,
        &config.logging.redaction,
        trace_ingest_layer.clone(),
    )
    .whatever_context("failed to initialise file logging")?;

    info!("aletheia starting");

    let oikos_arc = Arc::new(oikos);

    let runtime =
        Box::pin(RuntimeBuilder::production(Arc::clone(&oikos_arc), config.clone()).build())
            .await?;

    let disk_monitor =
        spawn_disk_space_monitor(log_dir.clone(), runtime.shutdown_token.child_token());

    spawn_log_retention(
        log_dir.clone(),
        config.logging.retention_days,
        runtime.shutdown_token.child_token(),
        Some(disk_monitor),
    );
    #[cfg(feature = "recall")]
    if let (Some(layer), Some(store)) = (trace_ingest_layer, runtime.state.knowledge_store.clone())
    {
        spawn_trace_ingest_flush(layer, store, runtime.shutdown_token.child_token());
    }

    let security = pylon::security::SecurityConfig::from_gateway(&config.gateway);

    #[cfg(feature = "mcp")]
    let app = {
        // WHY: auth_mode == "none" means no signing key; pass None so the
        // middleware skips JWT validation and injects anonymous claims.
        let mcp_jwt = if runtime.state.auth_mode == "none" {
            None
        } else {
            Some(Arc::clone(&runtime.state.jwt_manager))
        };
        let diaporeia_state = Arc::new(diaporeia::state::DiaporeiaState {
            session_store: Arc::clone(&runtime.state.session_store),
            nous_manager: Arc::clone(&runtime.state.nous_manager),
            tool_registry: Arc::clone(&runtime.state.tool_registry),
            oikos: Arc::clone(&runtime.state.oikos),
            jwt_manager: mcp_jwt,
            start_time: runtime.state.start_time,
            config: Arc::clone(&runtime.state.config),
            auth_mode: runtime.state.auth_mode.clone(),
            none_role: runtime.state.none_role.clone(),
            shutdown: runtime.shutdown_token.clone(),
            // WHY: pass the same KnowledgeStore Arc that pylon uses so the
            // MCP knowledge tools access the identical in-process instance.
            // The cfg matches AppState's knowledge_store gate (`recall`),
            // not a non-existent aletheia "knowledge-store" feature.
            #[cfg(feature = "recall")]
            knowledge_store: runtime.state.knowledge_store.clone(),
            note_store: Some(Arc::new(nous::adapters::SessionNoteAdapter(Arc::clone(
                &runtime.state.session_store,
            )))),
            blackboard_store: Some(Arc::new(nous::adapters::SessionBlackboardAdapter(
                Arc::clone(&runtime.state.session_store),
            ))),
        });
        let mcp_router = diaporeia::transport::streamable_http_router(diaporeia_state);
        info!("diaporeia MCP server mounted at /mcp");
        // WHY: merge MCP routes BEFORE pylon's middleware layers so they get
        // the full security stack (rate limiting, CSRF, metrics) (#3226).
        build_router_with(runtime.state.clone(), &security, Some(mcp_router))
    };

    #[cfg(not(feature = "mcp"))]
    let app = build_router(runtime.state.clone(), &security);

    let port = args.port.unwrap_or(config.gateway.port);
    // NOTE: Precedence is CLI flag > config gateway.bind > default 127.0.0.1.
    // "lan" is a semantic alias for "0.0.0.0" (listen on all interfaces).
    let bind_host = args.bind.as_deref().unwrap_or(&config.gateway.bind);
    let bind_addr_str = match bind_host {
        "lan" => "0.0.0.0",
        "localhost" => "127.0.0.1",
        other => other,
    };

    if config.gateway.auth.mode == "none" {
        warn!(
            none_role = %config.gateway.auth.none_role,
            "auth mode is 'none' -- all requests granted configured role"
        );
    }
    // WHY(#3716): refusing to boot with `auth.mode = "none"` on a non-loopback
    // bind address. Operators who genuinely want unauthenticated LAN/Tailscale
    // access must explicitly set `ALETHEIA_ALLOW_AUTH_NONE_LAN=1`. This
    // matches the existing opt-in pattern for accepting `auth.mode = "none"`
    // via the config API (`ALETHEIA_ALLOW_AUTH_NONE`) and prevents the
    // insecure-by-default posture where a LAN/tailnet process silently served
    // operator-privileged requests without credentials.
    if config.gateway.auth.mode == "none" && !taxis::validate::is_loopback_bind(bind_addr_str) {
        if taxis::validate::auth_none_lan_opt_in_enabled() {
            warn!(
                bind = %bind_addr_str,
                "authentication is disabled (auth.mode = \"none\") on a non-localhost address -- \
                 the API is accessible without credentials (ALETHEIA_ALLOW_AUTH_NONE_LAN=1 set)"
            );
        } else {
            return Err(crate::error::Error::msg(format!(
                "refusing to bind {bind_addr_str}: gateway.auth.mode = \"none\" on a non-localhost \
                 address would serve unauthenticated LAN/Tailscale traffic with role '{}'. \
                 Either (a) set gateway.auth.mode = \"token\" or \"jwt\", \
                 (b) bind to 127.0.0.1 or ::1, or \
                 (c) set {}=1 to opt in explicitly.",
                config.gateway.auth.none_role,
                taxis::validate::ALLOW_AUTH_NONE_LAN_ENV,
            )));
        }
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

    // WHY: notify systemd that initialization is complete and the HTTP
    // gateway is accepting connections. This is the authoritative READY=1
    // for the Type=notify service — it fires after the TCP listener binds.
    oikonomos::runner::systemd::sd_notify_ready();

    // WHY: spawn a dedicated watchdog heartbeat task that sends WATCHDOG=1
    // at half the systemd WatchdogSec interval. If the Tokio runtime
    // deadlocks, the heartbeat stops and systemd restarts the service.
    let watchdog_token = runtime.shutdown_token.child_token();
    if let Some(interval) = oikonomos::runner::systemd::sd_watchdog_interval() {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    biased;
                    () = watchdog_token.cancelled() => break,
                    _ = tick.tick() => {
                        oikonomos::runner::systemd::sd_notify_watchdog();
                    }
                }
            }
        });
        info!(
            interval_secs = interval.as_secs(),
            "systemd watchdog heartbeat started"
        );
    }

    // WHY: Without a SIGHUP handler, the signal hits the default Unix
    // disposition (terminate), crashing the server instead of reloading (#2350).
    #[cfg(unix)]
    let sighup_handle = pylon::server::spawn_sighup_handler(Arc::clone(&runtime.state));

    // WHY: Cancel root token on OS signal so all subsystems observe
    // shutdown simultaneously.
    let token_for_signal = runtime.shutdown_token.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // SAFETY: "token" here is a CancellationToken (shutdown coordination), not a credential
            info!("signal received -- cancelling shutdown token"); // kanon:ignore SECURITY/credential-logging -- logs signal event, not a credential
            token_for_signal.cancel();
        })
        .await
        .whatever_context("server error")?;

    // INVARIANT: Drain ordering must follow this sequence:
    // 0. Notify systemd that shutdown has begun (STOPPING=1).
    // 1. HTTP server has stopped accepting new requests (axum graceful_shutdown).
    // 2. Root token is cancelled: daemon tasks observe it and exit their loops.
    // 2a. Drain SIGHUP handler (observes shutdown token).
    // 3. Close TaskTracker + wait with timeout: all tracked background tasks
    //    (system daemon, per-agent daemon runners, lazy init) drain cleanly.
    // 4. Drain nous actors with a timeout, flushing fjall WAL and other state.
    // 5. Drop AppState (session store, registries).

    // WHY: notify systemd that graceful shutdown has begun so it does not
    // treat the drain period as a hang and send SIGKILL prematurely.
    oikonomos::runner::systemd::sd_notify_stopping();

    info!("shutting down");

    // WHY: cancel OAuth credential refresh background tasks before draining
    // in-flight LLM requests so tokens are not refreshed after shutdown begins.
    runtime.state.provider_registry.shutdown();

    let shutdown_timeout = std::time::Duration::from_secs(
        RealSystem
            .var("ALETHEIA_SHUTDOWN_TIMEOUT_SECS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10),
    );

    #[cfg(unix)]
    {
        if let Some(handle) = sighup_handle
            && tokio::time::timeout(shutdown_timeout, handle)
                .await
                .is_err()
        {
            warn!("sighup handler did not exit within drain timeout");
        }
    }

    // WHY: close() prevents new tasks from being spawned; wait() resolves
    // when every tracked task has completed. The timeout ensures we don't
    // hang indefinitely on a stuck daemon.
    runtime.task_tracker.close();
    tokio::select! {
        () = runtime.task_tracker.wait() => {
            info!("all tracked tasks drained");
        }
        () = tokio::time::sleep(shutdown_timeout) => {
            warn!(
                remaining = runtime.task_tracker.len(),
                timeout_secs = shutdown_timeout.as_secs(),
                "tracked task drain timed out"
            );
        }
    }

    runtime.state.nous_manager.drain(shutdown_timeout).await;

    // WHY: fjall's LSM-tree flushes synchronously on write-tx commit and
    // closes cleanly on Drop — no explicit checkpoint is needed. The SQLite
    // WAL checkpoint that lived here was removed alongside rusqlite (#3446).
    drop(runtime);

    info!("shutdown complete");

    Ok(())
}

mod tracing_setup;

#[cfg(feature = "recall")]
use tracing_setup::spawn_trace_ingest_flush;
use tracing_setup::{
    init_tracing, resolve_log_dir, shutdown_signal, spawn_disk_space_monitor, spawn_log_retention,
};
