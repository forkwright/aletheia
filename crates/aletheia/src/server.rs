//! Server startup, actor wiring, and HTTP gateway initialization.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};
use tracing_subscriber::{EnvFilter, fmt};

use aletheia_agora::listener::ChannelListener;
use aletheia_agora::registry::ChannelRegistry;
use aletheia_agora::router::MessageRouter;
use aletheia_agora::semeion::SignalProvider;
use aletheia_agora::semeion::client::SignalClient;
use aletheia_agora::types::ChannelProvider;
use aletheia_hermeneus::anthropic::AnthropicProvider;
use aletheia_hermeneus::provider::{ProviderConfig, ProviderRegistry};
use aletheia_koina::credential::{CredentialProvider, CredentialSource};
use aletheia_mneme::embedding::{EmbeddingConfig, EmbeddingProvider, create_provider};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::cross::CrossNousRouter;
use aletheia_nous::manager::NousManager;
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolServices;
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::credential::{
    CredentialChain, CredentialFile, EnvCredentialProvider, FileCredentialProvider,
    RefreshingCredentialProvider,
};
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::config::resolve_nous;
use aletheia_taxis::loader::load_config;
use aletheia_taxis::oikos::Oikos;

use crate::cli::Cli;
use crate::commands::build_maintenance_config;

