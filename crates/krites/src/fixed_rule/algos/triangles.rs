//! Local clustering coefficients and per-node triangle counts.
//!
//! The edge relation is treated as undirected. For each node, counts how
//! many pairs of its neighbours are themselves connected (a triangle
//! through that node), then normalises by the number of possible pairs.
//!
//! Reference: Watts, D.J., Strogatz, S.H. (1998). "Collective Dynamics of
//! 'Small-World' Networks." *Nature*, 393(6684), 440--442.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Local clustering coefficients and triangle counts (undirected).
///
/// **Complexity:** O(V * d^2) where d is average degree.
///
/// **When to use:** Measuring local density / transitivity, or finding
/// tightly-knit neighbourhoods.
pub(crate) struct ClusteringCoefficients;

#[expect(
    clippy::indexing_slicing,
    reason = "i and j both range over 0..neighbor_list.len() by construction"
)]
impl FixedRule for ClusteringCoefficients {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;

        let mut adjacency: BTreeMap<DataValue, BTreeSet<DataValue>> = BTreeMap::new();
        for row in edges.iter()? {
            let mut fields = row?.into_iter();
            let Some(a) = fields.next() else { continue };
            let Some(b) = fields.next() else { continue };
            if a == b {
                adjacency.entry(a).or_default();
            } else {
                adjacency.entry(a.clone()).or_default().insert(b.clone());
                adjacency.entry(b).or_default().insert(a);
            }
            poison.check()?;
        }

        for (node, neighbors) in &adjacency {
            let degree = neighbors.len();
            let neighbor_list: Vec<&DataValue> = neighbors.iter().collect();
            let mut triangle_count = 0usize;
            for i in 0..neighbor_list.len() {
                for j in (i + 1)..neighbor_list.len() {
                    if adjacency
                        .get(neighbor_list[i])
                        .is_some_and(|set| set.contains(neighbor_list[j]))
                    {
                        triangle_count += 1;
                    }
                }
                poison.check()?;
            }
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "triangle/degree counts fit comfortably in f64 mantissa for graph sizes this crate handles"
            )]
            let coefficient = if degree < 2 {
                0.0
            } else {
                2.0 * triangle_count as f64 / (degree as f64 * (degree as f64 - 1.0))
            };
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_wrap,
                reason = "triangle/degree counts fit comfortably in i64 for graph sizes this crate handles"
            )]
            out.put(vec![
                node.clone(),
                DataValue::from(coefficient),
                DataValue::from(triangle_count as i64),
                DataValue::from(degree as i64),
            ]);
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
