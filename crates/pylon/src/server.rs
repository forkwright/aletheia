//! Server entry point with graceful shutdown.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use snafu::{ResultExt, Snafu};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use hermeneus::provider::ProviderRegistry;
use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

use crate::router::build_router;
use crate::security::SecurityConfig;
use crate::state::{AppState, ConfigState};

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

struct ServerRuntime {
    oikos: Arc<Oikos>,
    aletheia_config: AletheiaConfig,
    session_store: Arc<Mutex<SessionStore>>,
    provider_registry: Arc<ProviderRegistry>,
    tool_registry: Arc<ToolRegistry>,
    nous_manager: NousManager,
}

/// Errors from the pylon HTTP server lifecycle.
#[derive(Debug, Snafu)]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (addr, source) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum ServerError {
    /// Configuration could not be loaded at startup.
    #[snafu(display("failed to load config from {}: {source}", path.display()))]
    Config {
        path: PathBuf,
        #[snafu(source(from(taxis::error::Error, Box::new)))]
        source: Box<taxis::error::Error>,
    },

    /// Failed to open or initialize the session store.
    #[snafu(display("failed to open session store: {source}"))]
    SessionStore { source: mneme::error::Error },

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
    Validation { source: taxis::error::Error },

    /// Default nous agent failed to spawn during startup.
    #[snafu(display("default nous spawn failed: {source}"))]
    NousSpawn { source: nous::error::Error },

    /// Authentication setup failed during startup.
    #[snafu(display("authentication setup failed: {message}"))]
    Auth { message: String },
}

/// Start the HTTP gateway and block until shutdown.
///
/// # Errors
///
/// Returns [`ServerError::Validation`] if the instance directory layout is invalid.
/// Returns [`ServerError::Config`] if the config cascade cannot be loaded.
/// Returns [`ServerError::SessionStore`] if the session database cannot be opened.
/// Returns [`ServerError::Bind`] if the server cannot bind to the configured address.
/// Returns [`ServerError::Serve`] if the HTTP server encounters a fatal I/O error.
/// Returns [`ServerError::NousSpawn`] if the default nous agent fails to spawn.
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
    let state = build_state(&config).await?;

    #[cfg(unix)]
    let sighup_handle = spawn_sighup_handler(Arc::clone(&state));

    let app = build_router(Arc::clone(&state), &config.security);

    write_discovery_file(&state, &config.bind_addr).await;
    serve_configured(app, &config).await?;

    #[cfg(unix)]
    finish_shutdown(state, sighup_handle).await;

    #[cfg(not(unix))]
    finish_shutdown(state).await;

    Ok(())
}

async fn build_state(config: &ServerConfig) -> Result<Arc<AppState>, ServerError> {
    let runtime = prepare_runtime(config).await?;
    assemble_state(runtime)
}

async fn prepare_runtime(config: &ServerConfig) -> Result<ServerRuntime, ServerError> {
    let oikos = Oikos::from_root(&config.instance_path);
    oikos.validate().context(ValidationSnafu)?;
    let oikos = Arc::new(oikos);

    let config_path = oikos.config().join("aletheia.toml");
    let aletheia_config =
        taxis::loader::load_config(&oikos).context(ConfigSnafu { path: config_path })?;

    let session_store = SessionStore::open(&oikos.sessions_db()).context(SessionStoreSnafu)?;
    let session_store = Arc::new(Mutex::new(session_store));

    let provider_registry = Arc::new(ProviderRegistry::new());
    let tool_registry = Arc::new(ToolRegistry::new());

    let nous_manager = spawn_default_nous(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        Arc::clone(&session_store),
    )
    .await?;

    // WHY: Emit a loud warning when authentication is disabled so operators
    // see the consequence in every log aggregator, not just buried in the
    // gateway config. (#3383)
    taxis::validate::warn_if_auth_disabled(&aletheia_config);

    Ok(ServerRuntime {
        oikos,
        aletheia_config,
        session_store,
        provider_registry,
        tool_registry,
        nous_manager,
    })
}

async fn spawn_default_nous(
    provider_registry: Arc<ProviderRegistry>,
    tool_registry: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    session_store: Arc<Mutex<SessionStore>>,
) -> Result<NousManager, ServerError> {
    let mut nous_manager = NousManager::new(
        provider_registry,
        tool_registry,
        oikos,
        None,
        None,
        Some(session_store),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(vec![]),
        None,
        None,
        taxis::config::NousBehaviorConfig::default(),
        taxis::config::ToolLimitsConfig::default(),
    );
    let nous_config = NousConfig::default();
    nous_manager
        .spawn(nous_config, PipelineConfig::default())
        .await
        .context(NousSpawnSnafu)?;

    Ok(nous_manager)
}

