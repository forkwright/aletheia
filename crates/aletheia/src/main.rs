//! Aletheia cognitive agent runtime — binary entrypoint.

mod daemon_bridge;
mod dispatch;
mod init;
#[cfg(feature = "recall")]
mod knowledge_adapter;
#[cfg(feature = "recall")]
mod knowledge_maintenance;
#[cfg(feature = "migrate-qdrant")]
mod migrate_memory;
mod planning_adapter;
mod status;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
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

#[derive(Debug, Parser)]
#[command(name = "aletheia", about = "Cognitive agent runtime", version)]
struct Cli {
    /// Path to instance root directory
    #[arg(short = 'r', long)]
    instance_root: Option<PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Bind address (overrides config gateway.bind when set)
    #[arg(long)]
    bind: Option<String>,

    /// Port (overrides config gateway.port when set)
    #[arg(short, long)]
    port: Option<u16>,

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
        /// Output file path (default: `{nous-id}-{date}.agent.json`)
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
    /// Launch the terminal dashboard
    Tui {
        /// Gateway URL
        #[arg(short, long, env = "ALETHEIA_URL")]
        url: Option<String>,
        /// Bearer token for authentication
        #[arg(short, long, env = "ALETHEIA_TOKEN")]
        token: Option<String>,
        /// Agent to focus on startup
        #[arg(short, long)]
        agent: Option<String>,
        /// Session to open
        #[arg(short, long)]
        session: Option<String>,
        /// Clear saved credentials
        #[arg(long)]
        logout: bool,
    },
    /// Migrate memories from Qdrant (Mem0) into embedded `KnowledgeStore`
    MigrateMemory {
        /// Qdrant server URL
        #[arg(long, default_value = "http://localhost:6333", env = "QDRANT_URL")]
        qdrant_url: String,
        /// Qdrant collection name
        #[arg(long, default_value = "aletheia_memories")]
        collection: String,
        /// Path to persistent knowledge store (redb)
        #[arg(long, env = "ALETHEIA_KNOWLEDGE_PATH")]
        knowledge_path: Option<PathBuf>,
        /// Write flagged facts to a review file
        #[arg(long)]
        review_file: Option<PathBuf>,
        /// Report only, don't insert
        #[arg(long)]
        dry_run: bool,
    },
    /// Initialize a new instance
    Init {
        /// Instance root directory
        #[arg(short = 'r', long, default_value = "./instance")]
        instance_root: PathBuf,

        /// Accept all defaults (non-interactive)
        #[arg(short = 'y', long)]
        yes: bool,

        /// API key (non-interactive mode)
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        api_key: Option<String>,
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
    /// Seed skills from SKILL.md files into the knowledge store
    SeedSkills {
        /// Directory containing skill subdirectories (each with SKILL.md)
        #[arg(short, long)]
        dir: PathBuf,
        /// Agent (nous) ID to attribute skills to
        #[arg(short, long)]
        nous_id: String,
        /// Overwrite existing skills with the same name
        #[arg(long)]
        force: bool,
        /// Show what would be seeded without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Export skills to Claude Code format (`.claude/skills/<slug>/SKILL.md`)
    ExportSkills {
        /// Agent (nous) ID whose skills to export
        #[arg(short, long)]
        nous_id: String,
        /// Output directory (default: .claude/skills)
        #[arg(short, long, default_value = ".claude/skills")]
        output: PathBuf,
        /// Filter by domain tags (comma-separated)
        #[arg(short, long)]
        domain: Option<String>,
    },
    /// Review pending auto-extracted skills (approve, reject, or list)
    ReviewSkills {
        /// Agent (nous) ID whose pending skills to review
        #[arg(short, long)]
        nous_id: String,
        /// Action: list, approve, reject
        #[arg(short, long, default_value = "list")]
        action: String,
        /// Fact ID of the pending skill (required for approve/reject)
        #[arg(short, long)]
        fact_id: Option<String>,
    },
    /// Generate shell completions for bash, zsh, or fish
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
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
#[expect(clippy::too_many_lines, reason = "CLI dispatch is inherently verbose")]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install ring crypto provider");

    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Init {
            instance_root,
            yes,
            api_key,
        }) => {
            return init::run(instance_root.clone(), *yes, api_key.clone());
        }
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
        #[cfg(feature = "tui")]
        Some(Command::Tui {
            url,
            token,
            agent,
            session,
            logout,
        }) => {
            return aletheia_tui::run_tui(
                url.clone(),
                token.clone(),
                agent.clone(),
                session.clone(),
                *logout,
            )
            .await;
        }
        #[cfg(not(feature = "tui"))]
        Some(Command::Tui { .. }) => {
            anyhow::bail!("TUI not available — rebuild with `--features tui`");
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
        }) => {
            return export_agent_cmd(
                &cli,
                nous_id,
                output.as_ref(),
                *archived,
                *max_messages,
                *compact,
            );
        }
        Some(Command::Import {
            file,
            target_id,
            skip_sessions,
            skip_workspace,
            force,
            dry_run,
        }) => {
            return import_agent_cmd(
                &cli,
                file,
                target_id.as_deref(),
                *skip_sessions,
                *skip_workspace,
                *force,
                *dry_run,
            );
        }
        Some(Command::SeedSkills {
            dir,
            nous_id,
            force,
            dry_run,
        }) => {
            return seed_skills_cmd(dir, nous_id, *force, *dry_run);
        }
        Some(Command::ExportSkills {
            nous_id,
            output,
            domain,
        }) => {
            return export_skills_cmd(&cli, nous_id, output, domain.as_deref());
        }
        Some(Command::ReviewSkills {
            nous_id,
            action,
            fact_id,
        }) => {
            return review_skills_cmd(&cli, nous_id, action, fact_id.as_deref());
        }
        Some(Command::Completions { shell }) => {
            let mut cmd = Cli::command();
            clap_complete::generate(*shell, &mut cmd, "aletheia", &mut std::io::stdout());
            return Ok(());
        }
        Some(Command::MigrateMemory {
            qdrant_url,
            collection,
            knowledge_path,
            review_file,
            dry_run,
        }) => {
            return run_migrate_memory(
                &cli,
                qdrant_url,
                collection,
                knowledge_path.as_ref(),
                review_file.as_ref(),
                *dry_run,
            )
            .await;
        }
        None => {}
    }

    serve(cli).await
}

