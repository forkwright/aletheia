//! In-memory storage backend.
//!
//! Land-dark dual state (PLAN.md §2, krites retirement): `derived` carries
//! the CozoDB-derived implementation forward unchanged and stays the
//! crate-visible default — it is also the differential oracle every other
//! backend (`storage::fjall_backend`) and this file's own conformance suite
//! is checked against, so it must not change shape while `sovereign` is
//! being proven (PLAN.md §4 wave 1b, §9 kill criterion "oracle inversion").
//! `sovereign` is written fresh against the same `Storage`/`StoreTx`
//! contract (`storage/mod.rs`) and is selected instead when the crate is
//! built with `--cfg krites_sovereign_storage_mem`. Both submodules compile
//! unconditionally — the cfg only decides which one is re-exported as
//! `MemStorage`/`MemTx`/`new_mem_db` — so `#[cfg(test)] mod conformance`
//! below can run identical operation sequences against both and assert
//! identical observable output regardless of which is active.

pub(crate) mod derived {
    use std::cmp::Ordering;
    use std::collections::BTreeMap;
    use std::collections::btree_map::Range;
    use std::default::Default;
    use std::iter::Fuse;
    use std::mem;
    use std::ops::Bound;
    use std::sync::Arc;

    use crossbeam::sync::{ShardedLock, ShardedLockReadGuard};
    use itertools::Itertools;

    use crate::data::tuple::{Tuple, check_key_for_validity};
    use crate::data::value::ValidityTs;
    use crate::error::InternalResult;
    use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
    use crate::storage::error::{StorageResult, WriteInReadTransactionSnafu};
    use crate::storage::{Storage, StoreTx};
    use crate::utils::swap_option_result;

    type Result<T> = StorageResult<T>;

