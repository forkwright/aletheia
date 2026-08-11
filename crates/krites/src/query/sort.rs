//! Sort operators for query output.
#![expect(
    clippy::indexing_slicing,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    reason = "engine-internal sort — indexing validated by head_indices lookup"
)]

use std::cmp::Ordering;
use std::collections::BTreeMap;

use itertools::Itertools;

use crate::data::program::SortDir;
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::runtime::temp_store::EpochStore;

/// Sort a result store by the query's `:sort` keys.
///
/// WHY a free function: this never read `self`. It was an inherent method on
/// `SessionTx`, which made `query/` extend a runtime type for a computation that
/// touches no transaction state -- and the file suppressed `clippy::unused_self`
/// to keep it that way. Sorting collected tuples needs a comparator and nothing
/// else.
pub(crate) fn sort_and_collect(
    original: EpochStore,
    sorters: &[(Symbol, SortDir)],
    head: &[Symbol],
) -> Vec<Tuple> {
    let head_indices: BTreeMap<_, _> = head.iter().enumerate().map(|(i, k)| (k, i)).collect();
    let idx_sorters = sorters
        .iter()
        .map(|(k, dir)| (head_indices[k], *dir))
        .collect_vec();

    let mut all_data: Vec<_> = original.all_iter().map(|v| v.into_tuple()).collect_vec();
    all_data.sort_by(|a, b| {
        for (idx, dir) in &idx_sorters {
            match a[*idx].cmp(&b[*idx]) {
                // NOTE: equal on this key, continue to next sort key
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

    all_data
}
