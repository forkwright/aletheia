//! Redb persistent storage backend.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use redb::ReadableDatabase;

use crate::engine::DbCore;
use crate::engine::data::tuple::{Tuple, check_key_for_validity};
use crate::engine::data::value::ValidityTs;
use crate::engine::error::InternalResult;
use crate::engine::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
use crate::engine::storage::error::{
    IoSnafu, StorageResult, TransactionFailedSnafu, WriteInReadTransactionSnafu,
};
use crate::engine::storage::{Storage, StoreTx};
use crate::engine::utils::swap_option_result;

type Result<T> = StorageResult<T>;

const TABLE: redb::TableDefinition<'_, &[u8], &[u8]> = redb::TableDefinition::new("data");

/// Opens or creates a redb-backed database at the given path.
///
/// Pure Rust, zero C dependencies, ACID transactions, single-file database.
///
/// # Cleanup contract
///
/// - The returned `DbCore` holds an `Arc<redb::Database>`.  All temporary
///   directories used during testing are managed by `tempfile::TempDir`,
///   which removes the directory (and its `data.redb` file) on drop — even
///   on panic.  Production paths live under `instance/` and are not
///   automatically removed.
/// - redb flushes its WAL on `Database` drop; no manual flush is needed.
pub fn new_cozo_redb(
    path: impl AsRef<Path>,
) -> crate::engine::error::InternalResult<DbCore<RedbStorage>> {
    use snafu::ResultExt as _;
    let path = path.as_ref();
    fs::create_dir_all(path)
        .context(IoSnafu { backend: "redb" })
        .map_err(|e| crate::engine::error::InternalError::from(e))?;

    let db_file = path.join("data.redb");
    let db = redb::Database::create(&db_file)
        .map_err(|e| {
            TransactionFailedSnafu {
                backend: "redb",
                message: format!("open {}: {e}", db_file.display()),
            }
            .build()
        })
        .map_err(|e| crate::engine::error::InternalError::from(e))?;

    // Ensure the data table exists before any reads
    {
        let write_txn = db
            .begin_write()
            .map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("begin_write: {e}"),
                }
                .build()
            })
            .map_err(|e| crate::engine::error::InternalError::from(e))?;
        write_txn
            .open_table(TABLE)
            .map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("open_table: {e}"),
                }
                .build()
            })
            .map_err(|e| crate::engine::error::InternalError::from(e))?;
        write_txn
            .commit()
            .map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("commit: {e}"),
                }
                .build()
            })
            .map_err(|e| crate::engine::error::InternalError::from(e))?;
    }

    let storage = RedbStorage { db: Arc::new(db) };
    let ret = DbCore::new(storage)?;
    ret.initialize()?;
    Ok(ret)
}

/// redb storage engine — pure Rust, zero C deps, single-file ACID database.
///
/// # Cleanup and WAL safety
///
/// `redb::Database` owns the write-ahead log.  When the last `Arc<Database>`
/// reference is dropped, redb flushes and closes the WAL automatically —
/// no explicit flush call is required.  Uncommitted `WriteTransaction`
/// values roll back when dropped without calling `commit()`.
///
/// The `DbCore<RedbStorage>` wrapper holds the `Arc<Database>` and may be
/// cloned; the underlying file is closed (and flushed) once *all* clones
/// are dropped.
#[derive(Clone)]
pub struct RedbStorage {
    db: Arc<redb::Database>,
}

impl<'s> Storage<'s> for RedbStorage {
    type Tx = RedbTx<'s>;

    fn storage_kind(&self) -> &'static str {
        "redb"
    }

    fn transact(&'s self, write: bool) -> Result<Self::Tx> {
        if write {
            let snapshot = self.db.begin_read().map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("begin_read for snapshot: {e}"),
                }
                .build()
            })?;
            Ok(RedbTx::Writer(RedbWriteTx {
                storage: self,
                snapshot,
                delta: BTreeMap::new(),
            }))
        } else {
            let read_txn = self.db.begin_read().map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("begin_read: {e}"),
                }
                .build()
            })?;
            Ok(RedbTx::Reader(RedbReadTx { read_txn }))
        }
    }

    fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
        // redb compacts internally
        Ok(())
    }

    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(|e| {
            TransactionFailedSnafu {
                backend: "redb",
                message: format!("begin_write: {e}"),
            }
            .build()
        })?;
        {
            let mut table = write_txn.open_table(TABLE).map_err(|e| {
                TransactionFailedSnafu {
                    backend: "redb",
                    message: format!("open_table: {e}"),
                }
                .build()
            })?;
            for pair in data {
                let (k, v) = pair?;
                table.insert(k.as_slice(), v.as_slice()).map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "redb",
                        message: format!("insert: {e}"),
                    }
                    .build()
                })?;
            }
        }
        write_txn.commit().map_err(|e| {
            TransactionFailedSnafu {
                backend: "redb",
                message: format!("commit: {e}"),
            }
            .build()
        })?;
        Ok(())
    }
}

