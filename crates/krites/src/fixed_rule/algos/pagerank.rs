//! `PageRank` fixed rule.
//!
//! Computes the `PageRank` score for every node in the graph using the
//! power-iteration method on the CSR representation.
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
/// **Complexity:** O(I * (V + E)) where I is iterations, V is vertices,
/// E is edges.  Each iteration performs a full graph traversal.
///
/// **When to use:** Ranking nodes by importance based on link structure.
/// Classic measure for citation networks, web graphs, and knowledge graphs.
pub(crate) struct PageRank;

#[expect(
    clippy::as_conversions,
    clippy::cast_lossless,
    reason = "f32-to-f64 promotion is lossless but clippy flags it — kept as `as` for graph output consistency"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "PageRank result indices are bounds-checked by the graph node count"
)]
impl FixedRule for PageRank {
    #[expect(
        unused_variables,
        reason = "poison is required by the FixedRule trait but PageRank does not poll for cancellation"
    )]
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional f64 to f32 reduction"
        )]
        let damping_factor = payload.unit_interval_option("theta", Some(0.85))? as f32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional f64 to f32 reduction"
        )]
        let tolerance = payload.unit_interval_option("epsilon", Some(0.0001))? as f32;
        let max_iterations = payload.pos_integer_option("iterations", Some(10))?;

        let (graph, indices, _) = edges.as_directed_graph(undirected)?;

        if indices.is_empty() {
            return Ok(());
        }

        let (ranks, _iterations_run, _final_error) = page_rank(
            &graph,
            PageRankConfig::new(max_iterations, tolerance as f64, damping_factor),
        );

        for (idx, score) in ranks.iter().enumerate() {
            out.put(vec![indices[idx].clone(), DataValue::from(*score as f64)]);
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
