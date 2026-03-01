//! Server entry point with graceful shutdown.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use snafu::{ResultExt, Snafu};
use tokio::net::TcpListener;
use tracing::info;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::NousConfig;
use aletheia_nous::session::SessionManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_taxis::oikos::Oikos;

use crate::router::build_router;
use crate::state::AppState;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind, e.g. `"0.0.0.0:3000"`.
    pub bind_addr: String,
    /// Path to the Aletheia instance directory.
    pub instance_path: PathBuf,
}

#[derive(Debug, Snafu)]
pub enum ServerError {
    #[snafu(display("failed to open session store: {source}"))]
    SessionStore { source: aletheia_mneme::error::Error },

    #[snafu(display("failed to bind to {addr}: {source}"))]
    Bind {
        addr: String,
        source: std::io::Error,
    },

    #[snafu(display("server error: {source}"))]
    Serve { source: std::io::Error },
}

/// Start the HTTP gateway and block until shutdown.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    let oikos = Oikos::from_root(&config.instance_path);

    let session_store =
        SessionStore::open(&oikos.sessions_db()).context(SessionStoreSnafu)?;

    let nous_config = NousConfig::default();
    let session_manager = SessionManager::new(nous_config);
    let provider_registry = ProviderRegistry::new();
    let tool_registry = ToolRegistry::new();

    let state = Arc::new(AppState {
        session_store: Mutex::new(session_store),
        provider_registry,
        session_manager,
        tool_registry,
        oikos,
        start_time: Instant::now(),
    });

    let app = build_router(state);

    let listener = TcpListener::bind(&config.bind_addr)
        .await
        .context(BindSnafu {
            addr: config.bind_addr.clone(),
        })?;

    info!(addr = %config.bind_addr, "pylon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context(ServeSnafu)?;

    info!("pylon shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received ctrl+c"),
        () = terminate => info!("received SIGTERM"),
    }
}