fn assemble_state(runtime: ServerRuntime) -> Result<Arc<AppState>, ServerError> {
    let ServerRuntime {
        oikos,
        aletheia_config,
        session_store,
        provider_registry,
        tool_registry,
        nous_manager,
    } = runtime;
    let (jwt_manager, auth_facade) = build_auth_state(&aletheia_config, &oikos)?;

    #[cfg(feature = "knowledge-store")]
    let knowledge_store = nous_manager.knowledge_store().cloned();

    let (config_tx, _config_rx) = tokio::sync::watch::channel(aletheia_config.clone());
    let workspace_root =
        crate::state::resolve_workspace_root(&oikos, aletheia_config.workspace.root.as_deref());
    let auth_mode = aletheia_config.gateway.auth.mode.clone();
    let none_role = aletheia_config.gateway.auth.none_role.clone();
    let loopback_only_metrics = aletheia_config.gateway.bind == "localhost";
    let idempotency_cache = Arc::new(crate::idempotency::IdempotencyCache::from_config(
        &aletheia_config.api_limits,
    ));
    let turn_buffer_registry = Arc::new(crate::turn_buffer::TurnBufferRegistry::from_config(
        &aletheia_config.gateway,
    ));

    // WHY: pylon's standalone server only registers its own metric families.
    // The aletheia binary uses `register_all_metrics` to register every
    // metrics-emitting crate's families against the shared registry.
    let metrics_registry = koina::metrics::MetricsRegistry::new();
    metrics_registry.with_registry(crate::metrics::register);

    let credential_runtime = Arc::new(crate::credential_runtime::CredentialRuntimeManager::new(
        Arc::clone(&provider_registry),
    ));

    Ok(Arc::new(AppState {
        session_store: Arc::clone(&session_store),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        workspace_root,
        jwt_manager,
        auth_facade,
        credential_runtime,
        start_time: Instant::now(),
        auth_mode,
        none_role,
        loopback_only_metrics,
        config: Arc::new(tokio::sync::RwLock::new(aletheia_config)),
        config_tx,
        idempotency_cache,
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store,
        // WHY: pylon's standalone server does not configure embeddings; the
        // aletheia binary owns the embedding pipeline. Health reports "warn".
        embedding_provider: None,
        turn_buffer_registry,
        metrics_registry,
        event_bus: Arc::new(crate::event_bus::EventBus::new(256)),
        approval_registry: Arc::new(crate::approval_registry::ApprovalRegistry::new()),
    }))
}

async fn write_discovery_file(state: &AppState, bind_addr: &str) {
    // WHY: Best-effort: failure is logged but does not abort startup.
    let data_dir = state.oikos.data();
    if let Err(e) = crate::discovery::write_discovery_file(&data_dir, bind_addr).await {
        warn!(error = %e, "failed to write discovery file, clients may need manual URL");
    }
}

async fn serve_configured(app: axum::Router, config: &ServerConfig) -> Result<(), ServerError> {
    if config.security.tls.enabled {
        serve_tls(app, config).await
    } else {
        serve_plain(app, &config.bind_addr).await
    }
}

#[cfg(unix)]
async fn finish_shutdown(state: Arc<AppState>, sighup_handle: Option<tokio::task::JoinHandle<()>>) {
    state.shutdown.cancel();
    drain_sighup_handler(sighup_handle).await;
    finish_shutdown_common(state).await;
}

#[cfg(not(unix))]
async fn finish_shutdown(state: Arc<AppState>) {
    state.shutdown.cancel();
    finish_shutdown_common(state).await;
}

#[cfg(unix)]
async fn drain_sighup_handler(sighup_handle: Option<tokio::task::JoinHandle<()>>) {
    let drain_timeout = std::time::Duration::from_secs(10);
    if let Some(handle) = sighup_handle
        && tokio::time::timeout(drain_timeout, handle).await.is_err()
    {
        warn!("sighup handler did not exit within drain timeout");
    }
}

