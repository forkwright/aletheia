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
use std::sync;

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
    /// Aletheia stores expose `&self` write methods (matching historical legacy `SQLite` backends
    /// that use interior mutability). This mutex ensures only one logical write
    /// runs at a time, matching that serial contract.
    pub write_lock: sync::Mutex<()>,
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
        let db = SingleWriterTxDatabase::builder(path).open().map_err(|e| {
            if matches!(e, fjall::Error::Locked) {
                FjallOpenError::Locked {
                    path: path.to_path_buf(),
                }
            } else {
                FjallOpenError::Open(format!("fjall open: {e}"))
            }
        })?;

        Ok(Self {
            db,
            write_lock: sync::Mutex::new(()),
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

        let db = SingleWriterTxDatabase::builder(path).open().map_err(|e| {
            if matches!(e, fjall::Error::Locked) {
                FjallOpenError::Locked {
                    path: path.to_path_buf(),
                }
            } else {
                FjallOpenError::Open(format!("fjall open: {e}"))
            }
        })?;

        for name in partitions {
            db.keyspace(name, KeyspaceCreateOptions::default)
                .map_err(|e| FjallOpenError::Open(format!("fjall open partition {name}: {e}")))?;
        }

        Ok(Self {
            db,
            write_lock: sync::Mutex::new(()),
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
            write_lock: sync::Mutex::new(()),
            _temp_dir: Some(dir),
        })
    }
}

/// Errors from [`FjallDb::open`] and [`FjallDb::open_temp`].
///
/// Callers map these into their crate-specific error type.
#[derive(Debug)]
#[non_exhaustive]
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
    /// The fjall keyspace at `path` is locked by another process holding the
    /// exclusive file lock (typically a running aletheia server).
    Locked {
        /// The locked store path.
        path: std::path::PathBuf,
    },
}

impl std::fmt::Display for FjallOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDir { path, source } => {
                write!(f, "failed to create directory {}: {source}", path.display())
            }
            Self::TempDir { source } => write!(f, "failed to create temp directory: {source}"),
            Self::Open(msg) => f.write_str(msg),
            Self::Locked { path } => write!(
                f,
                "the aletheia store at {} is locked — a running aletheia server holds it. Stop the server, or use the HTTP API (e.g. `aletheia memory`, `aletheia maintenance`, `aletheia session-export --url ...`) which talks to the running server instead of opening the store directly.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FjallOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir { source, .. } | Self::TempDir { source } => Some(source),
            Self::Open(_) | Self::Locked { .. } => None,
        }
    }
}

/// ISO 8601 UTC timestamp string for "now" using jiff.
///
/// Shared across fjall-backed stores that need consistent timestamp formatting.
///
/// # Canonical format
///
/// The returned string is always UTC, with millisecond precision and a literal
/// `Z` suffix: `YYYY-MM-DDTHH:MM:SS.sssZ`. Callers must NOT use local wall time
/// with a literal `Z`, because that mislabels non-UTC timestamps as UTC and
/// corrupts lexicographic ordering, TTL comparisons, and retention logic.
pub fn now_iso() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
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
    fn locked_error_includes_path() {
        let path = std::path::PathBuf::from("/tmp/aletheia-store");
        let err = FjallOpenError::Locked { path: path.clone() };
        let rendered = err.to_string();

        assert!(
            rendered.contains("is locked"),
            "missing lock hint: {rendered}"
        );
        assert!(
            rendered.contains(path.to_string_lossy().as_ref()),
            "missing path in message: {rendered}"
        );
    }

    #[test]
    fn now_iso_format() {
        let ts = now_iso();
        // Must match ISO 8601 with 3-digit milliseconds and Z suffix.
        assert!(ts.ends_with('Z'), "timestamp must end with Z: {ts}");
        assert!(ts.contains('T'), "timestamp must contain T separator: {ts}");
        assert_eq!(ts.len(), 24, "expected 24-char ISO timestamp: {ts}");
    }

    #[test]
    #[expect(
        unsafe_code,
        reason = "WHY(#4742): single-threaded test; TZ env mutation is isolated by catch_unwind and unconditionally restored before the function returns"
    )]
    fn now_iso_is_utc_under_non_utc_timezone() {
        // WHY(#4742): the helper must produce real UTC timestamps even when the
        // host uses a non-UTC timezone. A local wall-time value labeled with a
        // literal `Z` would shift ordering, TTL comparisons, and retention by
        // hours on non-UTC hosts.
        let original = std::env::var("TZ").ok();
        // SAFETY: tests are single-threaded; env mutation is acceptable in test
        // isolation. jiff reads TZ when resolving the local timezone.
        unsafe { std::env::set_var("TZ", "Pacific/Auckland") };
        let result = std::panic::catch_unwind(|| {
            let ts = now_iso();
            assert!(
                ts.ends_with('Z'),
                "timestamp must carry UTC indicator: {ts}"
            );
            let parsed = ts.parse::<jiff::Timestamp>().expect("valid ISO 8601");
            let now = jiff::Timestamp::now();
            let diff = parsed.duration_since(now);
            assert!(
                diff <= jiff::SignedDuration::from_secs(5)
                    && diff >= jiff::SignedDuration::from_secs(-5),
                "timestamp {ts} is not near UTC now"
            );
        });
        match original {
            Some(v) => unsafe { std::env::set_var("TZ", v) },
            None => unsafe { std::env::remove_var("TZ") },
        }
        result.expect("test did not panic");
    }
}
