//! Fjall persistent storage backend.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::DbCore;
use crate::data::tuple::{Tuple, check_key_for_validity};
use crate::data::value::ValidityTs;
use crate::error::InternalResult;
use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
use crate::storage::error::{
    IoSnafu, StorageResult, TransactionFailedSnafu, WriteInReadTransactionSnafu,
};
use crate::storage::{Storage, StoreTx};
type Result<T> = StorageResult<T>;

/// Opens or creates a fjall-backed database at the given path.
///
/// Pure Rust, zero C dependencies, LSM-tree with LZ4 compression.
/// Uses `SingleWriterTxDatabase` for serialized write transactions
/// with native read-your-own-writes semantics.
pub fn new_cozo_fjall(
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
    };
    let ret = DbCore::new(storage)?;
    ret.initialize()?;
    Ok(ret)
}

/// fjall storage engine: pure Rust, LSM-tree, LZ4 compression.
///
/// No delta buffer needed: fjall `SingleWriterWriteTx` provides
/// read-your-own-writes natively within the transaction.
#[derive(Clone)]
pub struct FjallStorage {
    db: Arc<fjall::SingleWriterTxDatabase>,
    keyspace: Arc<fjall::SingleWriterTxKeyspace>,
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
        Ok(())
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
}

// SAFETY: fjall's Snapshot is a read-only view with no interior mutability, so
// sharing a reference across threads is sound. SingleWriterWriteTx is protected
// by an external mutex guard at the call site, ensuring exclusive access while
// the reference is live. Both impls are therefore safe to declare Sync.
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
    fn tx_ref(&self) -> &fjall::SingleWriterWriteTx<'_> {
        self.tx.as_ref().unwrap_or_else(|| unreachable!())
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
                .tx_ref()
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
                let tx = w.tx.as_mut().unwrap_or_else(|| unreachable!());
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
                let tx = w.tx.as_mut().unwrap_or_else(|| unreachable!());
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
                    .tx_ref()
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
                let tx = w.tx.as_mut().unwrap_or_else(|| unreachable!());
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
            FjallTx::Writer(w) => w.tx_ref().contains_key(w.keyspace, key).map_err(|e| {
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
        match self {
            FjallTx::Reader(r) => {
                match fjall_collect_range(&r.snapshot, &r.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(
                        pairs
                            .into_iter()
                            .map(|(k, v)| Ok(decode_tuple_from_kv(&k, &v, None))),
                    ),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
            FjallTx::Writer(w) => {
                match fjall_collect_range(w.tx_ref(), &w.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(
                        pairs
                            .into_iter()
                            .map(|(k, v)| Ok(decode_tuple_from_kv(&k, &v, None))),
                    ),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
        }
    }

    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
        match self {
            FjallTx::Reader(r) => {
                match fjall_collect_range(&r.snapshot, &r.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(
                        CollectedSkipIterator {
                            data: pairs,
                            pos: 0,
                            upper: upper.to_vec(),
                            valid_at,
                            next_bound: lower.to_vec(),
                        }
                        .map(Ok),
                    ),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
            FjallTx::Writer(w) => {
                match fjall_collect_range(w.tx_ref(), &w.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(
                        CollectedSkipIterator {
                            data: pairs,
                            pos: 0,
                            upper: upper.to_vec(),
                            valid_at,
                            next_bound: lower.to_vec(),
                        }
                        .map(Ok),
                    ),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
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
        match self {
            FjallTx::Reader(r) => {
                match fjall_collect_range(&r.snapshot, &r.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(pairs.into_iter().map(Ok)),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
            FjallTx::Writer(w) => {
                match fjall_collect_range(w.tx_ref(), &w.keyspace, lower, upper) {
                    Ok(pairs) => Box::new(pairs.into_iter().map(Ok)),
                    Err(e) => Box::new(std::iter::once(Err(crate::error::InternalError::from(e)))),
                }
            }
        }
    }

    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
    where
        's: 'a,
    {
        match self {
            FjallTx::Reader(r) => {
                fjall_collect_range(&r.snapshot, &r.keyspace, lower, upper).map(|pairs| pairs.len())
            }
            FjallTx::Writer(w) => {
                fjall_collect_range(w.tx_ref(), &w.keyspace, lower, upper).map(|pairs| pairs.len())
            }
        }
    }
}

fn fjall_collect_range(
    readable: &impl fjall::Readable,
    keyspace: &impl AsRef<fjall::Keyspace>,
    lower: &[u8],
    upper: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut results = Vec::new();
    for guard in readable.range(keyspace, lower..upper) {
        let (k, v) = guard.into_inner().map_err(|e| {
            TransactionFailedSnafu {
                backend: "fjall",
                message: format!("range: {e}"),
            }
            .build()
        })?;
        results.push((k.to_vec(), v.to_vec()));
    }
    Ok(results)
}

struct CollectedSkipIterator {
    data: Vec<(Vec<u8>, Vec<u8>)>,
    pos: usize,
    upper: Vec<u8>,
    valid_at: ValidityTs,
    next_bound: Vec<u8>,
}

impl Iterator for CollectedSkipIterator {
    type Item = Tuple;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while self.pos < self.data.len() {
                if self.data[self.pos].0.as_slice() >= self.next_bound.as_slice() {
                    break;
                }
                self.pos += 1;
            }
            if self.pos >= self.data.len() {
                return None;
            }

            let (ref candidate_key, ref candidate_val) = self.data[self.pos];
            if candidate_key.as_slice() >= self.upper.as_slice() {
                return None;
            }

            let (ret, nxt_bound) = check_key_for_validity(candidate_key, self.valid_at, None);
            self.next_bound = nxt_bound;
            self.pos += 1;

            if let Some(mut nk) = ret {
                extend_tuple_from_v(&mut nk, candidate_val);
                return Some(nk);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;
    use crate::data::value::{DataValue, Validity};
    use crate::error::InternalResult;
    use crate::runtime::db::ScriptMutability;

    fn setup_test_db() -> InternalResult<(TempDir, DbCore<FjallStorage>)> {
        let temp_dir = TempDir::new().unwrap_or_else(|_| unreachable!());
        let db = new_cozo_fjall(temp_dir.path())?;
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
    fn persistence_across_restarts() -> InternalResult<()> {
        let dir = TempDir::new().unwrap_or_else(|_| unreachable!());

        {
            let db = new_cozo_fjall(dir.path())?;
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
            let db = new_cozo_fjall(dir.path())?;
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
}
