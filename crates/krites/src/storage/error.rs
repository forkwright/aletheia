//! Error types for the storage layer.
use snafu::Snafu;

/// Errors from the engine storage backends (fjall, mem, temp).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum StorageError {
    /// A storage backend operation failed (e.g., `begin_write`, `open_table`, commit).
    #[snafu(display("transaction failed ({backend}): {message}"))]
    TransactionFailed {
        backend: &'static str,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The on-disk store is locked by another process.
    ///
    /// WHY: fjall takes an exclusive lock per keyspace; without a dedicated
    /// variant the lock surfaces as an opaque "transaction failed: open"
    /// message. Mirrors `koina::fjall::FjallOpenError::Locked` so every
    /// store (session, auth, knowledge) reports lock contention the same
    /// actionable way.
    #[snafu(display(
        "the store at {} is locked — a running aletheia server holds it. Stop the server, or use the HTTP API (e.g. `aletheia memory`, `aletheia maintenance`) which talks to the running server instead of opening the store directly.",
        path.display()
    ))]
    Locked {
        path: std::path::PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Attempted a write operation on a read-only transaction.
    #[snafu(display("write attempted on a read-only transaction"))]
    WriteInReadTransaction {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Data corruption detected in storage.
    #[snafu(display("corrupted data: {message}"))]
    CorruptedData {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage I/O error (e.g., creating directories, reading files).
    #[snafu(display("storage I/O error ({backend}): {source}"))]
    Io {
        backend: &'static str,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Key encoding/decoding error.
    #[snafu(display("key encoding error: {message}"))]
    KeyEncoding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub(crate) type StorageResult<T> = std::result::Result<T, StorageError>;
