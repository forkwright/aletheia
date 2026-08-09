//! Temporary (session-scoped) storage backend.
//!
//! Land-dark dual state (PLAN.md §2, krites retirement): `derived` carries
//! the CozoDB-derived implementation forward unchanged and stays the
//! crate-visible default. `sovereign` is written fresh against the same
//! `Storage`/`StoreTx` contract (`storage/mod.rs`) and is selected instead
//! when the crate is built with `--cfg krites_sovereign_storage_temp`.
//! Both submodules compile unconditionally — the cfg only decides which one
//! is re-exported as `TempStorage`/`TempTx` — so `#[cfg(test)] mod
//! conformance` below can run identical operation sequences against both
//! and assert identical observable output regardless of which is active.
//!
//! Wave ordering (PLAN.md §4 wave 1b) puts this file first: `storage::mem`
//! is the mem-vs-fjall differential oracle used elsewhere in the crate and
//! must stay untouched while this file's sovereign half is proven, so only
//! `derived::TempStorage`'s own (unmodified) dependency on
//! `storage::mem::derived::SkipIterator` is kept — `sovereign` below has no
//! dependency on `storage::mem` at all, in either its derived or sovereign
//! form, so this file's land-dark lifecycle never blocks on that one.

mod derived {
    use std::collections::BTreeMap;
    use std::default::Default;

    use crate::data::tuple::Tuple;
    use crate::data::value::ValidityTs;
    use crate::error::InternalResult;
    use crate::runtime::relation::decode_tuple_from_kv;
    use crate::storage::error::StorageResult;
    use crate::storage::mem::derived::SkipIterator;
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
                store: BTreeMap::default(),
            })
        }

        fn range_compact(&'s self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            Err(crate::storage::error::TransactionFailedSnafu {
                backend: "temp",
                message: "range_compact is not supported on temp storage",
            }
            .build())
        }

        fn batch_put<'a>(
            &'a self,
            _data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
        ) -> Result<()> {
            Err(crate::storage::error::TransactionFailedSnafu {
                backend: "temp",
                message: "batch_put is not supported on temp storage",
            }
            .build())
        }
    }

    pub(crate) struct TempTx {
        store: BTreeMap<Vec<u8>, Vec<u8>>,
    }

    impl<'s> StoreTx<'s> for TempTx {
        fn get(&self, key: &[u8], _for_update: bool) -> Result<Option<Vec<u8>>> {
            Ok(self.store.get(key).cloned())
        }

        fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()> {
            self.store.insert(key.to_vec(), val.to_vec());
            Ok(())
        }

        fn supports_par_put(&self) -> bool {
            false
        }

        fn del(&mut self, key: &[u8]) -> Result<()> {
            self.store.remove(key);
            Ok(())
        }

        fn del_range_from_persisted(&mut self, _lower: &[u8], _upper: &[u8]) -> Result<()> {
            Ok(())
        }

        fn exists(&self, key: &[u8], _for_update: bool) -> Result<bool> {
            Ok(self.store.contains_key(key))
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
                self.store
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
                SkipIterator {
                    inner: &self.store,
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
                self.store
                    .range(lower.to_vec()..upper.to_vec())
                    .map(|(k, v)| Ok((k.clone(), v.clone()))),
            )
        }

        fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> Result<usize>
        where
            's: 'a,
        {
            Ok(self.store.range(lower.to_vec()..upper.to_vec()).count())
        }
    }
}

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

#[cfg(not(krites_sovereign_storage_temp))]
pub(crate) use derived::{TempStorage, TempTx};

#[cfg(krites_sovereign_storage_temp)]
pub(crate) use sovereign::{TempStorage, TempTx};

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod conformance {
    //! Differential proof (PLAN.md §4 wave 1b): the same operation sequence
    //! applied to `derived::TempStorage` and `sovereign::TempStorage` must
    //! produce identical observable state. Runs unconditionally — both
    //! submodules always compile regardless of which cfg is active.
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
    fn put_get_del_exists_agree() {
        let d = derived::TempStorage;
        let s = sovereign::TempStorage;
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();

        let keys: Vec<Vec<u8>> = (0..12).map(|n| key(1, n)).collect();

        for (i, k) in keys.iter().enumerate() {
            let val = vec![u8::try_from(i).unwrap(); (i % 4) + 1];
            dtx.put(k, &val).unwrap();
            stx.put(k, &val).unwrap();
        }
        // Delete every third key, overwrite every fourth.
        for (i, k) in keys.iter().enumerate() {
            if i % 3 == 0 {
                dtx.del(k).unwrap();
                stx.del(k).unwrap();
            } else if i % 4 == 0 {
                let val = vec![0xAA; 3];
                dtx.put(k, &val).unwrap();
                stx.put(k, &val).unwrap();
            }
        }

        for k in &keys {
            assert_eq!(
                dtx.get(k, false).unwrap(),
                stx.get(k, false).unwrap(),
                "get() disagrees for key {k:?}"
            );
            assert_eq!(
                dtx.exists(k, false).unwrap(),
                stx.exists(k, false).unwrap(),
                "exists() disagrees for key {k:?}"
            );
        }

        dtx.commit().unwrap();
        stx.commit().unwrap();
    }

    #[test]
    fn range_scan_and_count_agree() {
        let d = derived::TempStorage;
        let s = sovereign::TempStorage;
        let mut dtx = d.transact(true).unwrap();
        let mut stx = s.transact(true).unwrap();

        for n in 0..30 {
            let k = key(2, n);
            let v = vec![u8::try_from(n).unwrap()];
            dtx.put(&k, &v).unwrap();
            stx.put(&k, &v).unwrap();
        }

        let lower = key(2, 5);
        let upper = key(2, 20);

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
        assert_eq!(dtx.range_count(&lower, &upper).unwrap(), 15);
    }

    #[test]
    fn validity_skip_scan_agrees_across_assert_retract_chain() {
        let d = derived::TempStorage;
        let s = sovereign::TempStorage;
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
            let k = validity_key(3, 1, ts, asrt);
            dtx.put(&k, &no_extra_columns).unwrap();
            stx.put(&k, &no_extra_columns).unwrap();
        }
        // Fact 2: asserted once at t=150, never retracted.
        {
            let k = validity_key(3, 2, 150, true);
            dtx.put(&k, &no_extra_columns).unwrap();
            stx.put(&k, &no_extra_columns).unwrap();
        }

        let lower = key(3, 0);
        let upper = key(3, 10);

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
                "skip-scan disagrees at valid_at={valid_at_ts}"
            );
        }
    }
}
