//! Degree centrality computation.
//!
//! Counts in-degree, out-degree, and total degree for every node in a
//! directed edge relation.  Optionally includes isolated nodes from a
//! secondary node relation.
//!
//! Reference: Freeman, L.C. (1978). "Centrality in Social Networks:
//! Conceptual Clarification." *Social Networks*, 1(3), 215--239.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Degree centrality: total, out, and in degree per node.
///
/// **Complexity:** O(E) where E is edges.  Single pass counting.
///
/// **When to use:** Quick identification of hubs and authorities in directed
/// networks, or as a baseline centrality measure before computing more
/// expensive metrics.
pub(crate) struct DegreeCentrality;

#[expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap key"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "graph degree counter indices are bounds-checked by the node map"
)]
#[expect(
    clippy::explicit_into_iter_loop,
    reason = "explicit .into_iter() clarifies ownership transfer of collected results"
)]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "usize-to-isize degree counts are small positive values well within isize range"
)]
impl FixedRule for DegreeCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edge_iter = payload.get_input(0)?.ensure_min_len(2)?.iter()?;
        let mut counter: BTreeMap<DataValue, (usize, usize, usize)> = BTreeMap::new();
        for tuple in edge_iter {
            let tuple = tuple?;
            let from = tuple[0].clone();
            let (from_total, from_out, _) = counter.entry(from).or_default();
            *from_total += 1;
            *from_out += 1;

            let to = tuple[1].clone();
            let (to_total, _, to_in) = counter.entry(to).or_default();
            *to_total += 1;
            *to_in += 1;
            poison.check()?;
        }
        if let Ok(nodes) = payload.get_input(1) {
            for tuple in nodes.iter()? {
                let tuple = tuple?;
                let id = &tuple[0];
                if !counter.contains_key(id) {
                    counter.insert(id.clone(), (0, 0, 0));
                }
                poison.check()?;
            }
        }
        for (node, (total_degree, out_degree, in_degree)) in counter.into_iter() {
            let tuple = vec![
                node,
                DataValue::from(total_degree as i64),
                DataValue::from(out_degree as i64),
                DataValue::from(in_degree as i64),
            ];
            out.put(tuple);
        }
        Ok(())
    }

    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(4)
    }
}
