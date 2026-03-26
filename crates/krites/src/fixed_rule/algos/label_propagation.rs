//! Label propagation community detection.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use rand::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) struct LabelPropagation;

impl FixedRule for LabelPropagation {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let max_iter = payload.pos_integer_option("max_iter", Some(10))?;
        let (graph, indices, _inv_indices) = edges.as_directed_weighted_graph(undirected, true)?;
        let labels = label_propagation(&graph, max_iter, poison)?;
        for (idx, label) in labels.into_iter().enumerate() {
            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let node = indices[idx].clone();
            out.put(vec![DataValue::from(label as i64), node]);
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

fn label_propagation(
    graph: &DirectedCsrGraph<f32>,
    max_iter: usize,
    poison: Poison,
) -> Result<Vec<u32>> {
    let n_nodes = graph.node_count();
    let mut labels = (0..n_nodes).collect_vec();
    let mut rng = rand::rng();
    let mut iter_order = (0..n_nodes).collect_vec();
    for _ in 0..max_iter {
        iter_order.shuffle(&mut rng);
        let mut changed = false;
        for node in &iter_order {
            let mut labels_for_node: BTreeMap<u32, f32> = BTreeMap::new();
            for edge in graph.out_neighbors_with_values(*node) {
                let label = labels[edge.target as usize];
                *labels_for_node.entry(label).or_default() += edge.value;
            }
            if labels_for_node.is_empty() {
                continue;
            }
            let mut labels_by_score = labels_for_node.into_iter().collect_vec();
            labels_by_score.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());
            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let max_score = labels_by_score[0].1;
            let candidate_labels = labels_by_score
                .into_iter()
                .take_while(|(_, score)| *score == max_score)
                .map(|(l, _)| l)
                .collect_vec();
            let new_label = *candidate_labels
                .choose(&mut rng)
                .unwrap_or_else(|| unreachable!());
            if new_label != labels[*node as usize] {
                changed = true;
                labels[*node as usize] = new_label;
            }
            poison.check()?;
        }
        if !changed {
            break;
        }
    }
    Ok(labels)
}
