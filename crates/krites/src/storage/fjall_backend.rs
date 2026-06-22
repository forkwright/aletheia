//! Fjall persistent storage backend.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::DbCore;
use crate::data::tuple::{Tuple, check_key_for_validity};
use crate::data::value::ValidityTs;
use crate::error::InternalResult;
use crate::runtime::db::PersistMode as KritesPersistMode;
use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
use crate::storage::error::{
    CorruptedDataSnafu, IoSnafu, StorageResult, TransactionFailedSnafu, WriteInReadTransactionSnafu,
};
use crate::storage::{Storage, StoreTx};
type Result<T> = StorageResult<T>;

impl From<KritesPersistMode> for fjall::PersistMode {
    fn from(mode: KritesPersistMode) -> Self {
        match mode {
            KritesPersistMode::SyncAll => Self::SyncAll,
            KritesPersistMode::SyncData => Self::SyncData,
            KritesPersistMode::Buffer => Self::Buffer,
        }
    }
}

/// Opens or creates a fjall-backed database at the given path.
///
/// Pure Rust, zero C dependencies, LSM-tree with LZ4 compression.
/// Uses `SingleWriterTxDatabase` for serialized write transactions
/// with native read-your-own-writes semantics.
#[expect(
    clippy::result_large_err,
    reason = "InternalResult is the engine-wide error type — boxing deferred to avoid API churn across engine internals"
)]
#[expect(
    clippy::items_after_statements,
    reason = "scoped import keeps snafu::ResultExt close to its only use site"
)]
pub fn new_krites_fjall(
    path: impl AsRef<Path>,
) -> crate::error::InternalResult<DbCore<FjallStorage>> {
    let path = path.as_ref();
    use snafu::ResultExt as _;
    fs::create_dir_all(path)
        .context(IoSnafu { backend: "fjall" })
        .map_err(crate::error::InternalError::from)?;

    let db = fjall::SingleWriterTxDatabase::builder(path)
        .open()
        .map_err(|e| {
            if matches!(e, fjall::Error::Locked) {
                return crate::storage::error::LockedSnafu {
                    path: path.to_path_buf(),
                }
                .build();
            }
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("open: {e}"),
            }
            .build()
        })
        .map_err(crate::error::InternalError::from)?;

    let keyspace = db
        .keyspace("data", fjall::KeyspaceCreateOptions::default)
        .map_err(|e| {
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("open keyspace: {e}"),
            }
            .build()
        })
        .map_err(crate::error::InternalError::from)?;

    let storage = FjallStorage {
        db: Arc::new(db),
        keyspace: Arc::new(keyspace),
        persist_mode: KritesPersistMode::default(),
    };
    let ret = DbCore::new(storage)?;
    ret.initialize()?;
    Ok(ret)
}

/// fjall storage engine: pure Rust, LSM-tree, LZ4 compression.
///
/// No delta buffer needed: fjall `SingleWriterWriteTx` provides
/// read-your-own-writes natively within the transaction.
///
/// WHY: The default persist mode is `Buffer` so routine fact accumulation does
/// not fsync on every commit. Callers that need durability can set
/// `PersistMode::SyncAll` through `DbConfig`, and the test suite relies on
/// fjall's drop-time `SyncAll` flush for persistence across restarts.
#[derive(Clone)]
pub struct FjallStorage {
    db: Arc<fjall::SingleWriterTxDatabase>,
    keyspace: Arc<fjall::SingleWriterTxKeyspace>,
    pub(crate) persist_mode: KritesPersistMode,
}

impl<'s> Storage<'s> for FjallStorage {
    type Tx = FjallTx<'s>;

