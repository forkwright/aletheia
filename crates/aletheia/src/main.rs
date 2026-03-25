//! Aletheia cognitive agent runtime: binary entrypoint.

mod commands;
mod daemon_bridge;
mod dispatch;
mod error;
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

use anyhow::{Context, Result};
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

    /// Fork to background and write PID file at instance/aletheia.pid
    #[arg(long)]
    daemon: bool,

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
        /// Server URL for API routing when server is running
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        url: String,
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

    if let Some(cmd) = cli.command {
        return dispatch_command(cmd, cli.instance_root.as_ref()).await;
    }

    if cli.daemon && std::env::var("_ALETHEIA_DAEMON").is_err() {
        return do_daemon().await;
    }

    commands::server::run(commands::server::Args {
        instance_root: cli.instance_root,
        bind: cli.bind,
        port: cli.port,
        log_level: cli.log_level,
        json_logs: cli.json_logs,
    })
    .await
    .map_err(Into::into)
}

/// Route a CLI subcommand to its handler.
///
/// WHY: Extracted from `main` to stay under the `too_many_lines` lint threshold
/// while keeping each command's dispatch as a single match arm.
async fn dispatch_command(cmd: Command, instance_root: Option<&PathBuf>) -> Result<()> {
    match cmd {
        Command::Init(a) => {
            init::run(init::RunArgs {
                root: a.instance_root,
                yes: a.yes,
                non_interactive: a.non_interactive,
                // codequality:ignore -- raw CLI string immediately wrapped in SecretString
                api_key: a.api_key.map(aletheia_koina::secret::SecretString::from),
                auth_mode: a.auth_mode,
                api_provider: a.api_provider,
                model: a.model,
            })
            .map_err(anyhow::Error::from)
        }
        Command::Health(a) => commands::health::run(&a).await.map_err(Into::into),
        Command::Backup(a) => commands::backup::run(instance_root, &a).map_err(Into::into),
        Command::Maintenance { action } => {
            commands::maintenance::run(action, instance_root).map_err(Into::into)
        }
        Command::Memory { url, action } => commands::memory::run(action, &url, instance_root)
            .await
            .map_err(Into::into),
        Command::Tls { action } => commands::tls::run(&action).map_err(Into::into),
        Command::Status { url } => status::run(&url, instance_root)
            .await
            .map_err(anyhow::Error::from),
        Command::Credential { action } => commands::credential::run(action, instance_root)
            .await
            .map_err(Into::into),
        #[cfg(feature = "tui")]
        Command::Tui(a) => theatron_tui::run_tui(a.url, a.token, a.agent, a.session, a.logout)
            .await
            .map_err(anyhow::Error::from),
        #[cfg(not(feature = "tui"))]
        Command::Tui(_) => anyhow::bail!("TUI not available - rebuild with `--features tui`"),
        Command::Eval(a) => commands::eval::run(a).await.map_err(Into::into),
        Command::Export(a) => {
            commands::agent_io::export_agent(instance_root, &a).map_err(Into::into)
        }
        Command::SessionExport(a) => commands::session_export::run(&a).await.map_err(Into::into),
        Command::Import(a) => {
            commands::agent_io::import_agent(instance_root, &a).map_err(Into::into)
        }
        Command::SeedSkills(a) => commands::agent_io::seed_skills(&a).map_err(Into::into),
        Command::ExportSkills(a) => {
            commands::agent_io::export_skills(instance_root, &a).map_err(Into::into)
        }
        Command::ReviewSkills(a) => {
            commands::agent_io::review_skills(instance_root, &a).map_err(Into::into)
        }
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "aletheia",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::MigrateMemory(a) => commands::agent_io::migrate_memory(instance_root, a)
            .await
            .map_err(Into::into),
        Command::CheckConfig => commands::check_config::run(instance_root).map_err(Into::into),
        Command::Config { action } => {
            commands::config::run(&action, instance_root).map_err(Into::into)
        }
        Command::AddNous(a) => commands::add_nous::run(instance_root, &a)
            .await
            .map_err(Into::into),
    }
}

/// Fork the server to background by re-executing the binary without `--daemon`.
///
/// WHY: `fork()` is unsafe inside a running tokio multi-thread runtime. Re-executing
/// the binary avoids that hazard while still detaching from the terminal.
async fn do_daemon() -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate executable")?;

    // Strip --daemon from the child args
    let child_args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--daemon")
        .collect();

    let child = std::process::Command::new(&exe)
        .args(&child_args)
        .env("_ALETHEIA_DAEMON", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn background process")?;

    let pid = child.id();

    // Determine instance root to write PID file
    let instance_root = daemon_instance_root();
    tokio::fs::create_dir_all(&instance_root)
        .await
        .with_context(|| format!("failed to create {}", instance_root.display()))?;
    let pid_path = instance_root.join("aletheia.pid");
    tokio::fs::write(&pid_path, pid.to_string())
        .await
        .with_context(|| format!("failed to write PID file at {}", pid_path.display()))?;

    println!(
        "aletheia started in background (PID: {pid}, PID file: {})",
        pid_path.display()
    );
    Ok(())
}

/// Resolve the instance root from CLI args or environment for PID file placement.
fn daemon_instance_root() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        if arg == "-r" || arg == "--instance-root" {
            if let Some(path) = args.get(i + 1) {
                return PathBuf::from(path);
            }
        } else if let Some(path) = arg.strip_prefix("--instance-root=") {
            return PathBuf::from(path);
        }
    }
    std::env::var("ALETHEIA_ROOT").map_or_else(|_| PathBuf::from("instance"), PathBuf::from)
}

#[cfg(test)]
mod cli_tests;
