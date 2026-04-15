//! Triangle counting and local clustering coefficients.
//!
//! For each node, counts the number of triangles it participates in and
//! computes the local clustering coefficient (ratio of actual triangles to
//! possible triangles given the node's degree).
//!
//! Reference: Watts, D.J., Strogatz, S.H. (1998). "Collective Dynamics of
//! 'Small-World' Networks." *Nature*, 393(6684), 440--442.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use rayon::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Local clustering coefficients and triangle counts.
///
/// **Complexity:** O(V * d^2) where V is vertices and d is average degree.
/// For each node, checks all pairs of neighbours for triangle completion.
/// Parallelised across nodes.
///
/// **When to use:** Measuring local graph density, identifying tightly
/// connected neighbourhoods, or computing transitivity metrics.
pub(crate) struct ClusteringCoefficients;

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "graph triangle count and degree cast from usize to i64 — values are small graph metrics"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "graph clustering coefficient indices are bounds-checked by the CSR node count"
)]
impl FixedRule for ClusteringCoefficients {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let (graph, indices, _) = edges.as_directed_graph(true)?;
        let coefficients = clustering_coefficients(&graph, poison)?;
        for (idx, (coefficient, triangle_count, degree)) in coefficients.into_iter().enumerate() {
            out.put(vec![
                indices[idx].clone(),
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

/// Compute local clustering coefficients and triangle counts per node.
///
/// **Complexity:** O(V * d^2) where d is average degree.  Uses parallel
/// iteration over nodes.  For sparse graphs, this is much faster than
/// O(V^3) matrix methods.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
#[expect(
    clippy::as_conversions,
    reason = "triangle count and degree cast to f64 — values are small graph metrics"
)]
fn clustering_coefficients(
    graph: &DirectedCsrGraph,
    poison: Poison,
) -> Result<Vec<(f64, usize, usize)>> {
    let node_count = graph.node_count();

    (0..node_count)
        .into_par_iter()
        .map(|node_idx| -> Result<(f64, usize, usize)> {
            let neighbors = graph.out_neighbors(node_idx).collect_vec();
            let degree = neighbors.len();
            if degree < 2 {
                Ok((0., 0, degree))
            } else {
                let triangle_count = neighbors
                    .iter()
                    .map(|neighbor_a| {
                        neighbors
                            .iter()
                            .filter(|neighbor_b| {
                                if neighbor_a <= neighbor_b {
                                    return false;
                                }
                                for edge_target in graph.out_neighbors(*neighbor_a) {
                                    if edge_target == **neighbor_b {
                                        return true;
                                    }
                                }
                                false
                            })
                            .count()
                    })
                    .sum();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "i64 to f64: precision loss acceptable"
                )]
                let coefficient =
                    2. * triangle_count as f64 / ((degree as f64) * ((degree as f64) - 1.));
                poison.check()?;
                Ok((coefficient, triangle_count, degree))
            }
        })
        .collect::<Result<_>>()
}
