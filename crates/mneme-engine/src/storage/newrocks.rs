use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use snafu::Snafu;
use crate::error::DbResult as Result;
use crate::{bail};
use tracing::info;

use rocksdb::{OptimisticTransactionDB, Options, WriteBatchWithTransaction, DB};

use crate::data::tuple::{check_key_for_validity, Tuple};
use crate::data::value::ValidityTs;
use crate::runtime::db::{BadDbInit, DbManifest};
use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
use crate::storage::{Storage, StoreTx};
use crate::DbCore as Db;

const KEY_PREFIX_LEN: usize = 9;
const CURRENT_STORAGE_VERSION: u64 = 3;

/// Creates a RocksDB database object.
/// This is currently the fastest persistent storage and it can
/// sustain huge concurrency.
/// Supports concurrent readers and writers.
pub fn new_cozo_newrocksdb(path: impl AsRef<Path>) -> Result<Db<NewRocksDbStorage>> {
    fs::create_dir_all(&path).map_err(|err| {
        BadDbInit(format!(
            "cannot create directory {}: {}",
            path.as_ref().display(),
            err
        ))
    })?;
    let path_buf = path.as_ref().to_path_buf();

    let manifest_path = path_buf.join("manifest");
    let is_new = if manifest_path.exists() {
        let manifest_bytes = fs::read(&manifest_path)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .map_err(|e| crate::error::AdhocError(format!("failed to read manifest: {e}")))?;
        let existing: DbManifest = rmp_serde::from_slice(&manifest_bytes)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .map_err(|e| crate::error::AdhocError(format!("failed to parse manifest: {e}")))?;

        if existing.storage_version != CURRENT_STORAGE_VERSION {
            return Err(bail!(
                "Unsupported storage version {}",
                existing.storage_version
            ));
        }
        false
    } else {
        let manifest = DbManifest {
            storage_version: CURRENT_STORAGE_VERSION,
        };
        let manifest_bytes = rmp_serde::to_vec_named(&manifest)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .map_err(|e| crate::error::AdhocError(format!("failed to serialize manifest: {e}")))?;
        fs::write(&manifest_path, &manifest_bytes)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .map_err(|e| crate::error::AdhocError(format!("failed to write manifest: {e}")))?;
        true
    };

    let store_path = path_buf.join("data");
    let store_path_str = store_path.to_str().ok_or(crate::error::AdhocError("bad path name".to_string()))?;

    let mut options = Options::default();
    options.create_if_missing(is_new);
    // Add any necessary RocksDB options here

    let db = OptimisticTransactionDB::open(&options, store_path_str)
        .map_err(|e| crate::error::AdhocError(e.to_string()))
        .map_err(|e| crate::error::AdhocError(format!("Failed to open RocksDB: {e}")))?;

    let ret = Db::new(NewRocksDbStorage::new(db))?;
    ret.initialize()?;
    Ok(ret)
}

/// RocksDB storage engine
#[derive(Clone)]
pub struct NewRocksDbStorage {
    db: Arc<OptimisticTransactionDB>,
}

impl NewRocksDbStorage {
    pub(crate) fn new(db: OptimisticTransactionDB) -> Self {
        Self { db: Arc::new(db) }
    }
}

impl<'s> Storage<'s> for NewRocksDbStorage {
    type Tx = NewRocksDbTx<'s>;

    fn storage_kind(&self) -> &'static str {
        "rocksdb"
    }

    fn transact(&'s self, _write: bool) -> Result<Self::Tx> {
        Ok(NewRocksDbTx {
            db_tx: Some(self.db.transaction()),
        })
    }

    fn range_compact(&self, lower: &[u8], upper: &[u8]) -> Result<()> {
        self.db.compact_range(Some(lower), Some(upper));
        Ok(())
    }

    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> Result<()> {
        let mut batch = WriteBatchWithTransaction::<true>::default();
        for result in data {
            let (key, val) = result?;
            batch.put(&key, &val);
        }
        self.db
            .write(batch)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .wrap_err_with(|| "Batch put failed")
    }
}

pub struct NewRocksDbTx<'a> {
    db_tx: Option<rocksdb::Transaction<'a, OptimisticTransactionDB>>,
}

