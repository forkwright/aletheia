//! Aletheia cognitive agent runtime: binary entrypoint.

mod commands;
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

use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

use commands::add_nous::AddNousArgs;
use commands::agent_io::{
    ExportArgs, ExportSkillsArgs, ImportArgs, InitArgs, MigrateMemoryArgs, ReviewSkillsArgs,
    SeedSkillsArgs, TuiArgs,
};
use commands::backup::BackupArgs;
use commands::config;
use commands::credential;
use commands::eval::EvalArgs;
use commands::health::HealthArgs;
use commands::maintenance;
use commands::memory;
use commands::session_export::SessionExportArgs;
use commands::tls;

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
    Health(HealthArgs),
    /// Manage database backups
    Backup(BackupArgs),
    /// Instance maintenance tasks
    Maintenance {
        #[command(subcommand)]
        action: maintenance::Action,
    },
    /// Knowledge graph inspection and maintenance
    Memory {
        #[command(subcommand)]
        action: memory::Action,
    },
    /// TLS certificate management
    Tls {
        #[command(subcommand)]
        action: tls::Action,
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
        action: credential::Action,
    },
    /// Run behavioral evaluation scenarios against a live instance
    Eval(EvalArgs),
    /// Export an agent to a portable .agent.json file
    Export(ExportArgs),
    /// Export a session as Markdown or JSON
    SessionExport(SessionExportArgs),
    /// Launch the terminal dashboard
    Tui(TuiArgs),
    /// Migrate memories from Qdrant into embedded `KnowledgeStore`
    MigrateMemory(MigrateMemoryArgs),
    /// Initialize a new instance
    Init(InitArgs),
    /// Import an agent from a portable .agent.json file
    Import(ImportArgs),
    /// Seed skills from SKILL.md files into the knowledge store
    SeedSkills(SeedSkillsArgs),
    /// Export skills to Claude Code format (`.claude/skills/<slug>/SKILL.md`)
    ExportSkills(ExportSkillsArgs),
    /// Review pending auto-extracted skills (approve, reject, or list)
    ReviewSkills(ReviewSkillsArgs),
    /// Generate shell completions for bash, zsh, or fish
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
    /// Validate configuration without starting any services
    CheckConfig,
    /// Config encryption management
    Config {
        #[command(subcommand)]
        action: config::Action,
    },
    /// Scaffold a new nous agent directory
    AddNous(AddNousArgs),
}

#[tokio::main]
#[expect(
    clippy::expect_used,
    reason = "ring crypto provider installation is infallible unless already installed"
)]
async fn main() -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(%backtrace, "panic: {info}");
        default_hook(info);
    }));

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install ring crypto provider");

    let cli = Cli::parse();
    let instance_root = cli.instance_root.as_ref();

    match cli.command {
        Some(Command::Init(a)) => {
            return init::run(init::RunArgs {
                root: a.instance_root,
                yes: a.yes,
                non_interactive: a.non_interactive,
                // codequality:ignore — raw CLI string immediately wrapped in SecretString
                api_key: a.api_key.map(aletheia_koina::secret::SecretString::from),
                auth_mode: a.auth_mode,
                api_provider: a.api_provider,
                model: a.model,
            })
            .map_err(anyhow::Error::from);
        }
        Some(Command::Health(a)) => return commands::health::run(&a).await,
        Some(Command::Backup(a)) => return commands::backup::run(instance_root, &a),
        Some(Command::Maintenance { action }) => {
            return commands::maintenance::run(action, instance_root);
        }
        Some(Command::Memory { action }) => {
            return commands::memory::run(action, instance_root);
        }
        Some(Command::Tls { action }) => return commands::tls::run(&action),
        Some(Command::Status { url }) => {
            return status::run(&url, instance_root)
                .await
                .map_err(anyhow::Error::from);
        }
        Some(Command::Credential { action }) => {
            return commands::credential::run(action, instance_root).await;
        }
        #[cfg(feature = "tui")]
        Some(Command::Tui(a)) => {
            return theatron_tui::run_tui(a.url, a.token, a.agent, a.session, a.logout)
                .await
                .map_err(anyhow::Error::from);
        }
        #[cfg(not(feature = "tui"))]
        Some(Command::Tui(_)) => anyhow::bail!("TUI not available - rebuild with `--features tui`"),
        Some(Command::Eval(a)) => return commands::eval::run(a).await,
        Some(Command::Export(a)) => return commands::agent_io::export_agent(instance_root, &a),
        Some(Command::SessionExport(a)) => return commands::session_export::run(&a).await,
        Some(Command::Import(a)) => return commands::agent_io::import_agent(instance_root, &a),
        Some(Command::SeedSkills(a)) => return commands::agent_io::seed_skills(&a),
        Some(Command::ExportSkills(a)) => {
            return commands::agent_io::export_skills(instance_root, &a);
        }
        Some(Command::ReviewSkills(a)) => {
            return commands::agent_io::review_skills(instance_root, &a);
        }
        Some(Command::Completions { shell }) => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "aletheia",
                &mut std::io::stdout(),
            );
            return Ok(());
        }
        Some(Command::MigrateMemory(a)) => {
            return commands::agent_io::migrate_memory(instance_root, a).await;
        }
        Some(Command::CheckConfig) => return commands::check_config::run(instance_root),
        Some(Command::Config { action }) => return commands::config::run(&action, instance_root),
        Some(Command::AddNous(a)) => return commands::add_nous::run(instance_root, &a).await,
        // NOTE: no subcommand, fall through to default server startup
        None => {}
    }

    commands::server::run(commands::server::Args {
        instance_root: cli.instance_root,
        bind: cli.bind,
        port: cli.port,
        log_level: cli.log_level,
        json_logs: cli.json_logs,
    })
    .await
}

#[cfg(test)]
mod cli_tests;