    /// Create a database backed by memory.
    /// This is the fastest storage, but non-persistent.
    /// Concurrent readers are not blocked by an open writer; writers apply
    /// their deltas under a short exclusive lock only at commit time.
    #[expect(
        clippy::result_large_err,
        reason = "InternalError carries structured context — boxing deferred to avoid API churn across engine internals"
    )]
    // NOTE: dead under `--cfg krites_sovereign_storage_mem` — that build
    // re-exports `sovereign::new_mem_db` instead (see the cfg-gated `pub
    // use` below this file's two `mod` blocks). Both always compile
    // (PLAN.md §2); only one is ever reachable from a non-test build.
    #[cfg_attr(krites_sovereign_storage_mem, allow(dead_code))]
    pub fn new_mem_db() -> crate::error::InternalResult<crate::DbCore<MemStorage>> {
        let ret = crate::DbCore::new(MemStorage::default())?;
        ret.initialize()?;
        Ok(ret)
    }

    /// The non-persistent storage
    #[derive(Default, Clone)]
    pub struct MemStorage {
        store: Arc<ShardedLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    }

    impl<'s> Storage<'s> for MemStorage {
        type Tx = MemTx<'s>;

        fn storage_kind(&self) -> &'static str {
            "mem"
        }

        fn transact(&'s self, write: bool) -> Result<Self::Tx> {
            Ok(if write {
                // WHY: Take a point-in-time snapshot of the base map so the writer
                // body does not hold the exclusive lock. Readers can proceed while
                // the writer builds its delta. The snapshot is cloned under a brief
                // read lock; writers serialize only at commit time.
                let snapshot = self
                    .store
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                MemTx::Writer(Arc::clone(&self.store), snapshot, BTreeMap::default())
            } else {
                let rdr = self
                    .store
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                MemTx::Reader(rdr)
            })
        }

        fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            Ok(())
        }

        fn batch_put<'a>(
            &'a self,
            data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
        ) -> Result<()> {
            let mut store = self
                .store
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for pair in data {
                let (k, v) = pair?;
                store.insert(k, v);
            }
            Ok(())
        }
    }

    #[non_exhaustive]
    pub enum MemTx<'s> {
        Reader(ShardedLockReadGuard<'s, BTreeMap<Vec<u8>, Vec<u8>>>),
        Writer(
            Arc<ShardedLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
            BTreeMap<Vec<u8>, Vec<u8>>,
            BTreeMap<Vec<u8>, Option<Vec<u8>>>,
        ),
    }

    impl<'s> StoreTx<'s> for MemTx<'s> {
        fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
            Ok(match self {
                MemTx::Reader(rdr) => rdr.get(key).cloned(),
                MemTx::Writer(_, snapshot, cache) => match cache.get(key) {
                    Some(r) => r.clone(),
                    None => snapshot.get(key).cloned(),
                },
            })
        }

        fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
            match self {
                MemTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                MemTx::Writer(_, _, cache) => {
                    cache.insert(key.to_vec(), Some(val.to_vec()));
                    Ok(())
                }
            }
        }

        fn supports_par_put(&self) -> bool {
            false
        }

        fn par_put(&self, _key: &[u8], _val: &[u8]) -> Result<()> {
            Err(crate::storage::error::TransactionFailedSnafu {
                backend: "mem",
                message: "par_put is not supported",
            }
            .build())
        }

        fn del(&mut self, key: &[u8]) -> Result<()> {
            match self {
                MemTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                MemTx::Writer(_, _, cache) => {
                    cache.insert(key.to_vec(), None);
                    Ok(())
                }
            }
        }

        fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> Result<()> {
            match self {
                MemTx::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                MemTx::Writer(_, snapshot, cache) => {
                    // WHY: Range deletes are recorded in the delta instead of
                    // mutating the base map immediately. This keeps the writer
                    // body lock-free and lets commit merge all changes at once.
                    let keys = snapshot
                        .range(lower.to_vec()..upper.to_vec())
                        .map(|(k, _)| k.clone())
                        .collect_vec();
                    for k in keys {
                        cache.insert(k, None);
                    }
                    Ok(())
                }
            }
        }

        fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
            Ok(match self {
                MemTx::Reader(rdr) => rdr.contains_key(key),
                MemTx::Writer(_, snapshot, cache) => match cache.get(key) {
                    Some(r) => r.is_some(),
                    None => snapshot.contains_key(key),
                },
            })
        }

        fn commit(&mut self) -> Result<()> {
            match self {
                MemTx::Reader(_) => Ok(()),
                MemTx::Writer(store, _snapshot, cached) => {
                    let mut cache = BTreeMap::default();
                    mem::swap(&mut cache, cached);
                    let mut store_guard = store
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    for (k, mv) in cache {
                        match mv {
                            None => {
                                store_guard.remove(&k);
                            }
                            Some(v) => {
                                store_guard.insert(k, v);
                            }
                        }
                    }
                    Ok(())
                }
            }
        }

        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        fn range_scan_tuple<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
        ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>
        where
            's: 'a,
        {
            match self {
                MemTx::Reader(rdr) => Box::new(
                    rdr.range(lower.to_vec()..upper.to_vec())
                        .map(|(k, v)| Ok(decode_tuple_from_kv(k, v, None))),
                ),
                MemTx::Writer(_, snapshot, cache) => Box::new(CacheIter {
                    change_iter: cache.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: snapshot.range(lower.to_vec()..upper.to_vec()).fuse(),
                    change_cache: None,
                    db_cache: None,
                }),
            }
        }

        fn range_skip_scan_tuple<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
            valid_at: ValidityTs,
        ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
            match self {
                MemTx::Reader(stored) => Box::new(
                    SkipIterator {
                        inner: stored,
                        upper: upper.to_vec(),
                        valid_at,
                        next_bound: lower.to_vec(),
                        size_hint: None,
                    }
                    .map(Ok),
                ),
                MemTx::Writer(_, stored, delta) => Box::new(
                    SkipDualIterator {
                        stored,
                        delta,
                        upper: upper.to_vec(),
                        valid_at,
                        next_bound: lower.to_vec(),
                    }
                    .map(Ok),
                ),
            }
        }

        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        fn range_scan<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
        ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>
        where
            's: 'a,
        {
            match self {
                MemTx::Reader(rdr) => Box::new(
                    rdr.range(lower.to_vec()..upper.to_vec())
                        .map(|(k, v)| Ok((k.clone(), v.clone()))),
                ),
                MemTx::Writer(_, snapshot, cache) => Box::new(CacheIterRaw {
                    change_iter: cache.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: snapshot.range(lower.to_vec()..upper.to_vec()).fuse(),
                    change_cache: None,
                    db_cache: None,
                }),
            }
        }

        fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
        where
            's: 'a,
        {
            Ok(match self {
                MemTx::Reader(rdr) => rdr.range(lower.to_vec()..upper.to_vec()).count(),
                MemTx::Writer(_, snapshot, cache) => (CacheIterRaw {
                    change_iter: cache.range(lower.to_vec()..upper.to_vec()).fuse(),
                    db_iter: snapshot.range(lower.to_vec()..upper.to_vec()).fuse(),
                    change_cache: None,
                    db_cache: None,
                })
                .count(),
            })
        }
    }

    struct CacheIterRaw<'a, C, T>
    where
        C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
        T: Iterator<Item = (&'a Vec<u8>, &'a Vec<u8>)>,
    {
        change_iter: C,
        db_iter: T,
        change_cache: Option<(&'a Vec<u8>, &'a Option<Vec<u8>>)>,
        db_cache: Option<(&'a Vec<u8>, &'a Vec<u8>)>,
    }

    impl<'a, C, T> CacheIterRaw<'a, C, T>
    where
        C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
        T: Iterator<Item = (&'a Vec<u8>, &'a Vec<u8>)>,
    {
        #[inline]
        #[expect(
            clippy::unnecessary_wraps,
            reason = "returns Result for consistency with next_inner's ? chaining — fill_cache is always called via `self.fill_cache()?`"
        )]
        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        #[expect(
            clippy::semicolon_if_nothing_returned,
            reason = "let-chain in if-expression — adding semicolon changes semantics inside let-else"
        )]
        fn fill_cache(&mut self) -> InternalResult<()> {
            if self.change_cache.is_none()
                && let Some(kmv) = self.change_iter.next()
            {
                self.change_cache = Some(kmv)
            }

            if self.db_cache.is_none()
                && let Some(kv) = self.db_iter.next()
            {
                self.db_cache = Some(kv);
            }

            Ok(())
        }

        #[inline]
        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        #[expect(
            clippy::needless_continue,
            reason = "explicit continue clarifies the merge-iterator control flow across three match arms"
        )]
        fn next_inner(&mut self) -> InternalResult<Option<(Vec<u8>, Vec<u8>)>> {
            let corrupted = || {
                crate::error::InternalError::from(
                    crate::storage::error::CorruptedDataSnafu {
                        message: "cache unexpectedly empty after match confirmed Some",
                    }
                    .build(),
                )
            };
            loop {
                self.fill_cache()?;
                match (&self.change_cache, &self.db_cache) {
                    (None, None) => return Ok(None),
                    (Some(_), None) => {
                        let (k, cv) = self.change_cache.take().ok_or_else(corrupted)?;
                        match cv {
                            None => continue,
                            Some(v) => return Ok(Some((k.clone(), v.clone()))),
                        }
                    }
                    (None, Some(_)) => {
                        let (k, v) = self.db_cache.take().ok_or_else(corrupted)?;
                        return Ok(Some((k.clone(), v.clone())));
                    }
                    (Some((ck, _)), Some((dk, _))) => match ck.cmp(dk) {
                        Ordering::Less => {
                            let (k, sv) = self.change_cache.take().ok_or_else(corrupted)?;
                            match sv {
                                None => continue,
                                Some(v) => return Ok(Some((k.clone(), v.clone()))),
                            }
                        }
                        Ordering::Greater => {
                            let (k, v) = self.db_cache.take().ok_or_else(corrupted)?;
                            return Ok(Some((k.clone(), v.clone())));
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

    impl<'a, C, T> Iterator for CacheIterRaw<'a, C, T>
    where
        C: Iterator<Item = (&'a Vec<u8>, &'a Option<Vec<u8>>)> + 'a,
        T: Iterator<Item = (&'a Vec<u8>, &'a Vec<u8>)>,
    {
        type Item = InternalResult<(Vec<u8>, Vec<u8>)>;

        #[inline]
        fn next(&mut self) -> Option<Self::Item> {
            swap_option_result(self.next_inner())
        }
    }

    struct CacheIter<'a> {
        change_iter: Fuse<Range<'a, Vec<u8>, Option<Vec<u8>>>>,
        db_iter: Fuse<Range<'a, Vec<u8>, Vec<u8>>>,
        change_cache: Option<(&'a Vec<u8>, &'a Option<Vec<u8>>)>,
        db_cache: Option<(&'a Vec<u8>, &'a Vec<u8>)>,
    }

    impl CacheIter<'_> {
        #[inline]
        #[expect(
            clippy::unnecessary_wraps,
            reason = "returns Result for consistency with next_inner's ? chaining — fill_cache is always called via `self.fill_cache()?`"
        )]
        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        #[expect(
            clippy::semicolon_if_nothing_returned,
            reason = "let-chain in if-expression — adding semicolon changes semantics inside let-else"
        )]
        fn fill_cache(&mut self) -> InternalResult<()> {
            if self.change_cache.is_none()
                && let Some(kmv) = self.change_iter.next()
            {
                self.change_cache = Some(kmv)
            }

            if self.db_cache.is_none()
                && let Some(kv) = self.db_iter.next()
            {
                self.db_cache = Some(kv);
            }

            Ok(())
        }

        #[inline]
        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        #[expect(
            clippy::needless_continue,
            reason = "explicit continue clarifies the merge-iterator control flow across three match arms"
        )]
        fn next_inner(&mut self) -> InternalResult<Option<Tuple>> {
            let corrupted = || {
                crate::error::InternalError::from(
                    crate::storage::error::CorruptedDataSnafu {
                        message: "cache unexpectedly empty after match confirmed Some",
                    }
                    .build(),
                )
            };
            loop {
                self.fill_cache()?;
                match (&self.change_cache, &self.db_cache) {
                    (None, None) => return Ok(None),
                    (Some(_), None) => {
                        let (k, cv) = self.change_cache.take().ok_or_else(corrupted)?;
                        match cv {
                            None => continue,
                            Some(v) => return Ok(Some(decode_tuple_from_kv(k, v, None))),
                        }
                    }
                    (None, Some(_)) => {
                        let (k, v) = self.db_cache.take().ok_or_else(corrupted)?;
                        return Ok(Some(decode_tuple_from_kv(k, v, None)));
                    }
                    (Some((ck, _)), Some((dk, _))) => match ck.cmp(dk) {
                        Ordering::Less => {
                            let (k, sv) = self.change_cache.take().ok_or_else(corrupted)?;
                            match sv {
                                None => continue,
                                Some(v) => return Ok(Some(decode_tuple_from_kv(k, v, None))),
                            }
                        }
                        Ordering::Greater => {
                            let (k, v) = self.db_cache.take().ok_or_else(corrupted)?;
                            return Ok(Some(decode_tuple_from_kv(k, v, None)));
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

    impl Iterator for CacheIter<'_> {
        type Item = InternalResult<Tuple>;

        #[inline]
        fn next(&mut self) -> Option<Self::Item> {
            swap_option_result(self.next_inner())
        }
    }

    /// Skip-scan over a `BTreeMap`: each `next()` re-runs a range lookup from
    /// `next_bound` because `BTreeMap` has no seekable cursor
    /// (<https://github.com/rust-lang/rust/issues/49638>).
    pub(crate) struct SkipIterator<'a> {
        pub(crate) inner: &'a BTreeMap<Vec<u8>, Vec<u8>>,
        pub(crate) upper: Vec<u8>,
        pub(crate) valid_at: ValidityTs,
        pub(crate) next_bound: Vec<u8>,
        pub(crate) size_hint: Option<usize>,
    }

    impl Iterator for SkipIterator<'_> {
        type Item = Tuple;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                let nxt = self
                    .inner
                    .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                        Bound::Included(&self.next_bound),
                        Bound::Excluded(&self.upper),
                    ))
                    .next();
                match nxt {
                    None => return None,
                    Some((candidate_key, candidate_val)) => {
                        let (ret, nxt_bound) =
                            check_key_for_validity(candidate_key, self.valid_at, self.size_hint);
                        self.next_bound = nxt_bound;
                        if let Some(mut nk) = ret {
                            extend_tuple_from_v(&mut nk, candidate_val);
                            return Some(nk);
                        }
                    }
                }
            }
        }
    }

    struct SkipDualIterator<'a> {
        stored: &'a BTreeMap<Vec<u8>, Vec<u8>>,
        delta: &'a BTreeMap<Vec<u8>, Option<Vec<u8>>>,
        upper: Vec<u8>,
        valid_at: ValidityTs,
        next_bound: Vec<u8>,
    }

    impl Iterator for SkipDualIterator<'_> {
        type Item = Tuple;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                let stored_nxt = self
                    .stored
                    .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                        Bound::Included(&self.next_bound),
                        Bound::Excluded(&self.upper),
                    ))
                    .next();
                let delta_nxt = self
                    .delta
                    .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                        Bound::Included(&self.next_bound),
                        Bound::Excluded(&self.upper),
                    ))
                    .next();
                let (candidate_key, candidate_val) = match (stored_nxt, delta_nxt) {
                    (None, None) => return None,
                    (None, Some((delta_key, maybe_delta_val))) => match maybe_delta_val {
                        None => {
                            let (_, nxt_seek) =
                                check_key_for_validity(delta_key, self.valid_at, None);
                            self.next_bound = nxt_seek;
                            continue;
                        }
                        Some(delta_val) => (delta_key, delta_val),
                    },
                    (Some((stored_key, stored_val)), None) => (stored_key, stored_val),
                    (Some((stored_key, stored_val)), Some((delta_key, maybe_delta_val))) => {
                        if stored_key < delta_key {
                            (stored_key, stored_val)
                        } else {
                            match maybe_delta_val {
                                None => {
                                    let (_, nxt_seek) =
                                        check_key_for_validity(delta_key, self.valid_at, None);
                                    self.next_bound = nxt_seek;
                                    continue;
                                }
                                Some(delta_val) => (delta_key, delta_val),
                            }
                        }
                    }
                };
                let (ret, nxt_bound) = check_key_for_validity(candidate_key, self.valid_at, None);
                self.next_bound = nxt_bound;
                if let Some(mut nk) = ret {
                    extend_tuple_from_v(&mut nk, candidate_val);
                    return Some(nk);
                }
            }
        }
    }
}