#[expect(
    clippy::unused_async,
    reason = "async required when migrate-qdrant feature is enabled"
)]
async fn run_migrate_memory(
    cli: &Cli,
    qdrant_url: &str,
    collection: &str,
    knowledge_path: Option<&PathBuf>,
    review_file: Option<&PathBuf>,
    dry_run: bool,
) -> Result<()> {
    #[cfg(feature = "migrate-qdrant")]
    {
        return migrate_memory::run(
            cli,
            qdrant_url,
            collection,
            knowledge_path,
            review_file,
            dry_run,
        )
        .await;
    }
    #[cfg(not(feature = "migrate-qdrant"))]
    {
        let _ = (
            cli,
            qdrant_url,
            collection,
            knowledge_path,
            review_file,
            dry_run,
        );
        anyhow::bail!(
            "migrate-memory requires the `migrate-qdrant` feature.\n\
             Rebuild with: cargo build --features migrate-qdrant"
        );
    }
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
            let token = CancellationToken::new();
            let mut runner = TaskRunner::new("system", token).with_maintenance(maint);
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
        knowledge_maintenance: aletheia_oikonomos::maintenance::KnowledgeMaintenanceConfig {
            enabled: settings.knowledge_maintenance_enabled,
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
        let store = aletheia_mneme::knowledge_store::KnowledgeStore::open_redb(
            &kb_path,
            aletheia_mneme::knowledge_store::KnowledgeConfig::default(),
        )
        .context("failed to open knowledge store")?;
        info!(path = %kb_path.display(), dim = 384, "knowledge store opened (redb)");
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
            Arc::new(knowledge_adapter::KnowledgeSearchAdapter::new(
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
        TaskRunner::new("system", daemon_token).with_maintenance(maintenance_config);

    // Wire knowledge maintenance executor when recall feature is enabled
    #[cfg(feature = "recall")]
    if let Some(ks) = knowledge_store_for_daemon.as_ref() {
        let km_executor = Arc::new(knowledge_maintenance::KnowledgeMaintenanceAdapter::new(
            Arc::clone(ks),
        ));
        daemon_runner = daemon_runner.with_knowledge_maintenance(km_executor);
    }

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
        start_inbound_dispatch(&config, &nous_manager, ready_rx, signal_provider.as_ref());

    // Daemon runners — per-agent background task scheduling
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

    // Pylon HTTP gateway — shares registries with NousManager
    let aletheia_config = aletheia_taxis::loader::load_config(&oikos_arc).unwrap_or_else(|e| {
        tracing::warn!("failed to load config, using defaults: {e}");
        aletheia_taxis::config::AletheiaConfig::default()
    });
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
        shutdown: shutdown_token.clone(),
    });

    let security = aletheia_pylon::security::SecurityConfig::from_gateway(&config.gateway);
    let app = build_router(state.clone(), &security);

    let port = cli.port.unwrap_or(config.gateway.port);
    // Resolve bind address: CLI flag > config gateway.bind > default 127.0.0.1.
    // "lan" is a semantic alias for "0.0.0.0" (listen on all interfaces).
    let bind_host = cli.bind.as_deref().unwrap_or(&config.gateway.bind);
    let bind_addr_str = match bind_host {
        "lan" => "0.0.0.0",
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
    // 4. Drain nous actors with a timeout, flushing redb WAL and other state.
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

// -- Tool service adapters -------------------------------------------------

mod tool_adapters {
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
    let resp = reqwest::get(&endpoint).await.map_err(|e| {
        if e.is_connect() {
            anyhow::anyhow!(
                "FAILED: cannot connect to {url}\n  \
                 Is the server running? Start it with: aletheia"
            )
        } else {
            anyhow::anyhow!("FAILED: {e}")
        }
    })?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse health response")?;
    let health_status = body["status"].as_str().unwrap_or("unknown");
    let version = body["version"].as_str().unwrap_or("unknown");
    let uptime = body["uptime_seconds"].as_u64().unwrap_or(0);
    if status.is_success() {
        println!("OK — {health_status} | version {version} | uptime {uptime}s");
    } else {
        println!("{}", serde_json::to_string_pretty(&body)?);
        anyhow::bail!("FAILED: health check returned HTTP {status}");
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

    let agent_config =
        config
            .agents
            .list
            .iter()
            .find(|a| a.id == nous_id)
            .map_or(serde_json::Value::Null, |a| {
                serde_json::to_value(a).unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "failed to serialize agent config");
                    serde_json::Value::Null
                })
            });

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
    println!("Size: {} bytes", json.len());
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
        println!(
            "Agent: {} ({})",
            nous_id,
            agent_file.nous.name.as_deref().unwrap_or("unnamed")
        );
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
    let result =
        aletheia_mneme::import::import_agent(&agent_file, &store, &workspace_path, &id_gen, &opts)
            .context("import failed")?;

    println!("Imported agent: {}", result.nous_id);
    println!("Files restored: {}", result.files_restored);
    println!("Sessions: {}", result.sessions_imported);
    println!("Messages: {}", result.messages_imported);
    println!("Notes: {}", result.notes_imported);

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch is inherently verbose — splitting would hurt readability"
)]
fn seed_skills_cmd(dir: &Path, nous_id: &str, force: bool, dry_run: bool) -> Result<()> {
    use aletheia_mneme::skill::{SkillContent, parse_skill_md, scan_skill_dir};

    let entries = scan_skill_dir(dir)
        .with_context(|| format!("failed to scan skill directory: {}", dir.display()))?;

    if entries.is_empty() {
        println!("No SKILL.md files found in {}", dir.display());
        return Ok(());
    }

    println!("Found {} skill(s) in {}", entries.len(), dir.display());

    // Parse all skills first
    let mut parsed: Vec<(String, SkillContent)> = Vec::new();
    let mut parse_errors = 0u32;
    for (slug, content) in &entries {
        match parse_skill_md(content, slug) {
            Ok(skill) => parsed.push((slug.clone(), skill)),
            Err(e) => {
                eprintln!("  SKIP {slug}: {e}");
                parse_errors += 1;
            }
        }
    }

    if dry_run {
        println!(
            "\n[dry-run] Would seed {} skill(s) for nous '{nous_id}':",
            parsed.len()
        );
        for (slug, skill) in &parsed {
            println!(
                "  {slug}: {} steps, {} tools, tags: [{}]",
                skill.steps.len(),
                skill.tools_used.len(),
                skill.domain_tags.join(", ")
            );
        }
        if parse_errors > 0 {
            println!("\n{parse_errors} skill(s) skipped due to parse errors");
        }
        return Ok(());
    }

    // Open knowledge store (in-memory for seeding — caller must configure persistent path)
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge::{EpistemicTier, Fact, default_stability_hours};
        use aletheia_mneme::knowledge_store::KnowledgeStore;

        let store = KnowledgeStore::open_mem()
            .map_err(|e| anyhow::anyhow!("failed to open knowledge store: {e}"))?;

        let now = jiff::Timestamp::now();
        let mut seeded = 0u32;
        let mut skipped = 0u32;
        let mut overwritten = 0u32;

        for (slug, skill) in &parsed {
            // Check for duplicates
            let existing = store
                .find_skill_by_name(nous_id, &skill.name)
                .map_err(|e| anyhow::anyhow!("failed to query existing skills: {e}"))?;

            if let Some(existing_id) = existing {
                if force {
                    // Supersede the old fact
                    if let Err(e) = store.forget_fact(
                        &aletheia_mneme::id::FactId::from(existing_id),
                        aletheia_mneme::knowledge::ForgetReason::Outdated,
                    ) {
                        eprintln!("  WARN: failed to supersede {slug}: {e}");
                    }
                    overwritten += 1;
                } else {
                    println!("  SKIP {slug}: already exists (use --force to overwrite)");
                    skipped += 1;
                    continue;
                }
            }

            let content_json = serde_json::to_string(skill)
                .with_context(|| format!("failed to serialize skill: {slug}"))?;

            let fact_id = ulid::Ulid::new().to_string();
            let fact = Fact {
                id: aletheia_mneme::id::FactId::from(fact_id.clone()),
                nous_id: nous_id.to_owned(),
                content: content_json.clone(),
                confidence: 0.5,
                tier: EpistemicTier::Assumed,
                valid_from: now,
                valid_to: aletheia_mneme::knowledge::far_future(),
                superseded_by: None,
                source_session_id: None,
                recorded_at: now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: default_stability_hours("skill"),
                fact_type: "skill".to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };

            store
                .insert_fact(&fact)
                .map_err(|e| anyhow::anyhow!("failed to insert skill {slug}: {e}"))?;

            // Generate embedding for semantic search
            let embedding_text = format!("{}: {}", skill.name, skill.description);
            let emb_id = ulid::Ulid::new().to_string();
            let chunk = aletheia_mneme::knowledge::EmbeddedChunk {
                id: aletheia_mneme::id::EmbeddingId::from(emb_id),
                content: embedding_text,
                source_type: "fact".to_owned(),
                source_id: fact_id,
                nous_id: nous_id.to_owned(),
                embedding: generate_simple_embedding(&content_json),
                created_at: now,
            };
            if let Err(e) = store.insert_embedding(&chunk) {
                eprintln!("  WARN: failed to insert embedding for {slug}: {e}");
            }

            println!("  SEED {slug}");
            seeded += 1;
        }

        println!(
            "\nDone: {seeded} seeded, {skipped} skipped, {overwritten} overwritten, {parse_errors} parse errors"
        );
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (force, nous_id, parsed, parse_errors);
        anyhow::bail!(
            "seed-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }

    Ok(())
}

/// Export skills from the knowledge store to Claude Code's native format.
///
/// Reads skill facts from an in-process `KnowledgeStore`, converts them to
/// `SkillContent`, and writes `.claude/skills/<slug>/SKILL.md` files.
fn export_skills_cmd(cli: &Cli, nous_id: &str, output: &Path, domain: Option<&str>) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skill::{SkillContent, export_skills_to_cc};

        let oikos = match &cli.instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = oikos.knowledge_db();

        let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
        let store = KnowledgeStore::open_redb(&knowledge_path, config).map_err(|e| {
            anyhow::anyhow!(
                "failed to open knowledge store at {}: {e}",
                knowledge_path.display()
            )
        })?;

        let facts = store
            .find_skills_for_nous(nous_id, 500)
            .map_err(|e| anyhow::anyhow!("failed to query skills: {e}"))?;

        if facts.is_empty() {
            println!("No skills found for nous '{nous_id}'");
            return Ok(());
        }

        // Parse facts into SkillContent
        let mut skills: Vec<SkillContent> = Vec::new();
        let mut parse_errors = 0u32;
        for fact in &facts {
            match serde_json::from_str::<SkillContent>(&fact.content) {
                Ok(skill) => skills.push(skill),
                Err(e) => {
                    eprintln!("  SKIP {}: failed to parse content: {e}", fact.id);
                    parse_errors += 1;
                }
            }
        }

        // Apply domain filter
        let domain_tags: Vec<&str> = domain
            .map(|d| d.split(',').map(str::trim).collect())
            .unwrap_or_default();
        let filter = if domain_tags.is_empty() {
            None
        } else {
            Some(domain_tags.as_slice())
        };

        let exported = export_skills_to_cc(&skills, output, filter)
            .with_context(|| format!("failed to export skills to {}", output.display()))?;

        println!(
            "Exported {} skill(s) to {}",
            exported.len(),
            output.display()
        );
        for ex in &exported {
            println!("  {} → {}", ex.name, ex.path.display());
        }
        if parse_errors > 0 {
            println!("\n{parse_errors} skill(s) skipped due to parse errors");
        }

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (cli, nous_id, output, domain);
        anyhow::bail!(
            "export-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

fn review_skills_cmd(cli: &Cli, nous_id: &str, action: &str, fact_id: Option<&str>) -> Result<()> {
    #[cfg(feature = "recall")]
    {
        use aletheia_mneme::knowledge_store::KnowledgeStore;
        use aletheia_mneme::skills::extract::PendingSkill;

        let oikos = match &cli.instance_root {
            Some(root) => Oikos::from_root(root),
            None => Oikos::discover(),
        };
        let knowledge_path = oikos.knowledge_db();

        let config = aletheia_mneme::knowledge_store::KnowledgeConfig::default();
        let store = KnowledgeStore::open_redb(&knowledge_path, config).map_err(|e| {
            anyhow::anyhow!(
                "failed to open knowledge store at {}: {e}",
                knowledge_path.display()
            )
        })?;

        match action {
            "list" => {
                let pending = store
                    .find_pending_skills(nous_id)
                    .map_err(|e| anyhow::anyhow!("failed to query pending skills: {e}"))?;

                if pending.is_empty() {
                    println!("No pending skills for nous '{nous_id}'");
                    return Ok(());
                }

                println!(
                    "Found {} pending skill(s) for nous '{nous_id}':\n",
                    pending.len()
                );
                for fact in &pending {
                    match PendingSkill::from_json(&fact.content) {
                        Ok(ps) => {
                            println!("  ID: {}", fact.id);
                            println!("  Name: {}", ps.skill.name);
                            println!(
                                "  Description: {}",
                                ps.skill.description.lines().next().unwrap_or("")
                            );
                            println!("  Tools: {}", ps.skill.tools_used.join(", "));
                            println!("  Tags: {}", ps.skill.domain_tags.join(", "));
                            println!("  Steps: {}", ps.skill.steps.len());
                            println!("  Status: {}", ps.status);
                            println!("  Candidate: {}", ps.candidate_id);
                            println!("  Extracted: {}", ps.extracted_at);
                            println!();
                        }
                        Err(e) => {
                            eprintln!("  SKIP {}: failed to parse: {e}", fact.id);
                        }
                    }
                }
            }
            "approve" => {
                let fid = fact_id
                    .ok_or_else(|| anyhow::anyhow!("--fact-id required for approve action"))?;
                let fact_id = aletheia_mneme::id::FactId::from(fid);
                let new_id = store
                    .approve_pending_skill(&fact_id, nous_id)
                    .map_err(|e| anyhow::anyhow!("failed to approve skill: {e}"))?;
                println!("Approved: {fid} → new skill fact: {new_id}");
            }
            "reject" => {
                let fid = fact_id
                    .ok_or_else(|| anyhow::anyhow!("--fact-id required for reject action"))?;
                let fact_id = aletheia_mneme::id::FactId::from(fid);
                store
                    .reject_pending_skill(&fact_id)
                    .map_err(|e| anyhow::anyhow!("failed to reject skill: {e}"))?;
                println!("Rejected: {fid}");
            }
            other => {
                anyhow::bail!("unknown action '{other}'. Use: list, approve, reject");
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "recall"))]
    {
        let _ = (cli, nous_id, action, fact_id);
        anyhow::bail!(
            "review-skills requires the 'recall' feature (KnowledgeStore). \
             Build with: cargo build --features recall"
        );
    }
}

/// Generate a deterministic pseudo-embedding for seeding (384-dim).
///
/// Uses a simple hash-based approach. Real embeddings come from the
/// candle embedding provider at runtime.
fn generate_simple_embedding(text: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let dim = 384;
    let mut embedding = Vec::with_capacity(dim);
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());

    // Generate enough hash bytes to fill the embedding
    let mut seed = hasher.finalize().to_vec();
    while embedding.len() < dim {
        for byte in &seed {
            if embedding.len() >= dim {
                break;
            }
            // Map byte to [-1.0, 1.0] — value is in [-1.0, 1.0] so truncation is harmless
            #[expect(clippy::cast_possible_truncation, reason = "result fits in f32 range")]
            embedding.push((f64::from(*byte) / 127.5 - 1.0) as f32);
        }
        // Re-hash for more bytes
        let mut h = Sha256::new();
        h.update(&seed);
        seed = h.finalize().to_vec();
    }

    // L2-normalize
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut embedding {
            *v /= norm;
        }
    }

    embedding
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
        assert!(cli.port.is_none());
        assert!(cli.bind.is_none());
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
        }
    }

    #[test]
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
