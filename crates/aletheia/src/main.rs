//! Aletheia cognitive agent runtime — binary entrypoint.

mod daemon_bridge;
mod dispatch;
mod status;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
use aletheia_koina::credential::CredentialProvider;
use aletheia_mneme::embedding::{EmbeddingConfig, EmbeddingProvider, create_provider};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::cross::CrossNousRouter;
use aletheia_nous::manager::NousManager;
use aletheia_oikonomos::maintenance::{
    DbMonitor, DbMonitoringConfig, DriftDetectionConfig, DriftDetector, MaintenanceConfig,
    TraceRotationConfig, TraceRotator,
};
use aletheia_oikonomos::runner::TaskRunner;
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
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

#[derive(Debug, Parser)]
#[command(name = "aletheia", about = "Cognitive agent runtime")]
struct Cli {
    /// Path to instance root directory
    #[arg(short = 'r', long)]
    instance_root: Option<PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Bind address
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port
    #[arg(short, long, default_value_t = 18789)]
    port: u16,

    /// Emit JSON-structured logs
    #[arg(long)]
    json_logs: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Check if the server is running
    Health {
        /// Server URL to check
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        url: String,
    },
    /// Manage database backups
    Backup {
        /// List available backups
        #[arg(long)]
        list: bool,
        /// Prune old backups
        #[arg(long)]
        prune: bool,
        /// Number of backups to keep when pruning
        #[arg(long, default_value_t = 5)]
        keep: usize,
        /// Export sessions as JSON
        #[arg(long)]
        export_json: bool,
    },
    /// Instance maintenance tasks
    Maintenance {
        #[command(subcommand)]
        action: MaintenanceAction,
    },
    /// TLS certificate management
    Tls {
        #[command(subcommand)]
        action: TlsAction,
    },
    /// Show system status
    Status {
        /// Server URL to check
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        url: String,
    },
    /// Credential management
    Credential {
        #[command(subcommand)]
        action: CredentialAction,
    },
    /// Run behavioral evaluation scenarios against a live instance
    Eval {
        /// Server URL to evaluate
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        url: String,
        /// Bearer token for authenticated endpoints
        #[arg(long, env = "ALETHEIA_EVAL_TOKEN")]
        token: Option<String>,
        /// Filter scenarios by ID substring
        #[arg(long)]
        scenario: Option<String>,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Per-scenario timeout in seconds
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Export an agent to a portable .agent.json file
    Export {
        /// Agent (nous) ID to export
        nous_id: String,
        /// Output file path (default: <nous-id>-<date>.agent.json)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Include archived/distilled sessions
        #[arg(long)]
        archived: bool,
        /// Max messages per session (0 = all)
        #[arg(long, default_value_t = 500)]
        max_messages: usize,
        /// Compact JSON (no pretty printing)
        #[arg(long)]
        compact: bool,
    },
    /// Import an agent from a portable .agent.json file
    Import {
        /// Path to .agent.json file
        file: PathBuf,
        /// Override the target agent ID
        #[arg(long)]
        target_id: Option<String>,
        /// Skip importing session history
        #[arg(long)]
        skip_sessions: bool,
        /// Skip restoring workspace files
        #[arg(long)]
        skip_workspace: bool,
        /// Overwrite existing workspace files
        #[arg(long)]
        force: bool,
        /// Show what would be imported without making changes
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum CredentialAction {
    /// Show current credential source, expiry, and token prefix
    Status,
    /// Force-refresh OAuth token now
    Refresh,
}

#[derive(Debug, Clone, Subcommand)]
enum TlsAction {
    /// Generate self-signed certificates for development/LAN use
    Generate {
        /// Output directory for cert and key files
        #[arg(long, default_value = "instance/config/tls")]
        output_dir: PathBuf,
        /// Certificate validity in days
        #[arg(long, default_value_t = 365)]
        days: u32,
        /// Subject Alternative Names (hostnames/IPs)
        #[arg(long, default_values_t = vec!["localhost".to_owned(), "127.0.0.1".to_owned()])]
        san: Vec<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum MaintenanceAction {
    /// Show status of all maintenance tasks
    Status,
    /// Run a specific maintenance task immediately
    Run {
        /// Task name: trace-rotation, drift-detection, db-monitor, or all
        task: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Health { url }) => return health(url).await,
        Some(Command::Backup {
            list,
            prune,
            keep,
            export_json,
        }) => return backup(&cli, *list, *prune, *keep, *export_json),
        Some(Command::Maintenance { action }) => {
            return run_maintenance(action.clone(), cli.instance_root.as_ref());
        }
        Some(Command::Tls { action }) => return handle_tls(action),
        Some(Command::Status { url }) => return status::run(url, cli.instance_root.as_ref()).await,
        Some(Command::Credential { action }) => {
            return handle_credential(action.clone(), cli.instance_root.as_ref()).await;
        }
        Some(Command::Eval {
            url,
            token,
            scenario,
            json,
            timeout,
        }) => return eval(url, token.clone(), scenario.clone(), *json, *timeout).await,
        Some(Command::Export {
            nous_id,
            output,
            archived,
            max_messages,
            compact,
        }) => return export_agent_cmd(&cli, nous_id, output.as_ref(), *archived, *max_messages, *compact),
        Some(Command::Import {
            file,
            target_id,
            skip_sessions,
            skip_workspace,
            force,
            dry_run,
        }) => return import_agent_cmd(&cli, file, target_id.as_deref(), *skip_sessions, *skip_workspace, *force, *dry_run),
        None => {}
    }

    serve(cli).await
}

fn run_maintenance(action: MaintenanceAction, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).context("failed to load config")?;
    let maint = build_maintenance_config(&oikos, &config.maintenance);

    match action {
        MaintenanceAction::Status => {
            let (_tx, rx) = tokio::sync::watch::channel(false);
            let mut runner = TaskRunner::new("system", rx).with_maintenance(maint);
            runner.register_maintenance_tasks();
            let statuses = runner.status();
            println!("{}", serde_json::to_string_pretty(&statuses)?);
        }
        MaintenanceAction::Run { task } => {
            let tasks: Vec<&str> = if task == "all" {
                vec!["trace-rotation", "drift-detection", "db-monitor"]
            } else {
                vec![task.as_str()]
            };
            for name in tasks {
                match name {
                    "trace-rotation" => {
                        let report = TraceRotator::new(maint.trace_rotation.clone())
                            .rotate()
                            .context("trace rotation failed")?;
                        println!(
                            "trace-rotation: {} rotated, {} pruned, {} bytes freed",
                            report.files_rotated, report.files_pruned, report.bytes_freed
                        );
                    }
                    "drift-detection" => {
                        let report = DriftDetector::new(maint.drift_detection.clone())
                            .check()
                            .context("drift detection failed")?;
                        println!(
                            "drift-detection: {} missing, {} extra",
                            report.missing_files.len(),
                            report.extra_files.len()
                        );
                    }
                    "db-monitor" => {
                        let report = DbMonitor::new(maint.db_monitoring.clone())
                            .check()
                            .context("db monitor failed")?;
                        for db in &report.databases {
                            println!(
                                "db-monitor: {} {}MB ({})",
                                db.name,
                                db.size_bytes / (1024 * 1024),
                                db.status
                            );
                        }
                    }
                    other => anyhow::bail!(
                        "unknown task: {other}. Valid: trace-rotation, drift-detection, db-monitor, all"
                    ),
                }
            }
        }
    }
    Ok(())
}

fn build_maintenance_config(
    oikos: &Oikos,
    settings: &aletheia_taxis::config::MaintenanceSettings,
) -> MaintenanceConfig {
    MaintenanceConfig {
        trace_rotation: TraceRotationConfig {
            enabled: settings.trace_rotation.enabled,
            trace_dir: oikos.traces(),
            archive_dir: oikos.trace_archive(),
            max_age_days: settings.trace_rotation.max_age_days,
            max_total_size_mb: settings.trace_rotation.max_total_size_mb,
            compress: settings.trace_rotation.compress,
            max_archives: settings.trace_rotation.max_archives,
        },
        drift_detection: DriftDetectionConfig {
            enabled: settings.drift_detection.enabled,
            instance_root: oikos.root().to_path_buf(),
            example_root: PathBuf::from("instance.example"),
            alert_on_missing: settings.drift_detection.alert_on_missing,
            ignore_patterns: settings.drift_detection.ignore_patterns.clone(),
        },
        db_monitoring: DbMonitoringConfig {
            enabled: settings.db_monitoring.enabled,
            data_dir: oikos.data(),
            warn_threshold_mb: settings.db_monitoring.warn_threshold_mb,
            alert_threshold_mb: settings.db_monitoring.alert_threshold_mb,
        },
        retention: aletheia_oikonomos::maintenance::RetentionConfig {
            enabled: settings.retention.enabled,
        },
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "binary entrypoint — sequential init steps"
)]
async fn serve(cli: Cli) -> Result<()> {
    init_tracing(&cli.log_level, cli.json_logs);

    info!("aletheia starting");

    // Oikos — instance directory resolution
    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    info!(root = %oikos.root().display(), "instance discovered");

    // Config cascade: defaults → YAML → env
    let config = load_config(&oikos).context("failed to load config")?;
    info!(
        port = config.gateway.port,
        agents = config.agents.list.len(),
        "config loaded"
    );

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

    // Spawn nous actors
    // vector_search is None until Phase 2 (prompt 28) lands KnowledgeVectorSearch
    let mut nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos_arc),
        Some(embedding_provider),
        None,
        Some(Arc::clone(&session_store)),
        Arc::clone(&packs),
        Some(Arc::clone(&cross_router)),
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
                model: resolved.model,
                context_window: resolved.context_tokens,
                max_output_tokens: resolved.max_output_tokens,
                bootstrap_max_tokens: resolved.bootstrap_max_tokens,
                thinking_enabled: resolved.thinking_enabled,
                thinking_budget: resolved.thinking_budget,
                max_tool_iterations: resolved.max_tool_iterations,
                loop_detection_threshold: 3,
                domains,
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
    let (_daemon_shutdown_tx, daemon_shutdown_rx) = tokio::sync::watch::channel(false);
    let maintenance_config = build_maintenance_config(&oikos_arc, &config.maintenance);
    let mut daemon_runner =
        TaskRunner::new("system", daemon_shutdown_rx).with_maintenance(maintenance_config);
    daemon_runner.register_maintenance_tasks();
    let daemon_handle = tokio::spawn(async move {
        daemon_runner.run().await;
    });
    info!("daemon started");

    // Wrap in Arc — shared between dispatcher and AppState
    let nous_manager = Arc::new(nous_manager);

    // Signal ready — all actors spawned, safe to accept inbound messages
    nous_manager.ready();

    // Channel registry + inbound dispatch (gated on ready signal)
    let ready_rx = nous_manager.ready_rx();
    let (_channel_registry, _dispatch_handle) =
        start_inbound_dispatch(&config, &nous_manager, ready_rx);

    // Daemon runners — per-agent background task scheduling
    let (daemon_shutdown_tx, _) = tokio::sync::watch::channel(false);
    let daemon_bridge = Arc::new(daemon_bridge::NousDaemonBridge::new(Arc::clone(
        &nous_manager,
    )));
    for agent_def in &config.agents.list {
        let mut runner = aletheia_oikonomos::runner::TaskRunner::with_bridge(
            agent_def.id.clone(),
            daemon_shutdown_tx.subscribe(),
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
        });
        let span = tracing::info_span!("daemon", nous.id = %agent_def.id);
        tokio::spawn(
            async move {
                runner.run().await;
            }
            .instrument(span),
        );
    }
    if !config.agents.list.is_empty() {
        info!(count = config.agents.list.len(), "daemon runners spawned");
    }

    // Pylon HTTP gateway — shares registries with NousManager
    let state = Arc::new(AppState {
        session_store,
        nous_manager: Arc::clone(&nous_manager),
        provider_registry,
        tool_registry,
        oikos: oikos_arc,
        jwt_manager: Arc::new(jwt_manager),
        start_time: Instant::now(),
    });

    let security = aletheia_pylon::security::SecurityConfig::from_gateway(&config.gateway);
    let app = build_router(state.clone(), &security);

    let bind_addr = format!("{}:{}", cli.bind, cli.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind to {bind_addr}"))?;

    info!(addr = %bind_addr, "pylon listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    info!("shutting down");
    let _ = daemon_shutdown_tx.send(true);
    let _ = daemon_handle.await;
    state.nous_manager.shutdown_readonly().await;
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

    chain.push(Box::new(EnvCredentialProvider::new("ANTHROPIC_API_KEY")));

    let credential_chain: Arc<dyn CredentialProvider> = Arc::new(CredentialChain::new(chain));

    // Resolve once at startup for logging
    if let Some(cred) = credential_chain.get_credential() {
        info!(source = %cred.source, "credential resolved");
    } else {
        warn!("no credential available — no LLM provider");
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

async fn handle_credential(
    action: CredentialAction,
    instance_root: Option<&PathBuf>,
) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let cred_path = oikos.credentials().join("anthropic.json");

    match action {
        CredentialAction::Status => {
            match CredentialFile::load(&cred_path) {
                Some(cred) => {
                    let token_preview = if cred.token.len() > 10 {
                        format!(
                            "{}...{}",
                            &cred.token[..10],
                            &cred.token[cred.token.len() - 3..]
                        )
                    } else {
                        "***".to_owned()
                    };
                    let cred_type = if cred.has_refresh_token() {
                        "OAuth (auto-refresh)"
                    } else {
                        "static API key"
                    };
                    println!("Source:        file ({})", cred_path.display());
                    println!("Type:          {cred_type}");
                    println!("Token:         {token_preview}");
                    if let Some(remaining) = cred.seconds_remaining() {
                        let hours = remaining / 3600;
                        let mins = (remaining % 3600) / 60;
                        if remaining > 0 {
                            println!("Expires:       {hours}h {mins}m remaining");
                        } else {
                            println!("Expires:       EXPIRED");
                        }
                    } else {
                        println!("Expires:       no expiry set");
                    }
                    println!(
                        "Refresh token: {}",
                        if cred.has_refresh_token() {
                            "present"
                        } else {
                            "absent"
                        }
                    );
                }
                None => {
                    // Check env var fallback
                    match std::env::var("ANTHROPIC_API_KEY") {
                        Ok(key) if !key.is_empty() => {
                            let preview = if key.len() > 10 {
                                format!("{}...{}", &key[..10], &key[key.len() - 3..])
                            } else {
                                "***".to_owned()
                            };
                            println!("Source:        environment (ANTHROPIC_API_KEY)");
                            println!("Type:          static API key");
                            println!("Token:         {preview}");
                        }
                        _ => {
                            println!("No credential found.");
                            println!("Checked: {} (not found)", cred_path.display());
                            println!("Checked: ANTHROPIC_API_KEY (not set)");
                        }
                    }
                }
            }
        }
        CredentialAction::Refresh => {
            println!("Refreshing OAuth token...");
            match aletheia_symbolon::credential::force_refresh(&cred_path).await {
                Ok(updated) => {
                    if let Some(remaining) = updated.seconds_remaining() {
                        println!(
                            "Token refreshed — expires in {}h {}m",
                            remaining / 3600,
                            (remaining % 3600) / 60
                        );
                    } else {
                        println!("Token refreshed");
                    }
                }
                Err(e) => anyhow::bail!("refresh failed: {e}"),
            }
        }
    }
    Ok(())
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
) -> (Arc<ChannelRegistry>, Option<tokio::task::JoinHandle<()>>) {
    let mut channel_registry = ChannelRegistry::new();

    let signal_provider = build_signal_provider(&config.channels.signal);
    if let Some(ref provider) = signal_provider {
        channel_registry
            .register(Arc::clone(provider) as Arc<dyn ChannelProvider>)
            .expect("register signal provider");
    }
    let channel_registry = Arc::new(channel_registry);

    let handle = if let Some(ref provider) = signal_provider {
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

fn build_signal_provider(
    signal_config: &aletheia_taxis::config::SignalConfig,
) -> Option<Arc<SignalProvider>> {
    if !signal_config.enabled {
        info!("signal channel disabled");
        return None;
    }

    if signal_config.accounts.is_empty() {
        warn!("signal enabled but no accounts configured");
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

fn init_tracing(log_level: &str, json: bool) {
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

fn handle_tls(action: &TlsAction) -> Result<()> {
    match action {
        TlsAction::Generate {
            output_dir,
            days,
            san,
        } => generate_tls_certs(output_dir, *days, san),
    }
}

fn generate_tls_certs(output_dir: &Path, days: u32, sans: &[String]) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    let subject_alt_names: Vec<String> = sans.to_vec();
    let key_pair = rcgen::KeyPair::generate().context("failed to generate key pair")?;
    let mut params = rcgen::CertificateParams::new(subject_alt_names)
        .context("failed to build certificate params")?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Aletheia Dev");
    params.not_after = rcgen::date_time_ymd(2030, 1, 1);

    // Override validity if days is reasonable
    if days < 3650 {
        let now = time::OffsetDateTime::now_utc();
        let end = now + time::Duration::days(i64::from(days));
        params.not_before = now;
        params.not_after = end;
    }

    let cert = params
        .self_signed(&key_pair)
        .context("failed to generate self-signed certificate")?;

    let cert_path = output_dir.join("cert.pem");
    let key_path = output_dir.join("key.pem");

    std::fs::write(&cert_path, cert.pem())
        .with_context(|| format!("failed to write {}", cert_path.display()))?;
    std::fs::write(&key_path, key_pair.serialize_pem())
        .with_context(|| format!("failed to write {}", key_path.display()))?;

    println!("Certificate: {}", cert_path.display());
    println!("Private key: {}", key_path.display());
    println!("Valid for {days} days");

    Ok(())
}

fn backup(cli: &Cli, list: bool, prune: bool, keep: usize, export_json: bool) -> Result<()> {
    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let backup_dir = oikos.backups();
    let manager = aletheia_mneme::backup::BackupManager::new(store.conn(), &backup_dir);

    if list {
        let backups = manager.list_backups().context("failed to list backups")?;
        if backups.is_empty() {
            println!("No backups found.");
        } else {
            for b in &backups {
                println!("{} ({} bytes)", b.filename, b.size_bytes);
            }
        }
        return Ok(());
    }

    if prune {
        let removed = manager
            .prune_backups(keep)
            .context("failed to prune backups")?;
        println!("Pruned {removed} backup(s), kept {keep}.");
        return Ok(());
    }

    if export_json {
        let export_dir = oikos.archive().join("sessions");
        let result = manager
            .export_sessions_json(&export_dir)
            .context("failed to export sessions")?;
        println!(
            "Exported {} session(s) to {}",
            result.sessions_exported,
            result.output_dir.display()
        );
        return Ok(());
    }

    // Default: create a backup
    let result = manager.create_backup().context("failed to create backup")?;
    println!(
        "Backup created: {} ({} bytes, {} sessions, {} messages)",
        result.path.display(),
        result.size_bytes,
        result.sessions_count,
        result.messages_count,
    );

    Ok(())
}

async fn eval(
    url: &str,
    token: Option<String>,
    filter: Option<String>,
    json_output: bool,
    timeout: u64,
) -> Result<()> {
    let config = aletheia_dokimion::runner::RunConfig {
        base_url: url.to_owned(),
        token,
        filter,
        fail_fast: false,
        timeout_secs: timeout,
        json_output,
    };
    let runner = aletheia_dokimion::runner::ScenarioRunner::new(config);
    let report = runner.run().await;

    if json_output {
        aletheia_dokimion::report::print_report_json(&report);
    } else {
        aletheia_dokimion::report::print_report(&report, url);
    }

    if report.failed > 0 {
        anyhow::bail!("{} scenario(s) failed", report.failed);
    }
    Ok(())
}

async fn health(url: &str) -> Result<()> {
    let endpoint = format!("{url}/api/health");
    let resp = reqwest::get(&endpoint)
        .await
        .with_context(|| format!("failed to connect to {endpoint}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse health response")?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if !status.is_success() {
        anyhow::bail!("health check returned {status}");
    }
    Ok(())
}

fn export_agent_cmd(
    cli: &Cli,
    nous_id: &str,
    output: Option<&PathBuf>,
    archived: bool,
    max_messages: usize,
    compact: bool,
) -> Result<()> {
    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).context("failed to load config")?;
    let resolved = resolve_nous(&config, nous_id);

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let workspace_path = oikos.nous_dir(nous_id);

    let agent_config = config
        .agents
        .list
        .iter()
        .find(|a| a.id == nous_id)
        .map(|a| serde_json::to_value(a).unwrap_or_default())
        .unwrap_or_default();

    let opts = aletheia_mneme::export::ExportOptions {
        max_messages_per_session: max_messages,
        include_archived: archived,
    };
    let agent_file = aletheia_mneme::export::export_agent(
        nous_id,
        resolved.name.as_deref(),
        Some(&resolved.model),
        agent_config,
        &store,
        &workspace_path,
        &opts,
    )
    .context("export failed")?;

    let output_path = output.cloned().unwrap_or_else(|| {
        let date = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
        PathBuf::from(format!("{nous_id}-{date}.agent.json"))
    });

    let json = if compact {
        serde_json::to_string(&agent_file)?
    } else {
        serde_json::to_string_pretty(&agent_file)?
    };
    std::fs::write(&output_path, &json)
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    println!("Exported to: {}", output_path.display());
    println!("Size: {:.1} KB", json.len() as f64 / 1024.0);
    println!(
        "Sessions: {}, Workspace: {} text / {} binary",
        agent_file.sessions.len(),
        agent_file.workspace.files.len(),
        agent_file.workspace.binary_files.len()
    );

    Ok(())
}

#[expect(clippy::fn_params_excessive_bools, reason = "CLI flag passthrough")]
fn import_agent_cmd(
    cli: &Cli,
    file: &Path,
    target_id: Option<&str>,
    skip_sessions: bool,
    skip_workspace: bool,
    force: bool,
    dry_run: bool,
) -> Result<()> {
    let json = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let agent_file: aletheia_mneme::portability::AgentFile =
        serde_json::from_str(&json).context("failed to parse agent file")?;

    let nous_id = target_id.unwrap_or(&agent_file.nous.id);

    if dry_run {
        println!("Dry run — no changes will be made\n");
        println!("Agent: {} ({})", nous_id, agent_file.nous.name.as_deref().unwrap_or("unnamed"));
        println!("Generator: {}", agent_file.generator);
        println!("Exported at: {}", agent_file.exported_at);
        println!(
            "Workspace: {} text files, {} binary files",
            agent_file.workspace.files.len(),
            agent_file.workspace.binary_files.len()
        );
        println!("Sessions: {}", agent_file.sessions.len());
        let total_msgs: usize = agent_file.sessions.iter().map(|s| s.messages.len()).sum();
        let total_notes: usize = agent_file.sessions.iter().map(|s| s.notes.len()).sum();
        println!("Messages: {total_msgs}");
        println!("Notes: {total_notes}");
        if let Some(ref memory) = agent_file.memory {
            let vectors = memory.vectors.as_ref().map_or(0, Vec::len);
            let graph = memory.graph.is_some();
            println!("Memory: {vectors} vectors, graph={graph}");
        }
        return Ok(());
    }

    let oikos = match &cli.instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let db_path = oikos.sessions_db();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create data dir {}", parent.display()))?;
    }
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let workspace_path = oikos.nous_dir(nous_id);
    std::fs::create_dir_all(&workspace_path)
        .with_context(|| format!("failed to create workspace {}", workspace_path.display()))?;

    let opts = aletheia_mneme::import::ImportOptions {
        skip_sessions,
        skip_workspace,
        target_nous_id: target_id.map(String::from),
        force,
    };
    let id_gen = || ulid::Ulid::new().to_string();
    let result = aletheia_mneme::import::import_agent(
        &agent_file,
        &store,
        &workspace_path,
        &id_gen,
        &opts,
    )
    .context("import failed")?;

    println!("Imported agent: {}", result.nous_id);
    println!("Files restored: {}", result.files_restored);
    println!("Sessions: {}", result.sessions_imported);
    println!("Messages: {}", result.messages_imported);
    println!("Notes: {}", result.notes_imported);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_help_works() {
        let result = Cli::try_parse_from(["aletheia", "--help"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_defaults() {
        let cli = Cli::parse_from(["aletheia"]);
        assert_eq!(cli.port, 18789);
        assert_eq!(cli.bind, "127.0.0.1");
        assert_eq!(cli.log_level, "info");
        assert!(!cli.json_logs);
        assert!(cli.command.is_none());
    }

    #[test]
    fn health_subcommand_parses() {
        let cli = Cli::parse_from(["aletheia", "health", "--url", "http://localhost:9999"]);
        assert!(matches!(cli.command, Some(Command::Health { .. })));
    }

    #[test]
    fn maintenance_status_parses() {
        let cli = Cli::parse_from(["aletheia", "maintenance", "status"]);
        assert!(matches!(
            cli.command,
            Some(Command::Maintenance {
                action: MaintenanceAction::Status
            })
        ));
    }

    #[test]
    fn maintenance_run_parses() {
        let cli = Cli::parse_from(["aletheia", "maintenance", "run", "trace-rotation"]);
        assert!(matches!(
            cli.command,
            Some(Command::Maintenance {
                action: MaintenanceAction::Run { .. }
            })
        ));
    }

    #[test]
    fn status_subcommand_parses() {
        let cli = Cli::parse_from(["aletheia", "status"]);
        assert!(matches!(cli.command, Some(Command::Status { .. })));
    }

    #[test]
    fn status_custom_url_parses() {
        let cli = Cli::parse_from(["aletheia", "status", "--url", "http://example:9999"]);
        match cli.command {
            Some(Command::Status { url }) => assert_eq!(url, "http://example:9999"),
            _ => panic!("expected Status command"),
        }
    }

    #[test]
    fn eval_subcommand_parses() {
        let cli = Cli::parse_from(["aletheia", "eval"]);
        assert!(matches!(cli.command, Some(Command::Eval { .. })));
    }

    #[test]
    fn eval_with_options_parses() {
        let cli = Cli::parse_from([
            "aletheia",
            "eval",
            "--url",
            "http://example:9999",
            "--token",
            "my-jwt-token",
            "--scenario",
            "health",
            "--json",
            "--timeout",
            "60",
        ]);
        match cli.command {
            Some(Command::Eval {
                url,
                token,
                scenario,
                json,
                timeout,
            }) => {
                assert_eq!(url, "http://example:9999");
                assert_eq!(token.as_deref(), Some("my-jwt-token"));
                assert_eq!(scenario.as_deref(), Some("health"));
                assert!(json);
                assert_eq!(timeout, 60);
            }
            _ => panic!("expected Eval command"),

    fn export_subcommand_parses() {
        let cli = Cli::parse_from(["aletheia", "export", "syn", "--archived", "--compact"]);
        match cli.command {
            Some(Command::Export {
                nous_id,
                archived,
                compact,
                max_messages,
                ..
            }) => {
                assert_eq!(nous_id, "syn");
                assert!(archived);
                assert!(compact);
                assert_eq!(max_messages, 500);
            }
            _ => panic!("expected Export command"),
        }
    }

    #[test]
    fn export_with_output_parses() {
        let cli = Cli::parse_from([
            "aletheia",
            "export",
            "demiurge",
            "-o",
            "/tmp/backup.agent.json",
            "--max-messages",
            "100",
        ]);
        match cli.command {
            Some(Command::Export {
                nous_id,
                output,
                max_messages,
                ..
            }) => {
                assert_eq!(nous_id, "demiurge");
                assert_eq!(output.unwrap(), PathBuf::from("/tmp/backup.agent.json"));
                assert_eq!(max_messages, 100);
            }
            _ => panic!("expected Export command"),
        }
    }

    #[test]
    fn import_subcommand_parses() {
        let cli = Cli::parse_from([
            "aletheia",
            "import",
            "agent.json",
            "--target-id",
            "clone",
            "--force",
            "--dry-run",
        ]);
        match cli.command {
            Some(Command::Import {
                file,
                target_id,
                force,
                dry_run,
                skip_sessions,
                skip_workspace,
            }) => {
                assert_eq!(file, PathBuf::from("agent.json"));
                assert_eq!(target_id.as_deref(), Some("clone"));
                assert!(force);
                assert!(dry_run);
                assert!(!skip_sessions);
                assert!(!skip_workspace);
            }
            _ => panic!("expected Import command"),
        }
    }
}
