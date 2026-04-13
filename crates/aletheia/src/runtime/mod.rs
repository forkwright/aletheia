//! [`RuntimeBuilder`]: single-site construction of all server subsystems.

use std::sync::Arc;
use std::time::Instant;

use snafu::prelude::*;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

use agora::types::ChannelProvider;
use hermeneus::provider::ProviderRegistry;
use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};
use mneme::embedding::DegradedEmbeddingProvider;
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::cross::CrossNousRouter;
use nous::manager::NousManager;
use oikonomos::runner::TaskRunner;
use organon::registry::ToolRegistry;
use organon::types::ToolServices;
use pylon::state::AppState;
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::{AletheiaConfig, resolve_nous};
use taxis::oikos::Oikos;
use taxis::validate::{validate_section, validate_startup};

use crate::commands::maintenance;
use crate::daemon_bridge;
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
    pub daemon_handles: JoinSet<()>,
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
                 help: SET ALETHEIA_ROOT or run `aletheia init`",
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

        if !validate_jwt(&self.config) {
            all_ok = false;
        }

        if !validate_external_tools(&self.oikos) {
            all_ok = false;
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

        // WHY: Fail fast before any actors or stores initialise.
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

        // Initialize dianoia metrics (planning/project orchestration)
        dianoia::metrics::init();

        // JWT key resolution
        let jwt_key: Option<SecretString> =
            self.config.gateway.auth.signing_key.clone().or_else(|| {
                RealSystem.var("ALETHEIA_JWT_SECRET").map(SecretString::from)
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
            thesauros::loader::load_packs(&self.config.packs)
        } else {
            Vec::new()
        };
        let packs = Arc::new(loaded_packs);

        // Session store
        let db_path = self.oikos.sessions_db();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_whatever_context(|_| {
                format!("failed to CREATE data dir {}", parent.display())
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
                thesauros::tools::register_pack_tools(&packs, &mut tool_registry);
            for err in &tool_errors {
                warn!(error = %err, "failed to register pack tool");
            }
        }

        // Register external tools FROM [tools] config section
        let tools_config = crate::external_tools::load_tools_config(&self.oikos);
        let tool_manifest = crate::external_tools::register_external_tools(
            &tools_config,
            &mut tool_registry,
            &reqwest::Client::new(),
        );
        if tool_manifest.available_count() > 0 || !tools_config.required.is_empty() {
            info!(
                available = tool_manifest.available_count(),
                missing_required = tool_manifest.missing_required_count(),
                "external tools registered"
            );
        }
        let missing = tool_manifest.missing_required_count();
        if missing > 0 {
            warn!(
                count = missing,
                "required external tools unavailable -- agents will degrade gracefully"
            );
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
            build_signal_provider(&self.config.channels.signal, &self.config.messaging)
        } else {
            None
        };

        // Tool services
        let (cross_nous, messenger, note_store, blackboard_store, spawn, planning) = if self
            .tool_services
        {
            let cross_nous: Arc<dyn organon::types::CrossNousService> =
                Arc::new(tool_adapters::CrossNousAdapter(Arc::clone(&cross_router)));
            #[expect(
                clippy::as_conversions,
                reason = "coercion to dyn trait objects: required by Arc<dyn Trait> type annotations"
            )]
            let messenger: Option<Arc<dyn organon::types::MessageService>> =
                signal_provider.as_ref().map(|p| {
                    Arc::new(tool_adapters::SignalAdapter(
                        Arc::clone(p) as Arc<dyn ChannelProvider>
                    )) as Arc<dyn organon::types::MessageService>
                });
            let note_store: Option<Arc<dyn organon::types::NoteStore>> = Some(Arc::new(
                nous::adapters::SessionNoteAdapter(Arc::clone(&session_store)),
            ));
            let blackboard_store: Option<Arc<dyn organon::types::BlackboardStore>> =
                Some(Arc::new(nous::adapters::SessionBlackboardAdapter(
                    Arc::clone(&session_store),
                )));
            let spawn: Option<Arc<dyn organon::types::SpawnService>> =
                Some(Arc::new(nous::spawn_svc::SpawnServiceImpl::new(
                    Arc::clone(&provider_registry),
                    Arc::clone(&tool_registry),
                    Arc::clone(&self.oikos),
                )));
            let planning_root = self.oikos.data().join("planning");
            let planning: Option<Arc<dyn organon::types::PlanningService>> =
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
        let vector_search: Option<Arc<dyn nous::recall::VectorSearch>> =
            knowledge_store.as_ref().map(|ks| {
                Arc::new(nous::recall::KnowledgeVectorSearch::new(
                    Arc::clone(ks),
                )) as Arc<dyn nous::recall::VectorSearch>
            });
        #[cfg(not(feature = "recall"))]
        let vector_search: Option<Arc<dyn nous::recall::VectorSearch>> = None;

        // External recall sources (issue #2338)
        #[cfg(feature = "recall")]
        let recall_source_registry = {
            let mut registry = crate::recall_sources::RecallSourceRegistry::new();
            let http_client = Arc::new(reqwest::Client::new());

            // Academic source (Semantic Scholar)
            let api_key = RealSystem.var("SEMANTIC_SCHOLAR_API_KEY").or_else(|| {
                tracing::warn!("SEMANTIC_SCHOLAR_API_KEY not set");
                None
            });
            registry.register(Arc::new(
                crate::recall_sources::academic::AcademicSource::new(
                    Arc::clone(&http_client),
                    api_key,
                ),
            ));

            // LLM context source (model cards + pricing)
            registry.register(Arc::new(
                crate::recall_sources::llm_context::LlmContextSource::from_known_models(
                    &self.config.pricing,
                ),
            ));

            info!(
                count = registry.source_count(),
                "external recall sources registered"
            );
            Arc::new(registry)
        };

        // Knowledge search adapter for tool layer
        #[cfg(feature = "recall")]
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn trait object: required to satisfy Arc<dyn Trait> type annotation"
        )]
        let knowledge_search: Option<
            Arc<dyn organon::types::KnowledgeSearchService>,
        > = knowledge_store.as_ref().map(|ks| {
            Arc::new(crate::knowledge_adapter::KnowledgeSearchAdapter::new(
                Arc::clone(ks),
                Arc::clone(&embedding_provider),
                Arc::clone(&recall_source_registry),
            )) as Arc<dyn organon::types::KnowledgeSearchService>
        });
        #[cfg(not(feature = "recall"))]
        let knowledge_search: Option<
            Arc<dyn organon::types::KnowledgeSearchService>,
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
            server_tool_config: organon::types::ServerToolConfig::default(),
        });

        // Clone knowledge_store Arc before moving INTO NousManager
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
            self.config.nous_behavior.clone(),
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
                    generation: nous::config::NousGenerationConfig {
                        model: resolved.model.primary.to_string(),
                        context_window: resolved.limits.context_tokens,
                        max_output_tokens: resolved.limits.max_output_tokens,
                        bootstrap_max_tokens: resolved.limits.bootstrap_max_tokens,
                        thinking_enabled: resolved.capabilities.thinking_enabled,
                        thinking_budget: resolved.limits.thinking_budget,
                        chars_per_token: resolved.limits.chars_per_token,
                        prosoche_model: resolved.prosoche_model.to_string(),
                    },
                    limits: nous::config::NousLimits {
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
                    hooks: nous::config::HookConfig::default(),
                    behavior: resolved.behavior,
                };
                nous_manager
                    .spawn(
                        nous_config,
                        PipelineConfig {
                            extraction: Some(mneme::extract::ExtractionConfig::default()),
                            training: self.config.training.clone(),
                            ..PipelineConfig::default()
                        },
                    )
                    .await;
            }
            info!(count = nous_manager.count(), "nous actors spawned");
        }

        // Daemon handles collector
        let mut daemon_handles: JoinSet<()> = JoinSet::new();

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
            daemon_handles.spawn(
                async move {
                    daemon_runner.run().await;
                }
                .instrument(tracing::info_span!("daemon_runner")),
            );
            info!("daemon started");
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
                runner.register(oikonomos::schedule::TaskDef {
                    id: format!("{}-prosoche", agent_def.id),
                    name: "Prosoche attention check".to_owned(),
                    nous_id: agent_def.id.clone(),
                    schedule: oikonomos::schedule::Schedule::Interval(
                        std::time::Duration::from_mins(45),
                    ),
                    action: oikonomos::schedule::TaskAction::Builtin(
                        oikonomos::schedule::BuiltinTask::Prosoche,
                    ),
                    enabled: true,
                    active_window: Some((8, 23)),
                    catch_up: false,
                    ..oikonomos::schedule::TaskDef::default()
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
            idempotency_cache: Arc::new(pylon::idempotency::IdempotencyCache::with_config(
                std::time::Duration::from_secs(self.config.api_limits.idempotency_ttl_secs),
                self.config.api_limits.idempotency_capacity,
                self.config.api_limits.idempotency_max_key_length,
            )),
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

mod validate;

use validate::{validate_external_tools, validate_jwt};

mod setup;
mod tool_adapters;

use setup::{
    build_provider_registry, build_signal_provider, build_tool_registry,
    create_embedding_provider, start_inbound_dispatch,
};
#[cfg(feature = "recall")]
use setup::open_knowledge_store;