    fn storage_kind(&self) -> &'static str {
        "fjall"
    }

    fn transact(&'s self, write: bool) -> Result<Self::Tx> {
        if write {
            let tx = self.db.write_tx();
            Ok(FjallTx::Writer(Box::new(FjallWriteTx {
                tx: Some(tx),
                keyspace: &self.keyspace,
                db: Arc::clone(&self.db),
                persist_mode: self.persist_mode,
            })))
        } else {
            let snapshot = self.db.read_tx();
            Ok(FjallTx::Reader(FjallReadTx {
                snapshot,
                keyspace: &self.keyspace,
            }))
        }
    }

    fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
        self.keyspace.inner().major_compact().map_err(|e| {
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("major compact: {e}"),
            }
            .build()
        })
    }

    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> Result<()> {
        let mut tx = self.db.write_tx();
        for pair in data {
            let (k, v) = pair?;
            tx.insert(&self.keyspace, k, v);
        }
        tx.commit().map_err(|e| {
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("batch commit: {e}"),
            }
            .build()
        })?;
        self.db.persist(self.persist_mode.into()).map_err(|e| {
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("batch persist: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Verify the default fjall persist mode defers fsyncs for routine writes.
    ///
    /// WHY: The production memory path issues many small fact writes per turn.
    /// Buffering by default avoids an fsync per transaction; durability can be
    /// opted into through `DbConfig::with_persist_mode(PersistMode::SyncAll)`.
    #[test]
    fn default_persist_mode_is_buffer() -> InternalResult<()> {
        let temp_dir = TempDir::new().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let db = new_krites_fjall(temp_dir.path())?;
        assert_eq!(
            db.db.persist_mode,
            crate::runtime::db::PersistMode::Buffer,
            "default fjall persist mode should defer fsyncs"
        );
        Ok(())
    }
}

#[non_exhaustive]
pub enum FjallTx<'s> {
    Reader(FjallReadTx<'s>),
    Writer(Box<FjallWriteTx<'s>>),
}

pub struct FjallReadTx<'s> {
    snapshot: fjall::Snapshot,
    keyspace: &'s fjall::SingleWriterTxKeyspace,
}

pub struct FjallWriteTx<'s> {
    tx: Option<fjall::SingleWriterWriteTx<'s>>,
    keyspace: &'s fjall::SingleWriterTxKeyspace,
    db: Arc<fjall::SingleWriterTxDatabase>,
    persist_mode: KritesPersistMode,
}

// SAFETY: `FjallReadTx` and `FjallWriteTx` borrow fjall internals that are not
// currently marked `Sync` upstream, even though the public fjall API contract
// permits the shared read access krites uses. The `StoreTx` trait takes `&self`
// for read methods, and rayon shares `&SessionTx` while evaluating independent
// rules in one query. Asserting `Sync` manually is sound under these invariants:
//
// 1. `FjallReadTx::snapshot` is `fjall::Snapshot`, an immutable point-in-time
//    LSM view. Fjall documents snapshot repeatable reads, and its public
//    `Readable::get`/`contains_key`/`range` methods take `&self` and return
//    owned guards/iterators. Krites never mutates a snapshot.
//
// 2. `FjallWriteTx::tx` is `fjall::SingleWriterWriteTx` from fjall 3.1.4.
//    Upstream stores a `MutexGuard` inside that type to serialize writers for
//    the database. Krites never moves the write tx to another thread and never
//    calls mutating fjall methods through `&self`; `put`, `del`,
//    `del_range_from_persisted`, and `commit` require `&mut self` on the outer
//    `FjallTx`. The only shared paths are `Readable::{get,contains_key,range}`,
//    which fjall implements over the transaction's read-your-own-writes
//    snapshot without requiring mutation.
//
// 3. Both wrappers carry a `&'s fjall::SingleWriterTxKeyspace`. The keyspace
//    is a long-lived handle used only as a lookup key for reads; fjall already
//    provides thread-safe access to the keyspace through its own internal
//    synchronization.
//
// 4. `FjallWriteTx::db` is `Arc<fjall::SingleWriterTxDatabase>`. The database
//    handle itself is `Send + Sync` — fjall routes all write-path mutation
//    through its own internal mutex. Sharing the Arc across threads for the
//    sole purpose of calling `persist()` after commit is sound.
//
// If fjall upstream changes `SingleWriterWriteTx` internals or adds native
// `Sync`, revisit this boundary before changing the exact fjall version pin in
// `crates/krites/Cargo.toml`.
#[expect(
    unsafe_code,
    reason = "fjall transaction types require manual Sync; soundness documented above"
)]
unsafe impl Sync for FjallReadTx<'_> {}
#[expect(
    unsafe_code,
    reason = "fjall transaction types require manual Sync; soundness documented above"
)]
unsafe impl Sync for FjallWriteTx<'_> {}

