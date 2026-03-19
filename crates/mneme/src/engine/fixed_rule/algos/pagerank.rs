//! PageRank fixed rule.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::csr::{PageRankConfig, page_rank};
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct PageRank;

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
        let theta = payload.unit_interval_option("theta", Some(0.85))? as f32;
        let epsilon = payload.unit_interval_option("epsilon", Some(0.0001))? as f32;
        let iterations = payload.pos_integer_option("iterations", Some(10))?;

        let (graph, indices, _) = edges.as_directed_graph(undirected)?;

        if indices.is_empty() {
            return Ok(());
        }

        let (ranks, _n_run, _) = page_rank(
            &graph,
            PageRankConfig::new(iterations, epsilon as f64, theta),
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
