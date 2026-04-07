//! Storage backend abstraction for krites v2.
//!
//! The [`Storage`] trait defines the key-value interface that the Datalog
//! evaluator uses. Implementations provide different durability guarantees:
//!
//! - [`MemStorage`] — in-memory HashMap, for tests and ephemeral instances
//! - `FjallStorage` — fjall LSM-tree, for production persistence (future PR)
//!
//! WHY trait: The evaluator must not know or care about the storage backend.
//! Tests run against `MemStorage` (fast, no I/O). Production uses fjall.
//! Same queries, same results — the trait boundary guarantees this.

pub mod mem;

use crate::v2::error::Result;

// ---------------------------------------------------------------------------
// Storage trait
// ---------------------------------------------------------------------------

/// Key-value storage backend for the Datalog engine.
///
/// All keys and values are opaque byte slices. The engine handles
/// serialization of tuples and values above this layer.
pub trait Storage: Send + Sync + 'static {
    /// Transaction handle type.
    type Tx<'a>: StorageTx
    where
        Self: 'a;

    /// Begin a transaction.
    ///
    /// `write`: if true, the transaction may call `put` and `delete`.
    /// Read-only transactions may be cheaper (no WAL writes, no locking).
    fn begin(&self, write: bool) -> Result<Self::Tx<'_>>;
}

/// Operations within a storage transaction.
///
/// Transactions are the unit of atomicity. A write transaction either
/// commits all changes or rolls back all changes.
pub trait StorageTx {
    /// Get the value for a key, or `None` if absent.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Set a key-value pair. Overwrites if the key exists.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key. No-op if the key doesn't exist.
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Iterate all key-value pairs whose key starts with `prefix`.
    ///
    /// Results are ordered by key (lexicographic).
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    /// Commit the transaction, making all writes durable.
    fn commit(self) -> Result<()>;

    /// Roll back the transaction, discarding all writes.
    fn rollback(self) -> Result<()>;
}