pub(crate) mod sovereign {
    //! Fresh implementation of the mem-storage contract.
    //!
    //! Same observable contract as `derived`: `transact(true)` hands back a
    //! writer holding a point-in-time snapshot of the base map plus its own
    //! pending delta (read-your-own-writes, invisible to everyone else
    //! until `commit`); `transact(false)` hands back a plain read guard.
    //! `batch_put` bypasses MVCC entirely — it takes the write lock and
    //! inserts directly, for bulk/initial loads where isolation from
    //! concurrent transactions is not needed.
    //!
    //! Structured differently from `derived`: one merge-scan core
    //! (`MergeRangeIter`) backs both `range_scan` and `range_scan_tuple`
    //! instead of two parallel hand-rolled merge iterators, and one
    //! validity-aware skip-scan (`ValiditySkipIter`) serves both the reader
    //! and writer cases instead of two separate iterator types — a reader
    //! is simply a writer with no pending delta.

    use std::cmp::Ordering;
    use std::collections::BTreeMap;
    use std::collections::btree_map::Range;
    use std::iter::Fuse;
    use std::mem;
    use std::ops::Bound;
    use std::sync::Arc;

    use crossbeam::sync::{ShardedLock, ShardedLockReadGuard};

    use crate::data::tuple::{Tuple, check_key_for_validity};
    use crate::data::value::ValidityTs;
    use crate::error::InternalResult;
    use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
    use crate::storage::error::{
        StorageResult, TransactionFailedSnafu, WriteInReadTransactionSnafu,
    };
    use crate::storage::{Storage, StoreTx};

