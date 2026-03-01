//! Error types for Aletheia.
//!
//! Each crate defines its own error enum using `snafu`. This module provides
//! the koina-level errors (config, I/O, serialization) that other crates wrap.

use snafu::Snafu;
use std::path::PathBuf;

/// Errors from core operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write a file.
    #[snafu(display("failed to write {}", path.display()))]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to create a directory.
    #[snafu(display("failed to create directory {}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize to JSON.
    #[snafu(display("JSON serialization failed"))]
    JsonSerialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to deserialize from JSON.
    #[snafu(display("JSON deserialization failed"))]
    JsonDeserialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An identifier was invalid.
    #[snafu(display("invalid identifier: {message}"))]
    InvalidId {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use snafu::ResultExt;

    #[test]
    fn error_display_includes_path() {
        let err: Result<Vec<u8>> =
            std::fs::read("/nonexistent/path").context(ReadFileSnafu {
                path: PathBuf::from("/nonexistent/path"),
            });
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/path"));
    }

    #[test]
    fn error_source_chain() {
        let err: Result<Vec<u8>> =
            std::fs::read("/nonexistent/path").context(ReadFileSnafu {
                path: PathBuf::from("/nonexistent/path"),
            });
        let err = err.unwrap_err();
        // snafu chains preserve the source
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn json_deserialize_error() {
        let err: Result<serde_json::Value> =
            serde_json::from_str("not json").context(JsonDeserializeSnafu);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("JSON"));
    }
}
