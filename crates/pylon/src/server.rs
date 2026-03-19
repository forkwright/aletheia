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
#[expect(
    missing_docs,
    reason = "snafu error variant fields (addr, source) are self-documenting via display format"
)]
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
///
/// # Errors
///
/// Returns [`ServerError::Validation`] if the instance directory layout is invalid.
/// Returns [`ServerError::SessionStore`] if the session database cannot be opened.
/// Returns [`ServerError::Bind`] if the server cannot bind to the configured address.
/// Returns [`ServerError::Serve`] if the HTTP server encounters a fatal I/O error.
/// Returns [`ServerError::TlsConfig`] if TLS is enabled but certs cannot be loaded.
/// Returns [`ServerError::TlsNotCompiled`] if TLS is enabled but the feature is absent.
///
/// # Cancel safety
///
/// Not cancel-safe. Cancellation during startup (before `serve_plain`/`serve_tls`
/// returns) leaves partially initialised state. Once serving, the future blocks
/// until the OS delivers a shutdown signal; dropping it at that point skips the
/// SIGHUP-handler drain and `shutdown_readonly` call, which may leave actor tasks
/// running until the runtime exits.
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

    #[cfg(feature = "knowledge-store")]
    let knowledge_store = nous_manager.knowledge_store().cloned();

    let (config_tx, _config_rx) = tokio::sync::watch::channel(aletheia_config.clone());

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
        config_tx,
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store,
    });

    #[cfg(unix)]
    let sighup_handle = spawn_sighup_handler(Arc::clone(&state));

    let app = build_router(state.clone(), &config.security);

    if config.security.tls_enabled {
        serve_tls(app, &config).await?;
    } else {
        serve_plain(app, &config.bind_addr).await?;
    }

    // NOTE: Cancel shutdown token so background tasks (SIGHUP handler) observe shutdown.
    state.shutdown.cancel();

    #[cfg(unix)]
    {
        let drain_timeout = std::time::Duration::from_secs(10);
        if tokio::time::timeout(drain_timeout, sighup_handle)
            .await
            .is_err()
        {
            warn!("sighup handler did not exit within drain timeout");
        }
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

    // WHY: `into_make_service_with_connect_info` injects `ConnectInfo<SocketAddr>`
    // into request extensions so the rate limiter reads the real peer address
    // rather than spoofable `X-Forwarded-For` headers.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context(ServeSnafu)
}

#[cfg(feature = "tls")]
async fn serve_tls(app: axum::Router, config: &ServerConfig) -> Result<(), ServerError> {
    use std::time::Duration;

    use axum_server::tls_rustls::RustlsConfig;
    use tracing::Instrument;

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
    let _shutdown_task = tokio::spawn(
        async move {
            shutdown_signal().await;
            shutdown_handle.graceful_shutdown(Some(Duration::from_secs(30)));
        }
        .instrument(tracing::info_span!("shutdown_signal")),
    );

    info!(addr = %config.bind_addr, tls = true, "pylon listening (TLS)");

    axum_server::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
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

/// Apply a prepared config reload to the live state.
///
/// Acquires the config write lock, swaps the config, logs the diff,
/// and notifies subscribers via the watch channel.
pub(crate) async fn apply_reload(state: &AppState, outcome: aletheia_taxis::reload::ReloadOutcome) {
    aletheia_taxis::reload::log_diff(&outcome.diff);

    let mut config = state.config.write().await;
    *config = outcome.new_config.clone();
    drop(config);

    // WHY: send can only fail if all receivers are dropped, which means
    // no actors are listening. Safe to ignore.
    let _ = state.config_tx.send(outcome.new_config);
}

/// Spawn a background task that listens for SIGHUP and triggers config reload.
///
/// On each SIGHUP, re-reads config from disk, validates, diffs against the
/// current config, and applies hot-reloadable values. Invalid configs are
/// rejected with an error log; the running config is preserved.
///
/// Returns a `JoinHandle` so the caller can await graceful shutdown.
#[cfg(unix)]
fn spawn_sighup_handler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    use tracing::Instrument;

    let shutdown = state.shutdown.clone();
    let span = tracing::info_span!("sighup_reload");
    tokio::spawn(
        async move {
            #[expect(
                clippy::expect_used,
                reason = "signal handler installation is infallible on supported platforms"
            )]
            let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("failed to install SIGHUP handler");

            loop {
                tokio::select! {
                    biased;
                    // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                    // dropping it before it fires has no side effects. Polled first (biased)
                    // so shutdown is never starved by a flood of SIGHUP signals.
                    () = shutdown.cancelled() => break,
                    // SAFETY: cancel-safe. `tokio::signal::unix::Signal::recv()` is
                    // cancel-safe; if dropped before a signal arrives, no signal is lost
                    // from the OS perspective (the kernel keeps delivering SIGHUP and the
                    // next call to recv() will return it).
                    signal = sighup.recv() => {
                        if signal.is_none() {
                            break;
                        }
                        info!("received SIGHUP, reloading config");

                        let current = state.config.read().await.clone();
                        match aletheia_taxis::reload::prepare_reload(&state.oikos, &current) {
                            Ok(outcome) => {
                                if outcome.diff.is_empty() {
                                    info!("config reload: no changes detected");
                                } else {
                                    apply_reload(&state, outcome).await;
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "config reload failed, keeping current config");
                            }
                        }
                    }
                }
            }
        }
        .instrument(span),
    )
}

#[expect(
    clippy::expect_used,
    reason = "signal handler installation is infallible on supported platforms"
)]
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
        // SAFETY: cancel-safe. The `ctrl_c` future wraps `tokio::signal::ctrl_c()`,
        // which is cancel-safe: dropping it before it resolves simply re-arms the
        // handler; Ctrl+C will be caught by the next call.
        () = ctrl_c => info!("received ctrl+c"),
        // SAFETY: cancel-safe. The `terminate` future wraps `Signal::recv()`,
        // which is cancel-safe: if dropped before SIGTERM arrives, the signal
        // remains pending in the OS and will be delivered on the next recv() call.
        // On non-Unix platforms `terminate` is `pending::<()>()`, which is trivially
        // cancel-safe and never resolves.
        () = terminate => info!("received SIGTERM"),
    }
}