    type Result<T> = StorageResult<T>;
    type BaseMap = BTreeMap<Vec<u8>, Vec<u8>>;
    type PendingMap = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

    #[expect(
        clippy::result_large_err,
        reason = "InternalError carries structured context — boxing deferred to avoid API churn across engine internals"
    )]
    // NOTE: dead when `krites_sovereign_storage_mem` is NOT set — the
    // default build re-exports `derived::new_mem_db` instead. Both always
    // compile (PLAN.md §2); only one is ever reachable from a non-test
    // build.
    #[cfg_attr(not(krites_sovereign_storage_mem), allow(dead_code))]
    pub fn new_mem_db() -> crate::error::InternalResult<crate::DbCore<MemStorage>> {
        let db = crate::DbCore::new(MemStorage::default())?;
        db.initialize()?;
        Ok(db)
    }

    /// Non-persistent, MVCC-isolated in-memory storage.
    #[derive(Default, Clone)]
    pub struct MemStorage {
        rows: Arc<ShardedLock<BaseMap>>,
    }

    impl<'s> Storage<'s> for MemStorage {
        type Tx = MemTx<'s>;

        fn storage_kind(&self) -> &'static str {
            "mem"
        }

        fn transact(&'s self, write: bool) -> Result<Self::Tx> {
            if write {
                let snapshot = self
                    .rows
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                Ok(MemTx::Writer {
                    base: Arc::clone(&self.rows),
                    snapshot,
                    pending: PendingMap::new(),
                })
            } else {
                let guard = self
                    .rows
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Ok(MemTx::Reader(guard))
            }
        }

        fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            Ok(())
        }

        fn batch_put<'a>(
            &'a self,
            data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
        ) -> Result<()> {
            let mut guard = self
                .rows
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for pair in data {
                let (key, val) = pair?;
                guard.insert(key, val);
            }
            Ok(())
        }
    }

    #[non_exhaustive]
    pub enum MemTx<'s> {
        Reader(ShardedLockReadGuard<'s, BaseMap>),
        Writer {
            base: Arc<ShardedLock<BaseMap>>,
            snapshot: BaseMap,
            pending: PendingMap,
        },
    }

    impl<'s> StoreTx<'s> for MemTx<'s> {
        fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
            match self {
                Self::Reader(guard) => Ok(guard.get(key).cloned()),
                Self::Writer {
                    snapshot, pending, ..
                } => Ok(match pending.get(key) {
                    Some(shadowed) => shadowed.clone(),
                    None => snapshot.get(key).cloned(),
                }),
            }
        }

        fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
            match self {
                Self::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                Self::Writer { pending, .. } => {
                    pending.insert(key.to_vec(), Some(val.to_vec()));
                    Ok(())
                }
            }
        }

        fn supports_par_put(&self) -> bool {
            false
        }

        fn par_put(&self, _key: &[u8], _val: &[u8]) -> Result<()> {
            Err(TransactionFailedSnafu {
                backend: "mem",
                message: "par_put is not supported",
            }
            .build())
        }

        fn del(&mut self, key: &[u8]) -> Result<()> {
            match self {
                Self::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                Self::Writer { pending, .. } => {
                    pending.insert(key.to_vec(), None);
                    Ok(())
                }
            }
        }

        fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> Result<()> {
            match self {
                Self::Reader(_) => Err(WriteInReadTransactionSnafu.build()),
                Self::Writer {
                    snapshot, pending, ..
                } => {
                    // WHY: only the point-in-time `snapshot` is scanned — a
                    // key this same transaction has newly `put` is not yet
                    // part of the persisted view a "delete from persisted
                    // data only" range is defined against. Matches
                    // `derived::MemTx::del_range_from_persisted` exactly
                    // (this row's PROVENANCE.toml capability note cites both
                    // as the same behavior).
                    let doomed: Vec<Vec<u8>> = snapshot
                        .range(lower.to_vec()..upper.to_vec())
                        .map(|(key, _)| key.clone())
                        .collect();
                    for key in doomed {
                        pending.insert(key, None);
                    }
                    Ok(())
                }
            }
        }

        fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
            match self {
                Self::Reader(guard) => Ok(guard.contains_key(key)),
                Self::Writer {
                    snapshot, pending, ..
                } => Ok(match pending.get(key) {
                    Some(shadowed) => shadowed.is_some(),
                    None => snapshot.contains_key(key),
                }),
            }
        }

        fn commit(&mut self) -> Result<()> {
            match self {
                Self::Reader(_) => Ok(()),
                Self::Writer { base, pending, .. } => {
                    let ready = mem::take(pending);
                    let mut guard = base
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    for (key, value) in ready {
                        match value {
                            Some(v) => {
                                guard.insert(key, v);
                            }
                            None => {
                                guard.remove(&key);
                            }
                        }
                    }
                    Ok(())
                }
            }
        }

        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        fn range_scan_tuple<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
        ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>
        where
            's: 'a,
        {
            match self {
                Self::Reader(guard) => Box::new(
                    guard
                        .range(lower.to_vec()..upper.to_vec())
                        .map(|(k, v)| Ok(decode_tuple_from_kv(k, v, None))),
                ),
                Self::Writer {
                    snapshot, pending, ..
                } => Box::new(
                    MergeRangeIter::new(pending, snapshot, lower, upper)
                        .map(|(k, v)| Ok(decode_tuple_from_kv(k, v, None))),
                ),
            }
        }

        fn range_skip_scan_tuple<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
            valid_at: ValidityTs,
        ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
            let (base, pending) = match self {
                Self::Reader(guard) => (&**guard, None),
                Self::Writer {
                    snapshot, pending, ..
                } => (snapshot, Some(pending)),
            };
            Box::new(
                ValiditySkipIter {
                    base,
                    pending,
                    upper: upper.to_vec(),
                    valid_at,
                    next_bound: lower.to_vec(),
                    size_hint: None,
                }
                .map(Ok),
            )
        }

        #[expect(
            clippy::result_large_err,
            reason = "InternalResult is the engine-wide error type — cannot box without changing the trait contract"
        )]
        fn range_scan<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
        ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>
        where
            's: 'a,
        {
            match self {
                Self::Reader(guard) => Box::new(
                    guard
                        .range(lower.to_vec()..upper.to_vec())
                        .map(|(k, v)| Ok((k.clone(), v.clone()))),
                ),
                Self::Writer {
                    snapshot, pending, ..
                } => Box::new(
                    MergeRangeIter::new(pending, snapshot, lower, upper)
                        .map(|(k, v)| Ok((k.clone(), v.clone()))),
                ),
            }
        }

        fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
        where
            's: 'a,
        {
            Ok(match self {
                Self::Reader(guard) => guard.range(lower.to_vec()..upper.to_vec()).count(),
                Self::Writer {
                    snapshot, pending, ..
                } => MergeRangeIter::new(pending, snapshot, lower, upper).count(),
            })
        }
    }

    /// Interleaves a writer's uncommitted `pending` delta with its immutable
    /// `base` snapshot, both already key-sorted. A `pending` entry always
    /// wins a tie (it is the newer write); a `None` value is a tombstone
    /// and is skipped rather than yielded.
    struct MergeRangeIter<'a> {
        pending: Fuse<Range<'a, Vec<u8>, Option<Vec<u8>>>>,
        base: Fuse<Range<'a, Vec<u8>, Vec<u8>>>,
        pending_peek: Option<(&'a Vec<u8>, &'a Option<Vec<u8>>)>,
        base_peek: Option<(&'a Vec<u8>, &'a Vec<u8>)>,
    }

    impl<'a> MergeRangeIter<'a> {
        fn new(pending: &'a PendingMap, base: &'a BaseMap, lower: &[u8], upper: &[u8]) -> Self {
            Self {
                pending: pending.range(lower.to_vec()..upper.to_vec()).fuse(),
                base: base.range(lower.to_vec()..upper.to_vec()).fuse(),
                pending_peek: None,
                base_peek: None,
            }
        }

        fn fill(&mut self) {
            if self.pending_peek.is_none() {
                self.pending_peek = self.pending.next();
            }
            if self.base_peek.is_none() {
                self.base_peek = self.base.next();
            }
        }
    }

    impl<'a> Iterator for MergeRangeIter<'a> {
        type Item = (&'a Vec<u8>, &'a Vec<u8>);

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                self.fill();
                match (self.pending_peek, self.base_peek) {
                    (None, None) => return None,
                    (Some((k, v)), None) => {
                        self.pending_peek = None;
                        if let Some(val) = v {
                            return Some((k, val));
                        }
                    }
                    (None, Some(kv)) => {
                        self.base_peek = None;
                        return Some(kv);
                    }
                    (Some((pk, pv)), Some((bk, bv))) => match pk.cmp(bk) {
                        Ordering::Less => {
                            self.pending_peek = None;
                            if let Some(val) = pv {
                                return Some((pk, val));
                            }
                        }
                        Ordering::Greater => {
                            self.base_peek = None;
                            return Some((bk, bv));
                        }
                        Ordering::Equal => {
                            // `pending` shadows `base` at an equal key.
                            self.base_peek = None;
                            self.pending_peek = None;
                            if let Some(val) = pv {
                                return Some((pk, val));
                            }
                        }
                    },
                }
            }
        }
    }

    /// Validity-aware skip-scan, shared by both `MemTx` variants: a reader
    /// passes `pending = None` and scans `base` alone; a writer passes its
    /// own delta and gets the same shadow-and-tombstone treatment
    /// `MergeRangeIter` applies. Each `next()` re-seeks from `next_bound`
    /// because `BTreeMap` has no seekable cursor
    /// (<https://github.com/rust-lang/rust/issues/49638>) — matching
    /// `derived::SkipIterator`/`derived::SkipDualIterator`'s constraint,
    /// unified here into one type instead of two.
    struct ValiditySkipIter<'a> {
        base: &'a BaseMap,
        pending: Option<&'a PendingMap>,
        upper: Vec<u8>,
        valid_at: ValidityTs,
        next_bound: Vec<u8>,
        size_hint: Option<usize>,
    }

    impl<'a> ValiditySkipIter<'a> {
        /// The next candidate at or after `next_bound`, across `base` and —
        /// when present — `pending`. Returns `(key, None)` for a tombstone
        /// so the caller can still advance `next_bound` past it.
        fn candidate(&self) -> Option<(&'a [u8], Option<&'a [u8]>)> {
            let base_next = self
                .base
                .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                    Bound::Included(&self.next_bound),
                    Bound::Excluded(&self.upper),
                ))
                .next()
                .map(|(k, v)| (k.as_slice(), v.as_slice()));
            let Some(pending) = self.pending else {
                return base_next.map(|(k, v)| (k, Some(v)));
            };
            let pending_next = pending
                .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                    Bound::Included(&self.next_bound),
                    Bound::Excluded(&self.upper),
                ))
                .next()
                .map(|(k, v)| (k.as_slice(), v.as_deref()));
            match (base_next, pending_next) {
                (base, None) => base.map(|(k, v)| (k, Some(v))),
                (None, Some((k, v))) => Some((k, v)),
                (Some((bk, bv)), Some((pk, pv))) => {
                    if bk < pk {
                        Some((bk, Some(bv)))
                    } else {
                        Some((pk, pv))
                    }
                }
            }
        }
    }

    impl Iterator for ValiditySkipIter<'_> {
        type Item = Tuple;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                let (key, value) = self.candidate()?;
                let (visible, next_seek) =
                    check_key_for_validity(key, self.valid_at, self.size_hint);
                self.next_bound = next_seek;
                let Some(val) = value else {
                    // Tombstone: not visible at any `valid_at`, but
                    // `next_bound` still had to advance past this key's
                    // validity chain before retrying.
                    continue;
                };
                if let Some(mut tuple) = visible {
                    extend_tuple_from_v(&mut tuple, val);
                    return Some(tuple);
                }
            }
        }
    }
}

