//! Server entry point with graceful shutdown.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use snafu::{ResultExt, Snafu};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;

use crate::router::build_router;
use crate::security::SecurityConfig;
use crate::state::AppState;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind, e.g. `"0.0.0.0:3000"`.
    pub bind_addr: String,
    /// Path to the Aletheia instance directory.
    pub instance_path: PathBuf,
    /// Security configuration for middleware layers.
    pub security: SecurityConfig,
}

/// Errors from the pylon HTTP server lifecycle.
#[derive(Debug, Snafu)]
pub enum ServerError {
    /// Failed to open or initialize the session store.
    #[snafu(display("failed to open session store: {source}"))]
    SessionStore {
        source: aletheia_mneme::error::Error,
    },

    /// TCP listener failed to bind to the configured address.
    #[snafu(display("failed to bind to {addr}: {source}"))]
    Bind {
        addr: String,
        source: std::io::Error,
    },

    /// Error while serving HTTP requests.
    #[snafu(display("server error: {source}"))]
    Serve { source: std::io::Error },

    /// TLS certificate or key could not be loaded.
    #[snafu(display("TLS configuration error: {source}"))]
    TlsConfig { source: std::io::Error },

    /// Binary was compiled without the `tls` feature flag.
    #[snafu(display("TLS support not compiled — rebuild with --features tls"))]
    TlsNotCompiled,

    /// Instance directory layout validation failed.
    #[snafu(display("instance layout invalid: {source}"))]
    Validation {
        source: aletheia_taxis::error::Error,
    },
}

/// Start the HTTP gateway and block until shutdown.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    let oikos = Oikos::from_root(&config.instance_path);
    oikos.validate().context(ValidationSnafu)?;
    let oikos = Arc::new(oikos);

    let session_store = SessionStore::open(&oikos.sessions_db()).context(SessionStoreSnafu)?;
    let session_store = Arc::new(Mutex::new(session_store));

    let provider_registry = Arc::new(ProviderRegistry::new());
    let tool_registry = Arc::new(ToolRegistry::new());
    let jwt_manager = Arc::new(JwtManager::new(JwtConfig::default()));

    let mut nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        None,
        None,
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(vec![]),
        None,
        None,
    );
    let nous_config = NousConfig::default();
    nous_manager
        .spawn(nous_config, PipelineConfig::default())
        .await;

    let aletheia_config = aletheia_taxis::loader::load_config(&oikos).unwrap_or_else(|e| {
        tracing::warn!("failed to load config, using defaults: {e}");
        AletheiaConfig::default()
    });

    let state = Arc::new(AppState {
        session_store: Arc::clone(&session_store),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
        auth_mode: aletheia_config.gateway.auth.mode.clone(),
        config: Arc::new(tokio::sync::RwLock::new(aletheia_config)),
        shutdown: CancellationToken::new(),
    });

    let app = build_router(state.clone(), &config.security);

    if config.security.tls_enabled {
        serve_tls(app, &config).await?;
    } else {
        serve_plain(app, &config.bind_addr).await?;
    }

    state.nous_manager.shutdown_readonly().await;
    info!("pylon shutdown complete");
    Ok(())
}

async fn serve_plain(app: axum::Router, bind_addr: &str) -> Result<(), ServerError> {
    let is_loopback = bind_addr.starts_with("127.") || bind_addr.starts_with("[::1]");
    if !is_loopback {
        warn!(
            addr = %bind_addr,
            "serving without TLS on public address — connections are unencrypted"
        );
    }

    let listener = TcpListener::bind(bind_addr).await.context(BindSnafu {
        addr: bind_addr.to_owned(),
    })?;

    info!(addr = %bind_addr, tls = false, "pylon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context(ServeSnafu)
}

#[cfg(feature = "tls")]
async fn serve_tls(app: axum::Router, config: &ServerConfig) -> Result<(), ServerError> {
    use std::time::Duration;

    use axum_server::tls_rustls::RustlsConfig;

    let cert_path = config
        .security
        .tls_cert_path
        .as_deref()
        .unwrap_or(Path::new("instance/config/tls/cert.pem"));
    let key_path = config
        .security
        .tls_key_path
        .as_deref()
        .unwrap_or(Path::new("instance/config/tls/key.pem"));

    let tls_config = RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .context(TlsConfigSnafu)?;

    let addr: std::net::SocketAddr = config
        .bind_addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        .context(BindSnafu {
            addr: config.bind_addr.clone(),
        })?;

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(30)));
    });

    info!(addr = %config.bind_addr, tls = true, "pylon listening (TLS)");

    axum_server::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .context(ServeSnafu)
}

#[cfg(not(feature = "tls"))]
fn serve_tls(
    _app: axum::Router,
    _config: &ServerConfig,
) -> impl std::future::Future<Output = Result<(), ServerError>> {
    std::future::ready(TlsNotCompiledSnafu.fail())
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