async fn finish_shutdown_common(state: Arc<AppState>) {
    // WHY: remove discovery file so clients do not attempt to connect to
    // a stopped server. Best-effort: if removal fails, the file is stale
    // but harmless (clients will get connection-refused).
    crate::discovery::remove_discovery_file(&state.oikos.data()).await;

    state.nous_manager.shutdown_readonly().await;
    info!("pylon shutdown complete");
}

fn build_auth_state(
    config: &AletheiaConfig,
    oikos: &Oikos,
) -> Result<(Arc<JwtManager>, Arc<AuthFacade>), ServerError> {
    let jwt_config = jwt_config_from_gateway(config);
    jwt_config
        .validate_for_auth_mode(config.gateway.auth.mode.as_str())
        .map_err(|e| ServerError::Auth {
            message: e.to_string(),
        })?;
    let auth_store_path = oikos.data().join("auth.fjall");
    let auth_facade = Arc::new(
        AuthFacade::new(
            AuthConfig {
                jwt: jwt_config.clone(),
            },
            &auth_store_path,
        )
        .map_err(|e| ServerError::Auth {
            message: format!("failed to open {}: {e}", auth_store_path.display()),
        })?,
    );
    let jwt_manager = Arc::new(JwtManager::new(jwt_config));
    Ok((jwt_manager, auth_facade))
}

fn jwt_config_from_gateway(config: &AletheiaConfig) -> JwtConfig {
    let signing_key = config.gateway.auth.signing_key.clone().or_else(|| {
        RealSystem
            .var("ALETHEIA_JWT_SECRET")
            .map(SecretString::from)
    });
    let clock_skew_leeway_secs = config.jwt.clock_skew_leeway_secs;
    match signing_key {
        Some(signing_key) => JwtConfig {
            signing_key,
            clock_skew_leeway_secs,
            ..JwtConfig::default()
        },
        None => JwtConfig {
            clock_skew_leeway_secs,
            ..JwtConfig::default()
        },
    }
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
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context(ServeSnafu)
}

#[cfg(feature = "tls")]
async fn serve_tls(app: axum::Router, config: &ServerConfig) -> Result<(), ServerError> {
    use std::path::Path;
    use std::time::Duration;

    use axum_server::tls_rustls::RustlsConfig;
    use tracing::Instrument;

    let cert_path = config
        .security
        .tls
        .cert_path
        .as_deref()
        .unwrap_or(Path::new("instance/config/tls/cert.pem"));
    let key_path = config
        .security
        .tls
        .key_path
        .as_deref()
        .unwrap_or(Path::new("instance/config/tls/key.pem"));

    let tls_config = RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .context(TlsConfigSnafu)?;

    let addr: SocketAddr = config
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
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
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
/// and notifies subscribers via the watch channel. Cold changes
/// (fields that require a process restart) are excluded from the
/// applied config — subscribers only see hot-reloadable values.
pub(crate) async fn apply_reload(
    config_state: &ConfigState,
    outcome: taxis::reload::ReloadOutcome,
) {
    taxis::reload::log_diff(&outcome.diff);

    let cold = outcome.diff.cold_changes();

    // WHY: Cold changes (gateway.port, gateway.bind, gateway.tls,
    // gateway.auth.mode, channels, etc.) take effect only after restart.
    // Writing them to in-memory config misleads subscribers into seeing
    // values the runtime is not actually using.
    let config_to_apply = if cold.is_empty() {
        outcome.new_config
    } else {
        tracing::warn!(
            count = cold.len(),
            "cold config changes deferred until restart"
        );
        let current = config_state.config.read().await;
        match taxis::reload::preserve_restart_required_values(
            &current,
            &outcome.new_config,
            &outcome.diff,
        ) {
            Ok(config) => config,
            Err(e) => {
                tracing::error!(error = %e, "failed to preserve cold config values");
                current.clone()
            }
        }
    };

    let mut config = config_state.config.write().await;
    *config = config_to_apply.clone();
    drop(config);

    // WHY: send can only fail if all receivers are dropped, which means
    // no actors are listening. Safe to ignore.
    let _ = config_state.config_tx.send(config_to_apply);
}

/// Spawn a background task that listens for SIGHUP and triggers config reload.
///
/// On each SIGHUP, re-reads config from disk, validates, diffs against the
/// current config, and applies hot-reloadable values. Invalid configs are
/// rejected with an error log; the running config is preserved.
///
/// Returns `Some(JoinHandle)` on success so the caller can await graceful
/// shutdown. Returns `None` if signal handler installation fails (e.g., in
/// restricted containers); the server continues serving without config reload.
#[cfg(unix)]
pub fn spawn_sighup_handler(state: Arc<AppState>) -> Option<tokio::task::JoinHandle<()>> {
    spawn_sighup_handler_with(
        state,
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()),
    )
}

