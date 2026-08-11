//! In-memory storage backend.
//!
//! The implementation here replaced a CozoDB-derived one. A differential
//! conformance suite ran the same operation sequences against both and asserted
//! identical observable state, including MVCC isolation, on every CI run up to
//! the commit that retired the derived copy — that suite is why the replacement
//! could be trusted, and it went with the second implementation it compared.
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

pub use sovereign::{MemStorage, new_mem_db};