impl FjallWriteTx<'_> {
    fn tx_ref(&self) -> Result<&fjall::SingleWriterWriteTx<'_>> {
        self.tx.as_ref().ok_or_else(|| {
            CorruptedDataSnafu {
                message: "INVARIANT: tx is always Some while FjallWriteTx is live",
            }
            .build()
        })
    }
}

impl<'s> StoreTx<'s> for FjallTx<'s> {
    fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => r
                .snapshot
                .get(r.keyspace, key)
                .map(|opt| opt.map(|v| v.to_vec()))
                .map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "fjall",
                        message: format!("get: {e}"),
                    }
                    .build()
                }),
            FjallTx::Writer(w) => w
                .tx_ref()?
                .get(w.keyspace, key)
                .map(|opt| opt.map(|v| v.to_vec()))
                .map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "fjall",
                        message: format!("get: {e}"),
                    }
                    .build()
                }),
        }
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
        match self {
            FjallTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            FjallTx::Writer(w) => {
                let tx = w.tx.as_mut().ok_or_else(|| {
                    TransactionFailedSnafu {
                        backend: "fjall",
                        message: "write transaction already committed",
                    }
                    .build()
                })?;
                tx.insert(w.keyspace, key, val);
                Ok(())
            }
        }
    }

    fn supports_par_put(&self) -> bool {
        false
    }

    fn del(&mut self, key: &[u8]) -> Result<()> {
        match self {
            FjallTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            FjallTx::Writer(w) => {
                let tx = w.tx.as_mut().ok_or_else(|| {
                    TransactionFailedSnafu {
                        backend: "fjall",
                        message: "write transaction already committed",
                    }
                    .build()
                })?;
                tx.remove(w.keyspace, key);
                Ok(())
            }
        }
    }

    fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> Result<()> {
        match self {
            FjallTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            FjallTx::Writer(w) => {
                use fjall::Readable;
                let keys: Vec<Vec<u8>> = w
                    .tx_ref()?
                    .range(w.keyspace, lower..upper)
                    .map(|guard| {
                        guard.key().map(|k| k.to_vec()).map_err(|e| {
                            TransactionFailedSnafu {
                                backend: "fjall",
                                message: format!("range: {e}"),
                            }
                            .build()
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let tx = w.tx.as_mut().ok_or_else(|| {
                    TransactionFailedSnafu {
                        backend: "fjall",
                        message: "write transaction already committed",
                    }
                    .build()
                })?;
                for k in keys {
                    tx.remove(w.keyspace, k);
                }
                Ok(())
            }
        }
    }

    fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => r.snapshot.contains_key(r.keyspace, key).map_err(|e| {
                TransactionFailedSnafu {
                    backend: "fjall",
                    message: format!("contains_key: {e}"),
                }
                .build()
            }),
            FjallTx::Writer(w) => w.tx_ref()?.contains_key(w.keyspace, key).map_err(|e| {
                TransactionFailedSnafu {
                    backend: "fjall",
                    message: format!("contains_key: {e}"),
                }
                .build()
            }),
        }
    }

    fn commit(&mut self) -> Result<()> {
        match self {
            FjallTx::Reader(_) => Ok(()),
            FjallTx::Writer(w) => {
                if let Some(tx) = w.tx.take() {
                    tx.commit().map_err(|e| {
                        TransactionFailedSnafu {
                            backend: "fjall",
                            message: format!("commit: {e}"),
                        }
                        .build()
                    })?;
                    w.db.persist(w.persist_mode.into()).map_err(|e| {
                        TransactionFailedSnafu {
                            backend: "fjall",
                            message: format!("persist: {e}"),
                        }
                        .build()
                    })?;
                }
                Ok(())
            }
        }
    }

    fn range_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>
    where
        's: 'a,
    {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => {
                fjall_range_tuple_iter(r.snapshot.range(r.keyspace, lower..upper))
            }
            FjallTx::Writer(w) => match w.tx_ref() {
                Ok(tx) => fjall_range_tuple_iter(tx.range(w.keyspace, lower..upper)),
                Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
            },
        }
    }

    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => fjall_skip_iter(
                r.snapshot.range(r.keyspace, lower..upper),
                lower.to_vec(),
                valid_at,
            ),
            FjallTx::Writer(w) => match w.tx_ref() {
                Ok(tx) => {
                    fjall_skip_iter(tx.range(w.keyspace, lower..upper), lower.to_vec(), valid_at)
                }
                Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
            },
        }
    }

    fn range_scan<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>
    where
        's: 'a,
    {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => fjall_range_iter(r.snapshot.range(r.keyspace, lower..upper)),
            FjallTx::Writer(w) => match w.tx_ref() {
                Ok(tx) => fjall_range_iter(tx.range(w.keyspace, lower..upper)),
                Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
            },
        }
    }

    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
    where
        's: 'a,
    {
        use fjall::Readable;
        match self {
            FjallTx::Reader(r) => fjall_count_range(r.snapshot.range(r.keyspace, lower..upper)),
            FjallTx::Writer(w) => fjall_count_range(w.tx_ref()?.range(w.keyspace, lower..upper)),
        }
    }
}

