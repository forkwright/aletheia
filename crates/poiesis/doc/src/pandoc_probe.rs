//! Pandoc binary availability probe.
//!
//! Call [`PandocProbe::check`] before invoking any Pandoc render path.
//! If the binary is missing the error message tells the operator exactly
//! how to fix it (run `nix develop` — pandoc is pinned in `flake.nix`).

use std::path::PathBuf;
use std::process::Command;

use snafu::Snafu;

/// Confirmed pandoc installation: binary path and reported version.
#[derive(Debug, Clone)]
pub struct PandocProbe {
    /// Absolute path to the `pandoc` binary.
    pub path: PathBuf,
    /// Version string reported by `pandoc --version`, e.g. `"3.1.13"`.
    pub version: String,
}

impl PandocProbe {
    /// Probe for `pandoc` on `PATH`.
    ///
    /// Returns the binary path and version on success. Returns an error with
    /// an actionable message pointing at `nix develop` if pandoc is absent
    /// or unresponsive.
    ///
    /// # Errors
    ///
    /// - [`PandocProbeError::NotFound`] when the binary is not on `PATH`.
    /// - [`PandocProbeError::VersionCheckFailed`] when `pandoc --version` fails.
    pub fn check() -> Result<Self, PandocProbeError> {
        let path = which::which("pandoc").map_err(|_e| PandocProbeError::NotFound)?;

        let output = Command::new(&path).arg("--version").output().map_err(|e| {
            PandocProbeError::VersionCheckFailed {
                path: path.clone(),
                detail: e.to_string(),
            }
        })?;

        if !output.status.success() {
            return Err(PandocProbeError::VersionCheckFailed {
                path,
                detail: format!("exit status {}", output.status),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("unknown")
            .to_owned();

        Ok(Self { path, version })
    }
}

/// Error from the pandoc availability check.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum PandocProbeError {
    /// `pandoc` binary not found on `PATH`.
    #[snafu(display(
        "pandoc not found on PATH — install via `nix develop` \
         (pandoc is pinned in flake.nix), or use the `pdf` format which needs no pandoc"
    ))]
    NotFound,

    /// `pandoc --version` found the binary but the command failed.
    #[snafu(display("pandoc found at {} but `pandoc --version` failed: {detail}", path.display()))]
    VersionCheckFailed {
        /// Path where pandoc was found.
        path: PathBuf,
        /// Human-readable failure reason.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_check_returns_result() {
        // Passes whether pandoc is present or absent — must not panic.
        let _ = PandocProbe::check();
    }

    #[test]
    fn probe_error_message_is_actionable() {
        let msg = PandocProbeError::NotFound.to_string();
        assert!(
            msg.contains("nix develop"),
            "error must mention `nix develop`"
        );
        assert!(msg.contains("flake.nix"), "error must mention flake.nix");
    }
}
