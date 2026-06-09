//! System `LaTeX` engine availability probe.
//!
//! `LatexProbe::check()` classifies the system `xelatex`/`lualatex` path so
//! the PDF dispatcher can choose a system engine without bundling `TeX`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::pandoc::PdfEngine;
use snafu::Snafu;

/// Probe result for a system `LaTeX` engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatexProbe {
    /// A usable system engine was found.
    Present {
        /// Selected PDF engine.
        engine: PdfEngine,
        /// Absolute path to the chosen binary.
        path: PathBuf,
        /// First line reported by `--version`.
        version: String,
    },
    /// Neither `xelatex` nor `lualatex` could be used.
    Missing {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },
}

impl LatexProbe {
    /// Probe once and cache the first successful result for the process.
    #[must_use]
    pub fn check() -> Self {
        static CACHE: OnceLock<LatexProbe> = OnceLock::new();
        CACHE
            .get_or_init(|| Self::check_with(search_latex_candidates, real_version_source))
            .clone()
    }

    /// Convert a probe result into the selected engine or an actionable error.
    pub fn engine(self) -> Result<PdfEngine, LatexProbeError> {
        match self {
            Self::Present { engine, .. } => Ok(engine),
            Self::Missing { searched } => Err(LatexProbeError::NotInstalled { searched }),
        }
    }

    /// Probe with injected search and version sources.
    ///
    /// This is primarily for tests: callers can provide a synthetic PATH
    /// search result and a fake `--version` payload without spawning a real
    /// `LaTeX` binary.
    #[must_use]
    pub(crate) fn check_with<Search, Version>(search: Search, version_source: Version) -> Self
    where
        Search: FnOnce() -> Vec<(PdfEngine, PathBuf)>,
        Version: Fn(&Path) -> Result<String, String>,
    {
        let mut searched = Vec::new();

        for (engine, path) in search() {
            searched.push(path.clone());

            let Ok(output) = version_source(&path) else {
                continue;
            };

            let version = output.lines().next().unwrap_or_default().trim().to_owned();
            if version.is_empty() {
                continue;
            }

            return Self::Present {
                engine,
                path,
                version,
            };
        }

        Self::Missing { searched }
    }
}

/// Error returned when the system `LaTeX` engine probe cannot find a usable binary.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum LatexProbeError {
    /// Neither `xelatex` nor `lualatex` was usable on PATH.
    #[snafu(display(
        "latex engine not found (searched: {}). Install xelatex or lualatex via a TeX distribution such as TeX Live or MacTeX; on NixOS run `nix develop`",
        format_paths(searched)
    ))]
    NotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },
}

fn search_latex_candidates() -> Vec<(PdfEngine, PathBuf)> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    for dir in std::env::split_paths(&path_var) {
        let path = dir.join("xelatex");
        if path.exists() {
            candidates.push((PdfEngine::XeLaTeX, path));
        }
    }
    for dir in std::env::split_paths(&path_var) {
        let path = dir.join("lualatex");
        if path.exists() {
            candidates.push((PdfEngine::LuaLaTeX, path));
        }
    }

    candidates
}

fn real_version_source(path: &Path) -> Result<String, String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(format!("exit status {}", output.status));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn format_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "<none>".to_owned();
    }

    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_with_returns_xelatex_when_available() {
        let probe = LatexProbe::check_with(
            || vec![(PdfEngine::XeLaTeX, PathBuf::from("/tmp/xelatex"))],
            |path| {
                assert_eq!(path, Path::new("/tmp/xelatex"));
                Ok("XeTeX 3.141592653-2.6-0.999994 (TeX Live 2024)\n".to_owned())
            },
        );

        assert_eq!(
            probe,
            LatexProbe::Present {
                engine: PdfEngine::XeLaTeX,
                path: PathBuf::from("/tmp/xelatex"),
                version: "XeTeX 3.141592653-2.6-0.999994 (TeX Live 2024)".to_owned(),
            }
        );
    }

    #[test]
    fn check_with_falls_back_to_lualatex_when_xelatex_fails() {
        let probe = LatexProbe::check_with(
            || {
                vec![
                    (PdfEngine::XeLaTeX, PathBuf::from("/tmp/xelatex")),
                    (PdfEngine::LuaLaTeX, PathBuf::from("/tmp/lualatex")),
                ]
            },
            |path| {
                if path == Path::new("/tmp/xelatex") {
                    Err("broken xelatex".to_owned())
                } else {
                    Ok("LuaHBTeX, Version 1.18.0 (TeX Live 2023)\n".to_owned())
                }
            },
        );

        assert_eq!(
            probe,
            LatexProbe::Present {
                engine: PdfEngine::LuaLaTeX,
                path: PathBuf::from("/tmp/lualatex"),
                version: "LuaHBTeX, Version 1.18.0 (TeX Live 2023)".to_owned(),
            }
        );
    }

    #[test]
    fn engine_maps_missing_probe_to_actionable_error() {
        let Err(err) = (LatexProbe::Missing {
            searched: vec![
                PathBuf::from("/tmp/xelatex"),
                PathBuf::from("/tmp/lualatex"),
            ],
        })
        .engine() else {
            panic!("missing probe must become an error")
        };

        let message = err.to_string();
        assert!(message.contains("latex engine not found"));
        assert!(message.contains("xelatex"));
        assert!(message.contains("lualatex"));
    }
}
