//! CLI argument parsing: [`Cli`] and [`Command`] definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::add_nous::AddNousArgs;
use crate::commands::agent_io::{
    ExportArgs, ExportSkillsArgs, ImportArgs, InitArgs, MigrateMemoryArgs, ReviewSkillsArgs,
    SeedSkillsArgs, TuiArgs,
};
use crate::commands::backup::BackupArgs;
use crate::commands::benchmark::BenchmarkArgs;
use crate::commands::config;
use crate::commands::credential;
use crate::commands::desktop::DesktopArgs;
use crate::commands::eval::EvalArgs;
use crate::commands::eval_embeddings::EvalEmbeddingsArgs;
use crate::commands::health::HealthArgs;
use crate::commands::ingest::IngestArgs;
use crate::commands::maintenance;
use crate::commands::memory;
use crate::commands::migrate::MigrateArgs;
use crate::commands::prompt_audit;
use crate::commands::repl::ReplArgs;
use crate::commands::session_create::SessionCreateArgs;
use crate::commands::session_export::SessionExportArgs;
use crate::commands::tls;

#[derive(Debug, Parser)]
#[command(
    name = "aletheia",
    about = "Cognitive agent runtime. Run with no subcommand to start the HTTP server.",
    version
)]
pub(crate) struct Cli {
    /// Path to instance root directory
    #[arg(short = 'r', long)]
    pub instance_root: Option<PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// Bind address (overrides config gateway.bind when set)
    #[arg(long)]
    pub bind: Option<String>,

    /// Port (overrides config gateway.port when set)
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Emit JSON-structured logs
    #[arg(long)]
    pub json_logs: bool,

    /// Fork to background and write PID file at instance/aletheia.pid
    #[arg(long)]
    pub daemon: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Command {
    /// Start the HTTP server (same as running with no subcommand)
    Serve,
    /// Serve Aletheia MCP over stdio
    Mcp,
    /// Check if the server is running
    Health(HealthArgs),
    /// Manage database backups
    Backup(BackupArgs),
    /// Instance maintenance tasks
    Maintenance {
        #[command(subcommand)]
        action: maintenance::Action,
    },
    /// Inspect the prompt audit log (outbound LLM requests)
    PromptAudit {
        #[command(subcommand)]
        action: prompt_audit::Action,
    },
    /// Knowledge graph inspection and maintenance
    Memory {
        /// Server URL for API routing when server is running
        #[arg(long, default_value = "http://127.0.0.1:18789")]
        // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
        url: String,
        /// Bearer token for API routes that require authentication
        #[arg(long, env = "ALETHEIA_API_TOKEN")]
        token: Option<String>,
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
        // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
        url: String,
    },
    /// Credential management
    Credential {
        #[command(subcommand)]
        action: credential::Action,
    },
    /// Run memory benchmarks (`LongMemEval`, `LoCoMo`) against a live instance
    Benchmark(BenchmarkArgs),
    /// Run behavioral evaluation scenarios against a live instance
    Eval(EvalArgs),
    /// Embedding quality gate: Recall@K and MRR before model upgrades
    EvalEmbeddings(EvalEmbeddingsArgs),
    /// Export an agent to a portable .agent.json file
    Export(ExportArgs),
    /// Create a session directly in the local store
    SessionCreate(SessionCreateArgs),
    /// Export a session as Markdown or JSON
    SessionExport(SessionExportArgs),
    /// Launch the terminal dashboard
    Tui(TuiArgs),
    /// Launch the desktop app (discovers proskenion in PATH)
    Desktop(DesktopArgs),
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
    /// Migrate an instance to a new directory
    Migrate(MigrateArgs),
    /// Scaffold a new nous agent directory
    AddNous(AddNousArgs),
    /// Interactive Datalog REPL for querying the knowledge graph
    Repl(ReplArgs),
    /// Ingest files into the knowledge graph
    Ingest(IngestArgs),
}