type RawRangeIterator<'a> = Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>;
type TupleRangeIterator<'a> = Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>;

#[expect(
    clippy::result_large_err,
    reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
)]
fn fjall_range_iter<'a>(iter: fjall::Iter) -> RawRangeIterator<'a> {
    Box::new(iter.map(|guard| {
        guard
            .into_inner()
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .map_err(|e| crate::error::InternalError::from(range_error(&e)))
    }))
}

#[expect(
    clippy::result_large_err,
    reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
)]
fn fjall_range_tuple_iter<'a>(iter: fjall::Iter) -> TupleRangeIterator<'a> {
    Box::new(fjall_range_iter(iter).map(|res| res.map(|(k, v)| decode_tuple_from_kv(&k, &v, None))))
}

fn fjall_count_range(iter: fjall::Iter) -> Result<usize> {
    let mut count = 0;
    for guard in iter {
        guard.key().map_err(|e| range_error(&e))?;
        count += 1;
    }
    Ok(count)
}

fn range_error(source: &fjall::Error) -> crate::storage::error::StorageError {
    TransactionFailedSnafu {
        backend: "fjall",
        message: format!("range: {source}"),
    }
    .build()
}

fn fjall_skip_iter<'a>(
    iter: fjall::Iter,
    next_bound: Vec<u8>,
    valid_at: ValidityTs,
) -> TupleRangeIterator<'a> {
    Box::new(StreamingSkipIterator {
        iter,
        valid_at,
        next_bound,
    })
}

struct StreamingSkipIterator {
    // PERF: fjall 3.1.4 exposes bounded `range` iterators but no public seek
    // method on `fjall::Iter`; skip scans therefore stream with bounded memory.
    iter: fjall::Iter,
    valid_at: ValidityTs,
    next_bound: Vec<u8>,
}

