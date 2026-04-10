//! Aletheia cognitive agent runtime: binary entrypoint.
//!
//! Per basanos ARCHITECTURE.md: "binary entrypoint under 100 lines."

mod cli;
mod commands;
mod daemon_bridge;
mod dispatch;
mod error;
mod external_tools;
mod init;
#[cfg(feature = "recall")]
mod knowledge_adapter;
#[cfg(feature = "recall")]
mod knowledge_maintenance;
#[cfg(feature = "migrate-qdrant")]
mod migrate_memory;
mod planning_adapter;
#[cfg(feature = "recall")]
mod recall_sources;
mod runtime;
mod status;

use koina::system::{Environment, RealSystem};
use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(%backtrace, "panic: {info}");
        default_hook(info);
    }));

    // WHY: install_default returns Err if a provider is already installed
    // (e.g. a dependency called install_default first). That is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();

    // WHY: `Serve` is an explicit alias for running with no subcommand.
    match cli.command {
        Some(Command::Serve) | None => {}
        Some(cmd) => return commands::dispatch(cmd, cli.instance_root.as_ref()).await,
    }

    if cli.daemon && RealSystem.var("_ALETHEIA_DAEMON").is_none() {
        return commands::daemon::do_daemon().await;
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

#[cfg(test)]
mod cli_tests;