/// Core implementation of [`spawn_sighup_handler`] that accepts a pre-built
/// signal result.  This allows unit tests to inject failure without relying on
/// OS-level resource exhaustion.
#[cfg(unix)]
pub(crate) fn spawn_sighup_handler_with(
    state: Arc<AppState>,
    signal: std::io::Result<tokio::signal::unix::Signal>,
) -> Option<tokio::task::JoinHandle<()>> {
    use axum::extract::FromRef;
    use tracing::Instrument;

    let mut sighup = match signal {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to install SIGHUP handler");
            return None;
        }
    };

    let shutdown = state.shutdown.clone();
    let span = tracing::info_span!("sighup_reload");
    Some(tokio::spawn(
        async move {
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
                        match taxis::reload::prepare_reload(&state.oikos, &current) {
                            Ok(outcome) => {
                                if outcome.diff.is_empty() {
                                    info!("config reload: no changes detected");
                                } else {
                                    apply_reload(&ConfigState::from_ref(&state), outcome).await;
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
    ))
}

pub(crate) async fn shutdown_signal() {
    shutdown_signal_with(
        tokio::signal::ctrl_c().await,
        #[cfg(unix)]
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()),
    )
    .await;
}

/// Core implementation of [`shutdown_signal`] that accepts pre-built signal
/// results.  This allows unit tests to inject failure without relying on
/// OS-level resource exhaustion.
pub(crate) async fn shutdown_signal_with(
    ctrl_c_result: std::io::Result<()>,
    #[cfg(unix)] terminate_result: std::io::Result<tokio::signal::unix::Signal>,
) {
    let ctrl_c = async {
        if let Err(e) = ctrl_c_result {
            tracing::warn!(error = %e, "failed to install ctrl+c handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match terminate_result {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
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

#[cfg(test)]
mod tests {
    use snafu::IntoError as _;

    use super::*;
    use crate::security::{CorsConfig, CsrfConfig, RateLimitConfig, TlsConfig};

    fn make_security() -> SecurityConfig {
        SecurityConfig {
            body_limit_bytes: 10 * 1024 * 1024,
            cors: CorsConfig::default(),
            csrf: CsrfConfig::default(),
            tls: TlsConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }

    #[test]
    fn server_config_fields_are_accessible() {
        let config = ServerConfig {
            bind_addr: "127.0.0.1:3000".to_owned(),
            instance_path: std::path::PathBuf::from("/tmp/instance"),
            security: make_security(),
        };
        assert_eq!(config.bind_addr, "127.0.0.1:3000");
        assert_eq!(
            config.instance_path,
            std::path::PathBuf::from("/tmp/instance")
        );
    }

    #[test]
    fn server_error_bind_display_includes_addr() {
        let err = BindSnafu {
            addr: "0.0.0.0:3000".to_owned(),
        }
        .into_error(std::io::Error::from(std::io::ErrorKind::AddrInUse));
        assert!(err.to_string().contains("0.0.0.0:3000"));
    }

    #[test]
    fn server_error_tls_not_compiled_display() {
        let err: ServerError = TlsNotCompiledSnafu.build();
        assert!(err.to_string().contains("TLS"));
    }

    #[tokio::test]
    async fn run_fails_on_malformed_config() {
        let instance = tempfile::tempdir().unwrap_or_else(|e| panic!("create tempdir: {e}"));
        for dir in ["config", "data", "nous"] {
            let path = instance.path().join(dir);
            std::fs::create_dir_all(&path)
                .unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
        }
        let config_path = instance.path().join("config").join("aletheia.toml");
        koina::fs::write_restricted(&config_path, b"[gateway\nport = 9999\n")
            .unwrap_or_else(|e| panic!("write {}: {e}", config_path.display()));

        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_owned(),
            instance_path: instance.path().to_path_buf(),
            security: make_security(),
        };

        let err = match run(config).await {
            Ok(()) => panic!("malformed config should stop startup"),
            Err(err) => err,
        };

        let ServerError::Config { path, source } = err else {
            panic!("expected config error, got {err}");
        };
        assert_eq!(path, config_path);
        assert!(
            source.to_string().contains("failed to parse TOML config"),
            "source should preserve parse failure: {source}"
        );
    }
}
