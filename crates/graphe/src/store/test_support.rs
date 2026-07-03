//! Test-support helpers for backend-agnostic session-store fixtures.

use std::path::Path;

use fjall::{KeyspaceCreateOptions, PersistMode, SingleWriterTxDatabase};

use crate::error::{self, Result};

fn storage_error(message: impl Into<String>) -> error::Error {
    error::StorageSnafu {
        message: message.into(),
    }
    .build()
}

/// Write a raw key/value row into the session keyspace at `path`.
///
/// This bypasses `Session` serialization so downstream crate tests can create
/// corrupt or legacy rows without depending on graphe's storage backend.
///
/// # Errors
/// Returns an error if the store directory, session keyspace, transaction
/// commit, or durability flush fails.
pub fn inject_raw_session_row(path: &Path, key: &str, value: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| {
        storage_error(format!(
            "fjall test-support create session-store dir {}: {source}",
            path.display()
        ))
    })?;

    let db = SingleWriterTxDatabase::builder(path)
        .open()
        .map_err(|e| storage_error(format!("fjall test-support open: {e}")))?;
    let sessions = db
        .keyspace("sessions", KeyspaceCreateOptions::default)
        .map_err(|e| storage_error(format!("fjall test-support open sessions: {e}")))?;

    let mut tx = db.write_tx();
    tx.insert(&sessions, key, value);
    tx.commit()
        .map_err(|e| storage_error(format!("fjall test-support commit: {e}")))?;
    db.persist(PersistMode::SyncAll)
        .map_err(|e| storage_error(format!("fjall test-support persist: {e}")))?;

    Ok(())
}
