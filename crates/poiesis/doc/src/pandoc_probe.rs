//! Pandoc binary availability probe.
//!
//! `PandocProbe::check()` is a startup guard for the Pandoc-backed render
//! paths. It classifies the binary as present, missing, or too old so callers
//! can decide whether to render, degrade, or surface an actionable error.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::process::{CommandOutputError, output_with_timeout};
use snafu::Snafu;

/// Minimum Pandoc version required by `poiesis-doc`.
pub const REQUIRED_PANDOC_VERSION: PandocVersion = (3, 0, 0);
const PANDOC_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Semantic version triple used by the probe.
pub type PandocVersion = (u32, u32, u32);

/// Probe result for the Pandoc binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PandocProbe {
    /// `pandoc` was found and meets the minimum version requirement.
    Present {
        /// Absolute path to the binary that was selected.
        path: PathBuf,
        /// Version reported by `pandoc --version`.
        version: PandocVersion,
    },
    /// `pandoc` could not be found on PATH.
    Missing {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },
    /// `pandoc` was found but is too old for this crate.
    TooOld {
        /// Absolute path to the binary that was selected.
        path: PathBuf,
        /// Version reported by `pandoc --version`.
        found: PandocVersion,
        /// Minimum version required by the crate.
        required: PandocVersion,
    },
    /// `pandoc --version` exceeded its probe deadline.
    TimedOut {
        /// Absolute path to the binary that timed out.
        path: PathBuf,
        /// Timeout in seconds.
        timeout_secs: u64,
    },
}

impl PandocProbe {
    /// Probe for `pandoc` on `PATH`.
    ///
    /// The default implementation searches PATH for `pandoc`, runs
    /// `pandoc --version`, parses the first line, and compares the reported
    /// version against [`REQUIRED_PANDOC_VERSION`].
    #[must_use]
    pub fn check() -> Self {
        Self::check_with(find_pandoc_binary, real_version_source)
    }

    /// Convert a probe result into an actionable error.
    pub fn require(self) -> Result<(), PandocProbeError> {
        match self {
            Self::Present { .. } => Ok(()),
            Self::Missing { searched } => Err(PandocProbeError::NotInstalled { searched }),
            Self::TooOld {
                path,
                found,
                required,
            } => Err(PandocProbeError::VersionTooOld {
                path,
                found,
                required,
            }),
            Self::TimedOut { path, timeout_secs } => {
                Err(PandocProbeError::Timeout { path, timeout_secs })
            }
        }
    }

    /// Probe with injected search and version sources.
    ///
    /// This is primarily for tests: callers can provide a synthetic PATH
    /// search result and a fake `pandoc --version` payload without spawning a
    /// real Pandoc binary.
    #[must_use]
    pub(crate) fn check_with<Search, Version>(search: Search, version_source: Version) -> Self
    where
        Search: FnOnce() -> Result<PathBuf, Vec<PathBuf>>,
        Version: Fn(&Path) -> Result<String, ProbeCommandError>,
    {
        let path = match search() {
            Ok(path) => path,
            Err(searched) => return Self::Missing { searched },
        };

        let output = match version_source(&path) {
            Ok(output) => output,
            Err(ProbeCommandError::Failed(_detail)) => {
                return Self::Missing {
                    searched: vec![path],
                };
            }
            Err(ProbeCommandError::TimedOut { timeout_secs }) => {
                return Self::TimedOut { path, timeout_secs };
            }
        };

        let Some(found) = parse_pandoc_version(&output) else {
            return Self::Missing {
                searched: vec![path],
            };
        };

        if found < REQUIRED_PANDOC_VERSION {
            Self::TooOld {
                path,
                found,
                required: REQUIRED_PANDOC_VERSION,
            }
        } else {
            Self::Present {
                path,
                version: found,
            }
        }
    }
}

/// Error from the Pandoc availability check.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum PandocProbeError {
    /// `pandoc` binary not found on PATH.
    #[snafu(display(
        "pandoc not found (searched: {}). Install pandoc >= 3.0.0; on NixOS run `nix develop`, on Ubuntu/Debian run `apt install pandoc`, on macOS run `brew install pandoc`, or download from https://pandoc.org/installing.html",
        searched
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))]
    NotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },

    /// `pandoc` was found but the version is below the minimum requirement.
    #[snafu(display(
        "pandoc at {} is version {}, but >= {} is required. Upgrade via https://pandoc.org/installing.html or `nix develop`",
        path.display(),
        format_version(found),
        format_version(required)
    ))]
    VersionTooOld {
        /// Path to the too-old binary.
        path: PathBuf,
        /// Found version.
        found: PandocVersion,
        /// Minimum version required.
        required: PandocVersion,
    },

    /// The `pandoc --version` probe timed out.
    #[snafu(display(
        "pandoc at {} timed out after {timeout_secs}s while probing --version",
        path.display()
    ))]
    Timeout {
        /// Path to the binary that timed out.
        path: PathBuf,
        /// Timeout in seconds.
        timeout_secs: u64,
    },
}

