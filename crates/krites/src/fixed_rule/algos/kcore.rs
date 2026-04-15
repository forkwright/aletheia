//! K-core decomposition.
//!
//! Assigns each node the largest k such that it belongs to the k-core, i.e.,
//! the maximal induced subgraph where every node has degree >= k.
//!
//! Uses iterative peeling: for increasing k, remove every node whose current
//! effective degree (within the surviving subgraph) falls below k.  The
//! k-value assigned to each node is the largest k at which it survived.
//!
//! Reference: Batagelj, V., Zaversnik, M. (2003). "An O(m) Algorithm for
//! Cores Decomposition of Networks." *arXiv:cs/0310049*.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// K-core decomposition via iterative peeling.
///
/// **Complexity:** O(V + E) where V is vertices and E is edges.
///
/// **When to use:** Identifying densely connected subgraphs, filtering
/// peripheral nodes, or as a pre-processing step for community detection.
pub(crate) struct KCore;

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph k-core peeling indices are bounds-checked by the node count and degree arrays"
)]
impl FixedRule for KCore {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(true))?;

        let (graph, indices, _) = edges.as_directed_graph(undirected)?;

        let node_count = graph.node_count() as usize;
        if node_count == 0 {
            return Ok(());
        }

        #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
        let node_count_u32 = node_count as u32;
        let adjacency: Vec<Vec<u32>> = (0..node_count_u32)
            .map(|node| {
                let mut neighbors: Vec<u32> = graph.out_neighbors(node).collect();
                neighbors.sort_unstable();
                neighbors.dedup();
                neighbors
            })
            .collect_vec();

        let mut effective_degree: Vec<u32> = adjacency
            .iter()
            .map(|neighbors: &Vec<u32>| {
                #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
                let len = neighbors.len() as u32;
                len
            })
            .collect();
        let mut alive: Vec<bool> = vec![true; node_count];
        let mut core_number: Vec<u32> = vec![0; node_count];

        let max_degree = effective_degree.iter().copied().max().unwrap_or(0);
        let mut k: u32 = 1;
        while k <= max_degree {
            let mut queue: Vec<u32> = (0..node_count_u32)
                .filter(|&v| alive[v as usize] && effective_degree[v as usize] < k)
                .collect();

            while let Some(v) = queue.pop() {
                if !alive[v as usize] {
                    continue;
                }
                alive[v as usize] = false;
                core_number[v as usize] = k - 1;
                for &u in &adjacency[v as usize] {
                    if alive[u as usize] {
                        effective_degree[u as usize] -= 1;
                        if effective_degree[u as usize] < k {
                            queue.push(u);
                        }
                    }
                }
                poison.check()?;
            }

            k += 1;
        }

        for v in 0..node_count {
            if alive[v] {
                core_number[v] = k - 1;
            }
        }

        for (v, k_value) in core_number.into_iter().enumerate() {
            out.put(vec![
                indices[v].clone(),
                DataValue::from(i64::from(k_value)),
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
        Ok(2)
    }
}
