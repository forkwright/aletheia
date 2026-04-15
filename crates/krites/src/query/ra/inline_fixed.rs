//! Inline fixed (constant) relational algebra source.
//!
//! Represents literal data embedded in the query plan, such as `data[a, b] <- [[1, 2], [3, 4]]`.
//! This is the base case for query evaluation: no storage lookup, just constant tuples.
#![expect(
    clippy::indexing_slicing,
    clippy::mutable_key_type,
    clippy::result_large_err,
    reason = "engine-internal inline fixed RA -- indexing validated by join_indices, mutable keys are Symbol"
)]

use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;

use super::eliminate_from_tuple;
use crate::data::symb::Symbol;
use crate::data::tuple::TupleIter;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;

/// Inline constant data source.
///
/// Holds literal tuples injected at query compile time.
/// Used for singleton (unit) relations, small lookup tables,
/// and as the seed relation for joins.
#[derive(Debug)]
pub(crate) struct InlineFixedRA {
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) data: Vec<Vec<DataValue>>,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl InlineFixedRA {
    /// Create a unit relation (single empty tuple, no bindings).
    ///
    /// This is the identity element for joins: `unit ⋈ R = R`.
    pub(crate) fn unit(span: SourceSpan) -> Self {
        Self {
            bindings: vec![],
            data: vec![vec![]],
            to_eliminate: Default::default(),
            span,
        }
    }

    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in &self.bindings {
            if !used.contains(binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        Ok(())
    }
}

impl InlineFixedRA {
    /// Describe the join strategy for this source.
    pub(crate) fn join_type(&self) -> &str {
        if self.data.is_empty() {
            "null_join"
        } else if self.data.len() == 1 {
            "singleton_join"
        } else {
            "fixed_join"
        }
    }

    /// Join left tuples against this inline data.
    ///
    /// # Complexity
    ///
    /// - Empty data: O(0) (short-circuits to empty iterator).
    /// - Singleton: O(L) where L is left tuples (linear scan with equality check).
    /// - Multi-row: O(L * log F) where F is fixed rows (B-tree lookup per left tuple).
    pub(crate) fn join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
    ) -> Result<TupleIter<'a>> {
        use std::iter;
        Ok(if self.data.is_empty() {
            Box::new(iter::empty())
        } else if self.data.len() == 1 {
            // SAFETY: `self.data.len() == 1` check above ensures index 0 is valid.
            let data = self.data[0].clone();
            let right_join_values = right_join_indices
                .into_iter()
                .map(|v| data[v].clone())
                .collect_vec();
            Box::new(left_iter.filter_map_ok(move |tuple| {
                let left_join_values = left_join_indices.iter().map(|v| &tuple[*v]).collect_vec();
                if left_join_values.into_iter().eq(right_join_values.iter()) {
                    let mut ret = tuple;
                    ret.extend_from_slice(&data);
                    let ret = eliminate_from_tuple(ret, &eliminate_indices);
                    Some(ret)
                } else {
                    None
                }
            }))
        } else {
            let mut right_mapping = BTreeMap::new();
            for data in &self.data {
                let right_join_values = right_join_indices.iter().map(|v| &data[*v]).collect_vec();
                match right_mapping.get_mut(&right_join_values) {
                    None => {
                        right_mapping.insert(right_join_values, vec![data]);
                    }
                    Some(coll) => {
                        coll.push(data);
                    }
                }
            }
            Box::new(
                left_iter
                    .filter_map_ok(move |tuple| {
                        let left_join_values =
                            left_join_indices.iter().map(|v| &tuple[*v]).collect_vec();
                        right_mapping.get(&left_join_values).map(|v| {
                            v.iter()
                                .map(|right_values| {
                                    let mut left_data = tuple.clone();
                                    left_data.extend_from_slice(right_values);
                                    left_data
                                })
                                .collect_vec()
                        })
                    })
                    .flatten_ok(),
            )
        })
    }
}
