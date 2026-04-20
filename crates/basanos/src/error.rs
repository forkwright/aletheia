//! Error types for basanos.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors that can occur during linting.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read file {}", path.display()))]
    ReadFile {
        /// Path to the file.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to scan directory.
    #[snafu(display("failed to read directory {}", path.display()))]
    ReadDir {
        /// Path to the directory.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}

/// Shorthand for fallible operations.
pub(crate) type Result<T> = std::result::Result<T, Error>;
