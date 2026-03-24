//! Default server startup: runs when no subcommand is given.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

use aletheia_agora::types::ChannelProvider;
use aletheia_koina::secret::SecretString;
use aletheia_mneme::embedding::{
    DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::cross::CrossNousRouter;
use aletheia_nous::manager::NousManager;
use aletheia_oikonomos::runner::TaskRunner;
use aletheia_organon::types::ToolServices;
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::config::resolve_nous;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;
use aletheia_taxis::validate::validate_section;

use crate::commands::maintenance;
use crate::daemon_bridge;
use crate::planning_adapter;

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
    reason = "binary entrypoint — sequential init steps"
)]
pub(crate) async fn run(args: Args) -> Result<()> {
    // Oikos is pure path resolution: no IO, safe before tracing is set up.
    let oikos = match &args.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    // Load config early to get [logging] settings before tracing is initialised.
    // Errors here surface via anyhow to stderr before the subscriber is up.
    let config = load_config(&oikos).context("failed to load config")?;

    // Resolve and create the log directory.
    let log_dir = resolve_log_dir(&oikos, config.logging.log_dir.as_deref());
    std::fs::create_dir_all(&log_dir).context("failed to create log directory")?;

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
    .context("failed to initialise file logging")?;

    info!("aletheia starting");

    // Root cancellation token: cancelled on SIGTERM/SIGINT.
    // Child tokens are propagated to every actor and daemon task.
    let shutdown_token = CancellationToken::new();

    info!(root = %oikos.root().display(), "instance discovered");

    // Startup validation: fail fast before any actors or stores initialise
    oikos.validate().context("instance layout invalid")?;

    info!(
        port = config.gateway.port,
        agents = config.agents.list.len(),
        "config loaded"
    );

    // Validate all config sections: fail fast before any actors or stores initialise.
    let config_value =
        serde_json::to_value(&config).context("failed to serialize config for validation")?;
    for section in &[
        "agents",
        "gateway",
        "maintenance",
        "data",
        "embedding",
        "channels",
        "bindings",
        "credential",
    ] {
        if let Some(section_value) = config_value.get(section) {
            validate_section(section, section_value)
                .with_context(|| format!("invalid config section '{section}'"))?;
        }
    }
    info!("config validated");

    // JWT key validation: fail if auth mode requires JWT and the key is still the placeholder.
    let jwt_key: Option<SecretString> = config.gateway.auth.signing_key.clone().or_else(|| {
        std::env::var("ALETHEIA_JWT_SECRET")
            .ok()
            .map(SecretString::from)
    });
    let jwt_config = match jwt_key {
        Some(k) => JwtConfig {
            signing_key: k,
            ..JwtConfig::default()
        },
        None => JwtConfig::default(),
    };
    jwt_config
        .validate_for_auth_mode(config.gateway.auth.mode.as_str())
        .context("JWT key security check failed")?;

    // Validate per-agent workspace paths: fatal if any agent workspace is invalid.
    for agent in &config.agents.list {
        oikos.validate_workspace_path(&agent.workspace).with_context(|| {
            format!(
                "agent '{}' workspace path '{}' is invalid — create the directory or fix the config",
                agent.id, agent.workspace
            )
        })?;
    }

    // Domain packs: load external knowledge packs declared in config
    let loaded_packs = aletheia_thesauros::loader::load_packs(&config.packs);
    let packs = Arc::new(loaded_packs);

    // Session store
    let db_path = oikos.sessions_db();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data dir {}", parent.display()))?;
    }
    let session_store = Arc::new(Mutex::new(
        SessionStore::open(&db_path)
            .with_context(|| format!("failed to open session store at {}", db_path.display()))?,
    ));
    info!(path = %db_path.display(), "session store opened");

    // JWT manager: config validated above; placeholder only reaches here when mode="none".
    let jwt_manager = JwtManager::new(jwt_config);

    // Build shared registries: single instances used by both NousManager and AppState
    let provider_registry = Arc::new(build_provider_registry(&config, &oikos));
    let mut tool_registry = build_tool_registry(&config.sandbox)?;

    // Register domain pack tools alongside builtins
    let tool_errors = aletheia_thesauros::tools::register_pack_tools(&packs, &mut tool_registry);
    for err in &tool_errors {
        warn!(error = %err, "failed to register pack tool");
    }

    let tool_registry = Arc::new(tool_registry);
    let oikos_arc = Arc::new(oikos);

    // Embedding provider: drives recall query embedding.
    // WHY: start in degraded mode rather than refusing to start when the embedding model
    // fails to load (e.g., missing model files or disabled candle feature).  Recall and
    // vector search will be unavailable but basic conversation continues (#1451).
    let embedding_config = EmbeddingConfig {
        provider: config.embedding.provider.clone(),
        model: config.embedding.model.clone(),
        dimension: Some(config.embedding.dimension),
        api_key: None,
    };
    let embedding_provider: Arc<dyn EmbeddingProvider> = match create_provider(&embedding_config) {
        Ok(p) => {
            info!(
                provider = %config.embedding.provider,
                dim = config.embedding.dimension,
                "embedding provider created"
            );
            Arc::from(p)
        }
        Err(e) => {
            warn!(
                error = %e,
                provider = %config.embedding.provider,
                "embedding provider failed to load — starting in degraded mode \
                 (recall and vector search unavailable)"
            );
            Arc::new(DegradedEmbeddingProvider::new(config.embedding.dimension))
        }
    };

    // Cross-nous router for inter-agent messaging
    let cross_router = Arc::new(CrossNousRouter::default());

    // Build signal provider early so it can be shared with tool services
    let signal_provider = build_signal_provider(&config.channels.signal);

    // Build tool services for communication + memory executors
    let (cross_nous, messenger, note_store, blackboard_store, spawn, planning) = {
        let cross_nous: Arc<dyn aletheia_organon::types::CrossNousService> =
            Arc::new(tool_adapters::CrossNousAdapter(Arc::clone(&cross_router)));
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn trait objects: required by Arc<dyn Trait> type annotations"
        )]
        let messenger: Option<Arc<dyn aletheia_organon::types::MessageService>> =
            signal_provider.as_ref().map(|p| {
                Arc::new(tool_adapters::SignalAdapter(
                    Arc::clone(p) as Arc<dyn ChannelProvider>
                )) as Arc<dyn aletheia_organon::types::MessageService>
            });
        let note_store: Option<Arc<dyn aletheia_organon::types::NoteStore>> = Some(Arc::new(
            aletheia_nous::adapters::SessionNoteAdapter(Arc::clone(&session_store)),
        ));
        let blackboard_store: Option<Arc<dyn aletheia_organon::types::BlackboardStore>> =
            Some(Arc::new(aletheia_nous::adapters::SessionBlackboardAdapter(
                Arc::clone(&session_store),
            )));
        let spawn: Option<Arc<dyn aletheia_organon::types::SpawnService>> =
            Some(Arc::new(aletheia_nous::spawn_svc::SpawnServiceImpl::new(
                Arc::clone(&provider_registry),
                Arc::clone(&tool_registry),
                Arc::clone(&oikos_arc),
            )));
        let planning_root = oikos_arc.data().join("planning");
        let planning: Option<Arc<dyn aletheia_organon::types::PlanningService>> = Some(Arc::new(
            planning_adapter::FilesystemPlanningService::new(planning_root),
        ));
        (
            cross_nous,
            messenger,
            note_store,
            blackboard_store,
            spawn,
            planning,
        )
    };

    // Knowledge store for vector search and extraction persistence
    #[cfg(feature = "recall")]
    let knowledge_store = {
        let kb_path = oikos_arc.knowledge_db();
        if let Some(parent) = kb_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = aletheia_mneme::knowledge_store::KnowledgeStore::open_fjall(
            &kb_path,
            aletheia_mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .context("failed to open knowledge store")?;
        info!(path = %kb_path.display(), dim = 384, "knowledge store opened (fjall)");
        Some(store)
    };
    // Wire vector search from KnowledgeStore
    #[cfg(feature = "recall")]
    #[expect(
        clippy::as_conversions,
        reason = "coercion to dyn trait object: required to satisfy Arc<dyn Trait> type annotation"
    )]
    let vector_search: Option<Arc<dyn aletheia_nous::recall::VectorSearch>> =
        knowledge_store.as_ref().map(|ks| {
            Arc::new(aletheia_nous::recall::KnowledgeVectorSearch::new(
                Arc::clone(ks),
            )) as Arc<dyn aletheia_nous::recall::VectorSearch>
        });
    #[cfg(not(feature = "recall"))]
    let vector_search: Option<Arc<dyn aletheia_nous::recall::VectorSearch>> = None;

    // Knowledge search adapter for tool layer
    #[cfg(feature = "recall")]
    #[expect(
        clippy::as_conversions,
        reason = "coercion to dyn trait object: required to satisfy Arc<dyn Trait> type annotation"
    )]
    let knowledge_search: Option<Arc<dyn aletheia_organon::types::KnowledgeSearchService>> =
        knowledge_store.as_ref().map(|ks| {
            Arc::new(crate::knowledge_adapter::KnowledgeSearchAdapter::new(
                Arc::clone(ks),
                Arc::clone(&embedding_provider),
            )) as Arc<dyn aletheia_organon::types::KnowledgeSearchService>
        });
    #[cfg(not(feature = "recall"))]
    let knowledge_search: Option<Arc<dyn aletheia_organon::types::KnowledgeSearchService>> = None;

    let tool_services = Arc::new(ToolServices {
        cross_nous: Some(cross_nous),
        messenger,
        note_store,
        blackboard_store,
        spawn,
        planning,
        knowledge: knowledge_search,
        http_client: reqwest::Client::new(),
        lazy_tool_catalog: tool_registry.lazy_tool_catalog(),
        server_tool_config: aletheia_organon::types::ServerToolConfig::default(),
    });

    // Spawn nous actors
    // Clone knowledge_store Arc before moving into NousManager: needed for daemon executor.
    #[cfg(feature = "recall")]
    let knowledge_store_for_daemon = knowledge_store.clone();

    let mut nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos_arc),
        Some(embedding_provider),
        vector_search,
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "recall")]
        knowledge_store,
        Arc::clone(&packs),
        Some(Arc::clone(&cross_router)),
        Some(tool_services),
    );

    if config.agents.list.is_empty() {
        warn!("no agents configured — starting with zero actors");
    } else {
        for agent_def in &config.agents.list {
            let resolved = resolve_nous(&config, &agent_def.id);

            // Merge domains from static config and pack overlays
            let mut domains = resolved.domains.clone();
            for pack in packs.iter() {
                for d in pack.domains_for_agent(&agent_def.id) {
                    if !domains.contains(&d) {
                        domains.push(d);
                    }
                }
            }

            let nous_config = NousConfig {
                id: resolved.id,
                name: resolved.name,
                generation: aletheia_nous::config::NousGenerationConfig {
                    model: resolved.model.primary,
                    context_window: resolved.limits.context_tokens,
                    max_output_tokens: resolved.limits.max_output_tokens,
                    bootstrap_max_tokens: resolved.limits.bootstrap_max_tokens,
                    thinking_enabled: resolved.capabilities.thinking_enabled,
                    thinking_budget: resolved.limits.thinking_budget,
                    chars_per_token: resolved.limits.chars_per_token,
                    prosoche_model: resolved.prosoche_model,
                },
                limits: aletheia_nous::config::NousLimits {
                    max_tool_iterations: resolved.capabilities.max_tool_iterations,
                    loop_detection_threshold: 3,
                    consecutive_error_threshold: 4,
                    loop_max_warnings: 2,
                    session_token_cap: 500_000,
                    max_tool_result_bytes: resolved.limits.max_tool_result_bytes,
                    max_consecutive_tool_only_iterations: 3,
                },
                domains,
                server_tools: Vec::new(),
                cache_enabled: resolved.capabilities.cache_enabled,
                recall: resolved.recall.into(),
                tool_allowlist: None,
            };
            nous_manager
                .spawn(
                    nous_config,
                    PipelineConfig {
                        extraction: Some(aletheia_mneme::extract::ExtractionConfig::default()),
                        ..PipelineConfig::default()
                    },
                )
                .await;
        }
        info!(count = nous_manager.count(), "nous actors spawned");
    }

    // Daemon: background maintenance tasks
    let maintenance_config = maintenance::build_config(&oikos_arc, &config.maintenance);
    let daemon_token = shutdown_token.child_token();
    let mut daemon_runner =
        TaskRunner::new("system", daemon_token).with_maintenance(maintenance_config);

    // Wire knowledge maintenance executor when recall feature is enabled
    #[cfg(feature = "recall")]
    if let Some(ks) = knowledge_store_for_daemon.as_ref() {
        let km_executor = Arc::new(
            crate::knowledge_maintenance::KnowledgeMaintenanceAdapter::new(Arc::clone(ks)),
        );
        daemon_runner = daemon_runner.with_knowledge_maintenance(km_executor);
    }

    daemon_runner.register_maintenance_tasks();
    let daemon_handle = tokio::spawn(
        async move {
            daemon_runner.run().await;
        }
        .instrument(tracing::info_span!("daemon_runner")),
    );
    info!("daemon started");

    // Log retention: runs immediately at startup then every 24 h.
    // Reuses TraceRotator (from aletheia-oikonomos) to prune daily log files
    // older than logging.retention_days rather than duplicating the cleanup
    // logic. Files are moved to logs/archive/ and immediately pruned
    // (max_archives = 0), producing a net deletion of old log files.
    spawn_log_retention(
        log_dir.clone(),
        config.logging.retention_days,
        shutdown_token.child_token(),
    );

    // Wrap in Arc: shared between dispatcher and AppState
    let nous_manager = Arc::new(nous_manager);

    // Signal ready: all actors spawned, safe to accept inbound messages
    nous_manager.ready();

    // Channel registry + inbound dispatch (gated on ready signal)
    let ready_rx = nous_manager.ready_rx();
    let (_channel_registry, _dispatch_handle) = start_inbound_dispatch(
        &config,
        &nous_manager,
        ready_rx,
        signal_provider.as_ref(),
        &shutdown_token,
    );

    // Daemon runners: per-agent background task scheduling
    let daemon_bridge = Arc::new(daemon_bridge::NousDaemonBridge::new(Arc::clone(
        &nous_manager,
    )));
    for agent_def in &config.agents.list {
        let agent_token = shutdown_token.child_token();
        let mut runner = aletheia_oikonomos::runner::TaskRunner::with_bridge(
            agent_def.id.clone(),
            agent_token,
            daemon_bridge.clone(),
        );
        runner.register(aletheia_oikonomos::schedule::TaskDef {
            id: format!("{}-prosoche", agent_def.id),
            name: "Prosoche attention check".to_owned(),
            nous_id: agent_def.id.clone(),
            schedule: aletheia_oikonomos::schedule::Schedule::Interval(
                std::time::Duration::from_secs(45 * 60),
            ),
            action: aletheia_oikonomos::schedule::TaskAction::Builtin(
                aletheia_oikonomos::schedule::BuiltinTask::Prosoche,
            ),
            enabled: true,
            active_window: Some((8, 23)),
            catch_up: false,
            ..aletheia_oikonomos::schedule::TaskDef::default()
        });
        let daemon_span = tracing::info_span!("daemon", nous.id = %agent_def.id);
        tokio::spawn(
            async move {
                runner.run().await;
            }
            .instrument(daemon_span),
        );
    }
    if !config.agents.list.is_empty() {
        info!(count = config.agents.list.len(), "daemon runners spawned");
    }

    // Pylon HTTP gateway: shares registries with NousManager
    let aletheia_config = aletheia_taxis::loader::load_config(&oikos_arc).unwrap_or_else(|e| {
        tracing::warn!("failed to load config, using defaults: {e}");
        aletheia_taxis::config::AletheiaConfig::default()
    });
    #[cfg(feature = "recall")]
    let knowledge_store = nous_manager.knowledge_store().cloned();

    let (config_tx, _config_rx) = tokio::sync::watch::channel(aletheia_config.clone());
    let state = Arc::new(AppState {
        session_store,
        nous_manager: Arc::clone(&nous_manager),
        provider_registry,
        tool_registry,
        oikos: oikos_arc,
        jwt_manager: Arc::new(jwt_manager),
        start_time: Instant::now(),
        auth_mode: config.gateway.auth.mode.clone(),
        config: Arc::new(tokio::sync::RwLock::new(aletheia_config)),
        config_tx,
        idempotency_cache: Arc::new(aletheia_pylon::idempotency::IdempotencyCache::new()),
        shutdown: shutdown_token.clone(),
        #[cfg(feature = "recall")]
        knowledge_store,
    });

    let security = aletheia_pylon::security::SecurityConfig::from_gateway(&config.gateway);

    #[cfg(feature = "mcp")]
    let app = {
        // Diaporeia MCP server: shares state with pylon, zero overhead.
        let diaporeia_state = Arc::new(aletheia_diaporeia::state::DiaporeiaState {
            session_store: Arc::clone(&state.session_store),
            nous_manager: Arc::clone(&state.nous_manager),
            tool_registry: Arc::clone(&state.tool_registry),
            oikos: Arc::clone(&state.oikos),
            start_time: state.start_time,
            config: Arc::clone(&state.config),
            shutdown: shutdown_token.clone(),
        });
        let mcp_router = aletheia_diaporeia::transport::streamable_http_router(diaporeia_state);
        info!("diaporeia MCP server mounted at /mcp");
        build_router(state.clone(), &security).merge(mcp_router)
    };

    #[cfg(not(feature = "mcp"))]
    let app = build_router(state.clone(), &security);

    let port = args.port.unwrap_or(config.gateway.port);
    // Resolve bind address: CLI flag > config gateway.bind > default 127.0.0.1.
    // "lan" is a semantic alias for "0.0.0.0" (listen on all interfaces).
    let bind_host = args.bind.as_deref().unwrap_or(&config.gateway.bind);
    let bind_addr_str = match bind_host {
        "lan" => "0.0.0.0",
        "localhost" => "127.0.0.1",
        other => other,
    };

    // Warn unconditionally when auth is disabled: every request is granted Operator role.
    if config.gateway.auth.mode == "none" {
        warn!("auth mode is 'none' -- all requests granted Operator role");
    }
    // Additionally warn if auth is disabled on a non-localhost bind address.
    if config.gateway.auth.mode == "none"
        && bind_addr_str != "127.0.0.1"
        && bind_addr_str != "localhost"
        && bind_addr_str != "::1"
    {
        warn!(
            bind = %bind_addr_str,
            "authentication is disabled (auth.mode = \"none\") on a non-localhost address — \
             the API is accessible without credentials"
        );
    }

    let bind_addr = format!("{bind_addr_str}:{port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                anyhow::anyhow!(
                    "Port {port} is already in use.\n  \
                     Use --port to choose another port, or stop the process using port {port}."
                )
            } else {
                anyhow::anyhow!("failed to bind to {bind_addr}: {e}")
            }
        })?;

    info!(addr = %bind_addr, "pylon listening");

    // Axum graceful shutdown: wait for OS signal, then cancel root token so
    // all subsystems observe shutdown simultaneously.
    let token_for_signal = shutdown_token.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("signal received — cancelling shutdown token");
            token_for_signal.cancel();
        })
        .await
        .context("server error")?;

    // ── Drain ordering ──────────────────────────────────────────────────────
    // 1. HTTP server has stopped accepting new requests (axum graceful_shutdown).
    // 2. Root token is cancelled: daemon tasks observe it and exit their loops.
    // 3. Wait for system daemon to finish in-flight maintenance work.
    // 4. Drain nous actors with a timeout, flushing fjall WAL and other state.
    //    Awaiting join handles ensures Arc<Database> drops, checkpointing the WAL.
    // 5. Drop AppState (session store, registries).
    // ────────────────────────────────────────────────────────────────────────

    info!("shutting down");

    let shutdown_timeout = std::time::Duration::from_secs(10);

    // Step 2--3: daemon runners have already observed token cancel via child tokens.
    // Await system daemon handle to confirm it has exited.
    match tokio::time::timeout(shutdown_timeout, daemon_handle).await {
        // NOTE: daemon exited cleanly, nothing to do
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!(error = %e, "system daemon panicked during shutdown"),
        Err(_) => warn!(
            timeout_secs = shutdown_timeout.as_secs(),
            "system daemon did not exit within shutdown timeout"
        ),
    }

    // Step 4: drain nous actors: cancel tokens fire, messages drain, WAL flushed.
    state.nous_manager.drain(shutdown_timeout).await;

    // Step 4b: flush the SQLite session-store WAL explicitly (#1723).
    // SQLite auto-checkpoints on connection close, but an explicit TRUNCATE
    // checkpoint here ensures all writes land in the main DB file before exit.
    match state.session_store.try_lock() {
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

    // Step 5: AppState and session store drop here as `state` goes out of scope.
    drop(state);

    info!("shutdown complete");

    Ok(())
}