#[derive(Debug)]
pub(crate) enum ProbeCommandError {
    Failed(String),
    TimedOut { timeout_secs: u64 },
}

fn find_pandoc_binary() -> Result<PathBuf, Vec<PathBuf>> {
    let searched = searched_pandoc_candidates();

    match which::which("pandoc") {
        Ok(path) => Ok(path),
        Err(_e) => Err(searched),
    }
}

fn searched_pandoc_candidates() -> Vec<PathBuf> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    std::env::split_paths(&path_var)
        .map(|dir| dir.join("pandoc"))
        .collect()
}

fn real_version_source(path: &Path) -> Result<String, ProbeCommandError> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let output = output_with_timeout(&mut cmd, PANDOC_PROBE_TIMEOUT).map_err(|err| match err {
        CommandOutputError::Timeout { timeout, .. } => ProbeCommandError::TimedOut {
            timeout_secs: timeout.as_secs(),
        },
        CommandOutputError::Spawn { source }
        | CommandOutputError::TempFile { source }
        | CommandOutputError::Wait { source } => ProbeCommandError::Failed(source.to_string()),
    })?;

    if !output.status.success() {
        return Err(ProbeCommandError::Failed(format!(
            "exit status {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_pandoc_version(output: &str) -> Option<PandocVersion> {
    let first = output.lines().next()?;
    let version = first.strip_prefix("pandoc ")?.trim();
    let mut parts = version.split('.');

    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);

    Some((major, minor, patch))
}

fn format_version(version: &PandocVersion) -> String {
    let (major, minor, patch) = *version;
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_check_returns_present_for_new_version() {
        let probe = PandocProbe::check_with(
            || Ok(PathBuf::from("/tmp/fake-pandoc")),
            |_| Ok("pandoc 3.1.13\n".to_owned()),
        );

        assert_eq!(
            probe,
            PandocProbe::Present {
                path: PathBuf::from("/tmp/fake-pandoc"),
                version: (3, 1, 13),
            }
        );
    }

    #[test]
    fn probe_check_returns_missing_when_binary_is_absent() {
        let probe = PandocProbe::check_with(
            || Err(vec![PathBuf::from("/tmp/bin/pandoc")]),
            |_| unreachable!("version source must not be called when binary is missing"),
        );

        assert_eq!(
            probe,
            PandocProbe::Missing {
                searched: vec![PathBuf::from("/tmp/bin/pandoc")],
            }
        );
    }

    #[test]
    fn probe_check_returns_too_old_for_old_version() {
        let probe = PandocProbe::check_with(
            || Ok(PathBuf::from("/tmp/fake-pandoc")),
            |_| Ok("pandoc 2.19.2\n".to_owned()),
        );

        assert_eq!(
            probe,
            PandocProbe::TooOld {
                path: PathBuf::from("/tmp/fake-pandoc"),
                found: (2, 19, 2),
                required: REQUIRED_PANDOC_VERSION,
            }
        );
    }

    #[test]
    fn probe_check_returns_timeout_for_hung_version_source() {
        let probe = PandocProbe::check_with(
            || Ok(PathBuf::from("/tmp/fake-pandoc")),
            |_| Err(ProbeCommandError::TimedOut { timeout_secs: 5 }),
        );

        assert_eq!(
            probe,
            PandocProbe::TimedOut {
                path: PathBuf::from("/tmp/fake-pandoc"),
                timeout_secs: 5,
            }
        );
    }

    #[test]
    fn require_maps_probe_states_to_actionable_errors() {
        let missing = match (PandocProbe::Missing {
            searched: vec![PathBuf::from("/tmp/bin/pandoc")],
        })
        .require()
        {
            Ok(()) => panic!("missing probe must become an error"),
            Err(err) => err,
        };

        assert!(missing.to_string().contains("pandoc not found"));

        let old = match (PandocProbe::TooOld {
            path: PathBuf::from("/tmp/fake-pandoc"),
            found: (2, 19, 2),
            required: REQUIRED_PANDOC_VERSION,
        })
        .require()
        {
            Ok(()) => panic!("too-old probe must become an error"),
            Err(err) => err,
        };

        assert!(old.to_string().contains("2.19.2"));
        assert!(old.to_string().contains("3.0.0"));
    }
}
