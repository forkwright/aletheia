// kanon:ignore RUST/file-too-long — RuntimeBuilder and its impls are tightly coupled; splitting would require exposing private fields
//! [`RuntimeBuilder`]: single-site construction of all server subsystems.

#[cfg(feature = "recall")]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use snafu::prelude::*;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, error, info, warn};

use agora::types::ChannelProvider;
use aletheia_routing::{
    AfterActionStore, DEFAULT_ROUTING_WINDOW, EmpiricalRouter, FallthroughRouter, NoOpRouter,
};
use hermeneus::provider::ProviderRegistry;
use koina::disk_space::DiskSpaceMonitor;
use koina::id::ToolName;
use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};
use mneme::embedding::DegradedEmbeddingProvider;
use mneme::store::SessionStore;
use nous::cross::CrossNousRouter;
use nous::manager::NousManager;
use oikonomos::runner::{DaemonOutputMode, TaskRunner};
use organon::registry::ToolRegistry;
use organon::types::{ToolHttpClients, ToolServices};
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::AletheiaConfig;
use taxis::config::DaemonRunnerOutputMode;
#[cfg(feature = "recall")]
use taxis::config::resolve_nous;
use taxis::oikos::Oikos;
use taxis::validate::{validate_config, validate_startup};

use crate::commands::maintenance;
use crate::daemon_bridge;
use crate::error::Result;
use crate::planning_adapter;

#[derive(Clone)]
struct RuntimeSessionStoreHealthProbe {
    session_store: Arc<Mutex<SessionStore>>,
}

impl std::fmt::Debug for RuntimeSessionStoreHealthProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSessionStoreHealthProbe")
            .finish_non_exhaustive()
    }
}

impl oikonomos::maintenance::SessionStoreHealthProbe for RuntimeSessionStoreHealthProbe {
    fn check_session_store(&self) -> oikonomos::maintenance::DbHealth {
        match self.session_store.try_lock() {
            Ok(store) => match store.ping() {
                Ok(()) => oikonomos::maintenance::DbHealth::Healthy,
                Err(error) => oikonomos::maintenance::DbHealth::Unhealthy(error.to_string()),
            },
            Err(_) => oikonomos::maintenance::DbHealth::Locked(
                "active session store mutex is busy".to_owned(),
            ),
        }
    }
}

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

fn resolve_pack_path(oikos: &Oikos, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        let absolute = oikos.root().join(configured);
        absolute.canonicalize().unwrap_or(absolute)
    }
}

fn daemon_output_mode(mode: DaemonRunnerOutputMode) -> DaemonOutputMode {
    match mode {
        DaemonRunnerOutputMode::Brief => DaemonOutputMode::Brief,
        DaemonRunnerOutputMode::Full => DaemonOutputMode::Full,
        // WHY: taxis keeps config enums non-exhaustive. Unknown future modes
        // and the current default fail closed to metadata-only output.
        _ => DaemonOutputMode::Summary,
    }
}

/// Build the registry of external recall sources queried alongside the
/// knowledge store.
///
/// SECURITY(#6444): each network-backed source is registered only after its
/// own explicit `recall_sources.*.enabled` opt-in -- never merely because a
/// credential env var happens to be set. A default (or any config that
/// leaves a source unset) must not register it, so `memory_search` reaches
/// no third party until an operator turns one on.
#[cfg(feature = "recall")]
/// Build the external recall-source registry.
///
/// SECURITY(#6921): `http_client` must have redirects DISABLED, and `egress` is applied
/// to every hop. `AcademicSource` sends an `x-api-key`, and reqwest strips only
/// Authorization, Cookie, Proxy-Authorization and WWW-Authenticate across a cross-host
/// redirect -- so on a redirect-following client that key reached whatever host a
/// response named.
fn build_recall_source_registry(
    config: &AletheiaConfig,
    http_client: &Arc<reqwest::Client>,
    egress: &organon::sandbox::EgressGate,
) -> crate::recall_sources::RecallSourceRegistry {
    let mut registry = crate::recall_sources::RecallSourceRegistry::new();

    if config.recall_sources.academic.enabled {
        let api_key = RealSystem.var("SEMANTIC_SCHOLAR_API_KEY").or_else(|| {
            tracing::warn!(
                "recall_sources.academic.enabled = true but SEMANTIC_SCHOLAR_API_KEY not set \
                 -- querying at the unauthenticated rate limit"
            );
            None
        });
        registry.register(Arc::new(
            crate::recall_sources::academic::AcademicSource::new(
                Arc::clone(http_client),
                api_key,
                egress.clone(),
            ),
        ));
    }

    registry.register(Arc::new(
        crate::recall_sources::llm_context::LlmContextSource::from_known_models(&config.pricing),
    ));

    info!(
        count = registry.source_count(),
        "external recall sources registered"
    );
    registry
}

fn resolve_pack_paths(oikos: &Oikos, configured: &[PathBuf]) -> Vec<PathBuf> {
    configured
        .iter()
        .map(|path| resolve_pack_path(oikos, path))
        .collect()
}