impl Iterator for StreamingSkipIterator {
    type Item = InternalResult<Tuple>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (candidate_key, candidate_val) = match self.iter.next()?.into_inner() {
                Ok((k, v)) => (k.to_vec(), v.to_vec()),
                Err(e) => return Some(Err(crate::error::InternalError::from(range_error(&e)))),
            };
            if candidate_key.as_slice() < self.next_bound.as_slice() {
                continue;
            }

            let (ret, nxt_bound) = check_key_for_validity(&candidate_key, self.valid_at, None);
            self.next_bound = nxt_bound;

            if let Some(mut nk) = ret {
                extend_tuple_from_v(&mut nk, &candidate_val);
                return Some(Ok(nk));
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with known length"
)]
#[expect(
    clippy::default_trait_access,
    reason = "tests use `Default::default()` idiomatically for script option structs"
)]
#[expect(
    clippy::result_large_err,
    reason = "tests return InternalResult which carries rich context; size is intentional"
)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;
    use crate::data::value::{DataValue, Validity};
    use crate::error::InternalResult;
    use crate::runtime::db::ScriptMutability;

    fn setup_test_db() -> InternalResult<(TempDir, DbCore<FjallStorage>)> {
        let temp_dir = TempDir::new().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let db = new_krites_fjall(temp_dir.path())?;
        db.run_script(
            r"
            {:create plain {k: Int => v}}
            {:create tt_test {k: Int, vld: Validity => v}}
            ",
            Default::default(),
            ScriptMutability::Mutable,
        )?;
        Ok((temp_dir, db))
    }

    #[test]
    fn basic_operations() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

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
    fn time_travel() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

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
    fn range_operations() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

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

    #[test]
    fn range_scan_streams_first_result_before_counting_full_range() -> InternalResult<()> {
        const ROWS: u32 = 10_000;

        let (_dir, db) = setup_test_db()?;
        let mut tx = db.db.transact(true)?;
        for i in 0..ROWS {
            let mut key = vec![0xfe];
            key.extend_from_slice(&i.to_be_bytes());
            tx.put(&key, &i.wrapping_mul(2).to_be_bytes())?;
        }
        tx.commit()?;

        let tx = db.db.transact(false)?;
        let lower = vec![0xfe, 0, 0, 0, 0];
        let upper = vec![0xfe, 0xff];
        let mut scan = tx.range_scan(&lower, &upper);
        let first = scan
            .next()
            .unwrap_or_else(|| unreachable!("INVARIANT: seeded range is non-empty"))?;
        assert_eq!(first.0, lower, "first range item should be available");
        drop(scan);
        assert_eq!(
            tx.range_count(&lower, &upper)?,
            usize::try_from(ROWS).unwrap_or_else(|_| unreachable!("INVARIANT: ROWS fits usize")),
            "streaming count should still see the full range"
        );

        Ok(())
    }

    #[test]
    fn compact_system_operation_invokes_fjall_compaction() -> InternalResult<()> {
        const ROWS: u32 = 2_000;
        const VALUE_BYTES: usize = 4_096;

        let (_dir, db) = setup_test_db()?;
        let value = vec![b'x'; VALUE_BYTES];
        for i in 0..ROWS {
            db.db
                .keyspace
                .insert(format!("compact:{i:08}").as_bytes(), value.clone())
                .unwrap_or_else(|_| unreachable!("INVARIANT: fjall test insert should succeed"));
        }
        db.db
            .db
            .persist(fjall::PersistMode::SyncAll)
            .unwrap_or_else(|_| unreachable!("INVARIANT: fjall test persist should succeed"));
        for i in 0..ROWS {
            db.db
                .keyspace
                .remove(format!("compact:{i:08}").as_bytes())
                .unwrap_or_else(|_| unreachable!("INVARIANT: fjall test remove should succeed"));
        }
        db.db
            .db
            .persist(fjall::PersistMode::SyncAll)
            .unwrap_or_else(|_| unreachable!("INVARIANT: fjall test persist should succeed"));

        let before = db.db.keyspace.inner().disk_space();
        db.run_script("::compact", Default::default(), ScriptMutability::Mutable)?;
        let after = db.db.keyspace.inner().disk_space();
        assert!(
            after <= before,
            "compaction should not increase disk usage: before={before}, after={after}"
        );

        Ok(())
    }

    #[test]
    fn persistence_across_restarts() -> InternalResult<()> {
        let dir = TempDir::new().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });

        {
            let db = new_krites_fjall(dir.path())?;
            db.run_script(
                "{:create persist_test {k: Int => v: String}}",
                Default::default(),
                ScriptMutability::Mutable,
            )?;
            db.run_script(
                r#"?[k, v] <- [[1, "hello"], [2, "world"]] :put persist_test {k => v}"#,
                Default::default(),
                ScriptMutability::Mutable,
            )?;
        }

        {
            let db = new_krites_fjall(dir.path())?;
            let result = db.run_script(
                "?[k, v] := *persist_test{k, v}",
                Default::default(),
                ScriptMutability::Immutable,
            )?;
            assert_eq!(result.rows.len(), 2);
            assert_eq!(result.rows[0][0], DataValue::from(1));
            assert_eq!(result.rows[0][1], DataValue::Str("hello".into()));
            assert_eq!(result.rows[1][0], DataValue::from(2));
            assert_eq!(result.rows[1][1], DataValue::Str("world".into()));
        }

        Ok(())
    }

    #[test]
    fn concurrent_reads() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

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

        let r1 = db.run_script(
            "?[k, v] := *plain{k, v}, k < 5",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        let r2 = db.run_script(
            "?[k, v] := *plain{k, v}, k >= 5",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        assert_eq!(r1.rows.len(), 5);
        assert_eq!(r2.rows.len(), 5);

        Ok(())
    }

    /// Verify no delta buffer: fjall write tx reads its own writes natively.
    #[test]
    fn read_your_own_writes() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

        db.run_script(
            r#"
            ?[k, v] <- [[1, "first"]] :put plain {k => v}
            "#,
            Default::default(),
            ScriptMutability::Mutable,
        )?;

        let result = db.run_script(
            "?[v] := *plain{k: 1, v}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        assert_eq!(result.rows.len(), 1);

        Ok(())
    }

    /// Stress test: 16 threads × 1000 concurrent read queries against the same db.
    ///
    /// WHY: `FjallReadTx` carries a manual `unsafe impl Sync`. This test
    /// exercises the Sync boundary from many threads simultaneously and
    /// asserts each query returns the same deterministic result set. A crash
    /// or torn read would fail the correctness check.
    #[test]
    fn stress_concurrent_reads() -> InternalResult<()> {
        const NUM_THREADS: usize = 16;
        const QUERIES_PER_THREAD: usize = 1000;
        const NUM_ROWS: i64 = 100;

        let (_dir, db) = setup_test_db()?;

        // Seed deterministic data.
        let mut to_import = BTreeMap::new();
        to_import.insert(
            "plain".to_string(),
            crate::NamedRows {
                headers: vec!["k".to_string(), "v".to_string()],
                rows: (0..NUM_ROWS)
                    .map(|i| vec![DataValue::from(i), DataValue::from(i * 3)])
                    .collect(),
                next: None,
            },
        );
        db.import_relations(to_import)?;

        let expected_rows = usize::try_from(NUM_ROWS)
            .unwrap_or_else(|_| unreachable!("INVARIANT: NUM_ROWS fits in usize"));
        std::thread::scope(|s| {
            for _ in 0..NUM_THREADS {
                let db = db.clone();
                s.spawn(move || {
                    for _ in 0..QUERIES_PER_THREAD {
                        let result = db
                            .run_script(
                                "?[k, v] := *plain{k, v}",
                                Default::default(),
                                ScriptMutability::Immutable,
                            )
                            .unwrap_or_else(|_| {
                                unreachable!("INVARIANT: concurrent read query should not fail")
                            });
                        assert_eq!(
                            result.rows.len(),
                            expected_rows,
                            "concurrent read saw partial relation"
                        );
                    }
                });
            }
        });

        Ok(())
    }

    /// Stress test: 1 writer + 15 readers hitting the same db.
    ///
    /// WHY: fjall's `SingleWriterWriteTx` serializes writers internally, but
    /// readers open independent snapshot transactions. The `Sync` asserts
    /// on `FjallReadTx` and `FjallWriteTx` must hold while a live writer is
    /// mutating the LSM. This test verifies no reader observes a torn row
    /// (headers mismatched, partial vector, or panic) across 1000 writes.
    #[test]
    fn stress_mixed_read_write() -> InternalResult<()> {
        const NUM_READERS: usize = 15;
        const NUM_WRITES: i64 = 1000;

        let (_dir, db) = setup_test_db()?;

        // Seed some initial data so readers have something to read from the start.
        let mut to_import = BTreeMap::new();
        to_import.insert(
            "plain".to_string(),
            crate::NamedRows {
                headers: vec!["k".to_string(), "v".to_string()],
                rows: (0..10)
                    .map(|i| vec![DataValue::from(i), DataValue::from(i * 7)])
                    .collect(),
                next: None,
            },
        );
        db.import_relations(to_import)?;

        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

        std::thread::scope(|s| {
            // Readers: keep querying until the writer signals completion.
            for _ in 0..NUM_READERS {
                let db = db.clone();
                let done = Arc::clone(&done);
                s.spawn(move || {
                    while !done.load(std::sync::atomic::Ordering::Relaxed) {
                        let result = db
                            .run_script(
                                "?[k, v] := *plain{k, v}",
                                Default::default(),
                                ScriptMutability::Immutable,
                            )
                            .unwrap_or_else(|_| {
                                unreachable!("INVARIANT: concurrent read query should not fail")
                            });
                        // Every row must have exactly 2 columns matching the
                        // relation schema. A torn read would surface here.
                        for row in &result.rows {
                            assert_eq!(row.len(), 2, "schema consistent under concurrent writes");
                        }
                    }
                });
            }

            // Writer: upsert rows in a loop.
            let db_writer = db.clone();
            let done_writer = Arc::clone(&done);
            s.spawn(move || {
                for i in 10..NUM_WRITES {
                    db_writer
                        .run_script(
                            &format!("?[k, v] <- [[{i}, {}]] :put plain {{k => v}}", i * 7),
                            Default::default(),
                            ScriptMutability::Mutable,
                        )
                        .unwrap_or_else(|_| {
                            unreachable!("INVARIANT: sequential write should not fail")
                        });
                }
                done_writer.store(true, std::sync::atomic::Ordering::Relaxed);
            });
        });

        // Final consistency check: all writes visible.
        let result = db.run_script(
            "?[k, v] := *plain{k, v}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        let expected_total = usize::try_from(NUM_WRITES)
            .unwrap_or_else(|_| unreachable!("INVARIANT: NUM_WRITES fits in usize"));
        assert_eq!(result.rows.len(), expected_total, "all writes persisted");

        Ok(())
    }

    #[test]
    fn tx_ref_returns_error_when_tx_is_none() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;
        let keyspace: &fjall::SingleWriterTxKeyspace = &db.db.keyspace;
        let write_tx = FjallWriteTx {
            tx: None,
            keyspace,
            db: Arc::clone(&db.db.db),
            persist_mode: KritesPersistMode::default(),
        };
        let result = write_tx.tx_ref();
        assert!(
            result.is_err(),
            "tx_ref should return error when tx is None"
        );
        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("corrupted data"),
                    "error should indicate corrupted data: {msg}"
                );
            }
            Ok(_) => panic!("expected error"),
        }
        Ok(())
    }
}
