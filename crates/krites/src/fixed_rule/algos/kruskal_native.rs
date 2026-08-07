//! Minimum spanning forest via Kruskal's algorithm.
//!
//! Sorts all edges by ascending weight and greedily accepts each one that
//! connects two components not already joined, tracked with a union-find
//! (disjoint-set) structure using union-by-rank and path halving.
//!
//! Reference: Kruskal, J.B. (1956). "On the Shortest Spanning Subtree of a
//! Graph and the Traveling Salesman Problem." *Proceedings of the American
//! Mathematical Society*, 7(1), 48--50.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::InvalidInputSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Kruskal's minimum spanning forest (undirected, weighted).
///
/// **Complexity:** O(E log E) for the sort, plus near-linear union-find
/// operations.
///
/// **When to use:** Minimum-cost spanning tree/forest on sparse graphs.
pub(crate) struct MinimumSpanningForestKruskal;

#[expect(
    clippy::indexing_slicing,
    reason = "edge tuple has at least 2 elements by construction from the weighted edge scan"
)]
impl FixedRule for MinimumSpanningForestKruskal {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;

        let mut weighted_edges: Vec<(f64, DataValue, DataValue)> = vec![];
        let mut forest = DisjointSet::new();
        for row in edges.iter()? {
            let row = row?;
            let source = row[0].clone();
            let target = row[1].clone();
            let weight = match row.get(2) {
                None => 1.0,
                Some(w) => {
                    let f = w.get_float().ok_or_else(|| {
                        InvalidInputSnafu {
                            rule: "MinimumSpanningForestKruskal",
                            message: format!("edge weight {w:?} is not a number"),
                        }
                        .build()
                    })?;
                    if !f.is_finite() {
                        return Err(InvalidInputSnafu {
                            rule: "MinimumSpanningForestKruskal",
                            message: format!("edge weight {w:?} must be finite"),
                        }
                        .build()
                        .into());
                    }
                    f
                }
            };
            forest.touch(&source);
            forest.touch(&target);
            weighted_edges.push((weight, source, target));
            poison.check()?;
        }

        weighted_edges.sort_by(|a, b| a.0.total_cmp(&b.0));

        for (weight, source, target) in weighted_edges {
            if forest.union(&source, &target) {
                out.put(vec![source, target, DataValue::from(weight)]);
            }
            poison.check()?;
        }

        Ok(())
    }

    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(3)
    }
}

/// Disjoint-set over `DataValue` keys, union by rank with path halving.
struct DisjointSet {
    parent: BTreeMap<DataValue, DataValue>,
    rank: BTreeMap<DataValue, u32>,
}

impl DisjointSet {
    fn new() -> Self {
        Self {
            parent: BTreeMap::new(),
            rank: BTreeMap::new(),
        }
    }

    /// Register `node` as its own singleton set if not already present.
    fn touch(&mut self, node: &DataValue) {
        self.parent
            .entry(node.clone())
            .or_insert_with(|| node.clone());
        self.rank.entry(node.clone()).or_insert(0);
    }

    fn find(&mut self, node: &DataValue) -> DataValue {
        let mut cursor = node.clone();
        loop {
            let Some(next) = self.parent.get(&cursor).cloned() else {
                return cursor;
            };
            if next == cursor {
                return cursor;
            }
            // Path halving: point each visited node at its grandparent.
            let grandparent = self.parent.get(&next).cloned().unwrap_or(next.clone());
            self.parent.insert(cursor.clone(), grandparent.clone());
            cursor = grandparent;
        }
    }

    /// Union the sets containing `a` and `b`. Returns `true` when they were
    /// previously in different sets (i.e. the edge does not close a cycle).
    fn union(&mut self, a: &DataValue, b: &DataValue) -> bool {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a == root_b {
            return false;
        }
        let rank_a = self.rank.get(&root_a).copied().unwrap_or(0);
        let rank_b = self.rank.get(&root_b).copied().unwrap_or(0);
        match rank_a.cmp(&rank_b) {
            std::cmp::Ordering::Less => {
                self.parent.insert(root_a, root_b);
            }
            std::cmp::Ordering::Greater => {
                self.parent.insert(root_b, root_a);
            }
            std::cmp::Ordering::Equal => {
                self.parent.insert(root_b, root_a.clone());
                self.rank.insert(root_a, rank_a + 1);
            }
        }
        true
    }
}