/// Build the daemon's live maintenance configuration, wiring in the runtime
/// handles that `taxis::config::MaintenanceSettings` cannot carry: the
/// shared after-action store, the backup metrics recorder, and the
/// session-store health probe.
///
/// WHY: used both at initial [`Runtime`] construction and by the
/// config-reload task below, so the two call sites cannot drift from each
/// other (SSOT) -- a reload that rebuilt `MaintenanceConfig` differently
/// than startup would silently reset or duplicate these runtime handles
/// (most sharply, `after_action_store`: `maintenance::build_config` on its
/// own constructs a brand-new, empty store).
fn build_daemon_maintenance_config(
    oikos: &Oikos,
    config: &AletheiaConfig,
    after_action_store: &Arc<AfterActionStore>,
    session_store: &Arc<Mutex<SessionStore>>,
) -> oikonomos::maintenance::MaintenanceConfig {
    let mut maintenance_config =
        maintenance::build_config(oikos, &config.maintenance, &config.prompt_audit);
    maintenance_config.after_action_store = Some(Arc::clone(after_action_store));
    maintenance_config.backup_metrics = Some(Arc::new(RuntimeBackupMetricsRecorder));
    // WHY(#6445): publish backup freshness from the manifests on disk so a
    // reload -- not just a restart or a completed backup -- keeps exported
    // backup-interval/enabled metrics in sync with the live config.
    oikonomos::maintenance::instance_backup::publish_backup_state(
        &maintenance_config.instance_backup,
        &RuntimeBackupMetricsRecorder,
    );
    maintenance_config.session_store_health_probe =
        Some(Arc::new(RuntimeSessionStoreHealthProbe {
            session_store: Arc::clone(session_store),
        }));
    maintenance_config
}

/// Spawn the disk-space monitor from `maintenance.diskSpace` config.
///
/// Returns `None` (and spawns nothing) when `enabled = false`, per #5128's
/// acceptance criteria — write-guard callers must treat a `None` monitor as
/// "monitoring intentionally disabled", not "assume space is available".
/// Checks `oikos.data()`, matching the one-shot startup check in
/// `taxis::preflight::check_disk_space` (#5128).
///
/// WHY: previously created unconditionally in `commands::server` with
/// hardcoded `DEFAULT_WARNING_BYTES`/`DEFAULT_CRITICAL_BYTES` and a fixed
/// one-minute interval via an untracked `tokio::spawn`, ignoring config
/// entirely and outliving the `task_tracker` drain on shutdown. Spawning here
/// (config-driven, `task_tracker`-tracked) lets the monitor live on
/// `AppState` for health/diagnostics and be reused by log retention instead
/// of each caller building its own.
fn spawn_disk_space_monitor(
    oikos: &Oikos,
    settings: &taxis::config::DiskSpaceSettings,
    task_tracker: &TaskTracker,
    token: CancellationToken,
) -> Option<DiskSpaceMonitor> {
    // WHY: saturating conversion mirrors `koina::disk_space`'s own
    // `available_bytes_from_stat` — an operator-supplied MB value large
    // enough to overflow u64 bytes should clamp, not wrap into a tiny
    // (falsely critical) threshold.
    const BYTES_PER_MB: u64 = 1024 * 1024;

    if !settings.enabled {
        return None;
    }

    let warning_bytes = settings.warning_threshold_mb.saturating_mul(BYTES_PER_MB);
    let critical_bytes = settings.critical_threshold_mb.saturating_mul(BYTES_PER_MB);

    let monitor = DiskSpaceMonitor::new(warning_bytes, critical_bytes);
    let task_monitor = monitor.clone();
    let path = oikos.data();
    // WHY(#6080-style): config validation rejects a zero interval before this
    // point is ever reached; `.max(1)` is a defense-in-depth floor only —
    // `Duration::from_secs(0)` would tick as fast as possible and spin CPU.
    let interval = Duration::from_secs(settings.check_interval_secs.max(1));
    let span = tracing::info_span!("disk_space_monitor", path = %path.display());
    task_tracker.spawn(
        async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    biased;
                    () = token.cancelled() => break,
                    _ = ticker.tick() => {
                        match task_monitor.refresh(&path) {
                            Ok(status) => {
                                tracing::debug!(%status, "disk space monitor refreshed");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, path = %path.display(), "disk space monitor refresh failed");
                            }
                        }
                    }
                }
            }
        }
        .instrument(span),
    );
    Some(monitor)
}

