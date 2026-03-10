//! K-core decomposition.
//!
//! Assigns each node the largest k such that it belongs to the k-core, i.e.,
//! the maximal induced subgraph where every node has degree ≥ k.
//!
//! Algorithm: iterative peeling.  For increasing k, remove every node whose
//! current effective degree (within the surviving subgraph) falls below k.
//! The k-value assigned to each node is the largest k at which it survived.
use std::collections::BTreeMap;

use crate::engine::error::DbResult as Result;
use compact_str::CompactString;
use itertools::Itertools;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct KCore;

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

        let n = graph.node_count() as usize;
        if n == 0 {
            return Ok(());
        }

        // Build adjacency as plain degree counts; we need the full neighbour
        // set to recompute effective degrees after removals.
        let adj: Vec<Vec<u32>> = (0..n as u32)
            .map(|node| {
                let mut nb: Vec<u32> = graph.out_neighbors(node).cloned().collect();
                // Deduplicate — edges may have been mirrored by as_directed_graph.
                nb.sort_unstable();
                nb.dedup();
                nb
            })
            .collect_vec();

        // effective_degree[v] = number of neighbours that are still alive.
        let mut effective_degree: Vec<u32> = adj.iter().map(|nb| nb.len() as u32).collect();
        let mut alive: Vec<bool> = vec![true; n];
        // core[v] = k-core value; starts at 0 and is promoted as we peel.
        let mut core: Vec<u32> = vec![0; n];

        // Peeling: iterate k from 1 upward.
        let max_degree = effective_degree.iter().copied().max().unwrap_or(0);
        let mut k: u32 = 1;
        while k <= max_degree {
            // Collect seeds — alive nodes whose degree has dropped below k.
            let mut queue: Vec<u32> = (0..n as u32)
                .filter(|&v| alive[v as usize] && effective_degree[v as usize] < k)
                .collect();

            while let Some(v) = queue.pop() {
                if !alive[v as usize] {
                    continue;
                }
                alive[v as usize] = false;
                core[v as usize] = k - 1;
                for &u in &adj[v as usize] {
                    if alive[u as usize] {
                        effective_degree[u as usize] -= 1;
                        if effective_degree[u as usize] < k {
                            queue.push(u);
                        }
                    }
                }
                poison.check()?;
            }

            // All surviving nodes now have degree ≥ k inside the subgraph.
            k += 1;
        }

        // Survivors at the end belong to the highest k-core found.
        for v in 0..n {
            if alive[v] {
                core[v] = k - 1;
            }
        }

        for (v, k_val) in core.into_iter().enumerate() {
            out.put(vec![indices[v].clone(), DataValue::from(k_val as i64)]);
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
