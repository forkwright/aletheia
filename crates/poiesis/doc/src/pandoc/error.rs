use std::path::PathBuf;

use snafu::Snafu;

/// Errors produced by the Pandoc subprocess wrapper.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum PandocError {
    /// `pandoc` binary was not found on PATH.
    ///
    /// Install pandoc ≥ 3.0. On NixOS/nix: `nix develop` (pinned in flake).
    /// On Ubuntu/Debian: `apt install pandoc`. See <https://pandoc.org/installing.html>.
    #[snafu(display(
        "pandoc not found (searched: {}). Install pandoc \u{2265} 3.0 \
         — on NixOS run `nix develop` (pinned in flake.nix); \
         on Ubuntu/Debian `apt install pandoc`; see https://pandoc.org/installing.html",
        searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))]
    NotInstalled {
        /// Directories and paths that were searched.
        searched: Vec<PathBuf>,
    },

    /// `pandoc` was found but the version is below the minimum requirement.
    #[snafu(display(
        "pandoc at {} is version {found_major}.{found_minor}.{found_patch}, \
         but \u{2265} 3.0 is required. Run `nix develop` or upgrade via https://pandoc.org/installing.html",
        path.display()
    ))]
    VersionTooOld {
        /// Path to the too-old binary.
        path: PathBuf,
        /// Found major version.
        found_major: u32,
        /// Found minor version.
        found_minor: u32,
        /// Found patch version.
        found_patch: u32,
    },

    /// A LaTeX-backed PDF export needed `xelatex` or `lualatex`, but neither was usable.
    #[snafu(display(
        "latex engine not found (searched: {}). Install xelatex or lualatex via a TeX distribution such as TeX Live or MacTeX; on NixOS run `nix develop`",
        searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))]
    LatexEngineNotInstalled {
        /// Candidate paths that were searched.
        searched: Vec<PathBuf>,
    },

    /// The Pandoc writer returned a non-zero exit code.
    #[snafu(display("pandoc writer failed for format {fmt}: {stderr}"))]
    WriterFailed {
        /// The output format being written.
        fmt: String,
        /// stderr output from the Pandoc process.
        stderr: String,
    },

    /// Pandoc process could not be spawned (I/O error).
    #[snafu(display("failed to spawn pandoc: {source}"))]
    Spawn {
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Pandoc subprocess exceeded its deadline.
    #[snafu(display("pandoc {operation} timed out after {timeout_secs}s"))]
    Timeout {
        /// Operation being performed.
        operation: String,
        /// Timeout in seconds.
        timeout_secs: u64,
        /// Child-process kill error, when cleanup failed.
        kill_error: Option<String>,
        /// Child-process wait error, when cleanup failed.
        wait_error: Option<String>,
    },

    /// Pandoc subprocess I/O failed after spawning.
    #[snafu(display("pandoc {operation} I/O failed: {source}"))]
    SubprocessIo {
        /// Operation being performed.
        operation: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to write the AST temp file.
    #[snafu(display("failed to write pandoc AST temp file: {source}"))]
    TempFile {
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The in-memory Pandoc AST could not be serialized to JSON.
    #[snafu(display("failed to serialize document to Pandoc AST JSON: {source}"))]
    AstSerialize {
        /// Underlying JSON serialization error.
        source: serde_json::Error,
    },

    /// A chart figure could not be rendered into SVG.
    #[snafu(display("failed to render figure {figure_id}: {source}"))]
    FigureRenderFailed {
        /// Stable figure identifier.
        figure_id: String,
        /// Underlying figure rendering error.
        source: super::figure::FigureError,
    },

    /// A chart figure SVG could not be rasterized into PNG.
    #[snafu(display("failed to rasterize figure {figure_id}: {source}"))]
    FigureRasterizeFailed {
        /// Stable figure identifier.
        figure_id: String,
        /// Underlying SVG rasterization error.
        source: crate::raster::RasterError,
    },
}

impl From<crate::latex_probe::LatexProbeError> for PandocError {
    fn from(err: crate::latex_probe::LatexProbeError) -> Self {
        match err {
            crate::latex_probe::LatexProbeError::NotInstalled { searched } => {
                Self::LatexEngineNotInstalled { searched }
            }
            crate::latex_probe::LatexProbeError::Timeout { path, timeout_secs } => Self::Timeout {
                operation: format!("latex probe {}", path.display()),
                timeout_secs,
                kill_error: None,
                wait_error: None,
            },
        }
    }
}