/// Build a per-agent prosoche `TaskDef` from config values.
///
/// WHY: keep task ID/name/schedule conversion in one place so the runtime
/// registration stays readable and the mapping from config to `TaskDef` is
/// easy to unit-test without constructing a full `Runtime`.
fn prosoche_task_def(
    agent_id: &str,
    task_id: &str,
    name: &str,
    enabled: bool,
    interval_secs: u64,
    active_window: Option<(u8, u8)>,
    action: oikonomos::schedule::BuiltinTask,
) -> oikonomos::schedule::TaskDef {
    oikonomos::schedule::TaskDef {
        id: task_id.to_owned(),
        name: name.to_owned(),
        nous_id: agent_id.to_owned(),
        schedule: oikonomos::schedule::Schedule::Interval(std::time::Duration::from_secs(
            interval_secs,
        )),
        action: oikonomos::schedule::TaskAction::Builtin(action),
        enabled,
        active_window,
        catch_up: false,
        ..oikonomos::schedule::TaskDef::default()
    }
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

        if self.config_strict {
            // WHY(#5770): shares one derived section list with `check-config`
            // (see `builder_validation::validate`) so the two paths cannot
            // disagree about which sections a config must satisfy.
            validate_config(&self.config)
                .with_whatever_context(|error| format!("invalid config: {error}"))?;
            crate::embedding_config::validate_embedding_settings(&self.config.embedding)
                .with_whatever_context(|error| format!("invalid embedding config: {error}"))?;
            info!("config validated");

            validate_startup(&self.config, &self.oikos)
                .whatever_context("startup validation failed")?;
            let provider_errors = validate::provider_runtime_errors(&self.config, &self.oikos);
            if !provider_errors.is_empty() {
                snafu::whatever!(
                    "provider runtime validation failed:\n  - {}",
                    provider_errors.join("\n  - ")
                );
            }
            info!("startup validation passed");
        }

        // NOTE: per-crate metrics are registered with the shared
        // `MetricsRegistry` below via [`register_all_metrics`] during AppState
        // construction. No global init required — the registry is installed in
        // AppState and exposed on the /metrics endpoint.

        let jwt_key: Option<SecretString> =
            self.config.gateway.auth.signing_key.clone().or_else(|| {
                RealSystem
                    .var("ALETHEIA_JWT_SECRET")
                    .map(SecretString::from)
            });
        // WHY (#3379): honor the configured clock-skew leeway on every path so
        // the advertised 30s tolerance (or an operator override) applies
        // uniformly.
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

        // WHY: load_packs performs synchronous file I/O; wrap in spawn_blocking
        // so the async runtime thread is not stalled during pack discovery.
        let pack_outcome = if self.domain_packs {
            let packs = resolve_pack_paths(&self.oikos, &self.config.packs);
            let overlay_policy = overlay_policy_from_config(&self.config.pack_overlays);
            tokio::task::spawn_blocking(move || {
                thesauros::loader::load_packs_with_policy(&packs, &overlay_policy)
            })
            .await
            .whatever_context("pack loading task panicked")?
        } else {
            thesauros::loader::LoadOutcome::default()
        };
        let mut pack_report = pack_outcome.report;
        let packs = Arc::new(pack_outcome.packs);

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

        // WHY(#4588): default-on (`working_checkpoint_enabled`) tool +
        // hook surfaces both read this same Option via `ToolServices`
        // (see `nous::hooks::builtins::register_builtin_hooks` call site in
        // `nous::actor::turn`), so opening it once here wires both.
        let working_checkpoints_path = self.oikos.working_checkpoints_db();
        let working_checkpoint_store: Arc<dyn organon::types::WorkingCheckpointStore> = Arc::new(
            nous::working_memory::FjallWorkingCheckpointStore::open(&working_checkpoints_path)
                .with_whatever_context(|_| {
                    format!(
                        "failed to open working checkpoint store at {}",
                        working_checkpoints_path.display()
                    )
                })?,
        );
        info!(path = %working_checkpoints_path.display(), "working checkpoint store opened");

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

        let provider_registry = if self.credentials {
            Arc::new(build_provider_registry(&self.config, &self.oikos))
        } else {
            Arc::new(ProviderRegistry::new())
        };

        let after_action_log_dir = self.oikos.logs().join("after-actions");
        #[cfg(feature = "energeia")]
        let mut energeia_services: Option<
            Arc<organon::builtins::energeia::EnergeiaServices>,
        > = None;
        let mut tool_registry = if self.credentials {
            let built = build_tool_registry(
                &self.config,
                &self.oikos,
                &shutdown_token,
                Some(after_action_log_dir.clone()),
            )?;
            #[cfg(feature = "energeia")]
            {
                energeia_services = built.energeia_services;
            }
            built.registry
        } else {
            ToolRegistry::new()
        };

        if self.domain_packs {
            let pack_sandbox = sandbox_config(&self.config);
            pack_report.notes = thesauros::health::platform_notes(&pack_sandbox);
            let tool_failures = thesauros::tools::register_pack_tools_with_sandbox_and_limits(
                &packs,
                &mut tool_registry,
                pack_sandbox,
                self.config.tool_limits.subprocess_timeout_secs,
            );
            for failure in &tool_failures {
                warn!(
                    pack = %failure.pack_name,
                    tool = %failure.tool_name,
                    error = %failure.error,
                    "failed to register pack tool"
                );
            }
            pack_report.record_tool_failures(&tool_failures);

            for note in &pack_report.notes {
                warn!("{note}");
            }

            // WHY(#5208): a pack can be active, degraded, or failed; the
            // summary line is the daemon-visible record operators and log
            // scrapers key on, and pack_report carries the structured detail.
            let counts = pack_report.status_counts();
            if counts.failed > 0 || counts.degraded > 0 {
                warn!(
                    active = counts.active,
                    degraded = counts.degraded,
                    failed = counts.failed,
                    "domain pack health: some packs are degraded or failed"
                );
            } else if counts.active > 0 {
                info!(
                    loaded_cleanly = counts.active,
                    "domain packs loaded and registered without reported degradation"
                );
            }
            // WHY: informational overlay-admission records do not degrade a
            // pack, but they are still operator-facing health evidence. Emit
            // every recorded issue independently of the aggregate status, with
            // the configured occurrence identity needed to disambiguate
            // duplicate names and paths.
            for pack in &pack_report.packs {
                for issue in &pack.issues {
                    match issue.severity {
                        thesauros::health::Severity::Info => info!(
                            pack = %pack.name,
                            pack_ordinal = pack.instance_id.ordinal(),
                            pack_path = %pack.path.display(),
                            component = ?issue.component,
                            severity = ?issue.severity,
                            "{}",
                            issue.message,
                        ),
                        thesauros::health::Severity::Warning
                        | thesauros::health::Severity::Error => warn!(
                            pack = %pack.name,
                            pack_ordinal = pack.instance_id.ordinal(),
                            pack_path = %pack.path.display(),
                            component = ?issue.component,
                            severity = ?issue.severity,
                            "{}",
                            issue.message,
                        ),
                        _ => warn!(
                            pack = %pack.name,
                            pack_ordinal = pack.instance_id.ordinal(),
                            pack_path = %pack.path.display(),
                            component = ?issue.component,
                            severity = ?issue.severity,
                            "{}",
                            issue.message,
                        ),
                    }
                }
            }
        }

        // SECURITY(#4842): redirects disabled at the client level -- the
        // executor's `send_with_safe_redirects` call follows and revalidates
        // each hop itself. A client that auto-follows would return an
        // already-redirected response, silently skipping that revalidation.
        let external_tools_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let tool_manifest = crate::external_tools::register_external_tools(
            &self.config.tools,
            &mut tool_registry,
            &external_tools_client,
            &sandbox_config(&self.config),
        )
        .await;
        if tool_manifest.available_count() > 0 || !self.config.tools.required.is_empty() {
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

        // WHY: `tool_schema` was registered during built-in tool setup with a
        // snapshot of only the built-ins.  Domain packs and external tools are
        // registered after that, so refresh the snapshot now that the complete
        // tool set is known.
        if tool_registry
            .get_def(&ToolName::from_static("tool_schema"))
            .is_some()
        {
            tool_registry
                .finalize_tool_schema()
                .whatever_context("failed to finalize tool_schema snapshot")?;
        }

        let tool_registry = Arc::new(tool_registry);

        // WHY (#3474): the embedding model download/load can be slow or fail.
        // Loading synchronously here blocks the HTTP gateway from binding.
        // Wrapping in `LazyEmbeddingProvider` lets the gateway start
        // immediately and defers the real init to first use.
        let embedding_provider: Arc<dyn mneme::embedding::EmbeddingProvider> = if self.embedding {
            let lazy = Arc::new(LazyEmbeddingProvider::new(self.config.embedding.clone()));
            // WHY: eager background init warms the model without blocking startup.
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

        let cross_router = Arc::new(CrossNousRouter::default());

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
                    Arc::new(tool_adapters::SignalAdapter {
                        provider: Arc::clone(p) as Arc<dyn ChannelProvider>,
                        outbound_policy: self.config.messaging.outbound.clone(),
                    }) as Arc<dyn organon::types::MessageService>
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

        #[cfg(feature = "recall")]
        let knowledge_stores = if self.embedding {
            let mut cohorts = BTreeSet::from(["shared".to_owned()]);
            for agent_def in &self.config.agents.list {
                let resolved = resolve_nous(&self.config, agent_def.id.as_str());
                cohorts.insert(resolved.episteme_cohort.to_string());
            }
            open_knowledge_stores(
                &self.oikos,
                cohorts,
                &self.config.embedding,
                &self.config.knowledge,
            )?
        } else {
            std::collections::HashMap::new()
        };
        #[cfg(feature = "recall")]
        let shared_knowledge_store = knowledge_stores.get("shared").cloned();

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

        #[cfg(feature = "recall")]
        // SECURITY(#6921): redirects disabled here on purpose --
        // `send_with_safe_redirects` inside the source drives the chain so every hop
        // is revalidated. A client that followed them itself would hand back an
        // already-redirected response and skip the checkpoint.
        let recall_source_registry = Arc::new(build_recall_source_registry(
            &self.config,
            &Arc::new(
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            ),
            &organon::sandbox::EgressGate::from_config(&sandbox_config(&self.config)),
        ));

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
        let audit_log = Arc::new(nous::audit::PromptAuditLog::from_settings(
            audit_log_dir,
            &self.config.prompt_audit,
        ));
        let after_action_store = Arc::new(AfterActionStore::new(
            self.oikos.logs().join("after-actions"),
        ));
        // WHY(#3969): ripe precursor pulled forward ahead of the Q-learning
        // router — full Q-learning stays deferred (reward signal and
        // MetricClaim are undefined primitives), but the interactive path no
        // longer needs to stay pure-static-plus-recording in the meantime.
        // `EmpiricalRouter` makes a real data-driven decision over the same
        // shared `AfterActionStore` the dispatch-side router already learns
        // from; `FallthroughRouter` defers to the unconditionally-static
        // `NoOpRouter` whenever that decision lacks (or falls below) real
        // confidence, so a cold store behaves identically to the previous
        // `RecordingRouter`-only wiring. Thresholds mirror
        // `energeia::routing::DispatchRoutingConfig`'s own defaults
        // (min_samples=5, 7-day window, confidence_threshold=0.1) — the two
        // paths sharing one store benefits from sharing one policy, not just
        // one storage backend.
        let static_model: Arc<str> = Arc::from(
            self.config
                .agents
                .defaults
                .model_defaults
                .model
                .primary
                .as_str(),
        );
        let empirical_router: Arc<dyn aletheia_routing::Router> = Arc::new(FallthroughRouter::new(
            Arc::new(EmpiricalRouter::new(
                Arc::clone(&after_action_store),
                Arc::clone(&static_model),
                5,
                DEFAULT_ROUTING_WINDOW,
                0.1,
            )),
            Arc::new(NoOpRouter {
                provider: Arc::clone(&static_model),
            }),
            0.0,
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
                    tool_config: Arc::new(self.config.tool_limits.clone()),
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
            working_checkpoint_store: Some(working_checkpoint_store),
            cross_nous,
            messenger,
            note_store,
            blackboard_store,
            spawn,
            planning,
            knowledge: knowledge_search,
            http_clients: ToolHttpClients {
                general: reqwest::Client::new(),
                ssrf_safe: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            },
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: tool_registry.lazy_tool_catalog(),
            server_tool_config: self.config.server_tools.clone().into(),
        });
        if let Some(spawn_impl) = spawn_impl.as_ref() {
            spawn_impl.set_tool_services(Arc::clone(&tool_services));
        }

        // WHY: cloned before the cohort stores move into NousManager below.
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
            self.config.tool_limits.clone(),
        )
        .with_audit_log(Arc::clone(&audit_log))
        .with_empirical_router(Arc::clone(&empirical_router))
        .with_pack_report(pack_report);

        {
            for agent_def in &self.config.agents.list {
                let (nous_config, pipeline_config) = build_nous_runtime_config(
                    &self.config,
                    &self.oikos,
                    &packs,
                    agent_def.id.as_str(),
                );
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

        let maintenance_config = build_daemon_maintenance_config(
            &self.oikos,
            &self.config,
            &after_action_store,
            &session_store,
        );
        // WHY: the receiver half is moved into the system daemon_runner below
        // (the canonical maintenance scheduler) when daemons are enabled; the
        // sender half is moved into the config-reload task further down so
        // that any config reload (PUT section, POST reload, SIGHUP) pushes a
        // freshly-built MaintenanceConfig for live task reconciliation. When
        // daemons are disabled the receiver is simply dropped and later
        // `send` calls are ignored, matching the existing config_tx pattern.
        let (maintenance_reload_tx, maintenance_reload_rx) =
            tokio::sync::watch::channel(maintenance_config.clone());
        let task_state_root = self.oikos.data().join("daemon-task-state");
        // WHY(#5142): cloned handles (not reopened stores — a second open on
        // the same fjall path collides with the runner's own lock) so
        // AppState/HealthState can read real daemon task state instead of
        // reporting it as permanently unknown.
        let mut daemon_task_state_handles: Vec<(String, oikonomos::state::TaskStateStore)> =
            Vec::new();

        // WHY(#5128): spawned unconditionally regardless of `self.daemons` --
        // disk-space monitoring backs write guards (config persistence,
        // session storage) that matter even when the daemon subsystem itself
        // is disabled (e.g. `RuntimeBuilder::minimal`, whose `daemons: false`
        // still calls `build()`; `validation_only` never reaches this point
        // at all -- `check-config` calls `.validate()`, not `.build()`).
        let disk_monitor = spawn_disk_space_monitor(
            &self.oikos,
            &self.config.maintenance.disk_space,
            &task_tracker,
            shutdown_token.child_token(),
        );

        if self.daemons {
            let runner_output_mode =
                daemon_output_mode(self.config.daemon_behavior.runner_output_mode);
            let daemon_token = shutdown_token.child_token();
            let system_state_store =
                oikonomos::state::TaskStateStore::open(&task_state_root.join("system"))
                    .with_whatever_context(|_| "failed to open system daemon task-state store")?;
            daemon_task_state_handles.push(("system".to_owned(), system_state_store.clone()));
            let mut daemon_runner = TaskRunner::new("system", daemon_token)
                .with_output_mode(runner_output_mode)
                .with_daemon_behavior(self.config.daemon_behavior.clone())
                .with_watchdog_settings(&self.config.maintenance.watchdog)
                .with_state_store(system_state_store)
                .with_maintenance(maintenance_config.clone())
                .with_maintenance_reload(maintenance_reload_rx);
            let retention_executor = Arc::new(
                crate::session_retention::SessionRetentionAdapter::new(Arc::clone(&session_store)),
            );
            daemon_runner = daemon_runner.with_retention(retention_executor);

            #[cfg(feature = "recall")]
            if let Some(ks) = knowledge_store_for_daemon.as_ref() {
                daemon_runner = daemon_runner.with_knowledge_store(Arc::clone(ks));

                // WHY (#4165 Path A): hand the embedding provider to the
                // dedup task so it can populate `entities.name_embedding`
                // before scoring. Without this, the maintenance task
                // reports merges executed but the AutoMerge threshold
                // (≥ 0.90) is structurally unreachable.
                //
                // WHY (#4165 D): build a DedupTuning from the resolved
                // AgentBehaviorDefaults so operator-tunable
                // `knowledge_dedup_*` keys actually take effect in the
                // scheduled maintenance task instead of silently using
                // hardcoded module constants.
                let dedup_tuning = crate::knowledge_maintenance::tuning_from_behavior(
                    &self.config.agents.defaults.behavior,
                );
                // WHY (#5530): wire the LLM provider into the knowledge
                // consolidation engine so the scheduled daemon task can call
                // `consolidate_knowledge` instead of leaving it dead code.
                let consolidation_provider =
                    Arc::new(crate::knowledge_maintenance::LlmConsolidationProvider::new(
                        Arc::clone(&provider_registry),
                        self.config
                            .agents
                            .defaults
                            .model_defaults
                            .model
                            .primary
                            .model
                            .clone(),
                    ));
                let km_executor = Arc::new(
                    crate::knowledge_maintenance::KnowledgeMaintenanceAdapter::new(Arc::clone(ks))
                        .with_embedding_provider(Arc::clone(&embedding_provider))
                        .with_tuning(dedup_tuning)
                        .with_consolidation_provider(consolidation_provider)
                        // WHY (#5674): the graph-cleanup cron's audit retention
                        // window is an operator knob; without this the adapter
                        // would silently prune on its own hardcoded default.
                        .with_audit_retention_days(
                            self.config
                                .maintenance
                                .cron_tasks
                                .graph_cleanup_audit_retention_days,
                        ),
                );
                daemon_runner = daemon_runner.with_knowledge_maintenance(km_executor);
            }

            #[cfg(feature = "energeia")]
            if !self.config.dispatch.cron_tasks.is_empty() {
                if let Some(services) = energeia_services.as_ref() {
                    if let Some(cron_lock_store) = services.cron_lock_store.as_ref() {
                        cron_executor::start(
                            &self.config.dispatch.cron_tasks,
                            Arc::clone(&services.orchestrator),
                            &self.oikos,
                            Arc::clone(cron_lock_store),
                            &task_tracker,
                            &shutdown_token,
                        )
                        .whatever_context("failed to start cron executor")?;
                    } else {
                        warn!(
                            cron_tasks = self.config.dispatch.cron_tasks.len(),
                            "dispatch cron tasks configured but cron lock store unavailable; recurring dispatch not started"
                        );
                    }
                } else {
                    warn!(
                        cron_tasks = self.config.dispatch.cron_tasks.len(),
                        "dispatch cron tasks configured but energeia services unavailable; recurring dispatch not started"
                    );
                }
            }
            #[cfg(not(feature = "energeia"))]
            if !self.config.dispatch.cron_tasks.is_empty() {
                warn!(
                    cron_tasks = self.config.dispatch.cron_tasks.len(),
                    "dispatch cron tasks configured but the energeia feature is not built; recurring dispatch not started"
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

        let health_poller_interval =
            Duration::from_secs(self.config.nous_behavior.manager_health_interval_secs);
        let health_poller_cancel = shutdown_token.child_token();
        let health_poller_handle = NousManager::start_health_poller(
            Arc::clone(&nous_manager),
            health_poller_interval,
            health_poller_cancel,
        );
        task_tracker.spawn(async move {
            if let Err(e) = health_poller_handle.await {
                warn!(error = %e, "nous manager health poller supervisor exited with an error");
            }
        });

        nous_manager.ready();

        let ready_rx = nous_manager.ready_rx();
        let (_channel_registry, _dispatch_handle) = start_inbound_dispatch(
            &self.config,
            &nous_manager,
            Arc::clone(&session_store),
            ready_rx,
            signal_provider.as_ref(),
            matrix_provider.as_ref(),
            &shutdown_token,
        )?;

        if self.daemons {
            let runner_output_mode =
                daemon_output_mode(self.config.daemon_behavior.runner_output_mode);
            let daemon_bridge = Arc::new(daemon_bridge::NousDaemonBridge::new(Arc::clone(
                &nous_manager,
            )));
            for agent_def in &self.config.agents.list {
                let agent_token = shutdown_token.child_token();
                let agent_state_store = oikonomos::state::TaskStateStore::open(
                    &task_state_root.join(task_state_component(agent_def.id.as_str())),
                )
                .with_whatever_context(|_| {
                    format!(
                        "failed to open daemon task-state store for {}",
                        agent_def.id
                    )
                })?;
                daemon_task_state_handles
                    .push((agent_def.id.to_string(), agent_state_store.clone()));
                let mut runner = TaskRunner::with_bridge(
                    agent_def.id.clone(),
                    agent_token,
                    daemon_bridge.clone(),
                )
                .with_output_mode(runner_output_mode)
                .with_daemon_behavior(self.config.daemon_behavior.clone())
                .with_watchdog_settings(&self.config.maintenance.watchdog)
                .with_state_store(agent_state_store)
                .with_maintenance(maintenance_config.clone());
                let prosoche = &self.config.maintenance.prosoche;
                if prosoche.mode.runs_daemon_tasks() {
                    if prosoche.heartbeat.enabled {
                        runner.register(prosoche_task_def(
                            agent_def.id.as_str(),
                            &format!("{}-prosoche", agent_def.id),
                            "Prosoche attention check",
                            prosoche.heartbeat.enabled,
                            prosoche.heartbeat.interval_secs,
                            prosoche
                                .heartbeat
                                .active_window
                                .as_ref()
                                .map(|w| (w.start_hour, w.end_hour)),
                            oikonomos::schedule::BuiltinTask::Prosoche,
                        ));
                    }
                    if prosoche.self_audit.enabled {
                        runner.register(prosoche_task_def(
                            agent_def.id.as_str(),
                            &format!("{}-prosoche-self-audit", agent_def.id),
                            "Prosoche self-audit",
                            prosoche.self_audit.enabled,
                            prosoche.self_audit.interval_secs,
                            prosoche
                                .self_audit
                                .active_window
                                .as_ref()
                                .map(|w| (w.start_hour, w.end_hour)),
                            oikonomos::schedule::BuiltinTask::SelfAudit,
                        ));
                    }
                }
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
        // WHY these two are cloned into the reload task: the maintenance
        // config carries runtime handles that `maintenance::build_config`
        // cannot reconstruct on its own, so a reload must rebuild it through
        // the same `build_daemon_maintenance_config` startup uses. Rebuilding
        // it differently here is exactly the drift that helper exists to
        // prevent — most sharply for `after_action_store`, where the plain
        // builder would hand the scheduler a brand-new empty store.
        let reload_after_action_store = Arc::clone(&after_action_store);
        let reload_session_store = Arc::clone(&session_store);
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
                                agent.id.as_str(),
                            );
                            (agent.id.to_string(), nous_config, pipeline_config)
                        })
                        .collect();
                    if let Err(e) = reload_manager
                        .reload_actor_configs(actor_configs, config.tool_limits.clone())
                        .await
                    {
                        warn!(error = %e, "failed to apply hot-reloaded actor config");
                    }

                    // WHY(#5144): this send is what makes the maintenance
                    // settings genuinely hot-reloadable rather than merely
                    // classified as such. The system daemon runner holds the
                    // receiver and reconciles live scheduler tasks from it;
                    // without this the runner listens and nothing ever speaks.
                    // A send error means daemons are disabled so the receiver
                    // was dropped, which is expected, not a fault.
                    let reloaded_maintenance = build_daemon_maintenance_config(
                        &reload_oikos,
                        &config,
                        &reload_after_action_store,
                        &reload_session_store,
                    );
                    if maintenance_reload_tx.send(reloaded_maintenance).is_err() {
                        tracing::debug!(
                            "no maintenance reload receiver; daemons are disabled for this instance"
                        );
                    }
                }
            }
            .instrument(tracing::info_span!("config_reload_actor_sync")),
        );
        let workspace_root = pylon::state::resolve_workspace_root(
            &self.oikos,
            self.config.workspace.root.as_deref(),
        );
        let credential_runtime =
            Arc::new(pylon::credential_runtime::CredentialRuntimeManager::new(
                Arc::clone(&provider_registry),
            ));
        let turn_buffer_completed_ttl =
            std::time::Duration::from_secs(self.config.api_limits.turn_buffer_completed_ttl_secs);
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::clone(&nous_manager),
            provider_registry,
            tool_registry,
            oikos: self.oikos,
            workspace_root,
            jwt_manager: Arc::new(jwt_manager),
            auth_facade: Arc::new(auth_facade),
            credential_runtime,
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
            disk_monitor,
            turn_buffer_registry: Arc::new(pylon::turn_buffer::TurnBufferRegistry::with_limits(
                turn_buffer_completed_ttl,
                self.config.api_limits.turn_buffer_max_events_per_turn,
            )),
            metrics_registry,
            event_bus: Arc::new(pylon::event_bus::EventBus::new(256)),
            approval_registry: Arc::new(pylon::approval_registry::ApprovalRegistry::new()),
            // WHY(#5322): use the explicit metrics exposition config; default is
            // local-only so non-loopback binds do not expose operational metrics
            // without an operator opt-in.
            metrics_mode: self.config.gateway.metrics.mode,
            metrics_detailed: self.config.gateway.metrics.detailed,
            daemon_task_states: Arc::new(daemon_task_state_handles),
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
use nous_config::{build_nous_runtime_config, overlay_policy_from_config};

mod setup;
mod tool_adapters;

#[cfg(feature = "energeia")]
mod cron_executor;

#[cfg(feature = "recall")]
use setup::open_knowledge_stores;
use setup::{
    LazyEmbeddingProvider, build_matrix_provider, build_provider_registry, build_signal_provider,
    build_tool_registry, sandbox_config, start_inbound_dispatch,
};

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex};

    use taxis::config::AletheiaConfig;
    use taxis::oikos::Oikos;
    use tempfile::TempDir;
    use thesauros::loader::load_packs;

    use super::{
        build_recall_source_registry, prosoche_task_def, resolve_pack_paths, sandbox_config,
    };

    static CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn move_to(path: &std::path::Path) -> Self {
            let original = std::env::current_dir().expect("read current working directory");
            std::env::set_current_dir(path).expect("set test working directory");
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).expect("restore working directory");
        }
    }

    fn write_pack(dir: &std::path::Path, files: &[(&str, &str)]) {
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create pack parent directory");
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "test writes synthetic pack files on disk"
            )]
            fs::write(&path, content).expect("write pack file");
        }
    }

    #[test]
    fn relative_pack_paths_resolve_against_instance_root() {
        let _cwd_lock = CWD_LOCK.lock().expect("lock cwd mutation");
        let instance = TempDir::new().expect("instance temp directory");
        let pack_dir = instance.path().join("packs").join("my-pack");
        fs::create_dir_all(&pack_dir).expect("create pack directory");
        write_pack(
            &pack_dir,
            &[("pack.toml", "name = \"cwd-test\"\nversion = \"1.0\"\n")],
        );

        let unrelated = TempDir::new().expect("unrelated temp directory");
        let _cwd_guard = CwdGuard::move_to(unrelated.path());

        let oikos = Oikos::from_root(instance.path());
        let resolved = resolve_pack_paths(&oikos, &[PathBuf::from("packs/my-pack")]);
        let packs = load_packs(&resolved);

        assert_eq!(
            packs.len(),
            1,
            "relative pack path should resolve from instance root regardless of process cwd"
        );
        let pack = packs.first().expect("one pack loaded");
        assert_eq!(pack.name(), "cwd-test");
        assert_eq!(
            pack.root(),
            pack_dir,
            "loaded pack root should be the resolved absolute path"
        );
    }

    #[test]
    fn absolute_pack_paths_are_used_directly() {
        let instance = TempDir::new().expect("instance temp directory");
        let pack_dir = instance.path().join("external-pack");
        fs::create_dir_all(&pack_dir).expect("create pack directory");
        write_pack(
            &pack_dir,
            &[("pack.toml", "name = \"absolute-test\"\nversion = \"1.0\"\n")],
        );

        let oikos = Oikos::from_root(instance.path());
        let resolved = resolve_pack_paths(&oikos, std::slice::from_ref(&pack_dir));
        let packs = load_packs(&resolved);

        assert_eq!(
            packs.len(),
            1,
            "absolute pack path should be used without root resolution"
        );
        let pack = packs.first().expect("one pack loaded");
        assert_eq!(pack.name(), "absolute-test");
        assert_eq!(pack.root(), pack_dir);
    }

    #[test]
    fn prosoche_task_def_uses_config_values() {
        let def = prosoche_task_def(
            "alice",
            "alice-prosoche",
            "Prosoche attention check",
            true,
            42,
            Some((8, 23)),
            oikonomos::schedule::BuiltinTask::Prosoche,
        );

        assert_eq!(def.id, "alice-prosoche");
        assert_eq!(def.name, "Prosoche attention check");
        assert_eq!(def.nous_id, "alice");
        assert!(def.enabled);
        assert!(!def.catch_up);
        assert_eq!(def.active_window, Some((8, 23)));
        match def.schedule {
            oikonomos::schedule::Schedule::Interval(d) => {
                assert_eq!(d, std::time::Duration::from_secs(42));
            }
            other => panic!("expected Interval schedule, got {other:?}"),
        }
        match def.action {
            oikonomos::schedule::TaskAction::Builtin(
                oikonomos::schedule::BuiltinTask::Prosoche,
            ) => {}
            other => panic!("expected Prosoche builtin, got {other:?}"),
        }
    }

    #[test]
    fn prosoche_task_def_honors_disabled_flag_and_self_audit_action() {
        let def = prosoche_task_def(
            "bob",
            "bob-prosoche-self-audit",
            "Prosoche self-audit",
            false,
            3600,
            None,
            oikonomos::schedule::BuiltinTask::SelfAudit,
        );

        assert!(!def.enabled);
        assert_eq!(def.active_window, None);
        match def.action {
            oikonomos::schedule::TaskAction::Builtin(
                oikonomos::schedule::BuiltinTask::SelfAudit,
            ) => {}
            other => panic!("expected SelfAudit builtin, got {other:?}"),
        }
    }

    /// WHY: reqwest requires a rustls `CryptoProvider` installed process-wide
    /// before building a `Client`. Production installs it in `main()`; tests
    /// must do so themselves.
    #[cfg(feature = "recall")]
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// SECURITY(#6444): a default config -- no `[recall_sources]` section at
    /// all -- must register no network-backed recall source. Before the fix,
    /// `AcademicSource` was registered unconditionally, so this test fails
    /// (`source_count` == 2) without the opt-in gate.
    #[cfg(feature = "recall")]
    #[test]
    fn default_config_registers_no_network_recall_source() {
        ensure_crypto_provider();
        let config = AletheiaConfig::default();
        assert!(
            !config.recall_sources.academic.enabled,
            "default RecallSourcesConfig must be disabled"
        );

        let registry = build_recall_source_registry(
            &config,
            &std::sync::Arc::new(reqwest::Client::new()),
            &organon::sandbox::EgressGate::from_config(&sandbox_config(&config)),
        );

        // llm_context is local (no network); academic is the only
        // network-backed source and must be absent from the count.
        assert_eq!(
            registry.source_count(),
            1,
            "a default instance must register only the local llm_context source"
        );
    }

    /// SECURITY(#6444): explicit opt-in is the only path to registering the
    /// network-backed source.
    #[cfg(feature = "recall")]
    #[test]
    fn explicit_opt_in_registers_academic_source() {
        ensure_crypto_provider();
        let mut config = AletheiaConfig::default();
        config.recall_sources.academic.enabled = true;

        let registry = build_recall_source_registry(
            &config,
            &std::sync::Arc::new(reqwest::Client::new()),
            &organon::sandbox::EgressGate::from_config(&sandbox_config(&config)),
        );

        assert_eq!(
            registry.source_count(),
            2,
            "an explicit opt-in must register both the local and network sources"
        );
    }
}
