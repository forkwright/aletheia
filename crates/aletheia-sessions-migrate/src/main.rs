//! `aletheia-sessions-migrate` - one-shot `SQLite` v32 -> fjall importer
//! for legacy aletheia 0.15.x session DBs.
//!
//! See `FIELD_MAPPING.md` for column-by-column mapping.

mod cli;
mod commands;
mod dest;
mod error;
mod migrate;
mod schema;
mod source;
mod verify;

use std::io::Write as _;
use std::process::ExitCode;

use tracing::error;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    init_tracing();

    match cli::run_from_args() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!(error = ?err, "migration failed");
            if let Err(write_error) = writeln!(std::io::stderr().lock(), "error: {err:#}") {
                error!(error = %write_error, "failed to write migration error to stderr");
            }
            ExitCode::from(1)
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,aletheia_sessions_migrate=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
