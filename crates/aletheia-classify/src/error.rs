use std::path::PathBuf;

/// Errors from author-classifier operations.
#[derive(Debug, snafu::Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, path) are self-documenting via display format"
)]
pub enum ClassifyError {
    /// Failed to load classifier artifact from the filesystem.
    #[snafu(display("failed to load classifier artifact from {}: {source}", path.display()))]
    ArtifactMissing {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Classifier artifact version is incompatible with this runtime.
    #[snafu(display(
        "classifier artifact version incompatible: artifact schema {artifact_schema}, runtime expects {runtime_schema}"
    ))]
    VersionMismatch {
        artifact_schema: String,
        runtime_schema: String,
    },

    /// Failed to parse metadata JSON.
    #[snafu(display("failed to parse classifier metadata: {source}"))]
    InvalidMetadata { source: serde_json::Error },

    /// Input text is too long for classification.
    #[snafu(display("text too long for classification (max 100000 chars): {len} chars"))]
    TextTooLong { len: usize },

    /// Model produced invalid output shape.
    #[snafu(display(
        "classification produced invalid output shape (expected 4-element array, got {len} elements)"
    ))]
    InvalidOutputShape { len: usize },
}

/// Result type alias for author-classifier operations.
pub type Result<T> = std::result::Result<T, ClassifyError>;
