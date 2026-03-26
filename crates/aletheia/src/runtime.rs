//! [`RuntimeBuilder`]: single-site construction of all server subsystems.

use std::sync::Arc;
use std::time::Instant;

use snafu::prelude::*;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

use aletheia_agora::listener::ChannelListener;
use aletheia_agora::registry::ChannelRegistry;
use aletheia_agora::router::MessageRouter;
use aletheia_agora::semeion::SignalProvider;
use aletheia_agora::semeion::client::SignalClient;
use aletheia_agora::types::ChannelProvider;
use aletheia_hermeneus::anthropic::AnthropicProvider;
use aletheia_hermeneus::provider::{ProviderConfig, ProviderRegistry};
use aletheia_koina::credential::{CredentialProvider, CredentialSource};
use aletheia_koina::secret::SecretString;
use aletheia_mneme::embedding::{
    DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::cross::CrossNousRouter;
use aletheia_nous::manager::NousManager;
use aletheia_oikonomos::runner::TaskRunner;
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolServices;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::credential::{
    CredentialChain, CredentialFile, EnvCredentialProvider, FileCredentialProvider,
    RefreshingCredentialProvider, claude_code_default_path, claude_code_provider,
};
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::config::{AletheiaConfig, EmbeddingSettings, resolve_nous};
use aletheia_taxis::oikos::Oikos;
use aletheia_taxis::validate::{validate_section, validate_startup};

use crate::commands::maintenance;
use crate::daemon_bridge;
use crate::dispatch;
use crate::error::Result;
use crate::planning_adapter;

#[expect(
    clippy::struct_excessive_bools,
    reason = "builder flags are independent capability toggles; a bitfield would obscure semantics"
)]
pub(crate) struct RuntimeBuilder {
    oikos: Arc<Oikos>,
    config: AletheiaConfig,
    config_strict: bool,
    credentials: bool,
    embedding: bool,
    tool_services: bool,
    domain_packs: bool,
    daemons: bool,
}

pub(crate) struct Runtime {
    pub state: Arc<AppState>,
    #[expect(
        dead_code,
        reason = "accessible for callers that need direct NousManager access"
    )]
    pub nous_manager: Arc<NousManager>,
    pub daemon_handles: Vec<JoinHandle<()>>,
    pub shutdown_token: CancellationToken,
}

impl RuntimeBuilder {
    pub(crate) fn production(oikos: Arc<Oikos>, config: AletheiaConfig) -> Self {
        Self {
            oikos,
            config,
            config_strict: true,
            credentials: true,
            embedding: true,
            tool_services: true,
            domain_packs: true,
            daemons: true,
        }
    }