mod setup;
mod tracing_setup;

use setup::{
    build_provider_registry, build_signal_provider, build_tool_registry, start_inbound_dispatch,
};
use tracing_setup::{init_tracing, resolve_log_dir, shutdown_signal, spawn_log_retention};

// -- Tool service adapters -------------------------------------------------

mod tool_adapters {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use aletheia_agora::types::{ChannelProvider, SendParams};
    use aletheia_nous::cross::{CrossNousMessage, CrossNousRouter};
    use aletheia_organon::types::{CrossNousService, MessageService};

    pub(crate) struct CrossNousAdapter(pub Arc<CrossNousRouter>);

    impl CrossNousService for CrossNousAdapter {
        fn send(
            &self,
            from: &str,
            to: &str,
            session_key: &str,
            content: &str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            let msg = CrossNousMessage::new(from, to, content).with_target_session(session_key);
            let router = Arc::clone(&self.0);
            Box::pin(async move {
                router
                    .send(msg)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
        }

        fn ask(
            &self,
            from: &str,
            to: &str,
            session_key: &str,
            content: &str,
            timeout_secs: u64,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
            let msg = CrossNousMessage::new(from, to, content)
                .with_target_session(session_key)
                .with_reply(Duration::from_secs(timeout_secs));
            let router = Arc::clone(&self.0);
            Box::pin(async move {
                router
                    .ask(msg)
                    .await
                    .map(|reply| reply.content)
                    .map_err(|e| e.to_string())
            })
        }
    }

    pub(crate) struct SignalAdapter(pub Arc<dyn ChannelProvider>);

    impl MessageService for SignalAdapter {
        fn send_message(
            &self,
            to: &str,
            text: &str,
            _from_nous: &str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            let params = SendParams {
                to: to.to_owned(),
                text: text.to_owned(),
                account_id: None,
                thread_id: None,
                attachments: None,
            };
            let provider = Arc::clone(&self.0);
            Box::pin(async move {
                let result = provider.send(&params).await;
                if result.sent {
                    Ok(())
                } else {
                    Err(result
                        .error
                        .unwrap_or_else(|| "unknown send error".to_owned()))
                }
            })
        }
    }
}
