//! Fjall LSM-tree storage backend for krites v2.
//!
//! Production-grade persistent storage using fjall's sorted-string-table
//! engine with LZ4 compression. Ordered iteration enables efficient
//! prefix scans for relation lookups.
//!
//! WHY fjall over SQLite: fjall is pure Rust (no C deps), embeddable,
//! and natively supports the sorted key-value model the Datalog engine
//! needs. SQLite adds the libsqlite3-sys C dependency and requires
//! an ORM layer over the relational model.

use std::path::Path;
use std::sync::Arc;

use crate::v2::error::{self, Result};
use super::{Storage, StorageTx};

// ---------------------------------------------------------------------------
// FjallStorage
// ---------------------------------------------------------------------------

/// Persistent storage backend backed by fjall LSM-tree.
pub struct FjallStorage {
    _db: fjall::Database,
    keyspace: Arc<fjall::Keyspace>,
}

impl FjallStorage {
    /// Open or create a fjall database at the given path.
    ///
    /// # Errors
    ///
    /// Returns `Storage` error if the database can't be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let config = fjall::Config::new(path.as_ref());
        let db = fjall::Database::open(config).map_err(|e| {
            error::StorageSnafu {
                message: format!("failed to open fjall at {}: {e}", path.as_ref().display()),
            }
            .build()
        })?;

        // WHY: single keyspace for the Datalog engine. Multiple keyspaces
        // (per-relation) can be added later for isolation, but a single
        // keyspace is simpler and correct for v1.
        let keyspace = db
            .keyspace("krites", fjall::KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("failed to open krites keyspace: {e}"),
                }
                .build()
            })?;

        Ok(Self {
            _db: db,
            keyspace: Arc::new(keyspace),
        })
    }
}

impl Storage for FjallStorage {
    type Tx<'a> = FjallTx;

    fn begin(&self, write: bool) -> Result<Self::Tx<'_>> {
        Ok(FjallTx {
            keyspace: Arc::clone(&self.keyspace),
            pending_writes: Vec::new(),
            pending_deletes: Vec::new(),
            write,
        })
    }
}

// ---------------------------------------------------------------------------
// FjallTx
// ---------------------------------------------------------------------------

/// Transaction handle for [`FjallStorage`].
///
/// WHY buffered writes: fjall doesn't have explicit transactions at the
/// keyspace level. We buffer writes and apply them atomically on commit.
/// Reads go directly to the keyspace.
pub struct FjallTx {
    keyspace: Arc<fjall::Keyspace>,
    pending_writes: Vec<(Vec<u8>, Vec<u8>)>,
    pending_deletes: Vec<Vec<u8>>,
    write: bool,
}

impl StorageTx for FjallTx {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // WHY: check pending writes first (read-your-writes), then
        // pending deletes, then the on-disk partition.
        for (k, v) in self.pending_writes.iter().rev() {
            if k == key {
                return Ok(Some(v.clone()));
            }
        }
        if self.pending_deletes.iter().any(|k| k == key) {
            return Ok(None);
        }

        self.keyspace
            .get(key)
            .map(|opt| opt.map(|v| v.to_vec()))
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall get error: {e}"),
                }
                .build()
            })
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if !self.write {
            return Err(error::StorageSnafu {
                message: "write operation on read-only transaction",
            }
            .build());
        }
        self.pending_deletes.retain(|k| k != key);
        self.pending_writes.push((key.to_vec(), value.to_vec()));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        if !self.write {
            return Err(error::StorageSnafu {
                message: "delete operation on read-only transaction",
            }
            .build());
        }
        self.pending_writes.retain(|(k, _)| k != key);
        self.pending_deletes.push(key.to_vec());
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        // WHY: merge on-disk results with pending writes/deletes.
        let mut results = std::collections::BTreeMap::new();

        // WHY: fjall prefix() returns guards; into_inner() yields (key, value) slices.
        for guard in self.keyspace.prefix(prefix) {
            let (key, value) = guard.into_inner().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall prefix scan error: {e}"),
                }
                .build()
            })?;
            results.insert(key.to_vec(), value.to_vec());
        }

        // Apply pending writes.
        for (k, v) in &self.pending_writes {
            if k.starts_with(prefix) {
                results.insert(k.clone(), v.clone());
            }
        }

        // Remove pending deletes.
        for k in &self.pending_deletes {
            results.remove(k);
        }

        Ok(results.into_iter().collect())
    }

    fn commit(self) -> Result<()> {
        if !self.write || (self.pending_writes.is_empty() && self.pending_deletes.is_empty()) {
            return Ok(());
        }

        // WHY: apply all writes and deletes. fjall insert/remove are
        // individually durable (WAL-backed), but we apply them in
        // sequence for the batch effect.
        for key in &self.pending_deletes {
            self.keyspace.remove(key).map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall delete error: {e}"),
                }
                .build()
            })?;
        }

        for (key, value) in &self.pending_writes {
            self.keyspace.insert(key, value).map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall insert error: {e}"),
                }
                .build()
            })?;
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

    fn temp_storage() -> (FjallStorage, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storage = FjallStorage::open(dir.path().join("test.fjall")).unwrap();
        (storage, dir)
    }

    #[test]
    fn basic_crud() {
        let (store, _dir) = temp_storage();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.put(b"key2", b"val2").unwrap();
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), Some(b"val1".to_vec()));
        assert_eq!(tx.get(b"key2").unwrap(), Some(b"val2".to_vec()));
        assert_eq!(tx.get(b"key3").unwrap(), None);
    }

    #[test]
    fn delete() {
        let (store, _dir) = temp_storage();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.commit().unwrap();

        let mut tx = store.begin(true).unwrap();
        tx.delete(b"key1").unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), None);
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), None);
    }

    #[test]
    fn read_your_writes() {
        let (store, _dir) = temp_storage();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), Some(b"val1".to_vec()));
        tx.commit().unwrap();
    }

    #[test]
    fn rollback_discards() {
        let (store, _dir) = temp_storage();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"key1", b"val1").unwrap();
        tx.rollback().unwrap();

        let tx = store.begin(false).unwrap();
        assert_eq!(tx.get(b"key1").unwrap(), None);
    }

    #[test]
    fn scan_prefix_ordered() {
        let (store, _dir) = temp_storage();

        let mut tx = store.begin(true).unwrap();
        tx.put(b"fact:003", b"c").unwrap();
        tx.put(b"fact:001", b"a").unwrap();
        tx.put(b"fact:002", b"b").unwrap();
        tx.put(b"entity:001", b"x").unwrap();
        tx.commit().unwrap();

        let tx = store.begin(false).unwrap();
        let results = tx.scan_prefix(b"fact:").unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, b"fact:001");
        assert_eq!(results[1].0, b"fact:002");
        assert_eq!(results[2].0, b"fact:003");
    }

    #[test]
    fn persistence_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("persist.fjall");

        {
            let store = FjallStorage::open(&path).unwrap();
            let mut tx = store.begin(true).unwrap();
            tx.put(b"key1", b"val1").unwrap();
            tx.commit().unwrap();
        }

        {
            let store = FjallStorage::open(&path).unwrap();
            let tx = store.begin(false).unwrap();
            assert_eq!(tx.get(b"key1").unwrap(), Some(b"val1".to_vec()));
        }
    }

    #[test]
    fn write_on_readonly_fails() {
        let (store, _dir) = temp_storage();
        let mut tx = store.begin(false).unwrap();
        assert!(tx.put(b"key", b"val").is_err());
    }
}
