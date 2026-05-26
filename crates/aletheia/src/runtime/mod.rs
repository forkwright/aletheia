//! [`RuntimeBuilder`]: single-site construction of all server subsystems.

#[cfg(feature = "recall")]
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use snafu::prelude::*;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, error, info, warn};

use agora::types::ChannelProvider;
use aletheia_routing::{AfterActionStore, RecordingRouter};
use hermeneus::provider::ProviderRegistry;
use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};
use mneme::embedding::DegradedEmbeddingProvider;
use mneme::store::SessionStore;
use nous::cross::CrossNousRouter;
use nous::manager::NousManager;
use oikonomos::runner::TaskRunner;
use organon::registry::ToolRegistry;
use organon::types::ToolServices;
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::AletheiaConfig;
#[cfg(feature = "recall")]
use taxis::config::resolve_nous;
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
    /// Tracks all long-running background tasks (daemons, per-agent runners,
    /// lazy init jobs). On shutdown: close the tracker, then `wait()` with a
    /// timeout so every task gets a chance to drain cleanly.
    pub task_tracker: TaskTracker,
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

    #[expect(
        clippy::too_many_lines,
        reason = "sequential init steps: splitting would fragment the startup flow"
    )]
    pub(crate) async fn build(self) -> Result<Runtime> {
        let shutdown_token = CancellationToken::new();
        let task_tracker = TaskTracker::new();

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

        // NOTE: per-crate metrics are registered with the shared
        // `MetricsRegistry` below via [`register_all_metrics`] during AppState
        // construction. No global init required — the registry is installed in
        // AppState and exposed on the /metrics endpoint.

        // JWT key resolution
        let jwt_key: Option<SecretString> =
            self.config.gateway.auth.signing_key.clone().or_else(|| {
                RealSystem
                    .var("ALETHEIA_JWT_SECRET")
                    .map(SecretString::from)
            });
        // WHY: honor the configured clock-skew leeway on every path so the
        // advertised 30s tolerance (or an operator override) applies uniformly.
        // Fixes #3379.
        let jwt_leeway = self.config.jwt.clock_skew_leeway_secs;
        let jwt_config = match jwt_key {
            Some(k) => JwtConfig {
                signing_key: k,
                clock_skew_leeway_secs: jwt_leeway,
                ..JwtConfig::default()
            },
            None => JwtConfig {
                clock_skew_leeway_secs: jwt_leeway,
                ..JwtConfig::default()
            },
        };
        jwt_config
            .validate_for_auth_mode(self.config.gateway.auth.mode.as_str())
            .whatever_context("JWT key security check failed")?;

        // Domain packs
        // WHY: load_packs performs synchronous file I/O; wrap in spawn_blocking
        // so the async runtime thread is not stalled during pack discovery.
        let loaded_packs = if self.domain_packs {
            let packs = self.config.packs.clone();
            tokio::task::spawn_blocking(move || thesauros::loader::load_packs(&packs))
                .await
                .whatever_context("pack loading task panicked")?
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

        let auth_store_path = self.oikos.data().join("auth.fjall");
        let auth_facade = AuthFacade::new(
            AuthConfig {
                jwt: jwt_config.clone(),
            },
            &auth_store_path,
        )
        .with_whatever_context(|_| {
            format!("failed to open auth store at {}", auth_store_path.display())
        })?;
        let jwt_manager = JwtManager::new(jwt_config);

        // Provider registry
        let provider_registry = if self.credentials {
            Arc::new(build_provider_registry(&self.config, &self.oikos))
        } else {
            Arc::new(ProviderRegistry::new())
        };

        // Tool registry
        let after_action_log_dir = self.oikos.logs().join("after-actions");
        let mut tool_registry = if self.credentials {
            build_tool_registry(
                &self.config,
                &self.oikos,
                &shutdown_token,
                Some(after_action_log_dir.clone()),
            )?
        } else {
            ToolRegistry::new()
        };

        // Register domain pack tools
        if self.domain_packs {
            let tool_errors = thesauros::tools::register_pack_tools(&packs, &mut tool_registry);
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
        )
        .await;
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

        // Embedding provider — lazy initialization (#3474)
        //
        // WHY: the embedding model download/load can be slow or fail. Loading
        // synchronously here blocks the HTTP gateway from binding. Wrapping in
        // `LazyEmbeddingProvider` lets the gateway start immediately and defers
        // the real init to first use.
        let embedding_provider: Arc<dyn mneme::embedding::EmbeddingProvider> = if self.embedding {
            let lazy = Arc::new(LazyEmbeddingProvider::new(self.config.embedding.clone()));
            // Spawn background init so the model loads without blocking startup.
            let lazy_clone = Arc::clone(&lazy);
            task_tracker.spawn(async move {
                lazy_clone.get().await;
            });
            lazy
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
        let matrix_provider = if self.tool_services {
            build_matrix_provider(&self.config.channels.matrix, &self.config.messaging)
        } else {
            None
        };

        // Tool services
        let (cross_nous, messenger, note_store, blackboard_store, planning) = if self.tool_services
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
            let planning_root = self.oikos.data().join("planning");
            let planning: Option<Arc<dyn organon::types::PlanningService>> = Some(Arc::new(
                planning_adapter::FilesystemPlanningService::new(planning_root),
            ));
            (
                Some(cross_nous),
                messenger,
                note_store,
                blackboard_store,
                planning,
            )
        } else {
            (None, None, None, None, None)
        };

        // Knowledge stores
        #[cfg(feature = "recall")]
        let knowledge_stores = if self.embedding {
            let mut cohorts = BTreeSet::from(["shared".to_owned()]);
            for agent_def in &self.config.agents.list {
                let resolved = resolve_nous(&self.config, &agent_def.id);
                cohorts.insert(resolved.episteme_cohort.to_string());
            }
            open_knowledge_stores(&self.oikos, cohorts)?
        } else {
            std::collections::HashMap::new()
        };
        #[cfg(feature = "recall")]
        let shared_knowledge_store = knowledge_stores.get("shared").cloned();

        // Vector search
        #[cfg(feature = "recall")]
        #[expect(
            clippy::as_conversions,
            reason = "coercion to dyn trait object: required to satisfy Arc<dyn Trait> type annotation"
        )]
        let vector_search: Option<Arc<dyn nous::recall::VectorSearch>> =
            shared_knowledge_store.as_ref().map(|ks| {
                Arc::new(nous::recall::KnowledgeVectorSearch::new(Arc::clone(ks)))
                    as Arc<dyn nous::recall::VectorSearch>
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
        let knowledge_search: Option<Arc<dyn organon::types::KnowledgeSearchService>> =
            shared_knowledge_store.as_ref().map(|ks| {
                Arc::new(crate::knowledge_adapter::KnowledgeSearchAdapter::new(
                    Arc::clone(ks),
                    Arc::clone(&embedding_provider),
                    Arc::clone(&recall_source_registry),
                )) as Arc<dyn organon::types::KnowledgeSearchService>
            });
        #[cfg(not(feature = "recall"))]
        let knowledge_search: Option<Arc<dyn organon::types::KnowledgeSearchService>> = None;

        let audit_log_dir = self
            .config
            .prompt_audit
            .log_dir
            .clone()
            .unwrap_or_else(|| self.oikos.logs().join("prompt-audit"));
        let audit_log = Arc::new(nous::audit::PromptAuditLog::new(
            audit_log_dir,
            self.config.prompt_audit.enabled,
        ));
        let after_action_store = Arc::new(AfterActionStore::new(
            self.oikos.logs().join("after-actions"),
        ));
        let empirical_router: Arc<dyn aletheia_routing::Router> = Arc::new(RecordingRouter::new(
            Arc::clone(&after_action_store),
            self.config
                .agents
                .defaults
                .model_defaults
                .model
                .primary
                .as_str(),
        ));

        let spawn_impl = if self.tool_services {
            #[cfg(feature = "recall")]
            let child_knowledge_store = shared_knowledge_store.clone();
            Some(Arc::new(
                nous::spawn_svc::SpawnServiceImpl::new(
                    Arc::clone(&provider_registry),
                    Arc::clone(&tool_registry),
                    Arc::clone(&self.oikos),
                )
                .with_runtime_services(nous::spawn_svc::InheritedSpawnServices {
                    embedding_provider: Some(Arc::clone(&embedding_provider)),
                    vector_search: vector_search.clone(),
                    session_store: Some(Arc::clone(&session_store)),
                    #[cfg(feature = "recall")]
                    knowledge_store: child_knowledge_store,
                    router: Some(Arc::clone(&cross_router)),
                    audit_log: Some(Arc::clone(&audit_log)),
                    empirical_router: Some(Arc::clone(&empirical_router)),
                }),
            ))
        } else {
            None
        };
        let spawn: Option<Arc<dyn organon::types::SpawnService>> =
            spawn_impl.as_ref().map(|service| {
                let service: Arc<dyn organon::types::SpawnService> = service.clone();
                service
            });

        let tool_services = Arc::new(ToolServices {
            working_checkpoint_store: None,
            cross_nous,
            messenger,
            note_store,
            blackboard_store,
            spawn,
            planning,
            knowledge: knowledge_search,
            http_client: reqwest::Client::new(),
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: tool_registry.lazy_tool_catalog(),
            server_tool_config: organon::types::ServerToolConfig::default(),
        });
        if let Some(spawn_impl) = spawn_impl.as_ref() {
            spawn_impl.set_tool_services(Arc::clone(&tool_services));
        }

        // Clone shared store Arc before moving cohort stores into NousManager
        #[cfg(feature = "recall")]
        let knowledge_store_for_daemon = shared_knowledge_store.clone();

        let mut nous_manager = NousManager::new(
            Arc::clone(&provider_registry),
            Arc::clone(&tool_registry),
            Arc::clone(&self.oikos),
            Some(Arc::clone(&embedding_provider)),
            vector_search,
            Some(Arc::clone(&session_store)),
            #[cfg(feature = "recall")]
            Some(knowledge_stores),
            Arc::clone(&packs),
            Some(Arc::clone(&cross_router)),
            Some(tool_services),
            self.config.nous_behavior.clone(),
        )
        .with_audit_log(Arc::clone(&audit_log))
        .with_empirical_router(Arc::clone(&empirical_router));

        // Spawn nous actors
        {
            for agent_def in &self.config.agents.list {
                let (nous_config, pipeline_config) =
                    build_nous_runtime_config(&self.config, &self.oikos, &packs, &agent_def.id);
                if let Err(e) = nous_manager.spawn(nous_config, pipeline_config).await {
                    error!(
                        agent = %agent_def.id,
                        error = %e,
                        "failed to spawn agent — skipping"
                    );
                }
            }
            info!(count = nous_manager.count(), "nous actors spawned");
        }

        let mut maintenance_config = maintenance::build_config(
            &self.oikos,
            &self.config.maintenance,
            &self.config.prompt_audit,
        );
        maintenance_config.after_action_store = Some(Arc::clone(&after_action_store));
        maintenance_config.backup_metrics = Some(Arc::new(RuntimeBackupMetricsRecorder));
        let task_state_root = self.oikos.data().join("daemon-task-state");

        if self.daemons {
            // System maintenance daemon
            let daemon_token = shutdown_token.child_token();
            let system_state_store =
                oikonomos::state::TaskStateStore::open(&task_state_root.join("system"))
                    .with_whatever_context(|_| "failed to open system daemon task-state store")?;
            let mut daemon_runner = TaskRunner::new("system", daemon_token)
                .with_daemon_behavior(self.config.daemon_behavior.clone())
                .with_state_store(system_state_store)
                .with_maintenance(maintenance_config.clone());
            let retention_executor = Arc::new(
                crate::session_retention::SessionRetentionAdapter::new(Arc::clone(&session_store)),
            );
            daemon_runner = daemon_runner.with_retention(retention_executor);

            #[cfg(feature = "recall")]
            if let Some(ks) = knowledge_store_for_daemon.as_ref() {
                let km_executor = Arc::new(
                    crate::knowledge_maintenance::KnowledgeMaintenanceAdapter::new(Arc::clone(ks)),
                );
                daemon_runner = daemon_runner.with_knowledge_maintenance(km_executor);
            }

            if !self.config.dispatch.cron_tasks.is_empty() {
                warn!(
                    cron_tasks = self.config.dispatch.cron_tasks.len(),
                    "dispatch cron tasks configured but not started; recurring energeia dispatch is disabled until the daemon can execute real dispatch actions"
                );
            }

            daemon_runner.register_maintenance_tasks();
            task_tracker.spawn(
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
            matrix_provider.as_ref(),
            &shutdown_token,
        )?;

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
                )
                .with_daemon_behavior(self.config.daemon_behavior.clone())
                .with_state_store(
                    oikonomos::state::TaskStateStore::open(
                        &task_state_root.join(task_state_component(&agent_def.id)),
                    )
                    .with_whatever_context(|_| {
                        format!(
                            "failed to open daemon task-state store for {}",
                            agent_def.id
                        )
                    })?,
                )
                .with_maintenance(maintenance_config.clone());
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
                runner.register(oikonomos::schedule::TaskDef {
                    id: format!("{}-prosoche-self-audit", agent_def.id),
                    name: "Prosoche self-audit".to_owned(),
                    nous_id: agent_def.id.clone(),
                    schedule: oikonomos::schedule::Schedule::Interval(
                        std::time::Duration::from_hours(6),
                    ),
                    action: oikonomos::schedule::TaskAction::Builtin(
                        oikonomos::schedule::BuiltinTask::SelfAudit,
                    ),
                    enabled: true,
                    active_window: Some((8, 23)),
                    catch_up: false,
                    ..oikonomos::schedule::TaskDef::default()
                });
                let daemon_span = tracing::info_span!("daemon", nous.id = %agent_def.id);
                task_tracker.spawn(
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

        // WHY: prometheus-client has no process-wide global registry — every
        // metrics-emitting crate registers its families against a single
        // shared Registry created here. Pylon's /metrics handler encodes the
        // same registry on every scrape.
        let metrics_registry = koina::metrics::MetricsRegistry::new();
        register_all_metrics(&metrics_registry);

        let (config_tx, _config_rx) = tokio::sync::watch::channel(aletheia_config.clone());
        let mut reload_rx = config_tx.subscribe();
        let reload_manager = Arc::clone(&nous_manager);
        let reload_oikos = Arc::clone(&self.oikos);
        let reload_packs = Arc::clone(&packs);
        task_tracker.spawn(
            async move {
                loop {
                    if reload_rx.changed().await.is_err() {
                        break;
                    }
                    let config = reload_rx.borrow().clone();
                    let actor_configs = config
                        .agents
                        .list
                        .iter()
                        .map(|agent| {
                            let (nous_config, pipeline_config) = build_nous_runtime_config(
                                &config,
                                &reload_oikos,
                                &reload_packs,
                                &agent.id,
                            );
                            (agent.id.clone(), nous_config, pipeline_config)
                        })
                        .collect();
                    if let Err(e) = reload_manager.reload_actor_configs(actor_configs).await {
                        warn!(error = %e, "failed to apply hot-reloaded actor config");
                    }
                }
            }
            .instrument(tracing::info_span!("config_reload_actor_sync")),
        );
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::clone(&nous_manager),
            provider_registry,
            tool_registry,
            oikos: self.oikos,
            jwt_manager: Arc::new(jwt_manager),
            auth_facade: Arc::new(auth_facade),
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
            embedding_provider: Some(Arc::clone(&embedding_provider)),
            turn_buffer_registry: Arc::new(pylon::turn_buffer::TurnBufferRegistry::new()),
            metrics_registry,
            event_bus: Arc::new(pylon::event_bus::EventBus::new(256)),
        });

        Ok(Runtime {
            state,
            nous_manager,
            task_tracker,
            shutdown_token,
        })
    }
}

mod validate;

mod builder_validation;
mod metrics;
mod nous_config;

use metrics::{RuntimeBackupMetricsRecorder, register_all_metrics, task_state_component};
use nous_config::build_nous_runtime_config;

mod setup;
mod tool_adapters;

#[cfg(feature = "recall")]
use setup::open_knowledge_stores;
use setup::{
    LazyEmbeddingProvider, build_matrix_provider, build_provider_registry, build_signal_provider,
    build_tool_registry, start_inbound_dispatch,
};