// SAFETY: NewRocksDbTx wraps `rocksdb::Transaction<'a, OptimisticTransactionDB>`.
// RocksDB's OptimisticTransactionDB is internally synchronized — concurrent reads
// are safe and writes use optimistic locking (conflict detection at commit time).
// Shared references (&NewRocksDbTx) only perform reads via `get()`, which RocksDB
// guarantees are thread-safe. Mutable access (put/delete) goes through &mut self
// or internal locking in the par_put path.
// Reference: https://github.com/facebook/rocksdb/wiki/Basic-Operations#concurrency
unsafe impl<'a> Sync for NewRocksDbTx<'a> {}

impl<'s> StoreTx<'s> for NewRocksDbTx<'s> {
    fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
        let db_tx = self
            .db_tx
            .as_ref()
            .ok_or_else(|| crate::error::AdhocError("Transaction already committed".to_string()))?;

        db_tx
            .get(key)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .wrap_err("failed to get value")
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
        let db_tx = self
            .db_tx
            .as_mut()
            .ok_or_else(|| crate::error::AdhocError("Transaction already committed".to_string()))?;

        db_tx
            .put(key, val)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .wrap_err("failed to put value")
    }

    fn supports_par_put(&self) -> bool {
        true
    }

    #[inline]
    fn par_put(&self, key: &[u8], val: &[u8]) -> Result<()> {
        match self.db_tx {
            Some(ref db_tx) => db_tx
                .put(key, val)
                .map_err(|e| crate::error::AdhocError(e.to_string()))
                .wrap_err_with(|| "Parallel put failed"),
            None => Err(crate::error::AdhocError("Transaction already committed".to_string())),
        }
    }

    #[inline]
    fn del(&mut self, key: &[u8]) -> Result<()> {
        match self.db_tx {
            Some(ref mut db_tx) => db_tx
                .delete(key)
                .map_err(|e| crate::error::AdhocError(e.to_string()))
                .wrap_err_with(|| "Delete operation failed"),
            None => Err(crate::error::AdhocError("Transaction already committed".to_string())),
        }
    }

    #[inline]
    fn par_del(&self, key: &[u8]) -> Result<()> {
        match self.db_tx {
            Some(ref db_tx) => db_tx
                .delete(key)
                .map_err(|e| crate::error::AdhocError(e.to_string()))
                .wrap_err_with(|| "Parallel delete failed"),
            None => Err(crate::error::AdhocError("Transaction already committed".to_string())),
        }
    }

    fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> Result<()> {
        match self.db_tx {
            Some(ref mut db_tx) => {
                let iter = db_tx.iterator(rocksdb::IteratorMode::From(
                    lower,
                    rocksdb::Direction::Forward,
                ));
                for item in iter {
                    let (k, _) = item
                        .map_err(|e| crate::error::AdhocError(e.to_string()))
                        .map_err(|e| crate::error::AdhocError(format!("{}: {e}", "Error iterating during range delete")))?;
                    if k >= upper.into() {
                        break;
                    }
                    db_tx
                        .delete(&k)
                        .map_err(|e| crate::error::AdhocError(e.to_string()))
                        .map_err(|e| crate::error::AdhocError(format!("{}: {e}", "Error deleting during range delete")))?;
                }
                Ok(())
            }
            None => Err(crate::error::AdhocError("Transaction already committed".to_string())),
        }
    }

    #[inline]
    fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
        let db_tx = self
            .db_tx
            .as_ref()
            .ok_or(crate::error::AdhocError("Transaction already committed".to_string()))?;
        db_tx
            .get(key)
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .wrap_err("Error during exists check")
            .map(|opt| opt.is_some())
    }

    fn commit(&mut self) -> Result<()> {
        let db_tx = self.db_tx.take().expect("Transaction already committed");
        db_tx
            .commit()
            .map_err(|e| crate::error::AdhocError(e.to_string()))
            .wrap_err_with(|| "Commit failed")
    }

    fn range_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = Result<Tuple>> + 'a>
    where
        's: 'a,
    {
        match &self.db_tx {
            Some(db_tx) => Box::new(NewRocksDbIterator {
                inner: db_tx.iterator(rocksdb::IteratorMode::From(
                    lower,
                    rocksdb::Direction::Forward,
                )),
                upper_bound: upper.to_vec(),
            }),
            None => Box::new(std::iter::once(Err(bail!(
                "Transaction already committed"
            )))),
        }
    }

    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = Result<Tuple>> + 'a> {
        match self.db_tx {
            Some(ref db_tx) => Box::new(NewRocksDbSkipIterator {
                inner: db_tx.iterator(rocksdb::IteratorMode::From(
                    lower,
                    rocksdb::Direction::Forward,
                )),
                upper_bound: upper.to_vec(),
                valid_at,
                next_bound: lower.to_vec(),
            }),
            None => Box::new(std::iter::once(Err(bail!(
                "Transaction already committed"
            )))),
        }
    }

    fn range_scan<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>
    where
        's: 'a,
    {
        match self.db_tx {
            Some(ref db_tx) => {
                let iter = db_tx.iterator(rocksdb::IteratorMode::From(
                    lower,
                    rocksdb::Direction::Forward,
                ));
                Box::new(NewRocksDbIteratorRaw {
                    inner: iter,
                    upper_bound: upper.to_vec(),
                })
            }
            None => Box::new(std::iter::once(Err(bail!(
                "Transaction already committed"
            )))),
        }
    }

    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
    where
        's: 'a,
    {
        let db_tx = self
            .db_tx
            .as_ref()
            .ok_or(crate::error::AdhocError("Transaction already committed".to_string()))?;
        let iter = db_tx.iterator(rocksdb::IteratorMode::From(
            lower,
            rocksdb::Direction::Forward,
        ));
        let count = iter
            .take_while(|item| match item {
                Ok((k, _)) => k.as_ref() < upper,
                Err(_) => false,
            })
            .count();
        Ok(count)
    }

    fn total_scan<'a>(&'a self) -> Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>
    where
        's: 'a,
    {
        match self.db_tx {
            Some(ref db_tx) => Box::new(db_tx.iterator(rocksdb::IteratorMode::Start).map(|item| {
                item.map(|(k, v)| (k.to_vec(), v.to_vec()))
                    .map_err(|e| crate::error::AdhocError(e.to_string()))
                    .wrap_err_with(|| "Error during total scan")
            })),
            None => Box::new(std::iter::once(Err(bail!(
                "Transaction already committed"
            )))),
        }
    }
}

