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

/// Write a raw key/value row into a named partition of the store at `path`.
///
/// This bypasses the partition's normal JSON type so downstream crate tests
/// can create corrupt or legacy rows without depending on graphe's storage
/// backend. Opens its own fresh, short-lived handle to `path` (fjall is
/// single-writer) — call this before or after a [`crate::store::SessionStore`]
/// held by the same process is open at the same path, never while.
///
/// # Errors
/// Returns an error if the store directory, the named keyspace, transaction
/// commit, or durability flush fails.
fn inject_raw_row(path: &Path, partition: &str, key: &str, value: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| {
        storage_error(format!(
            "fjall test-support create session-store dir {}: {source}",
            path.display()
        ))
    })?;

    let db = SingleWriterTxDatabase::builder(path)
        .open()
        .map_err(|e| storage_error(format!("fjall test-support open: {e}")))?;
    let keyspace = db
        .keyspace(partition, KeyspaceCreateOptions::default)
        .map_err(|e| storage_error(format!("fjall test-support open {partition}: {e}")))?;

    let mut tx = db.write_tx();
    tx.insert(&keyspace, key, value);
    tx.commit()
        .map_err(|e| storage_error(format!("fjall test-support commit: {e}")))?;
    db.persist(PersistMode::SyncAll)
        .map_err(|e| storage_error(format!("fjall test-support persist: {e}")))?;

    Ok(())
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
    inject_raw_row(path, "sessions", key, value)
}

/// Write a raw key/value row into the `tool_audit` keyspace at `path`.
///
/// This bypasses `ToolAuditRecord` serialization so downstream crate tests
/// can plant a malformed audit row (aletheia#7217) without depending on
/// graphe's storage backend.
///
/// # Errors
/// Returns an error if the store directory, `tool_audit` keyspace,
/// transaction commit, or durability flush fails.
pub fn inject_raw_tool_audit_row(path: &Path, key: &str, value: &[u8]) -> Result<()> {
    inject_raw_row(path, "tool_audit", key, value)
}
