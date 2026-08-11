//! Temporary (spill) storage backend.
//!
//! The implementation here replaced a CozoDB-derived one, proved equivalent by
//! a differential conformance suite that ran both against identical operation
//! sequences until the derived copy was retired.
mod sovereign {
    //! Fresh implementation of the temp-storage contract. Session-scoped
    //! scratch storage has no snapshot/delta split to reinvent: every
    //! `transact()` call hands back an independent, empty backing map and
    //! nothing persists across transactions (`runtime/db.rs`'s `temp_db`
    //! field lives for the `Db`'s lifetime, but each transaction's rows are
    //! its own — see `runtime/temp_store.rs`'s per-stratum epoch stores for
    //! the actual scratch-space consumer).

    use std::collections::BTreeMap;
    use std::ops::Bound;

    use crate::data::tuple::{Tuple, check_key_for_validity};
    use crate::data::value::ValidityTs;
    use crate::error::InternalResult;
    use crate::runtime::relation::{decode_tuple_from_kv, extend_tuple_from_v};
    use crate::storage::error::{StorageResult, TransactionFailedSnafu};
    use crate::storage::{Storage, StoreTx};

    type Result<T> = StorageResult<T>;

    #[derive(Default, Clone)]
    pub(crate) struct TempStorage;

    impl<'s> Storage<'s> for TempStorage {
        type Tx = TempTx;

        fn storage_kind(&self) -> &'static str {
            "temp"
        }

        fn transact(&'s self, _write: bool) -> Result<Self::Tx> {
            Ok(TempTx {
                rows: BTreeMap::new(),
            })
        }

        fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            Err(TransactionFailedSnafu {
                backend: "temp",
                message: "range_compact is not supported on temp storage",
            }
            .build())
        }

        fn batch_put<'a>(
            &'a self,
            _data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
        ) -> Result<()> {
            Err(TransactionFailedSnafu {
                backend: "temp",
                message: "batch_put is not supported on temp storage",
            }
            .build())
        }
    }

    pub(crate) struct TempTx {
        rows: BTreeMap<Vec<u8>, Vec<u8>>,
    }

    impl<'s> StoreTx<'s> for TempTx {
        fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
            Ok(self.rows.get(key).cloned())
        }

        fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
            self.rows.insert(key.to_vec(), val.to_vec());
            Ok(())
        }

        fn supports_par_put(&self) -> bool {
            false
        }

        fn del(&mut self, key: &[u8]) -> Result<()> {
            self.rows.remove(key);
            Ok(())
        }

        fn del_range_from_persisted(&mut self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            // NOTE: there is no separate "persisted" view to delete from —
            // `rows` IS the transaction's whole world — so this is a no-op,
            // matching `derived`'s behavior exactly (see `derived::TempTx`
            // above; upstream's real persisted-vs-delta distinction only
            // applies to `storage::mem`).
            Ok(())
        }

        fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
            Ok(self.rows.contains_key(key))
        }

        fn commit(&mut self) -> Result<()> {
            Ok(())
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
            Box::new(
                self.rows
                    .range(lower.to_vec()..upper.to_vec())
                    .map(|(k, v)| Ok(decode_tuple_from_kv(k, v, None))),
            )
        }

        fn range_skip_scan_tuple<'a>(
            &'a self,
            lower: &[u8],
            upper: &[u8],
            valid_at: ValidityTs,
        ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> {
            Box::new(
                ValiditySkipIter {
                    rows: &self.rows,
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
            Box::new(
                self.rows
                    .range(lower.to_vec()..upper.to_vec())
                    .map(|(k, v)| Ok((k.clone(), v.clone()))),
            )
        }

        fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
        where
            's: 'a,
        {
            Ok(self.rows.range(lower.to_vec()..upper.to_vec()).count())
        }
    }

    /// Validity-aware skip-scan over a single `BTreeMap`. Re-seeks from
    /// `next_bound` on every `next()` because `BTreeMap` has no seekable
    /// cursor (rust-lang/rust#49638) — the same constraint `storage::mem`
    /// works around, solved independently here (see the module doc above:
    /// this file carries no dependency on `storage::mem`, in either its
    /// `derived` or `sovereign` form).
    ///
    /// `size_hint` is latent capability carried from the `Storage` contract
    /// (`storage::mem`'s skip-scan takes the same parameter) — every
    /// current call site passes `None`, same as `derived`'s.
    struct ValiditySkipIter<'a> {
        rows: &'a BTreeMap<Vec<u8>, Vec<u8>>,
        upper: Vec<u8>,
        valid_at: ValidityTs,
        next_bound: Vec<u8>,
        size_hint: Option<usize>,
    }

    impl Iterator for ValiditySkipIter<'_> {
        type Item = Tuple;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                let (key, val) = self
                    .rows
                    .range::<Vec<u8>, (Bound<&Vec<u8>>, Bound<&Vec<u8>>)>((
                        Bound::Included(&self.next_bound),
                        Bound::Excluded(&self.upper),
                    ))
                    .next()?;
                let (visible, next_seek) =
                    check_key_for_validity(key, self.valid_at, self.size_hint);
                self.next_bound = next_seek;
                if let Some(mut tuple) = visible {
                    extend_tuple_from_v(&mut tuple, val);
                    return Some(tuple);
                }
            }
        }
    }
}

pub(crate) use sovereign::{TempStorage, TempTx};