pub(crate) struct NewRocksDbIterator<'a> {
    inner: rocksdb::DBIteratorWithThreadMode<'a, rocksdb::Transaction<'a, OptimisticTransactionDB>>,
    upper_bound: Vec<u8>,
}

impl<'a> Iterator for NewRocksDbIterator<'a> {
    type Item = Result<Tuple>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(result) = self.inner.next() {
            match result {
                Ok((k, v)) => {
                    if k.as_ref() >= self.upper_bound.as_slice() {
                        return None;
                    }
                    return Some(Ok(decode_tuple_from_kv(&k, &v, None)));
                }
                Err(e) => return Some(Err(crate::error::AdhocError(format!("Iterator error: {}", e)))),
            }
        }
        None
    }
}

pub(crate) struct NewRocksDbSkipIterator<'a> {
    inner: rocksdb::DBIteratorWithThreadMode<'a, rocksdb::Transaction<'a, OptimisticTransactionDB>>,
    upper_bound: Vec<u8>,
    valid_at: ValidityTs,
    next_bound: Vec<u8>,
}

impl<'a> Iterator for NewRocksDbSkipIterator<'a> {
    type Item = Result<Tuple>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.inner.set_mode(rocksdb::IteratorMode::From(
                &self.next_bound,
                rocksdb::Direction::Forward,
            ));
            match self.inner.next() {
                None => return None,
                Some(Ok((k_slice, v_slice))) => {
                    if self.upper_bound.as_slice() <= k_slice.as_ref() {
                        return None;
                    }

                    let (ret, nxt_bound) =
                        check_key_for_validity(k_slice.as_ref(), self.valid_at, None);
                    self.next_bound = nxt_bound;
                    if let Some(mut tup) = ret {
                        extend_tuple_from_v(&mut tup, v_slice.as_ref());
                        return Some(Ok(tup));
                    }
                }
                Some(Err(e)) => return Some(Err(crate::error::AdhocError(format!("Iterator Error: {}", e)))),
            }
        }
    }
}