pub enum RedbTx<'s> {
    Reader(RedbReadTx),
    Writer(RedbWriteTx<'s>),
}

pub struct RedbReadTx {
    read_txn: redb::ReadTransaction,
}

pub struct RedbWriteTx<'s> {
    storage: &'s RedbStorage,
    snapshot: redb::ReadTransaction,
    delta: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

fn redb_table_get(read_txn: &redb::ReadTransaction, key: &[u8]) -> Result<Option<Vec<u8>>> {
    let table = read_txn.open_table(TABLE).map_err(|e| {
        TransactionFailedSnafu {
            backend: "redb",
            message: format!("open_table: {e}"),
        }
        .build()
    })?;
    let val = table
        .get(key)
        .map_err(|e| {
            TransactionFailedSnafu {
                backend: "redb",
                message: format!("get: {e}"),
            }
            .build()
        })?
        .map(|guard: redb::AccessGuard<'_, &[u8]>| guard.value().to_vec());
    Ok(val)
}

fn redb_collect_range(
    read_txn: &redb::ReadTransaction,
    lower: &[u8],
    upper: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let table = read_txn.open_table(TABLE).map_err(|e| {
        TransactionFailedSnafu {
            backend: "redb",
            message: format!("open_table: {e}"),
        }
        .build()
    })?;
    let range = table.range(lower..upper).map_err(|e| {
        TransactionFailedSnafu {
            backend: "redb",
            message: format!("range: {e}"),
        }
        .build()
    })?;
    let mut results = Vec::new();
    for entry in range {
        let entry = entry.map_err(|e| {
            TransactionFailedSnafu {
                backend: "redb",
                message: format!("iter: {e}"),
            }
            .build()
        })?;
        results.push((entry.0.value().to_vec(), entry.1.value().to_vec()));
    }
    Ok(results)
}

impl<'s> StoreTx<'s> for RedbTx<'s> {
    fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
        match self {
            RedbTx::Reader(r) => redb_table_get(&r.read_txn, key),
            RedbTx::Writer(w) => match w.delta.get(key) {
                Some(cached) => Ok(cached.clone()),
                None => redb_table_get(&w.snapshot, key),
            },
        }
    }

    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
        match self {
            RedbTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            RedbTx::Writer(w) => {
                w.delta.insert(key.to_vec(), Some(val.to_vec()));
                Ok(())
            }
        }
    }

    fn supports_par_put(&self) -> bool {
        false
    }

    fn del(&mut self, key: &[u8]) -> Result<()> {
        match self {
            RedbTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            RedbTx::Writer(w) => {
                w.delta.insert(key.to_vec(), None);
                Ok(())
            }
        }
    }

    fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> Result<()> {
        match self {
            RedbTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
            RedbTx::Writer(w) => {
                let persisted = redb_collect_range(&w.snapshot, lower, upper)?;
                for (k, _) in persisted {
                    w.delta.insert(k, None);
                }
                Ok(())
            }
        }
    }

    fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
        Ok(match self {
            RedbTx::Reader(r) => redb_table_get(&r.read_txn, key)?.is_some(),
            RedbTx::Writer(w) => match w.delta.get(key) {
                Some(cached) => cached.is_some(),
                None => redb_table_get(&w.snapshot, key)?.is_some(),
            },
        })
    }

    fn commit(&mut self) -> Result<()> {
        match self {
            RedbTx::Reader(_) => Ok(()),
            RedbTx::Writer(w) => {
                if w.delta.is_empty() {
                    return Ok(());
                }

                let write_txn = w.storage.db.begin_write().map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "redb",
                        message: format!("begin_write: {e}"),
                    }
                    .build()
                })?;
                {
                    let mut table = write_txn.open_table(TABLE).map_err(|e| {
                        TransactionFailedSnafu {
                            backend: "redb",
                            message: format!("open_table: {e}"),
                        }
                        .build()
                    })?;
                    for (k, mv) in &w.delta {
                        match mv {
                            None => {
                                table.remove(k.as_slice()).map_err(|e| {
                                    TransactionFailedSnafu {
                                        backend: "redb",
                                        message: format!("remove: {e}"),
                                    }
                                    .build()
                                })?;
                            }
                            Some(v) => {
                                table.insert(k.as_slice(), v.as_slice()).map_err(|e| {
                                    TransactionFailedSnafu {
                                        backend: "redb",
                                        message: format!("insert: {e}"),
                                    }
                                    .build()
                                })?;
                            }
                        }
                    }
                }
                write_txn.commit().map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "redb",
                        message: format!("commit: {e}"),
                    }
                    .build()
                })?;

                w.delta.clear();
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
            RedbTx::Reader(r) => match redb_collect_range(&r.read_txn, lower, upper) {
                Ok(pairs) => Box::new(
                    pairs
                        .into_iter()
                        .map(|(k, v)| Ok(decode_tuple_from_kv(&k, &v, None))),
                ),
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
            },
            RedbTx::Writer(w) => match redb_collect_range(&w.snapshot, lower, upper) {
                Ok(persisted) => Box::new(DeltaMergeIter {
                    change_iter: w.delta.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: persisted.into_iter().fuse(),
                    change_cache: None,
                    db_cache: None,
                }),
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
            },
        }
    }

    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
        match self {
            RedbTx::Reader(r) => match redb_collect_range(&r.read_txn, lower, upper) {
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
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
            },
            RedbTx::Writer(w) => match redb_collect_range(&w.snapshot, lower, upper) {
                Ok(persisted) => {
                    // Collect merged view (persisted + delta) then apply skip logic
                    let merged = merge_with_delta(persisted, &w.delta, lower, upper);
                    Box::new(
                        CollectedSkipIterator {
                            data: merged,
                            pos: 0,
                            upper: upper.to_vec(),
                            valid_at,
                            next_bound: lower.to_vec(),
                        }
                        .map(Ok),
                    )
                }
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
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
        match self {
            RedbTx::Reader(r) => match redb_collect_range(&r.read_txn, lower, upper) {
                Ok(pairs) => Box::new(pairs.into_iter().map(Ok)),
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
            },
            RedbTx::Writer(w) => match redb_collect_range(&w.snapshot, lower, upper) {
                Ok(persisted) => Box::new(DeltaMergeIterRaw {
                    change_iter: w.delta.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: persisted.into_iter().fuse(),
                    change_cache: None,
                    db_cache: None,
                }),
                Err(e) => Box::new(std::iter::once(Err(
                    crate::engine::error::InternalError::from(e),
                ))),
            },
        }
    }

    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
    where
        's: 'a,
    {
        match self {
            RedbTx::Reader(r) => {
                let table = r.read_txn.open_table(TABLE).map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "redb",
                        message: format!("open_table: {e}"),
                    }
                    .build()
                })?;
                let range = table.range(lower..upper).map_err(|e| {
                    TransactionFailedSnafu {
                        backend: "redb",
                        message: format!("range: {e}"),
                    }
                    .build()
                })?;
                Ok(range.count())
            }
            RedbTx::Writer(w) => {
                let persisted = redb_collect_range(&w.snapshot, lower, upper)?;
                Ok(DeltaMergeIterRaw {
                    change_iter: w.delta.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: persisted.into_iter().fuse(),
                    change_cache: None,
                    db_cache: None,
                }
                .count())
            }
        }
    }
}