#[expect(
    clippy::too_many_lines,
    reason = "binary entrypoint — sequential init steps"
)]
pub(crate) async fn serve(cli: Cli) -> Result<()> {
    init_tracing(&cli.log_level, cli.json_logs);

    info!("aletheia starting");

    // Root cancellation token — cancelled on SIGTERM/SIGINT.
    // Child tokens are propagated to every actor and daemon task.
    let shutdown_token = CancellationToken::new();

    // Oikos — instance directory resolution
    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    info!(root = %oikos.root().display(), "instance discovered");

    // Startup validation — fail fast before any actors or stores initialise
    oikos.validate().context("instance layout invalid")?;

    // Config cascade: defaults → YAML → env
    let config = load_config(&oikos).context("failed to load config")?;
    info!(
        port = config.gateway.port,
        agents = config.agents.list.len(),
        "config loaded"
    );

    // Validate per-agent workspace paths declared in config
    for agent in &config.agents.list {
        if let Err(e) = oikos.validate_workspace_path(&agent.workspace) {
            tracing::warn!(
                agent = %agent.id,
                workspace = %agent.workspace,
                error = %e,
                "agent workspace path invalid — agent may fail to start"
            );
        }
    }

    // Domain packs — load external knowledge packs declared in config
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

    // JWT manager
    let jwt_manager = JwtManager::new(JwtConfig::default());

    // Build shared registries — single instances used by both NousManager and AppState
    let provider_registry = Arc::new(build_provider_registry(&config, &oikos));
    let mut tool_registry = build_tool_registry()?;

    // Register domain pack tools alongside builtins
    let tool_errors = aletheia_thesauros::tools::register_pack_tools(&packs, &mut tool_registry);
    for err in &tool_errors {
        warn!(error = %err, "failed to register pack tool");
    }

    let tool_registry = Arc::new(tool_registry);
    let oikos_arc = Arc::new(oikos);

    // Embedding provider — drives recall query embedding
    let embedding_config = EmbeddingConfig {
        provider: config.embedding.provider.clone(),
        model: config.embedding.model.clone(),
        dimension: Some(config.embedding.dimension),
        api_key: None,
    };
    let embedding_provider: Arc<dyn EmbeddingProvider> = Arc::from(
        create_provider(&embedding_config).context("failed to create embedding provider")?,
    );
    info!(
        provider = %config.embedding.provider,
        dim = config.embedding.dimension,
        "embedding provider created"
    );

    // Cross-nous router for inter-agent messaging
    let cross_router = Arc::new(CrossNousRouter::default());

    // Build signal provider early so it can be shared with tool services
    let signal_provider = build_signal_provider(&config.channels.signal);

    // Build tool services for communication + memory executors
    let (cross_nous, messenger, note_store, blackboard_store, spawn, planning) = {
        let cross_nous: Arc<dyn aletheia_organon::types::CrossNousService> =
            Arc::new(tool_adapters::CrossNousAdapter(Arc::clone(&cross_router)));
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
            crate::planning_adapter::FilesystemPlanningService::new(planning_root),
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
    #[cfg(not(feature = "recall"))]
    let knowledge_store: Option<
        std::sync::Arc<aletheia_mneme::knowledge_store::KnowledgeStore>,
    > = None;

    // Wire vector search from KnowledgeStore
    #[cfg(feature = "recall")]
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
    // Clone knowledge_store Arc before moving into NousManager — needed for daemon executor.
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
                model: resolved.model,
                context_window: resolved.context_tokens,
                max_output_tokens: resolved.max_output_tokens,
                bootstrap_max_tokens: resolved.bootstrap_max_tokens,
                thinking_enabled: resolved.thinking_enabled,
                thinking_budget: resolved.thinking_budget,
                max_tool_iterations: resolved.max_tool_iterations,
                loop_detection_threshold: 3,
                domains,
                server_tools: Vec::new(),
                cache_enabled: resolved.cache_enabled,
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

    // Daemon — background maintenance tasks
    let maintenance_config = build_maintenance_config(&oikos_arc, &config.maintenance);
    let daemon_token = shutdown_token.child_token();
    let mut daemon_runner =
        aletheia_oikonomos::runner::TaskRunner::new("system", daemon_token)
            .with_maintenance(maintenance_config);

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

    // Wrap in Arc — shared between dispatcher and AppState
    let nous_manager = Arc::new(nous_manager);

    // Signal ready — all actors spawned, safe to accept inbound messages
    nous_manager.ready();

    // Channel registry + inbound dispatch (gated on ready signal)
    let ready_rx = nous_manager.ready_rx();
    let (_channel_registry, _dispatch_handle) =
        start_inbound_dispatch(&config, &nous_manager, ready_rx, signal_provider.as_ref());

    // Daemon runners — per-agent background task scheduling
    let daemon_bridge = Arc::new(crate::daemon_bridge::NousDaemonBridge::new(Arc::clone(
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

    // Pylon HTTP gateway — shares registries with NousManager
    let aletheia_config = aletheia_taxis::loader::load_config(&oikos_arc).unwrap_or_else(|e| {
        tracing::warn!("failed to load config, using defaults: {e}");
        aletheia_taxis::config::AletheiaConfig::default()
    });
    #[cfg(feature = "recall")]
    let knowledge_store = nous_manager.knowledge_store().cloned();

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
        idempotency_cache: Arc::new(aletheia_pylon::idempotency::IdempotencyCache::new()),
        shutdown: shutdown_token.clone(),
        #[cfg(feature = "recall")]
        knowledge_store,
    });

    let security = aletheia_pylon::security::SecurityConfig::from_gateway(&config.gateway);
    let app = build_router(state.clone(), &security);

    let port = cli.port.unwrap_or(config.gateway.port);
    // Resolve bind address: CLI flag > config gateway.bind > default 127.0.0.1.
    // "lan" is a semantic alias for "0.0.0.0" (listen on all interfaces).
    // "localhost" is normalised to "127.0.0.1" to avoid IPv6 resolution on dual-stack hosts.
    let bind_host = cli.bind.as_deref().unwrap_or(&config.gateway.bind);
    let bind_addr_str = match bind_host {
        "lan" => "0.0.0.0",
        "localhost" => "127.0.0.1",
        other => other,
    };
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
    // 2. Root token is cancelled — daemon tasks observe it and exit their loops.
    // 3. Wait for system daemon to finish in-flight maintenance work.
    // 4. Drain nous actors with a timeout, flushing fjall WAL and other state.
    //    Awaiting join handles ensures Arc<Database> drops, checkpointing the WAL.
    // 5. Drop AppState (session store, registries).
    // ────────────────────────────────────────────────────────────────────────

    info!("shutting down");

    let shutdown_timeout = std::time::Duration::from_secs(10);

    // Step 2–3: daemon runners have already observed token cancel via child tokens.
    // Await system daemon handle to confirm it has exited.
    match tokio::time::timeout(shutdown_timeout, daemon_handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!(error = %e, "system daemon panicked during shutdown"),
        Err(_) => warn!(
            timeout_secs = shutdown_timeout.as_secs(),
            "system daemon did not exit within shutdown timeout"
        ),
    }

    // Step 4: drain nous actors — cancel tokens fire, messages drain, WAL flushed.
    state.nous_manager.drain(shutdown_timeout).await;

    // Step 5: AppState and session store drop here as `state` goes out of scope.
    drop(state);

    info!("shutdown complete");

    Ok(())
}

/// Build a provider registry using the credential resolution chain.
///
/// Resolution order: credential file (with OAuth refresh if available) → env var.
fn build_provider_registry(
    config: &aletheia_taxis::config::AletheiaConfig,
    oikos: &Oikos,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    let pricing: std::collections::HashMap<String, aletheia_hermeneus::provider::ModelPricing> =
        config
            .pricing
            .iter()
            .map(|(model, p)| {
                (
                    model.clone(),
                    aletheia_hermeneus::provider::ModelPricing {
                        input_cost_per_mtok: p.input_cost_per_mtok,
                        output_cost_per_mtok: p.output_cost_per_mtok,
                    },
                )
            })
            .collect();

    // Build credential chain: file (with refresh) → env
    let cred_file = oikos.credentials().join("anthropic.json");
    let mut chain: Vec<Box<dyn CredentialProvider>> = Vec::new();

    if cred_file.exists() {
        // Check if file has a refresh token for OAuth mode
        if let Some(cred) = CredentialFile::load(&cred_file) {
            if cred.has_refresh_token() {
                if let Some(refreshing) = RefreshingCredentialProvider::new(cred_file.clone()) {
                    info!(path = %cred_file.display(), "credential file found (OAuth auto-refresh)");
                    chain.push(Box::new(refreshing));
                } else {
                    info!(path = %cred_file.display(), "credential file found (static)");
                    chain.push(Box::new(FileCredentialProvider::new(cred_file.clone())));
                }
            } else {
                info!(path = %cred_file.display(), "credential file found (static API key)");
                chain.push(Box::new(FileCredentialProvider::new(cred_file.clone())));
            }
        }
    }

    // ANTHROPIC_AUTH_TOKEN is the Claude Code OAuth convention — always treat as OAuth
    chain.push(Box::new(EnvCredentialProvider::with_source(
        "ANTHROPIC_AUTH_TOKEN",
        CredentialSource::OAuth,
    )));
    // ANTHROPIC_API_KEY: auto-detects OAuth tokens by sk-ant-oat prefix
    chain.push(Box::new(EnvCredentialProvider::new("ANTHROPIC_API_KEY")));

    let credential_chain: Arc<dyn CredentialProvider> = Arc::new(CredentialChain::new(chain));

    // Resolve once at startup for logging
    if let Some(cred) = credential_chain.get_credential() {
        info!(source = %cred.source, "credential resolved");
    } else {
        warn!(
            "no credential found — server will start in degraded mode (no LLM)\n  \
             Fix: set ANTHROPIC_API_KEY env var, or run `aletheia credential status`"
        );
        return registry;
    }

    let provider_config = ProviderConfig {
        pricing,
        ..ProviderConfig::default()
    };
    match AnthropicProvider::with_credential_provider(credential_chain, &provider_config) {
        Ok(provider) => {
            registry.register(Box::new(provider));
            info!("anthropic provider registered");
        }
        Err(e) => warn!(error = %e, "failed to init anthropic provider"),
    }

    registry
}

/// Build a tool registry with all builtins.
fn build_tool_registry() -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    builtins::register_all(&mut registry).context("failed to register builtin tools")?;
    info!(count = registry.definitions().len(), "tools registered");
    Ok(registry)
}

/// Build channel registry, start inbound listener, and spawn dispatch loop.
fn start_inbound_dispatch(
    config: &aletheia_taxis::config::AletheiaConfig,
    nous_manager: &Arc<NousManager>,
    ready_rx: tokio::sync::watch::Receiver<bool>,
    signal_provider: Option<&Arc<SignalProvider>>,
) -> (Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>) {
    let mut channel_registry = ChannelRegistry::new();

    if let Some(provider) = signal_provider {
        channel_registry
            .register(Arc::clone(provider) as Arc<dyn ChannelProvider>)
            .expect("register signal provider");
    }
    let channel_registry = Arc::new(channel_registry);

    let handle = if let Some(provider) = signal_provider {
        let listener = ChannelListener::start(provider, None);
        info!("signal channel listener started");
        let rx = listener.into_receiver();

        let default_nous_id = config
            .agents
            .list
            .iter()
            .find(|a| a.default)
            .or_else(|| config.agents.list.first())
            .map(|a| a.id.clone());
        let router = Arc::new(MessageRouter::new(config.bindings.clone(), default_nous_id));

        Some(crate::dispatch::spawn_dispatcher(
            rx,
            router,
            Arc::clone(nous_manager),
            Arc::clone(&channel_registry),
            ready_rx,
        ))
    } else {
        None
    };

    (channel_registry, handle)
}

fn build_signal_provider(
    signal_config: &aletheia_taxis::config::SignalConfig,
) -> Option<Arc<SignalProvider>> {
    if !signal_config.enabled {
        info!("signal channel disabled");
        return None;
    }

    if signal_config.accounts.is_empty() {
        tracing::debug!("signal enabled but no accounts configured");
        return None;
    }

    let mut provider = SignalProvider::new();
    for (account_id, account_cfg) in &signal_config.accounts {
        if !account_cfg.enabled {
            continue;
        }
        let base_url = format!("http://{}:{}", account_cfg.http_host, account_cfg.http_port);
        match SignalClient::new(&base_url) {
            Ok(client) => {
                provider.add_account(account_id.clone(), client);
                info!(account = %account_id, "signal account added");
            }
            Err(e) => {
                warn!(account = %account_id, error = %e, "failed to create signal client");
            }
        }
    }

    Some(Arc::new(provider))
}

pub(crate) fn init_tracing(log_level: &str, json: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("aletheia={log_level},{log_level}")));

    if json {
        fmt()
            .with_env_filter(filter)
            .json()
            .with_target(true)
            .init();
    } else {
        fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }
}

pub(crate) async fn shutdown_signal() {
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

// -- Tool service adapters -------------------------------------------------

pub(crate) mod tool_adapters {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use aletheia_agora::types::{ChannelProvider, SendParams};
    use aletheia_nous::cross::{CrossNousMessage, CrossNousRouter};
    use aletheia_organon::types::{CrossNousService, MessageService};

    pub struct CrossNousAdapter(pub Arc<CrossNousRouter>);

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

    pub struct SignalAdapter(pub Arc<dyn ChannelProvider>);

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
