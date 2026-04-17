//! Shared fjall storage helpers for Aletheia crates.
//!
//! Four crates (graphe, symbolon, daemon, energeia) use fjall as an LSM-tree
//! storage backend. This module extracts the common initialization patterns so
//! each crate only contains domain-specific logic.
//!
//! # Usage
//!
//! ```rust,ignore
//! use koina::fjall::FjallDb;
//!
//! // Persistent store
//! let db = FjallDb::open(path, &["sessions", "messages"])?;
//!
//! // Ephemeral store (tests)
//! let db = FjallDb::open_temp(&["sessions", "messages"])?;
//! ```

use std::path::Path;
use std::sync::Mutex;

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};

/// A fjall database handle with a write-serialization mutex and optional temp
/// directory for ephemeral (test) stores.
///
/// The `_temp_dir` field's `Drop` implementation deletes the temporary directory
/// when the store is dropped. The leading underscore suppresses `dead_code`
/// warnings since the field is only needed for its `Drop` side effect.
pub struct FjallDb {
    /// The underlying fjall database.
    pub db: SingleWriterTxDatabase,
    /// Shared write mutex.
    ///
    /// WHY: `SingleWriterTxDatabase` serialises writers internally, but many
    /// Aletheia stores expose `&self` write methods (matching the `SQLite` backends
    /// that use interior mutability). This mutex ensures only one logical write
    /// runs at a time, matching that serial contract.
    pub write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    ///
    /// WHY: the leading underscore signals that the field is unused for its value
    /// but needed for its `Drop` side effect. Clippy flags this as
    /// `pub_underscore_fields`, but consumers destructure `FjallDb` and need
    /// access to transfer ownership of the temp directory into their own struct.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "consumers destructure FjallDb and transfer ownership of the TempDir guard"
    )]
    pub _temp_dir: Option<tempfile::TempDir>,
}

impl FjallDb {
    /// Open an existing persistent fjall database at `path` without creating
    /// any partitions.
    ///
    /// # Errors
    ///
    /// Returns a `String` error message if the database cannot be opened.
    pub fn open_existing(path: &Path) -> Result<Self, FjallOpenError> {
        let db = SingleWriterTxDatabase::builder(path)
            .open()
            .map_err(|e| FjallOpenError::Open(format!("fjall open: {e}")))?;

        Ok(Self {
            db,
            write_lock: Mutex::new(()),
            _temp_dir: None,
        })
    }

    /// Open (or create) a persistent fjall database at `path`, eagerly creating
    /// all named partitions.
    ///
    /// # Errors
    ///
    /// Returns a `String` error message if directory creation, database open, or
    /// partition initialization fails. Callers should map this into their
    /// crate-specific error type.
    pub fn open(path: &Path, partitions: &[&str]) -> Result<Self, FjallOpenError> {
        std::fs::create_dir_all(path).map_err(|source| FjallOpenError::CreateDir {
            path: path.to_path_buf(),
            source,
        })?;

        let db = SingleWriterTxDatabase::builder(path)
            .open()
            .map_err(|e| FjallOpenError::Open(format!("fjall open: {e}")))?;

        for name in partitions {
            db.keyspace(name, KeyspaceCreateOptions::default)
                .map_err(|e| FjallOpenError::Open(format!("fjall open partition {name}: {e}")))?;
        }

        Ok(Self {
            db,
            write_lock: Mutex::new(()),
            _temp_dir: None,
        })
    }

    /// Open an ephemeral fjall database backed by a `TempDir`, eagerly creating
    /// all named partitions.
    ///
    /// The directory and all data are deleted when the returned `FjallDb` is
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns a `String` error message if temp-dir creation, database open, or
    /// partition initialization fails.
    pub fn open_temp(partitions: &[&str]) -> Result<Self, FjallOpenError> {
        let dir = tempfile::TempDir::new().map_err(|source| FjallOpenError::TempDir { source })?;

        let db = SingleWriterTxDatabase::builder(dir.path())
            .open()
            .map_err(|e| FjallOpenError::Open(format!("fjall open temp: {e}")))?;

        for name in partitions {
            db.keyspace(name, KeyspaceCreateOptions::default)
                .map_err(|e| FjallOpenError::Open(format!("fjall open partition {name}: {e}")))?;
        }

        Ok(Self {
            db,
            write_lock: Mutex::new(()),
            _temp_dir: Some(dir),
        })
    }
}

/// Errors from [`FjallDb::open`] and [`FjallDb::open_temp`].
///
/// Callers map these into their crate-specific error type.
#[derive(Debug)]
pub enum FjallOpenError {
    /// Failed to create the store directory.
    CreateDir {
        /// The path that could not be created.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to create a temporary directory.
    TempDir {
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to open the fjall database or a partition.
    Open(String),
}

impl std::fmt::Display for FjallOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDir { path, source } => {
                write!(f, "failed to create directory {}: {source}", path.display())
            }
            Self::TempDir { source } => write!(f, "failed to create temp directory: {source}"),
            Self::Open(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for FjallOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir { source, .. } | Self::TempDir { source } => Some(source),
            Self::Open(_) => None,
        }
    }
}

/// ISO 8601 timestamp string for "now" using jiff.
///
/// Shared across fjall-backed stores that need consistent timestamp formatting.
pub fn now_iso() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn open_temp_creates_partitions() {
        let db = FjallDb::open_temp(&["p1", "p2"]).unwrap();
        // Verify partitions exist by opening them again (idempotent).
        let ks = db
            .db
            .keyspace("p1", KeyspaceCreateOptions::default)
            .unwrap();
        ks.insert("test-key", b"test-val").unwrap();
    }

    #[test]
    fn open_persistent_creates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("subdir").join("db");
        let db = FjallDb::open(&db_path, &["data"]).unwrap();
        assert!(db_path.exists());
        let ks = db
            .db
            .keyspace("data", KeyspaceCreateOptions::default)
            .unwrap();
        ks.insert("k", b"v").unwrap();
    }

    #[test]
    fn now_iso_format() {
        let ts = now_iso();
        // Must match ISO 8601 with 3-digit milliseconds and Z suffix.
        assert!(ts.ends_with('Z'), "timestamp must end with Z: {ts}");
        assert!(ts.contains('T'), "timestamp must contain T separator: {ts}");
        assert_eq!(ts.len(), 24, "expected 24-char ISO timestamp: {ts}");
    }
}
