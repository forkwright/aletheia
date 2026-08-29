//! `PageRank` fixed rule: power iteration over a CSR graph.
//!
//! The numerics live in the crate's sovereign CSR core
//! (`fixed_rule::csr::page_rank`); this module is the fixed-rule shell that
//! reads the public option contract, builds the graph, and emits one
//! `(node, score)` row per vertex.
//!
//! Reference: Page, L. et al. (1999). "The `PageRank` Citation Ranking:
//! Bringing Order to the Web." Stanford technical report.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::{PageRankConfig, page_rank};
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// `PageRank` via power iteration.
///
/// **Complexity:** O(I * (V + E)) where I is the iteration cap, V the
/// vertex count, E the edge count.
///
/// **When to use:** ranking nodes by link-structure importance — citation
/// networks, web graphs, knowledge graphs.
pub(crate) struct PageRank;

#[expect(
    clippy::cast_possible_truncation,
    reason = "the theta/epsilon options are small unit-interval magnitudes; the f32 narrowing is the core's established config contract"
)]
impl FixedRule for PageRank {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let damping = payload.unit_interval_option("theta", Some(0.85))? as f32;
        let tolerance = payload.unit_interval_option("epsilon", Some(0.0001))? as f32;
        let iteration_cap = payload.pos_integer_option("iterations", Some(10))?;

        let (graph, node_ids, _) = edges.as_directed_graph(undirected)?;
        if node_ids.is_empty() {
            return Ok(());
        }

        let (scores, _, _) = page_rank(
            &graph,
            PageRankConfig::new(iteration_cap, f64::from(tolerance), damping),
        );

        for (node_id, score) in node_ids.into_iter().zip(scores) {
            out.put(vec![node_id, DataValue::from(f64::from(score))]);
            poison.check()?;
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
