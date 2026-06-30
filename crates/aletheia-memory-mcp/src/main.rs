//! `aletheia-memory-mcp` — stdio MCP binary.
//!
//! Opens a fjall-backed knowledge store and serves read tools plus token-gated
//! write tools over stdio JSON-RPC. Configuration:
//!
//! - `ALETHEIA_ROOT` — instance root (default `./instance`). The store is
//!   opened at `<root>/data/knowledge.fjall/shared` (the shared episteme
//!   cohort).
//! - `ALETHEIA_MEMORY_MCP_STORE` — override the store path directly, e.g. to
//!   target a different cohort at `<root>/data/knowledge.fjall/<cohort>`.
//! - `ALETHEIA_MEMORY_MCP_NOUS_ID` — bind the server to a single caller
//!   identity (nous). Read tools fail closed when this is unset.
//! - `ALETHEIA_MEMORY_MCP_WRITE_TOKEN` — enable write tools. The capability is
//!   configured out-of-band and is never accepted as a tool argument.
//! - `ALETHEIA_MEMORY_MCP_ADMIN_DIAGNOSTICS` — allow full store-path
//!   diagnostics when the write token is also configured.
//! - `RUST_LOG` — tracing filter; defaults to `info`. Logs go to stderr so
//!   stdout stays clean for JSON-RPC.
//!
//! Exit codes:
//!
//! - `0` — clean shutdown (peer closed the connection).
//! - `1` — startup or transport error (details on stderr).

use std::path::PathBuf;
use std::process::ExitCode;

use aletheia_memory_mcp::error;
use aletheia_memory_mcp::server::MemoryServer;
use taxis::oikos::Oikos;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    // WHY: tracing must go to stderr because stdout is the MCP JSON-RPC
    // transport. A stray INFO log on stdout would corrupt the protocol.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "failed to build tokio runtime");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "aletheia-memory-mcp exited with error");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> error::Result<()> {
    let store_path = resolve_store_path();
    tracing::info!(
        path = %store_path.display(),
        "opening knowledge store (fjall)"
    );

    let server = MemoryServer::open_fjall(&store_path)?;
    tracing::info!("memory MCP server ready on stdio");
    server.serve_stdio().await?;
    Ok(())
}

/// Resolve the knowledge store path from environment.
///
/// Precedence:
/// 1. `ALETHEIA_MEMORY_MCP_STORE` — explicit override.
/// 2. `ALETHEIA_ROOT` via `Oikos::discover` — canonical instance layout.
fn resolve_store_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("ALETHEIA_MEMORY_MCP_STORE") {
        return PathBuf::from(explicit);
    }
    Oikos::discover().knowledge_cohort_db("shared")
}
