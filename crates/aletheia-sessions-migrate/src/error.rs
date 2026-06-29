//! Error type for the migrator library.
//!
//! Per `STANDARDS/RUST.md`, library crates use snafu for error context;
//! `anyhow` is reserved for the binary surface.

use std::path::PathBuf;

use snafu::Snafu;

/// Result alias for migrator operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Migrator error surface.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, message) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum Error {
    #[snafu(display("opening SQLite source at {}: {source}", path.display()))]
    SqliteOpen {
        path: PathBuf,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("SQLite query error ({context}): {source}"))]
    Sqlite {
        context: String,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "legacy session column '{column}' for session '{session_id}' could not be read: {source}"
    ))]
    LegacyExtraRead {
        session_id: String,
        column: String,
        source: Box<rusqlite::Error>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "unknown legacy enum value in {table}.{column} for row '{row_id}': {raw_value:?}"
    ))]
    UnknownLegacyEnum {
        table: String,
        row_id: String,
        column: String,
        raw_value: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "schema mismatch: expected user_version = {expected}, found {found}; \
         this migrator only supports the final pre-fjall SQLite schema (PR #3446)"
    ))]
    SchemaUserVersion {
        expected: i64,
        found: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("schema mismatch: required table '{table}' not present in source DB"))]
    SchemaMissingTable {
        table: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "schema mismatch: table '{table}' missing column '{column}' (found columns: {found:?})"
    ))]
    SchemaMissingColumn {
        table: String,
        column: String,
        found: Vec<String>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("opening fjall destination at {}: {message}", path.display()))]
    FjallOpen {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "destination '{}' is non-empty; pass --replace-existing --i-understand-this-replaces-destination to replace it. \
         Replacement writes and verifies staging first, moves the current destination to a temporary backup during publish, \
         and deletes that backup after publish succeeds",
        path.display()
    ))]
    DestinationNotEmpty {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fjall partition '{partition}': {message}"))]
    FjallPartition {
        partition: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fjall {operation} failed: {message}"))]
    FjallOp {
        operation: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("JSON {operation}: {source}"))]
    Json {
        operation: String,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("graphe SessionStore error: {source}"))]
    Graphe {
        source: graphe::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("io error ({context}): {source}"))]
    Io {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "destination '{}' is incomplete: previous migration left {} behind. \
         Pass --replace-existing --i-understand-this-replaces-destination to remove the leftover staging directory and rerun, \
         or inspect it manually",
        path.display(),
        marker
    ))]
    MigrationIncomplete {
        path: PathBuf,
        marker: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "atomic rename failed: {} -> {}: {source}",
        source_path.display(),
        dest_path.display()
    ))]
    AtomicRenameFailed {
        source_path: PathBuf,
        dest_path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "{field} value {value} cannot be encoded as a u64 (must be non-negative and fit in 64 bits)"
    ))]
    NumericRange {
        field: String,
        value: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("verification failed before publish: {mismatches} mismatch(es): {summary}"))]
    VerificationFailed {
        mismatches: usize,
        summary: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