struct DeltaMergeIterRaw<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    change_iter: C,
    db_iter: std::iter::Fuse<std::vec::IntoIter<(Vec<u8>, Vec<u8>)>>,
    change_cache: Option<(&'a Vec<u8>, &'a Option<Vec<u8>>)>,
    db_cache: Option<(Vec<u8>, Vec<u8>)>,
}

impl<'a, C> DeltaMergeIterRaw<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    #[inline]
    fn fill_cache(&mut self) {
        if self.change_cache.is_none() {
            if let Some(kmv) = self.change_iter.next() {
                self.change_cache = Some(kmv);
            }
        }
        if self.db_cache.is_none() {
            if let Some(kv) = self.db_iter.next() {
                self.db_cache = Some(kv);
            }
        }
    }

    #[inline]
    fn next_inner(&mut self) -> InternalResult<Option<(Vec<u8>, Vec<u8>)>> {
        loop {
            self.fill_cache();
            match (&self.change_cache, &self.db_cache) {
                (None, None) => return Ok(None),
                (Some(_), None) => {
                    let (k, cv) = self
                        .change_cache
                        .take()
                        .expect("change_cache present: matched Some(_) arm");
                    match cv {
                        None => continue,
                        Some(v) => return Ok(Some((k.clone(), v.clone()))),
                    }
                }
                (None, Some(_)) => {
                    let (k, v) = self
                        .db_cache
                        .take()
                        .expect("db_cache present: matched Some(_) arm");
                    return Ok(Some((k, v)));
                }
                (Some((ck, _)), Some((dk, _))) => match ck.as_slice().cmp(dk.as_slice()) {
                    Ordering::Less => {
                        let (k, sv) = self
                            .change_cache
                            .take()
                            .expect("change_cache present: matched Some(_) arm");
                        match sv {
                            None => continue,
                            Some(v) => return Ok(Some((k.clone(), v.clone()))),
                        }
                    }
                    Ordering::Greater => {
                        let (k, v) = self
                            .db_cache
                            .take()
                            .expect("db_cache present: matched Some(_) arm");
                        return Ok(Some((k, v)));
                    }
                    Ordering::Equal => {
                        // Delta overrides persisted
                        self.db_cache.take();
                        continue;
                    }
                },
            }
        }
    }
}

