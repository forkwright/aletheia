//! In-memory storage backend.
//!
//! Uses a `BTreeMap` for ordered iteration (required by `scan_prefix`).
//! Fast, no I/O, no persistence — ideal for tests and ephemeral instances.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::v2::error::Result;
use super::{Storage, StorageTx};

// ---------------------------------------------------------------------------
// MemStorage
// ---------------------------------------------------------------------------

/// In-memory key-value store backed by a `BTreeMap`.
///
/// Thread-safe via `RwLock`. Write transactions take an exclusive lock;
/// read transactions take a shared lock. Suitable for tests and small
/// datasets where persistence isn't needed.
pub struct MemStorage {
    data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl MemStorage {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

impl Default for MemStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for MemStorage {
    type Tx<'a> = MemTx<'a>;

    fn begin(&self, write: bool) -> Result<Self::Tx<'_>> {
        // WHY: snapshot the current state. Write transactions accumulate
        // changes in a local buffer, then merge on commit. This avoids
        // holding the write lock for the entire transaction duration.
        let snapshot = self
            .data
            .read()
            .map_err(|_| {
                crate::v2::error::StorageSnafu {
                    message: "rwlock poisoned",
                }
                .build()
            })?
            .clone();

        Ok(MemTx {
            store: &self.data,
            snapshot,
            pending_writes: BTreeMap::new(),
            pending_deletes: Vec::new(),
            write,
        })
    }
}

// ---------------------------------------------------------------------------
// MemTx
// ---------------------------------------------------------------------------

/// Transaction handle for [`MemStorage`].
pub struct MemTx<'a> {
    store: &'a Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    snapshot: BTreeMap<Vec<u8>, Vec<u8>>,
    pending_writes: BTreeMap<Vec<u8>, Vec<u8>>,
    pending_deletes: Vec<Vec<u8>>,
    write: bool,
}

impl StorageTx for MemTx<'_> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check pending writes first (read-your-writes).
        if let Some(val) = self.pending_writes.get(key) {
            return Ok(Some(val.clone()));
        }
        // Check if pending delete.
        if self.pending_deletes.iter().any(|k| k.as_slice() == key) {
            return Ok(None);
        }
        Ok(self.snapshot.get(key).cloned())
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if !self.write {
            return Err(crate::v2::error::StorageSnafu {
                message: "write operation on read-only transaction",
            }
            .build());
        }
        self.pending_deletes.retain(|k| k.as_slice() != key);
        self.pending_writes.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        if !self.write {
            return Err(crate::v2::error::StorageSnafu {
                message: "delete operation on read-only transaction",
            }
            .build());
        }
        self.pending_writes.remove(key);
        self.pending_deletes.push(key.to_vec());
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        // WHY: merge snapshot + pending writes, exclude pending deletes.
        // BTreeMap range gives us ordered iteration.
        let mut merged = BTreeMap::new();

        // Start from snapshot.
        for (k, v) in self.snapshot.range(prefix.to_vec()..) {
            if !k.starts_with(prefix) {
                break;
            }
            merged.insert(k.clone(), v.clone());
        }

        // Apply pending writes.
        for (k, v) in &self.pending_writes {
            if k.starts_with(prefix) {
                merged.insert(k.clone(), v.clone());
            }
        }

        // Remove pending deletes.
        for k in &self.pending_deletes {
            merged.remove(k);
        }

        Ok(merged.into_iter().collect())
    }

    fn commit(self) -> Result<()> {
        if !self.write {
            return Ok(());
        }

        let mut store = self.store.write().map_err(|_| {
            crate::v2::error::StorageSnafu {
                message: "rwlock poisoned on commit",
            }
            .build()
        })?;

        // Apply deletes.
        for key in &self.pending_deletes {
            store.remove(key);
        }

        // Apply writes.
        for (key, value) in self.pending_writes {
            store.insert(key, value);
        }

        Ok(())
    }

    fn rollback(self) -> Result<()> {
        // WHY: pending changes are local to the transaction.
        // Dropping without committing = rollback.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn basic_crud() {
        let store = MemStorage::new();

        // Write.
        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.put(b"key2", b"val2").unwrap();
        tx.commit().unwrap();

        // Read.
        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), Some(b"val1".to_vec()));
        assert_eq!(tx.get(b"key2").unwrap(), Some(b"val2".to_vec()));
        assert_eq!(tx.get(b"key3").unwrap(), None);
        tx.rollback().unwrap();
    }

    #[test]
    fn delete() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.commit().unwrap();

        let mut tx = store.begin(true).unwrap();
        tx.delete(b"key1").unwrap();
        // Read-your-deletes.
        assert_eq!(tx.get(b"key1").unwrap(), None);
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), None);
    }

    #[test]
    fn read_your_writes() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        // Read within same transaction sees the write.
        assert_eq!(tx.get(b"key1").unwrap(), Some(b"val1".to_vec()));
        tx.commit().unwrap();
    }

    #[test]
    fn rollback_discards_writes() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.rollback().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), None);
    }

    #[test]
    fn scan_prefix_ordered() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"fact:003", b"c").unwrap();
        tx.put(b"fact:001", b"a").unwrap();
        tx.put(b"fact:002", b"b").unwrap();
        tx.put(b"entity:001", b"x").unwrap();
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        let results = tx.scan_prefix(b"fact:").unwrap();
        assert_eq!(results.len(), 3);
        // Ordered by key.
        assert_eq!(results[0].0, b"fact:001");
        assert_eq!(results[1].0, b"fact:002");
        assert_eq!(results[2].0, b"fact:003");
    }

    #[test]
    fn scan_prefix_with_pending() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"fact:001", b"a").unwrap();
        tx.commit().unwrap();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"fact:002", b"b").unwrap();
        tx.delete(b"fact:001").unwrap();
        let results = tx.scan_prefix(b"fact:").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, b"fact:002");
    }

    #[test]
    fn write_on_readonly_fails() {
        let store = MemStorage::new();
        let mut tx = store.begin(false).unwrap();
        assert!(tx.put(b"key", b"val").is_err());
        assert!(tx.delete(b"key").is_err());
    }

    #[test]
    fn overwrite_value() {
        let store = MemStorage::new();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key", b"v1").unwrap();
        tx.commit().unwrap();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key", b"v2").unwrap();
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn empty_scan() {
        let store = MemStorage::new();
        let tx = store.begin(false).unwrap();
        let results = tx.scan_prefix(b"anything:").unwrap();
        assert!(results.is_empty());
    }
}