// NOTE: `MemTx` (the `Storage::Tx` associated type) is never named outside
// this module — it flows through `Db<MemStorage>` structurally — so only
// `MemStorage` and `new_mem_db` are re-exported; adding `MemTx` here would
// be an unused-import warning under `-D warnings`.
#[cfg(not(krites_sovereign_storage_mem))]
pub use derived::{MemStorage, new_mem_db};

#[cfg(krites_sovereign_storage_mem)]
pub use sovereign::{MemStorage, new_mem_db};

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod conformance {
    //! Differential proof (PLAN.md §4 wave 1b): the same operation sequence
    //! applied to `derived::MemStorage` and `sovereign::MemStorage` must
    //! produce identical observable state, including MVCC isolation
    //! (writer sees its own uncommitted writes; nobody else does until
    //! `commit`). Runs unconditionally — both submodules always compile
    //! regardless of which cfg is active.
    use std::cmp::Reverse;

    use crate::data::tuple::TupleT as _;
    use crate::data::value::{DataValue, Num, Validity, ValidityTs};
    use crate::runtime::relation::RelationId;
    use crate::storage::{Storage, StoreTx};

    use super::{derived, sovereign};

    fn key(rel: u64, n: i64) -> Vec<u8> {
        vec![DataValue::Num(Num::Int(n))].encode_as_key(RelationId::new(rel).unwrap())
    }

    fn validity_key(rel: u64, n: i64, ts: i64, is_assert: bool) -> Vec<u8> {
        vec![
            DataValue::Num(Num::Int(n)),
            DataValue::Validity(Validity::from((ts, is_assert))),
        ]
        .encode_as_key(RelationId::new(rel).unwrap())
    }

    #[test]
    fn writer_sees_own_writes_reader_does_not_until_commit() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();
        let k = key(1, 1);

        let mut dw = d.transact(true).unwrap();
        let mut sw = s.transact(true).unwrap();
        dw.put(&k, b"v1").unwrap();
        sw.put(&k, b"v1").unwrap();

        // Read-your-own-writes inside the still-open writer.
        assert_eq!(dw.get(&k, false).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(sw.get(&k, false).unwrap(), Some(b"v1".to_vec()));

        // A concurrently-opened reader must not see the uncommitted write.
        // WARNING: the read guards must be dropped before `commit()` — both
        // backends' `Reader` variant holds the `ShardedLock` read guard for
        // its own lifetime, and `commit()` takes the write lock on the same
        // lock; leaving `dr`/`sr` alive across `commit()` self-deadlocks
        // (single thread, non-reentrant lock).
        {
            let dr = d.transact(false).unwrap();
            let sr = s.transact(false).unwrap();
            assert_eq!(dr.get(&k, false).unwrap(), None);
            assert_eq!(sr.get(&k, false).unwrap(), None);
        }

        dw.commit().unwrap();
        sw.commit().unwrap();

        let dr2 = d.transact(false).unwrap();
        let sr2 = s.transact(false).unwrap();
        assert_eq!(dr2.get(&k, false).unwrap(), Some(b"v1".to_vec()));
        assert_eq!(sr2.get(&k, false).unwrap(), Some(b"v1".to_vec()));
    }

    #[test]
    fn put_del_overwrite_exists_agree() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();

        let keys: Vec<Vec<u8>> = (0..16).map(|n| key(2, n)).collect();
        for (i, k) in keys.iter().enumerate() {
            let val = vec![u8::try_from(i).unwrap()];
            dtx.put(k, &val).unwrap();
            stx.put(k, &val).unwrap();
        }
        dtx.commit().unwrap();
        stx.commit().unwrap();

        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();
        for (i, k) in keys.iter().enumerate() {
            if i % 3 == 0 {
                dtx.del(k).unwrap();
                stx.del(k).unwrap();
            } else if i % 5 == 0 {
                let val = vec![0xFF; 2];
                dtx.put(k, &val).unwrap();
                stx.put(k, &val).unwrap();
            }
        }

        for k in &keys {
            assert_eq!(dtx.get(k, false).unwrap(), stx.get(k, false).unwrap());
            assert_eq!(dtx.exists(k, false).unwrap(), stx.exists(k, false).unwrap());
        }

        let lower = key(2, 0);
        let upper = key(2, 16);
        let d_scan: Vec<(Vec<u8>, Vec<u8>)> = dtx
            .range_scan(&lower, &upper)
            .collect::<Result<_, _>>()
            .unwrap();
        let s_scan: Vec<(Vec<u8>, Vec<u8>)> = stx
            .range_scan(&lower, &upper)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(d_scan, s_scan);
        assert_eq!(
            dtx.range_count(&lower, &upper).unwrap(),
            stx.range_count(&lower, &upper).unwrap()
        );

        dtx.commit().unwrap();
        stx.commit().unwrap();
    }

    #[test]
    fn del_range_from_persisted_ignores_same_txn_puts() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();

        // Seed committed rows 0..10.
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();
        for n in 0..10 {
            let k = key(3, n);
            dtx.put(&k, b"x").unwrap();
            stx.put(&k, b"x").unwrap();
        }
        dtx.commit().unwrap();
        stx.commit().unwrap();

        // New transaction: put a fresh row inside the range, then
        // del_range_from_persisted over the whole range. The fresh row is
        // "persisted" from nobody's view but this transaction's own
        // pending delta — it must survive, only the 10 committed rows go.
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();
        let fresh = key(3, 5000);
        dtx.put(&fresh, b"new").unwrap();
        stx.put(&fresh, b"new").unwrap();

        let lower = key(3, 0);
        let upper = key(3, 10_000);
        dtx.del_range_from_persisted(&lower, &upper).unwrap();
        stx.del_range_from_persisted(&lower, &upper).unwrap();

        for n in 0..10 {
            let k = key(3, n);
            assert!(!dtx.exists(&k, false).unwrap());
            assert!(!stx.exists(&k, false).unwrap());
        }
        assert_eq!(dtx.get(&fresh, false).unwrap(), Some(b"new".to_vec()));
        assert_eq!(stx.get(&fresh, false).unwrap(), Some(b"new".to_vec()));
    }

    // NOTE: `storage_kind` and `batch_put` are deliberately not exercised
    // here — `storage::mod`'s `Storage` trait carries an `#[expect(dead_code,
    // reason = "... required by backends even if unused by current
    // callers")]` for exactly these two (wave 3's file, out of this wave's
    // scope — PLAN.md §4 wave 1b touches only `storage::{mem,temp}`).
    // Calling either from here would give that `#[expect]` a real call site
    // and flip it to "unfulfilled" under `-D warnings`, which is a
    // storage/mod.rs concern to resolve when wave 3 lands, not this one's.
    // Both methods are still implemented identically in `derived` and
    // `sovereign` (`storage_kind` returns `"mem"` in each; `batch_put`
    // shares the same MVCC-bypassing write-lock-and-insert shape) — the
    // type system already holds them to one signature via `impl Storage`.

    #[test]
    fn unsupported_ops_agree() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();
        assert!(d.range_compact(b"", b"\xff").is_ok());
        assert!(s.range_compact(b"", b"\xff").is_ok());

        let dtx = d.transact(true).unwrap();
        let stx = s.transact(true).unwrap();
        assert!(dtx.par_put(b"k", b"v").is_err());
        assert!(stx.par_put(b"k", b"v").is_err());
        assert_eq!(dtx.supports_par_put(), stx.supports_par_put());

        let dr = d.transact(false).unwrap();
        let sr = s.transact(false).unwrap();
        assert_eq!(dr.supports_par_put(), sr.supports_par_put());
    }

    #[test]
    fn reader_cannot_write() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();
        let mut dr = d.transact(false).unwrap();
        let mut sr = s.transact(false).unwrap();
        assert!(dr.put(b"k", b"v").is_err());
        assert!(sr.put(b"k", b"v").is_err());
        assert!(dr.del(b"k").is_err());
        assert!(sr.del(b"k").is_err());
        assert!(dr.del_range_from_persisted(b"a", b"z").is_err());
        assert!(sr.del_range_from_persisted(b"a", b"z").is_err());
    }

    #[test]
    fn validity_skip_scan_agrees_across_assert_retract_chain() {
        let d = derived::MemStorage::default();
        let s = sovereign::MemStorage::default();
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();

        // Value is empty on purpose: `extend_tuple_from_v`
        // (`runtime/relation/handles.rs`) treats a non-empty value as a
        // msgpack-encoded `Vec<DataValue>` continuation of the tuple, and
        // this test only needs the key-encoded `(id, Validity)` columns
        // `range_skip_scan_tuple` decodes and compares.
        let no_extra_columns: Vec<u8> = Vec::new();

        // Fact 1: asserted at t=100, retracted at t=200, re-asserted at t=300.
        for (ts, asrt) in [(100, true), (200, false), (300, true)] {
            let k = validity_key(5, 1, ts, asrt);
            dtx.put(&k, &no_extra_columns).unwrap();
            stx.put(&k, &no_extra_columns).unwrap();
        }
        // Fact 2: asserted once at t=150, never retracted, still uncommitted.
        {
            let k = validity_key(5, 2, 150, true);
            dtx.put(&k, &no_extra_columns).unwrap();
            stx.put(&k, &no_extra_columns).unwrap();
        }

        let lower = key(5, 0);
        let upper = key(5, 10);

        for valid_at_ts in [50, 100, 150, 200, 250, 300, 400] {
            let valid_at = ValidityTs(Reverse(valid_at_ts));
            let d_rows: Vec<_> = dtx
                .range_skip_scan_tuple(&lower, &upper, valid_at)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let s_rows: Vec<_> = stx
                .range_skip_scan_tuple(&lower, &upper, valid_at)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                d_rows, s_rows,
                "writer skip-scan disagrees at valid_at={valid_at_ts}"
            );
        }

        dtx.commit().unwrap();
        stx.commit().unwrap();

        let dr = d.transact(false).unwrap();
        let sr = s.transact(false).unwrap();
        for valid_at_ts in [50, 100, 150, 200, 250, 300, 400] {
            let valid_at = ValidityTs(Reverse(valid_at_ts));
            let d_rows: Vec<_> = dr
                .range_skip_scan_tuple(&lower, &upper, valid_at)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let s_rows: Vec<_> = sr
                .range_skip_scan_tuple(&lower, &upper, valid_at)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                d_rows, s_rows,
                "reader skip-scan disagrees at valid_at={valid_at_ts}"
            );
        }
    }
}