impl<'a, C> Iterator for DeltaMergeIterRaw<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    type Item = InternalResult<(Vec<u8>, Vec<u8>)>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        swap_option_result(self.next_inner())
    }
}

struct DeltaMergeIter<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    change_iter: C,
    db_iter: std::iter::Fuse<std::vec::IntoIter<(Vec<u8>, Vec<u8>)>>,
    change_cache: Option<(&'a Vec<u8>, &'a Option<Vec<u8>>)>,
    db_cache: Option<(Vec<u8>, Vec<u8>)>,
}

impl<'a, C> DeltaMergeIter<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    #[inline]
    fn fill_cache(&mut self) {
        if self.change_cache.is_none() {
            if let Some(kmv) = self.change_iter.next() {
                self.change_cache = Some(kmv);
            }
        }
        if self.db_cache.is_none() {
            if let Some(kv) = self.db_iter.next() {
                self.db_cache = Some(kv);
            }
        }
    }

    #[inline]
    fn next_inner(&mut self) -> InternalResult<Option<Tuple>> {
        loop {
            self.fill_cache();
            match (&self.change_cache, &self.db_cache) {
                (None, None) => return Ok(None),
                (Some(_), None) => {
                    let (k, cv) = self
                        .change_cache
                        .take()
                        .expect("change_cache present: matched Some(_) arm");
                    match cv {
                        None => continue,
                        Some(v) => return Ok(Some(decode_tuple_from_kv(k, v, None))),
                    }
                }
                (None, Some(_)) => {
                    let (k, v) = self
                        .db_cache
                        .take()
                        .expect("db_cache present: matched Some(_) arm");
                    return Ok(Some(decode_tuple_from_kv(&k, &v, None)));
                }
                (Some((ck, _)), Some((dk, _))) => match ck.as_slice().cmp(dk.as_slice()) {
                    Ordering::Less => {
                        let (k, sv) = self
                            .change_cache
                            .take()
                            .expect("change_cache present: matched Some(_) arm");
                        match sv {
                            None => continue,
                            Some(v) => return Ok(Some(decode_tuple_from_kv(k, v, None))),
                        }
                    }
                    Ordering::Greater => {
                        let (k, v) = self
                            .db_cache
                            .take()
                            .expect("db_cache present: matched Some(_) arm");
                        return Ok(Some(decode_tuple_from_kv(&k, &v, None)));
                    }
                    Ordering::Equal => {
                        self.db_cache.take();
                        continue;
                    }
                },
            }
        }
    }
}

impl<'a, C> Iterator for DeltaMergeIter<'a, C>
where
    C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
{
    type Item = InternalResult<Tuple>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        swap_option_result(self.next_inner())
    }
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
            // Find the next entry >= next_bound and < upper
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

