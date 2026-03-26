//! PageRank fixed rule.
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
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional f64 to f32 reduction"
        )]
        let theta = payload.unit_interval_option("theta", Some(0.85))? as f32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional f64 to f32 reduction"
        )]
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
