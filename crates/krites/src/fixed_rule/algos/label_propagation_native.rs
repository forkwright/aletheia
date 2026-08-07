//! Label propagation community detection.
//!
//! Every node starts labelled with its own identity. Each round, nodes are
//! visited in random order and each adopts the label with the highest
//! total incoming edge weight among its out-neighbours, breaking ties
//! randomly. Propagation stops once a full round produces no changes, or
//! `max_iter` rounds have run.
//!
//! Reference: Raghavan, U.N., Albert, R., Kumara, S. (2007). "Near Linear
//! Time Algorithm to Detect Community Structures in Large-Scale Networks."
//! *Physical Review E*, 76(3), 036106.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::BTreeMap;

use compact_str::CompactString;
use rand::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::InvalidInputSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Label propagation community detection.
///
/// **Complexity:** O(I * (V + E)), typically converging in a handful of
/// rounds.
///
/// **When to use:** Fast, parameter-free community detection. Less stable
/// than Louvain but scales further.
pub(crate) struct LabelPropagation;

#[expect(
    clippy::indexing_slicing,
    reason = "edge tuple has at least 2 elements by construction from the weighted edge scan"
)]
#[expect(
    clippy::float_cmp,
    reason = "best-score ties are detected via exact equality against a value drawn from the same tally map"
)]
impl FixedRule for LabelPropagation {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let max_iterations = payload.pos_integer_option("max_iter", Some(10))?;

        let mut adjacency: BTreeMap<DataValue, Vec<(DataValue, f64)>> = BTreeMap::new();
        let mut nodes: Vec<DataValue> = vec![];
        let mut seen = std::collections::BTreeSet::new();
        for row in edges.iter()? {
            let row = row?;
            let source = row[0].clone();
            let target = row[1].clone();
            let weight = match row.get(2) {
                None => 1.0,
                Some(w) => w.get_float().ok_or_else(|| {
                    InvalidInputSnafu {
                        rule: "LabelPropagation",
                        message: format!("edge weight {w:?} is not a number"),
                    }
                    .build()
                })?,
            };
            if !weight.is_finite() {
                return Err(InvalidInputSnafu {
                    rule: "LabelPropagation",
                    message: format!("edge weight {weight:?} must be finite"),
                }
                .build()
                .into());
            }
            for node in [&source, &target] {
                if seen.insert(node.clone()) {
                    nodes.push(node.clone());
                }
            }
            adjacency
                .entry(source.clone())
                .or_default()
                .push((target.clone(), weight));
            if undirected {
                adjacency.entry(target).or_default().push((source, weight));
            } else {
                adjacency.entry(target).or_default();
            }
            poison.check()?;
        }

        let mut label: BTreeMap<DataValue, DataValue> =
            nodes.iter().map(|n| (n.clone(), n.clone())).collect();
        let mut rng = rand::rng();
        let mut order = nodes.clone();

        for _ in 0..max_iterations {
            order.shuffle(&mut rng);
            let mut changed = false;
            for node in &order {
                let mut tally: BTreeMap<DataValue, f64> = BTreeMap::new();
                for (neighbor, weight) in adjacency.get(node).into_iter().flatten() {
                    let neighbor_label = label
                        .get(neighbor)
                        .cloned()
                        .unwrap_or_else(|| neighbor.clone());
                    *tally.entry(neighbor_label).or_insert(0.0) += weight;
                }
                if tally.is_empty() {
                    continue;
                }
                let best_score = tally.values().copied().fold(f64::NEG_INFINITY, f64::max);
                let contenders: Vec<&DataValue> = tally
                    .iter()
                    .filter(|&(_, &score)| score == best_score)
                    .map(|(candidate_label, _)| candidate_label)
                    .collect();
                let Some(&chosen) = contenders.choose(&mut rng) else {
                    continue;
                };
                if label.get(node) != Some(chosen) {
                    label.insert(node.clone(), chosen.clone());
                    changed = true;
                }
                poison.check()?;
            }
            if !changed {
                break;
            }
        }

        for node in nodes {
            let node_label = label.get(&node).cloned().unwrap_or_else(|| node.clone());
            out.put(vec![node_label, node]);
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
