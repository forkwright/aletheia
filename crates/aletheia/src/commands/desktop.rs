//! `aletheia desktop` subcommand — PATH-based discovery and exec of the desktop app.

use std::path::PathBuf;
use std::process::Command;

use clap::Args;
use snafu::{ResultExt, Snafu};

const BINARY_NAME: &str = "theatron-desktop";

/// Errors that can occur when running the desktop command.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum DesktopError {
    #[snafu(display(
        "`{BINARY_NAME}` not found in PATH\n\n\
             Build and install it from the workspace:\n  \
             cd crates/theatron/desktop && cargo build --release\n  \
             cp target/release/{BINARY_NAME} ~/.cargo/bin/\n\n\
             Or add its build directory to PATH."
    ))]
    BinaryNotFound {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to exec `{binary:?}`: {source}"))]
    ExecFailed {
        binary: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("`{BINARY_NAME}` exited with status {code}"))]
    ExitStatus {
        code: i32,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

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
pub(crate) fn run(args: &DesktopArgs) -> Result<(), DesktopError> {
    let binary = find_in_path().ok_or_else(|| BinaryNotFoundSnafu.build())?;

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

    let status = cmd
        .status()
        .context(ExecFailedSnafu { binary: binary.clone() })?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        return Err(ExitStatusSnafu { code }.build());
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
