use std::path::PathBuf;

/// Errors from author-classifier operations.
#[derive(Debug, snafu::Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, path) are self-documenting via display format"
)]
#[non_exhaustive]
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

    /// Artifact declares the classes in an order the runtime cannot interpret.
    #[snafu(display(
        "classifier artifact class order mismatch: expected [{expected}], artifact declares [{actual}]"
    ))]
    ClassOrderMismatch { expected: String, actual: String },

    /// Calibration artifact schema version is incompatible with this runtime.
    #[snafu(display(
        "calibration artifact version incompatible: artifact schema {artifact_schema}, runtime expects {runtime_schema}"
    ))]
    CalibrationVersionMismatch {
        artifact_schema: String,
        runtime_schema: String,
    },

    /// Calibration artifact declares a per-class confidence threshold
    /// outside the valid `[0.0, 1.0]` range.
    #[snafu(display(
        "calibration threshold out of range for class index {index}: {value} (expected 0.0..=1.0)"
    ))]
    InvalidCalibrationThreshold { index: usize, value: f32 },

    /// Failed to load calibration artifact from the filesystem.
    #[snafu(display("failed to load classifier calibration from {}: {source}", path.display()))]
    CalibrationMissing {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to parse calibration JSON.
    #[snafu(display("failed to parse classifier calibration: {source}"))]
    InvalidCalibrationJson { source: serde_json::Error },
}

/// Result type alias for author-classifier operations.
pub type Result<T> = std::result::Result<T, ClassifyError>;
