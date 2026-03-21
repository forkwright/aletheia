//! Error types for the storage layer.
use snafu::Snafu;

/// Errors from the engine storage backends (redb, fjall, mem, temp).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum StorageError {
    /// A storage backend operation failed (e.g., begin_write, open_table, commit).
    #[snafu(display("transaction failed ({backend}): {message}"))]
    TransactionFailed {
        backend: &'static str,
        message: String,
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
