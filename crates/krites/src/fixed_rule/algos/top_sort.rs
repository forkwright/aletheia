//! Topological ordering of a directed acyclic graph via Kahn's algorithm.
//!
//! Nodes that participate in a cycle are omitted from the output — Kahn's
//! algorithm only ever dequeues nodes whose in-degree has reached zero, so
//! a cyclic residue never drains.
//!
//! Reference: Kahn, A.B. (1962). "Topological Sorting of Large Networks."
//! *Communications of the ACM*, 5(11), 558--562.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, VecDeque};

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Topological sort (Kahn's algorithm).
///
/// **Complexity:** O(V + E).
///
/// **When to use:** Dependency-ordered scheduling, or cycle detection (a
/// result shorter than the node count means a cycle exists).
pub(crate) struct TopSort;

impl FixedRule for TopSort {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;

        let mut out_edges: BTreeMap<DataValue, Vec<DataValue>> = BTreeMap::new();
        let mut in_degree: BTreeMap<DataValue, i64> = BTreeMap::new();

        for row in edges.iter()? {
            let mut fields = row?.into_iter();
            let Some(source) = fields.next() else {
                continue;
            };
            let Some(target) = fields.next() else {
                continue;
            };
            in_degree.entry(source.clone()).or_insert(0);
            *in_degree.entry(target.clone()).or_insert(0) += 1;
            out_edges.entry(source).or_default().push(target);
            poison.check()?;
        }

        let mut ready: VecDeque<DataValue> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(node, _)| node.clone())
            .collect();

        let mut position: i64 = 0;
        while let Some(node) = ready.pop_front() {
            out.put(vec![DataValue::from(position), node.clone()]);
            position += 1;

            if let Some(successors) = out_edges.get(&node) {
                for successor in successors {
                    if let Some(deg) = in_degree.get_mut(successor) {
                        *deg -= 1;
                        if *deg == 0 {
                            ready.push_back(successor.clone());
                        }
                    }
                }
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
        Ok(2)
    }
}
