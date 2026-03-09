// Originally derived from CozoDB v0.7.6 (MPL-2.0).
// Copyright 2022, The Cozo Project Authors — see NOTICE for details.

use std::collections::BTreeMap;

use crate::engine::error::DbResult as Result;
use crate::engine::fixed_rule::csr::DirectedCsrGraph;
use itertools::Itertools;
use rayon::prelude::*;
use compact_str::CompactString;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct ClusteringCoefficients;

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
        for (idx, (cc, n_triangles, degree)) in coefficients.into_iter().enumerate() {
            out.put(vec![
                indices[idx].clone(),
                DataValue::from(cc),
                DataValue::from(n_triangles as i64),
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

fn clustering_coefficients(
    graph: &DirectedCsrGraph,
    poison: Poison,
) -> Result<Vec<(f64, usize, usize)>> {
    let node_size = graph.node_count();

    (0..node_size)
        .into_par_iter()
        .map(|node_idx| -> Result<(f64, usize, usize)> {
            let edges = graph.out_neighbors(node_idx).collect_vec();
            let degree = edges.len();
            if degree < 2 {
                Ok((0., 0, degree))
            } else {
                let n_triangles = edges
                    .iter()
                    .map(|e_src| {
                        edges
                            .iter()
                            .filter(|e_dst| {
                                if e_src <= e_dst {
                                    return false;
                                }
                                for nb in graph.out_neighbors(**e_src) {
                                    if nb == **e_dst {
                                        return true;
                                    }
                                }
                                false
                            })
                            .count()
                    })
                    .sum();
                let cc = 2. * n_triangles as f64 / ((degree as f64) * ((degree as f64) - 1.));
                poison.check()?;
                Ok((cc, n_triangles, degree))
            }
        })
        .collect::<Result<_>>()
}
