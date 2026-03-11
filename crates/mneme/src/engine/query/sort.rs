//! Sort operators for query output.
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::engine::error::InternalResult as Result;
use itertools::Itertools;

use crate::engine::data::program::SortDir;
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::Tuple;
use crate::engine::runtime::temp_store::EpochStore;
use crate::engine::runtime::transact::SessionTx;

impl<'a> SessionTx<'a> {
    pub(crate) fn sort_and_collect(
        &mut self,
        original: EpochStore,
        sorters: &[(Symbol, SortDir)],
        head: &[Symbol],
    ) -> Result<Vec<Tuple>> {
        let head_indices: BTreeMap<_, _> = head.iter().enumerate().map(|(i, k)| (k, i)).collect();
        let idx_sorters = sorters
            .iter()
            .map(|(k, dir)| (head_indices[k], *dir))
            .collect_vec();

        let mut all_data: Vec<_> = original.all_iter().map(|v| v.into_tuple()).collect_vec();
        all_data.sort_by(|a, b| {
            for (idx, dir) in &idx_sorters {
                match a[*idx].cmp(&b[*idx]) {
                    Ordering::Equal => {}
                    o => {
                        return match dir {
                            SortDir::Asc => o,
                            SortDir::Dsc => o.reverse(),
                        };
                    }
                }
            }
            Ordering::Equal
        });

        Ok(all_data)
    }
}
