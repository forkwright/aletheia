//! CLI subcommand handlers: one module per subcommand.

pub(crate) mod add_nous;
pub(crate) mod agent_io;
pub(crate) mod backup;
pub(crate) mod check_config;
pub(crate) mod config;
pub(crate) mod credential;
pub(crate) mod daemon;
pub(crate) mod desktop;
pub(crate) mod eval;
pub(crate) mod eval_embeddings;
pub(crate) mod health;
pub(crate) mod maintenance;
pub(crate) mod memory;
pub(crate) mod repl;
pub(crate) mod server;
pub(crate) mod session_export;
pub(crate) mod tls;

use std::path::PathBuf;

use aletheia_taxis::oikos::Oikos;
use anyhow::Result;
use clap::CommandFactory;

use crate::cli::{Cli, Command};
use crate::init;
use crate::status;

/// Resolve the instance root and verify it exists.
///
/// Returns a clear error message directing the user to `aletheia init` or `-r`
/// instead of letting downstream code fail with opaque SQLite/config errors.
pub(crate) fn resolve_oikos(instance_root: Option<&PathBuf>) -> crate::error::Result<Oikos> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    if !oikos.root().exists() {
        snafu::whatever!(
            "instance not found at {}\n  \
             Use -r /path/to/instance or set ALETHEIA_ROOT.\n  \
             To create a new instance: aletheia init",
            oikos.root().display()
        );
    }
    Ok(oikos)
}

/// Route a CLI subcommand to its handler.
///
/// WHY: Extracted from `main` to keep the binary entrypoint under 100 lines
/// per basanos ARCHITECTURE.md guidelines.
pub(crate) async fn dispatch(cmd: Command, instance_root: Option<&PathBuf>) -> Result<()> {
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
        Command::Health(a) => health::run(&a).await.map_err(Into::into),
        Command::Backup(a) => backup::run(instance_root, &a).map_err(Into::into),
        Command::Maintenance { action } => {
            maintenance::run(action, instance_root).map_err(Into::into)
        }
        Command::Memory { url, action } => memory::run(action, &url, instance_root)
            .await
            .map_err(Into::into),
        Command::Tls { action } => tls::run(&action).map_err(Into::into),
        Command::Status { url } => status::run(&url, instance_root)
            .await
            .map_err(anyhow::Error::from),
        Command::Credential { action } => credential::run(action, instance_root)
            .await
            .map_err(Into::into),
        #[cfg(feature = "tui")]
        Command::Tui(a) => {
            theatron_tui::run_tui(a.url, a.token, a.agent, a.session, a.logout)
                .await
                .map_err(anyhow::Error::from)
        }
        #[cfg(not(feature = "tui"))]
        Command::Tui(_) => anyhow::bail!("TUI not available - rebuild with `--features tui`"),
        Command::Desktop(a) => desktop::run(&a),
        Command::Eval(a) => eval::run(a).await.map_err(Into::into),
        Command::EvalEmbeddings(a) => eval_embeddings::run(a).map_err(Into::into),
        Command::Export(a) => agent_io::export_agent(instance_root, &a).map_err(Into::into),
        Command::SessionExport(a) => session_export::run(&a).await.map_err(Into::into),
        Command::Import(a) => agent_io::import_agent(instance_root, &a).map_err(Into::into),
        Command::SeedSkills(a) => agent_io::seed_skills(&a).map_err(Into::into),
        Command::ExportSkills(a) => {
            agent_io::export_skills(instance_root, &a).map_err(Into::into)
        }
        Command::ReviewSkills(a) => {
            agent_io::review_skills(instance_root, &a).map_err(Into::into)
        }
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "aletheia", &mut std::io::stdout());
            Ok(())
        }
        Command::MigrateMemory(a) => agent_io::migrate_memory(instance_root, a)
            .await
            .map_err(Into::into),
        Command::CheckConfig => check_config::run(instance_root).map_err(Into::into),
        Command::Config { action } => config::run(&action, instance_root).map_err(Into::into),
        Command::AddNous(a) => add_nous::run(instance_root, &a).await.map_err(Into::into),
        Command::Repl(a) => repl::run(instance_root, &a).map_err(Into::into),
        // NOTE: Serve is intercepted in main() before dispatch is called.
        // This arm exists only for match exhaustiveness.
        Command::Serve => unreachable!("Serve handled in main"),
    }
}