    #[expect(
        dead_code,
        reason = "preset available for test harness and future CLI modes"
    )]
    pub(crate) fn minimal(oikos: Arc<Oikos>, config: AletheiaConfig) -> Self {
        Self {
            oikos,
            config,
            config_strict: true,
            credentials: false,
            embedding: false,
            tool_services: false,
            domain_packs: false,
            daemons: false,
        }
    }

    pub(crate) fn validation_only(oikos: Arc<Oikos>, config: AletheiaConfig) -> Self {
        Self {
            oikos,
            config,
            config_strict: true,
            credentials: false,
            embedding: false,
            tool_services: false,
            domain_packs: false,
            daemons: false,
        }
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "builder method available for selective configuration"
    )]
    pub(crate) fn with_credentials(mut self, enabled: bool) -> Self {
        self.credentials = enabled;
        self
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "builder method available for selective configuration"
    )]
    pub(crate) fn with_embedding(mut self, enabled: bool) -> Self {
        self.embedding = enabled;
        self
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "builder method available for selective configuration"
    )]
    pub(crate) fn with_tool_services(mut self, enabled: bool) -> Self {
        self.tool_services = enabled;
        self
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "builder method available for selective configuration"
    )]
    pub(crate) fn with_domain_packs(mut self, enabled: bool) -> Self {
        self.domain_packs = enabled;
        self
    }

    #[must_use]
    #[expect(
        dead_code,
        reason = "builder method available for selective configuration"
    )]
    pub(crate) fn with_daemons(mut self, enabled: bool) -> Self {
        self.daemons = enabled;
        self
    }

    /// Validate config without building the runtime. Used by `check-config`.
    pub(crate) fn validate(&self) -> Result<()> {
        let mut all_ok = true;

        println!("Instance root: {}", self.oikos.root().display());

        if !self.oikos.root().exists() {
            println!(
                "  [FAIL] instance layout: instance root not found: {}\n         \
                 help: set ALETHEIA_ROOT or run `aletheia init`",
                self.oikos.root().display()
            );
            snafu::whatever!("Cannot validate: instance root does not exist");
        }

        match self.oikos.validate() {
            Ok(()) => println!("  [pass] instance layout"),
            Err(e) => {
                println!("  [FAIL] instance layout: {e}");
                all_ok = false;
            }
        }

        println!("  [pass] config loaded");

        let config_value = match serde_json::to_value(&self.config) {
            Ok(v) => v,
            Err(e) => {
                println!("  [FAIL] config serialization: {e}");
                snafu::whatever!("config validation aborted: could not serialize config");
            }
        };

        for section in &[
            "agents",
            "gateway",
            "maintenance",
            "data",
            "embedding",
            "channels",
            "bindings",
        ] {
            if let Some(section_value) = config_value.get(section) {
                match validate_section(section, section_value) {
                    Ok(()) => println!("  [pass] {section}"),
                    Err(e) => {
                        println!("  [FAIL] {section}: {e}");
                        all_ok = false;
                    }
                }
            } else {
                println!("  [pass] {section} (using defaults)");
            }
        }

        for agent in &self.config.agents.list {
            match self.oikos.validate_workspace_path(&agent.workspace) {
                Ok(()) => println!("  [pass] agent '{}' workspace", agent.id),
                Err(e) => {
                    println!("  [FAIL] agent '{}' workspace: {e}", agent.id);
                    all_ok = false;
                }
            }
        }

        let jwt_key = self
            .config
            .gateway
            .auth
            .signing_key
            .as_ref()
            .map(|s| s.expose_secret().to_owned())
            .or_else(|| std::env::var("ALETHEIA_JWT_SECRET").ok());
        let auth_mode = self.config.gateway.auth.mode.as_str();
        let jwt_check_label = "gateway.auth JWT key";
        if matches!(auth_mode, "token" | "jwt") {
            match jwt_key.as_deref() {
                Some("CHANGE-ME-IN-PRODUCTION") | None => {
                    println!(
                        "  [FAIL] {jwt_check_label}: key is still the default placeholder\n         \
                         Set gateway.auth.signingKey in aletheia.toml or ALETHEIA_JWT_SECRET env var.\n         \
                         Generate one with: openssl rand -hex 32"
                    );
                    all_ok = false;
                }
                Some(_) => println!("  [pass] {jwt_check_label}"),
            }
        } else {
            println!("  [pass] {jwt_check_label} (auth mode '{auth_mode}' -- JWT not required)");
        }

        println!();
        if all_ok {
            println!("Configuration OK");
            Ok(())
        } else {
            snafu::whatever!("Configuration has errors -- see above");
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "sequential init steps: splitting would fragment the startup flow"
    )]
    pub(crate) async fn build(self) -> Result<Runtime> {
        let shutdown_token = CancellationToken::new();

        info!(root = %self.oikos.root().display(), "instance discovered");

        // Startup validation: fail fast before any actors or stores initialise
        self.oikos
            .validate()
            .whatever_context("instance layout invalid")?;

        info!(
            port = self.config.gateway.port,
            agents = self.config.agents.list.len(),
            "config loaded"
        );

        // Validate all config sections
        if self.config_strict {
            let config_value = serde_json::to_value(&self.config)
                .whatever_context("failed to serialize config for validation")?;
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
                        .with_whatever_context(|_| format!("invalid config section '{section}'"))?;
                }
            }
            info!("config validated");

            validate_startup(&self.config, &self.oikos)
                .whatever_context("startup validation failed")?;
            info!("startup validation passed");
        }

        // JWT key resolution
        let jwt_key: Option<SecretString> =
            self.config.gateway.auth.signing_key.clone().or_else(|| {
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
            .validate_for_auth_mode(self.config.gateway.auth.mode.as_str())
            .whatever_context("JWT key security check failed")?;

        // Domain packs
        let loaded_packs = if self.domain_packs {
            aletheia_thesauros::loader::load_packs(&self.config.packs)
        } else {
            Vec::new()
        };
        let packs = Arc::new(loaded_packs);

        // Session store
        let db_path = self.oikos.sessions_db();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_whatever_context(|_| {
                format!("failed to create data dir {}", parent.display())
            })?;
        }
        let session_store = Arc::new(Mutex::new(
            SessionStore::open(&db_path).with_whatever_context(|_| {
                format!("failed to open session store at {}", db_path.display())
            })?,
        ));
        info!(path = %db_path.display(), "session store opened");

        let jwt_manager = JwtManager::new(jwt_config);

        // Provider registry
        let provider_registry = if self.credentials {
            Arc::new(build_provider_registry(&self.config, &self.oikos))
        } else {
            Arc::new(ProviderRegistry::new())
        };

        // Tool registry
        let mut tool_registry = if self.credentials {
            build_tool_registry(&self.config.sandbox)?
        } else {
            ToolRegistry::new()
        };

        // Register domain pack tools
        if self.domain_packs {
            let tool_errors =
                aletheia_thesauros::tools::register_pack_tools(&packs, &mut tool_registry);
            for err in &tool_errors {
                warn!(error = %err, "failed to register pack tool");
            }
        }

        let tool_registry = Arc::new(tool_registry);

        // Embedding provider
        let embedding_provider = if self.embedding {
            create_embedding_provider(&self.config.embedding)
        } else {
            Arc::new(DegradedEmbeddingProvider::new(
                self.config.embedding.dimension,
            ))
        };

        // Cross-nous router
        let cross_router = Arc::new(CrossNousRouter::default());

        // Signal provider
        let signal_provider = if self.tool_services {
            build_signal_provider(&self.config.channels.signal)
        } else {
            None
        };

        // Tool services
        let (cross_nous, messenger, note_store, blackboard_store, spawn, planning) = if self
            .tool_services
        {
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
                    Arc::clone(&self.oikos),
                )));
            let planning_root = self.oikos.data().join("planning");
            let planning: Option<Arc<dyn aletheia_organon::types::PlanningService>> =
                Some(Arc::new(planning_adapter::FilesystemPlanningService::new(
                    planning_root,
                )));
            (
                Some(cross_nous),
                messenger,
                note_store,
                blackboard_store,
                spawn,
                planning,
            )
        } else {
            (None, None, None, None, None, None)
        };

        // Knowledge store
        #[cfg(feature = "recall")]
        let knowledge_store = if self.embedding {
            open_knowledge_store(&self.oikos)?
        } else {
            None
        };

        // Vector search
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
        let knowledge_search: Option<
            Arc<dyn aletheia_organon::types::KnowledgeSearchService>,
        > = knowledge_store.as_ref().map(|ks| {
            Arc::new(crate::knowledge_adapter::KnowledgeSearchAdapter::new(
                Arc::clone(ks),
                Arc::clone(&embedding_provider),
            )) as Arc<dyn aletheia_organon::types::KnowledgeSearchService>
        });
        #[cfg(not(feature = "recall"))]
        let knowledge_search: Option<
            Arc<dyn aletheia_organon::types::KnowledgeSearchService>,
        > = None;

        let tool_services = Arc::new(ToolServices {
            cross_nous,
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

        // Clone knowledge_store Arc before moving into NousManager
        #[cfg(feature = "recall")]
        let knowledge_store_for_daemon = knowledge_store.clone();

        let mut nous_manager = NousManager::new(
            Arc::clone(&provider_registry),
            Arc::clone(&tool_registry),
            Arc::clone(&self.oikos),
            Some(embedding_provider),
            vector_search,
            Some(Arc::clone(&session_store)),
            #[cfg(feature = "recall")]
            knowledge_store,
            Arc::clone(&packs),
            Some(Arc::clone(&cross_router)),
            Some(tool_services),
        );

        // Spawn nous actors
        {
            for agent_def in &self.config.agents.list {
                let resolved = resolve_nous(&self.config, &agent_def.id);

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

        // Daemon handles collector
        let mut daemon_handles: Vec<JoinHandle<()>> = Vec::new();

        if self.daemons {
            // System maintenance daemon
            let maintenance_config =
                maintenance::build_config(&self.oikos, &self.config.maintenance);
            let daemon_token = shutdown_token.child_token();
            let mut daemon_runner =
                TaskRunner::new("system", daemon_token).with_maintenance(maintenance_config);

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
            daemon_handles.push(daemon_handle);
        }

        let nous_manager = Arc::new(nous_manager);

        // Signal ready
        nous_manager.ready();

        // Channel registry + inbound dispatch
        let ready_rx = nous_manager.ready_rx();
        let (_channel_registry, _dispatch_handle) = start_inbound_dispatch(
            &self.config,
            &nous_manager,
            ready_rx,
            signal_provider.as_ref(),
            &shutdown_token,
        );

        // Per-agent daemon runners (need Arc<NousManager>)
        if self.daemons {
            let daemon_bridge = Arc::new(daemon_bridge::NousDaemonBridge::new(Arc::clone(
                &nous_manager,
            )));
            for agent_def in &self.config.agents.list {
                let agent_token = shutdown_token.child_token();
                let mut runner = TaskRunner::with_bridge(
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
            if !self.config.agents.list.is_empty() {
                info!(
                    count = self.config.agents.list.len(),
                    "daemon runners spawned"
                );
            }
        }

        // AppState construction
        let aletheia_config = self.config.clone();
        #[cfg(feature = "recall")]
        let knowledge_store = nous_manager.knowledge_store().cloned();

        let (config_tx, _config_rx) = tokio::sync::watch::channel(aletheia_config.clone());
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::clone(&nous_manager),
            provider_registry,
            tool_registry,
            oikos: self.oikos,
            jwt_manager: Arc::new(jwt_manager),
            start_time: Instant::now(),
            auth_mode: self.config.gateway.auth.mode.clone(),
            none_role: self.config.gateway.auth.none_role.clone(),
            config: Arc::new(tokio::sync::RwLock::new(aletheia_config)),
            config_tx,
            idempotency_cache: Arc::new(aletheia_pylon::idempotency::IdempotencyCache::new()),
            shutdown: shutdown_token.clone(),
            #[cfg(feature = "recall")]
            knowledge_store,
        });

        Ok(Runtime {
            state,
            nous_manager,
            daemon_handles,
            shutdown_token,
        })
    }
}

// -- Setup helpers (moved from commands/server/setup.rs) -----------------------

fn build_provider_registry(config: &AletheiaConfig, oikos: &Oikos) -> ProviderRegistry {
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

    let cred_source = config.credential.source.as_str();
    let cred_file = oikos.credentials().join("anthropic.json");
    let mut chain: Vec<Box<dyn CredentialProvider>> = Vec::new();

    let claude_code_path = config
        .credential
        .claude_code_credentials
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(claude_code_default_path);

    if cred_source == "claude-code"
        && let Some(ref cc_path) = claude_code_path
        && let Some(provider) = claude_code_provider(cc_path)
    {
        chain.push(provider);
    }

    if cred_file.exists()
        && let Some(cred) = CredentialFile::load(&cred_file)
    {
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

    #[cfg(feature = "keyring")]
    {
        use aletheia_symbolon::credential::KeyringCredentialProvider;
        chain.push(Box::new(KeyringCredentialProvider::new()));
    }

    chain.push(Box::new(EnvCredentialProvider::with_source(
        "ANTHROPIC_AUTH_TOKEN",
        CredentialSource::OAuth,
    )));
    chain.push(Box::new(EnvCredentialProvider::new("ANTHROPIC_API_KEY")));

    if cred_source == "auto"
        && let Some(ref cc_path) = claude_code_path
        && let Some(provider) = claude_code_provider(cc_path)
    {
        chain.push(provider);
    }

    let credential_chain: Arc<dyn CredentialProvider> = Arc::new(CredentialChain::new(chain));

    if let Some(cred) = credential_chain.get_credential() {
        info!(source = %cred.source, "credential resolved");
    } else {
        warn!(
            "no credential found -- server will start in degraded mode (no LLM)\n  \
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

fn build_tool_registry(
    sandbox_settings: &aletheia_taxis::config::SandboxSettings,
) -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    let sandbox = aletheia_organon::sandbox::SandboxConfig {
        enabled: sandbox_settings.enabled,
        enforcement: match sandbox_settings.enforcement {
            aletheia_taxis::config::SandboxEnforcementMode::Enforcing => {
                aletheia_organon::sandbox::SandboxEnforcement::Enforcing
            }
            _ => aletheia_organon::sandbox::SandboxEnforcement::Permissive,
        },
        allowed_root: sandbox_settings.allowed_root.clone(),
        extra_read_paths: sandbox_settings.extra_read_paths.clone(),
        extra_write_paths: sandbox_settings.extra_write_paths.clone(),
        extra_exec_paths: sandbox_settings.extra_exec_paths.clone(),
        egress: match sandbox_settings.egress {
            aletheia_taxis::config::EgressPolicy::Deny => {
                aletheia_organon::sandbox::EgressPolicy::Deny
            }
            aletheia_taxis::config::EgressPolicy::Allowlist => {
                aletheia_organon::sandbox::EgressPolicy::Allowlist
            }
            _ => aletheia_organon::sandbox::EgressPolicy::Allow,
        },
        egress_allowlist: sandbox_settings.egress_allowlist.clone(),
        nproc_limit: sandbox_settings.nproc_limit,
    };
    builtins::register_all_with_sandbox(&mut registry, sandbox)
        .whatever_context("failed to register builtin tools")?;
    info!(count = registry.definitions().len(), "tools registered");
    Ok(registry)
}

fn create_embedding_provider(settings: &EmbeddingSettings) -> Arc<dyn EmbeddingProvider> {
    let embedding_config = EmbeddingConfig {
        provider: settings.provider.clone(),
        model: settings.model.clone(),
        dimension: Some(settings.dimension),
        api_key: None,
    };
    match create_provider(&embedding_config) {
        Ok(p) => {
            info!(
                provider = %settings.provider,
                dim = settings.dimension,
                "embedding provider created"
            );
            Arc::from(p)
        }
        Err(e) => {
            warn!(
                error = %e,
                provider = %settings.provider,
                "embedding provider failed to load: starting in degraded mode \
                 (recall and vector search unavailable)"
            );
            Arc::new(DegradedEmbeddingProvider::new(settings.dimension))
        }
    }
}

#[cfg(feature = "recall")]
fn open_knowledge_store(
    oikos: &Oikos,
) -> Result<Option<Arc<aletheia_mneme::knowledge_store::KnowledgeStore>>> {
    let kb_path = oikos.knowledge_db();
    if let Some(parent) = kb_path.parent() {
        std::fs::create_dir_all(parent)
            .whatever_context("failed to create knowledge store directory")?;
    }
    let store = aletheia_mneme::knowledge_store::KnowledgeStore::open_fjall(
        &kb_path,
        aletheia_mneme::knowledge_store::KnowledgeConfig::default(),
    )
    .whatever_context("failed to open knowledge store")?;
    info!(path = %kb_path.display(), dim = 384, "knowledge store opened (fjall)");
    Ok(Some(store))
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

#[expect(
    clippy::expect_used,
    reason = "channel registration is infallible for unique providers"
)]
fn start_inbound_dispatch(
    config: &AletheiaConfig,
    nous_manager: &Arc<NousManager>,
    ready_rx: tokio::sync::watch::Receiver<bool>,
    signal_provider: Option<&Arc<SignalProvider>>,
    shutdown_token: &CancellationToken,
) -> (Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>) {
    let mut channel_registry = ChannelRegistry::new();

    if let Some(provider) = signal_provider {
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn ChannelProvider trait object: required by registry API"
        )]
        channel_registry
            .register(Arc::clone(provider) as Arc<dyn ChannelProvider>)
            .expect("register signal provider");
    }
    let channel_registry = Arc::new(channel_registry);

    let handle = if let Some(provider) = signal_provider {
        let listener = ChannelListener::start(provider, None, shutdown_token.child_token());
        info!("signal channel listener started");
        let (rx, _poll_handles) = listener.into_receiver();

        let default_nous_id = config
            .agents
            .list
            .iter()
            .find(|a| a.default)
            .or_else(|| config.agents.list.first())
            .map(|a| a.id.clone());
        let router = Arc::new(MessageRouter::new(config.bindings.clone(), default_nous_id));

        Some(dispatch::spawn_dispatcher(
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

// -- Tool service adapters (moved from commands/server/mod.rs) -----------------

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