/// Merge persisted data with delta, producing a sorted Vec of live (non-tombstoned) pairs.
fn merge_with_delta(
    persisted: Vec<(Vec<u8>, Vec<u8>)>,
    delta: &BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    lower: &[u8],
    upper: &[u8],
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut result = Vec::new();
    let mut db_iter = persisted.into_iter().peekable();
    let mut delta_iter = delta.range(lower.to_vec()..upper.to_vec()).peekable();

    loop {
        match (db_iter.peek(), delta_iter.peek()) {
            (None, None) => break,
            (Some(_), None) => {
                result.extend(db_iter);
                break;
            }
            (None, Some(_)) => {
                for (k, mv) in delta_iter {
                    if let Some(v) = mv {
                        result.push((k.clone(), v.clone()));
                    }
                }
                break;
            }
            (Some((dk, _)), Some((ck, _))) => match dk.as_slice().cmp(ck.as_slice()) {
                Ordering::Less => {
                    result.push(db_iter.next().expect("db_iter present: just peeked Some"));
                }
                Ordering::Greater => {
                    let (k, mv) = delta_iter
                        .next()
                        .expect("delta_iter present: just peeked Some");
                    if let Some(v) = mv {
                        result.push((k.clone(), v.clone()));
                    }
                }
                Ordering::Equal => {
                    db_iter.next(); // discard persisted
                    let (k, mv) = delta_iter
                        .next()
                        .expect("delta_iter present: just peeked Some");
                    if let Some(v) = mv {
                        result.push((k.clone(), v.clone()));
                    }
                }
            },
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::data::value::{DataValue, Validity};
    use crate::engine::error::InternalResult;
    use crate::engine::runtime::db::ScriptMutability;
    use snafu::ResultExt;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn setup_test_db() -> InternalResult<(TempDir, DbCore<RedbStorage>)> {
        let temp_dir = TempDir::new().context(IoSnafu { backend: "test" })?;
        let db = new_cozo_redb(temp_dir.path())?;
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

    /// Verify that `TempDir` cleans up the database file on drop — including
    /// when the outer scope exits normally.  This documents the RAII contract
    /// relied on by all redb tests.
    #[test]
    fn temp_dir_raii_removes_database_file_on_drop() {
        let db_path = {
            let dir = TempDir::new().expect("create temp dir");
            let db_file = dir.path().join("data.redb");
            let path_copy = db_file.clone();

            // Create the database so the file exists.
            new_cozo_redb(dir.path()).expect("create db");
            assert!(
                db_file.exists(),
                "data.redb should exist while TempDir is live"
            );

            path_copy
            // `dir` (TempDir) drops here, removing the directory tree.
        };
        assert!(
            !db_path.exists(),
            "data.redb should be removed after TempDir drop"
        );
    }

    /// Verify that data written and committed before dropping the Database
    /// is readable after reopening — confirming WAL flush on drop.
    #[test]
    fn redb_flushes_wal_on_database_drop() -> InternalResult<()> {
        use crate::engine::runtime::db::ScriptMutability;

        let dir = TempDir::new().context(IoSnafu { backend: "test" })?;

        // Write and commit data, then drop the Database.
        {
            let db = new_cozo_redb(dir.path())?;
            db.run_script(
                "{:create wal_check {k: Int => v: String}}",
                Default::default(),
                ScriptMutability::Mutable,
            )?;
            db.run_script(
                r#"?[k, v] <- [[42, "sentinel"]] :put wal_check {k => v}"#,
                Default::default(),
                ScriptMutability::Mutable,
            )?;
            // `db` dropped here — redb flushes WAL.
        }

        // Reopen and confirm the data survived.
        let db2 = new_cozo_redb(dir.path())?;
        let result = db2.run_script(
            "?[v] := *wal_check{k: 42, v}",
            Default::default(),
            ScriptMutability::Immutable,
        )?;
        assert_eq!(result.rows.len(), 1, "row should survive WAL flush on drop");
        assert_eq!(result.rows[0][0], DataValue::Str("sentinel".into()));
        Ok(())
    }

    #[test]
    fn basic_operations() -> InternalResult<()> {
        let (_dir, db) = setup_test_db()?;

        let mut to_import = BTreeMap::new();
        to_import.insert(
            "plain".to_string(),
            crate::engine::NamedRows {
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
            crate::engine::NamedRows {
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
            crate::engine::NamedRows {
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
        let dir = TempDir::new().context(IoSnafu { backend: "test" })?;

        // Write data
        {
            let db = new_cozo_redb(dir.path())?;
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

        // Reopen and verify
        {
            let db = new_cozo_redb(dir.path())?;
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
            crate::engine::NamedRows {
                headers: vec!["k".to_string(), "v".to_string()],
                rows: (0..10)
                    .map(|i| vec![DataValue::from(i), DataValue::from(i)])
                    .collect(),
                next: None,
            },
        );
        db.import_relations(to_import)?;

        // Multiple concurrent read queries should all succeed
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
}
