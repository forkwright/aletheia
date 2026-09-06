//! Utility fixed rules.
//!
//! `constant` and `reorder_sort` are sovereign; the `_native.rs` filenames they
//! were authored under are retired along with the derived siblings that
//! justified them. `ReciprocalRankFusion` lives directly in this module,
//! rather than in its own file the way it used to: wave 1.3 of the
//! fleet-wiring plan deleted its sovereign rank-fusion math in favor of the
//! fleet primitive `heurema::rrf`, and what remains here is Datalog
//! marshalling glue too small to justify a dedicated file. `heurema` owns
//! the fusion arithmetic and its ordering contract now, not this crate.

use std::collections::BTreeMap;

use compact_str::CompactString;
use heurema::DEFAULT_RRF_K_CONSTANT;
use rustc_hash::FxHashMap;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::FixedRuleError;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) mod constant;
pub(crate) mod reorder_sort;

pub(crate) use constant::Constant;
pub(crate) use reorder_sort::ReorderSort;

/// Reciprocal-rank fusion over up to three ranked signal relations (BM25,
/// vector, graph). The fusion arithmetic — score accumulation, `k`
/// dampening, and the deterministic descending-score/ascending-id tie-break
/// — belongs to `heurema::rrf`; this struct only marshals the Datalog input
/// relations into heurema's ranking shape and the fused output back into
/// this rule's five-column contract.
pub(crate) struct ReciprocalRankFusion;

fn invalid(message: impl Into<String>) -> crate::error::InternalError {
    FixedRuleError::InvalidInput {
        rule: "ReciprocalRankFusion".to_string(),
        message: message.into(),
        location: snafu::location!(),
    }
    .into()
}

impl FixedRule for ReciprocalRankFusion {
    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(5)
    }

    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let bm25_input = payload.get_input(0)?;
        let vec_input = payload.get_input(1)?;
        let graph_input = payload.get_input(2)?;

        let (bm25_ranking, bm25_ranks) = signal_ranking(bm25_input)?;
        let (vec_ranking, vec_ranks) = signal_ranking(vec_input)?;
        let (graph_ranking, graph_ranks) = signal_ranking(graph_input)?;

        let k_constant = payload.float_option("k", Some(f64::from(DEFAULT_RRF_K_CONSTANT)))?;
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "the `k` option is evaluated as f64 by float_option; heurema::rrf takes f32 -- RRF's dampening constant (paper default 60) needs no precision beyond f32"
        )]
        let k_constant = k_constant as f32;

        let fused = heurema::rrf(&[bm25_ranking, vec_ranking, graph_ranking], k_constant)
            .map_err(|err| invalid(format!("invalid `k` option: {err}")))?;

        for (id, score) in fused {
            let bm25_rank = bm25_ranks.get(&id).copied().unwrap_or(0);
            let vec_rank = vec_ranks.get(&id).copied().unwrap_or(0);
            let graph_rank = graph_ranks.get(&id).copied().unwrap_or(0);

            out.put(vec![
                DataValue::Str(id),
                DataValue::from(f64::from(score)),
                DataValue::from(rank_to_output(bm25_rank)),
                DataValue::from(rank_to_output(vec_rank)),
                DataValue::from(rank_to_output(graph_rank)),
            ]);
            poison.check()?;
        }
        Ok(())
    }
}

/// One signal's descending-score ranking for `heurema::rrf`, alongside a
/// lookup from id to that signal's 1-based rank -- kept for this rule's own
/// diagnostic rank columns, which heurema's fused output does not carry.
type SignalRanking = (Vec<(CompactString, f32)>, FxHashMap<CompactString, usize>);

/// Reads one signal's `[id, score]` input relation into a [`SignalRanking`].
/// An id repeated within one input relation keeps only its best
/// (highest-scoring) occurrence, matching `heurema::rrf`'s own
/// within-ranking dedup rule.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn signal_ranking(
    input: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
) -> Result<SignalRanking> {
    let mut scored: Vec<(CompactString, f32)> = Vec::new();
    for row in input.iter()? {
        let row = row?;
        if let (Some(id_val), Some(score_val)) = (row.first(), row.get(1))
            && let Some(id_str) = id_val.get_str()
        {
            let score = score_val.get_float().unwrap_or(0.0);
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "input score narrows to heurema's f32 ranking domain; only this signal's relative order matters for fusion, not the value carried into it"
            )]
            let score = score as f32;
            scored.push((CompactString::from(id_str), score));
        }
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut ranks: FxHashMap<CompactString, usize> = FxHashMap::default();
    for (idx, (id, _)) in scored.iter().enumerate() {
        ranks.entry(id.clone()).or_insert(idx + 1);
    }
    Ok((scored, ranks))
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "rank-to-i64 cast — rank values are small positive numbers well within i64 range"
)]
fn rank_to_output(rank: usize) -> i64 {
    if rank == 0 { -1 } else { rank as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_to_output_absent() {
        assert_eq!(rank_to_output(0), -1);
    }

    #[test]
    fn rank_to_output_present() {
        assert_eq!(rank_to_output(5), 5);
    }
}
