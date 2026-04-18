// Error types for poiesis-verify.

use snafu::Snafu;

/// Errors that can occur during verify operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum VerifyError {
    /// Failed to evaluate an arithmetic formula.
    #[snafu(display("arithmetic evaluation failed for formula {formula:?}: {detail}"))]
    Eval {
        /// The formula that could not be evaluated.
        formula: String,
        /// Human-readable description of the parse/eval error.
        detail: String,
    },
    /// Failed to read the manifest file.
    #[snafu(display("failed to read manifest {path:?}: {source}"))]
    ReadManifest {
        /// Path that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to parse the manifest JSON.
    #[snafu(display("failed to parse manifest {path:?}: {detail}"))]
    ParseManifest {
        /// Path whose contents could not be parsed.
        path: String,
        /// JSON parse error description.
        detail: String,
    },
}
