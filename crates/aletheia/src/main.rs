//! Aletheia cognitive agent runtime: binary entrypoint.
//!
//! Binary entrypoint kept intentionally small; command logic lives in modules.

#![deny(clippy::unwrap_used)]

mod cli;
mod commands;
mod daemon_bridge;
mod dispatch;
mod embedding_config;
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
mod session_retention;
mod status;

use anyhow::Result;
use clap::Parser;
use koina::system::{Environment, RealSystem};

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
    // kanon:ignore RUST/no-silent-result-swallow — install_default returns Err when provider already installed by dependency; harmless
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();

    // WHY: `Serve` is an explicit alias for running with no subcommand.
    match cli.command {
        Some(Command::Serve) | None => {}
        Some(cmd) => return Box::pin(commands::dispatch(cmd, cli.instance_root.as_ref())).await,
    }

    if cli.daemon && RealSystem.var("_ALETHEIA_DAEMON").is_none() {
        return commands::daemon::do_daemon().await;
    }

    Box::pin(commands::server::run(commands::server::Args {
        instance_root: cli.instance_root,
        bind: cli.bind,
        port: cli.port,
        log_level: cli.log_level,
        json_logs: cli.json_logs,
    }))
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod cli_tests;
