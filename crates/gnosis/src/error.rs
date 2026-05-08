//! Error types for the `gnosis` crate.
//!
//! All errors are represented via the `snafu` pattern: variants carry context
//! (source file, workspace path, etc.) so callers get actionable messages.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors produced by gnosis operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum GnosisError {
    /// `cargo metadata` failed to run or returned a non-zero exit.
    #[snafu(display("cargo metadata failed: {source}"))]
    CargoMetadata { source: cargo_metadata::Error },

    /// A Rust source file could not be read.
    #[snafu(display("failed to read source file {}: {source}", path.display()))]
    ReadSource {
        path: PathBuf,
        source: std::io::Error,
    },

    /// `syn` failed to parse a Rust source file.
    #[snafu(display("failed to parse {}: {source}", path.display()))]
    Parse { path: PathBuf, source: syn::Error },

    /// A `SQLite` operation failed.
    #[snafu(display("sqlite error: {source}"))]
    Sqlite { source: rusqlite::Error },

    /// The index cache directory could not be created.
    #[snafu(display("failed to create cache directory {}: {source}", dir.display()))]
    CreateCacheDir {
        dir: PathBuf,
        source: std::io::Error,
    },

    /// The stale index cache file could not be removed.
    #[snafu(display("failed to remove stale cache file {}: {source}", path.display()))]
    RemoveCacheFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A query was passed an unsupported operation string.
    #[snafu(display("unknown query operation: '{op}'"))]
    UnknownOp { op: String },

    /// A required argument was missing from a query.
    #[snafu(display("missing required argument '{arg}' for query '{query}'"))]
    MissingArg {
        arg: &'static str,
        query: &'static str,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, GnosisError>;
