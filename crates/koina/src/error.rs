//! Error types for Aletheia.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from core operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to write a file.
    #[snafu(display("failed to write {}", path.display()))]
    WriteFile {
        /// The path that could not be written.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to create a directory.
    #[snafu(display("failed to create directory {}", path.display()))]
    CreateDir {
        /// The directory path that could not be created.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to serialize to JSON.
    #[snafu(display("JSON serialization failed"))]
    JsonSerialize {
        /// The underlying serialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to deserialize from JSON.
    #[snafu(display("JSON deserialization failed"))]
    JsonDeserialize {
        /// The underlying deserialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// An identifier was invalid.
    #[snafu(display("invalid identifier: {message}"))]
    InvalidId {
        /// Description of why the identifier is invalid.
        message: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use snafu::ResultExt;

    #[test]
    fn error_display_includes_path() {
        let err: Result<Vec<u8>> = std::fs::read("/nonexistent/path").context(ReadFileSnafu {
            path: PathBuf::from("/nonexistent/path"),
        });
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/path"));
    }

    #[test]
    fn error_source_chain() {
        let err: Result<Vec<u8>> = std::fs::read("/nonexistent/path").context(ReadFileSnafu {
            path: PathBuf::from("/nonexistent/path"),
        });
        let err = err.unwrap_err();
        // NOTE: snafu chains preserve the source error
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn json_deserialize_error() {
        let err: Result<serde_json::Value> =
            serde_json::from_str("not json").context(JsonDeserializeSnafu);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("JSON"));
    }

    #[test]
    fn write_file_error_display() {
        let err: Result<()> =
            std::fs::write("/nonexistent/dir/file.txt", "data").context(WriteFileSnafu {
                path: PathBuf::from("/nonexistent/dir/file.txt"),
            });
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/dir/file.txt"));
    }

    #[test]
    fn create_dir_error_display() {
        let err: Result<()> =
            std::fs::create_dir("/nonexistent/parent/child").context(CreateDirSnafu {
                path: PathBuf::from("/nonexistent/parent/child"),
            });
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("/nonexistent/parent/child"));
    }

    #[test]
    fn invalid_id_error_display() {
        let err = Error::InvalidId {
            message: "bad format".to_owned(),
            location: snafu::Location::new("test", 0, 0),
        };
        assert!(err.to_string().contains("bad format"));
    }

    #[test]
    fn errors_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }
}