pub(crate) struct NewRocksDbIteratorRaw<'a> {
    inner: rocksdb::DBIteratorWithThreadMode<'a, rocksdb::Transaction<'a, OptimisticTransactionDB>>,
    upper_bound: Vec<u8>,
}

impl<'a> Iterator for NewRocksDbIteratorRaw<'a> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next() {
            Some(Ok((k, v))) => {
                if k.as_ref() >= self.upper_bound.as_slice() {
                    return None;
                }
                Some(Ok((k.to_vec(), v.to_vec())))
            }
            Some(Err(e)) => Some(Err(crate::error::AdhocError(format!("Iterator error: {}", e)))),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::value::{DataValue, Validity};
    use crate::runtime::db::ScriptMutability;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn setup_test_db() -> Result<(TempDir, Db<NewRocksDbStorage>)> {
        let temp_dir = TempDir::new().map_err(|e| crate::error::AdhocError(e.to_string()))?;
        let db = new_cozo_newrocksdb(temp_dir.path())?;

        // Create test tables with proper ScriptMutability parameter
        db.run_script(
            r#"
            {:create plain {k: Int => v}}
            {:create tt_test {k: Int, vld: Validity => v}}
            "#,
            Default::default(),
            ScriptMutability::Mutable,
        )?;

        Ok((temp_dir, db))
    }

    #[test]
    fn test_basic_operations() -> Result<()> {
        let (_temp_dir, db) = setup_test_db()?;

        // Test data insertion
        let mut to_import = BTreeMap::new();
        to_import.insert(
            "plain".to_string(),
            crate::NamedRows {
                headers: vec!["k".to_string(), "v".to_string()],
                rows: (0..100)
                    .map(|i| vec![DataValue::from(i), DataValue::from(i * 2)])
                    .collect(),
                next: None,
            },
        );
        db.import_relations(to_import)?;

        // Test simple query with ScriptMutability parameter
        let result = db.run_script(
            "?[v] := *plain{k: 5, v}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], DataValue::from(10));

        Ok(())
    }
    #[test]
    fn test_time_travel() -> Result<()> {
        let (_temp_dir, db) = setup_test_db()?;

        // Insert time travel data
        let mut to_import = BTreeMap::new();
        to_import.insert(
            "tt_test".to_string(),
            crate::NamedRows {
                headers: vec!["k".to_string(), "vld".to_string(), "v".to_string()],
                rows: vec![
                    vec![
                        DataValue::from(1),
                        DataValue::Validity(Validity::from((0, true))),
                        DataValue::from(100),
                    ],
                    vec![
                        DataValue::from(1),
                        DataValue::Validity(Validity::from((1, true))),
                        DataValue::from(200),
                    ],
                ],
                next: None,
            },
        );
        db.import_relations(to_import)?;

        // Query at different timestamps
        let result = db.run_script(
            "?[v] := *tt_test{k: 1, v @ 0}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        assert_eq!(result.rows[0][0], DataValue::from(100));

        let result = db.run_script(
            "?[v] := *tt_test{k: 1, v @ 1}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        assert_eq!(result.rows[0][0], DataValue::from(200));

        Ok(())
    }

    #[test]
    fn test_range_operations() -> Result<()> {
        let (_temp_dir, db) = setup_test_db()?;

        // Insert test data
        let mut to_import = BTreeMap::new();
        to_import.insert(
            "plain".to_string(),
            crate::NamedRows {
                headers: vec!["k".to_string(), "v".to_string()],
                rows: (0..10)
                    .map(|i| vec![DataValue::from(i), DataValue::from(i)])
                    .collect(),
                next: None,
            },
        );
        db.import_relations(to_import)?;

        // Test range query
        let result = db.run_script(
            "?[k, v] := *plain{k, v}, k >= 3, k < 7",
            Default::default(),
            ScriptMutability::Immutable,
        )?;

        assert_eq!(result.rows.len(), 4);
        assert_eq!(result.rows[0][0], DataValue::from(3));
        assert_eq!(result.rows[3][0], DataValue::from(6));

        Ok(())
    }
}
