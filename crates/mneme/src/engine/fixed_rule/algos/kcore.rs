//! K-core decomposition.
//!
//! Assigns each node the largest k such that it belongs to the k-core, i.e.,
//! the maximal induced subgraph where every node has degree ≥ k.
//!
//! Algorithm: iterative peeling.  For increasing k, remove every node whose
//! current effective degree (within the surviving subgraph) falls below k.
//! The k-value assigned to each node is the largest k at which it survived.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;

use crate::engine::error::InternalResult as Result;
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

        #[expect(
            clippy::cast_possible_truncation,
            reason = "graph node count bounded by u32"
        )]
        let n_u32 = n as u32;
        let adj: Vec<Vec<u32>> = (0..n_u32)
            .map(|node| {
                let mut nb: Vec<u32> = graph.out_neighbors(node).collect();
                nb.sort_unstable();
                nb.dedup();
                nb
            })
            .collect_vec();

        let mut effective_degree: Vec<u32> = adj
            .iter()
            .map(|nb: &Vec<u32>| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "neighbour count bounded by u32 node count"
                )]
                let len = nb.len() as u32;
                len
            })
            .collect();
        let mut alive: Vec<bool> = vec![true; n];
        let mut core: Vec<u32> = vec![0; n];

        let max_degree = effective_degree.iter().copied().max().unwrap_or(0);
        let mut k: u32 = 1;
        while k <= max_degree {
            let mut queue: Vec<u32> = (0..n_u32)
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

            k += 1;
        }

        for v in 0..n {
            if alive[v] {
                core[v] = k - 1;
            }
        }

        for (v, k_val) in core.into_iter().enumerate() {
            out.put(vec![indices[v].clone(), DataValue::from(i64::from(k_val))]);
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
