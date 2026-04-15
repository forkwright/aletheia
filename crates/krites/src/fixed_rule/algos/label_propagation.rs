//! Label propagation community detection.
//!
//! Each node starts with its own unique label.  In each iteration, every node
//! adopts the label most common (by edge weight) among its neighbours.  Ties
//! are broken randomly.  The algorithm converges when no node changes its
//! label.
//!
//! Reference: Raghavan, U.N., Albert, R., Kumara, S. (2007). "Near Linear
//! Time Algorithm to Detect Community Structures in Large-Scale Networks."
//! *Physical Review E*, 76(3), 036106.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use rand::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Label propagation community detection.
///
/// **Complexity:** O(I * (V + E)) where I is iterations, V is vertices,
/// E is edges.  Typically converges in 5--10 iterations.
///
/// **When to use:** Fast, parameter-free community detection on large
/// graphs.  Less stable than Louvain but faster on very large networks.
pub(crate) struct LabelPropagation;

#[expect(
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::indexing_slicing,
    reason = "graph label propagation indices are bounds-checked by the CSR adjacency structure and node count"
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
        let (graph, indices, _inv_indices) = edges.as_directed_weighted_graph(undirected, true)?;
        let labels = label_propagation(&graph, max_iterations, poison)?;
        for (idx, label) in labels.into_iter().enumerate() {
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

/// Core label propagation loop.
///
/// **Complexity:** O(I * (V + E)) where I is iterations until convergence.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph label propagation indices are bounds-checked by the CSR node count and label arrays"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
#[expect(
    clippy::float_cmp,
    reason = "label propagation ties detected via exact f32 equality — scores computed by same accumulation path"
)]
fn label_propagation(
    graph: &DirectedCsrGraph<f32>,
    max_iterations: usize,
    poison: Poison,
) -> Result<Vec<u32>> {
    let node_count = graph.node_count();
    let mut labels = (0..node_count).collect_vec();
    let mut rng = rand::rng();
    let mut iteration_order = (0..node_count).collect_vec();
    for _ in 0..max_iterations {
        iteration_order.shuffle(&mut rng);
        let mut changed = false;
        for node in &iteration_order {
            let mut label_scores: BTreeMap<u32, f32> = BTreeMap::new();
            for edge in graph.out_neighbors_with_values(*node) {
                let neighbor_label = labels[edge.target as usize];
                *label_scores.entry(neighbor_label).or_default() += edge.value;
            }
            if label_scores.is_empty() {
                continue;
            }
            let mut labels_by_score = label_scores.into_iter().collect_vec();
            labels_by_score.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());
            // SAFETY: `labels_by_score` is non-empty due to the `is_empty()` check above.
            let max_score = labels_by_score[0].1;
            let candidate_labels = labels_by_score
                .into_iter()
                .take_while(|(_, score)| *score == max_score)
                .map(|(label, _)| label)
                .collect_vec();
            let new_label = candidate_labels
                .choose(&mut rng)
                .ok_or_else(|| {
                    GraphAlgorithmSnafu {
                        algorithm: "label_propagation",
                        message: "candidate label set is unexpectedly empty",
                    }
                    .build()
                })?;
            if *new_label != labels[*node as usize] {
                changed = true;
                labels[*node as usize] = *new_label;
            }
            poison.check()?;
        }
        if !changed {
            break;
        }
    }
    Ok(labels)
}
