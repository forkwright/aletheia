//! `aletheia desktop` subcommand — PATH-based discovery and exec of the desktop app.

use std::process::Command;

use clap::Args;

const BINARY_NAME: &str = "theatron-desktop";

#[derive(Debug, Clone, Args)]
pub(crate) struct DesktopArgs {
    /// Gateway URL
    #[arg(short, long, env = "ALETHEIA_URL")]
    pub url: Option<String>,
    /// Bearer token for authentication
    #[arg(short, long, env = "ALETHEIA_TOKEN")]
    pub token: Option<String>,
    /// Agent to focus on startup
    #[arg(short, long)]
    pub agent: Option<String>,
    /// Session to open
    #[arg(short, long)]
    pub session: Option<String>,
}

/// Search PATH for `theatron-desktop` and exec it with forwarded flags.
pub(crate) fn run(args: &DesktopArgs) -> anyhow::Result<()> {
    let binary = find_in_path().ok_or_else(|| {
        anyhow::anyhow!(
            "`{BINARY_NAME}` not found in PATH\n\n\
             Build and install it from the workspace:\n  \
             cd crates/theatron/desktop && cargo build --release\n  \
             cp target/release/{BINARY_NAME} ~/.cargo/bin/\n\n\
             Or add its build directory to PATH."
        )
    })?;

    let mut cmd = Command::new(&binary);

    if let Some(ref url) = args.url {
        cmd.args(["--url", url]);
    }
    if let Some(ref token) = args.token {
        cmd.args(["--token", token]);
    }
    if let Some(ref agent) = args.agent {
        cmd.args(["--agent", agent]);
    }
    if let Some(ref session) = args.session {
        cmd.args(["--session", session]);
    }

    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!("failed to exec `{}`: {e}", binary.display())
    })?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Search `$PATH` for the desktop binary.
fn find_in_path() -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(BINARY_NAME))
        .find(|candidate| candidate.is_file())
}
