// ReciprocalRankFusion fixed rule: fuses BM25, vector, and graph ranked lists.

use std::collections::BTreeMap;

use crate::error::DbResult as Result;
use rustc_hash::FxHashMap;
use smartstring::{LazyCompact, SmartString};

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

const RRF_K: f64 = 60.0;

pub(crate) struct ReciprocalRankFusion;

impl FixedRule for ReciprocalRankFusion {
    fn arity(
        &self,
        _options: &BTreeMap<SmartString<LazyCompact>, Expr>,
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

        let bm25_scores = collect_signal_scores(bm25_input)?;
        let vec_scores = collect_signal_scores(vec_input)?;
        let graph_scores = collect_signal_scores(graph_input)?;

        let bm25_ranks = assign_ranks(&bm25_scores);
        let vec_ranks = assign_ranks(&vec_scores);
        let graph_ranks = assign_ranks(&graph_scores);

        let mut all_ids: FxHashMap<SmartString<LazyCompact>, ()> = FxHashMap::default();
        for id in bm25_scores.keys() {
            all_ids.insert(id.clone(), ());
        }
        for id in vec_scores.keys() {
            all_ids.insert(id.clone(), ());
        }
        for id in graph_scores.keys() {
            all_ids.insert(id.clone(), ());
        }

        for (id, _) in all_ids {
            let bm25_rank = bm25_ranks.get(&id).copied().unwrap_or(0);
            let vec_rank = vec_ranks.get(&id).copied().unwrap_or(0);
            let graph_rank = graph_ranks.get(&id).copied().unwrap_or(0);

            let rrf_score = signal_contribution(bm25_rank)
                + signal_contribution(vec_rank)
                + signal_contribution(graph_rank);

            out.put(vec![
                DataValue::Str(id),
                DataValue::from(rrf_score),
                DataValue::from(rank_to_output(bm25_rank)),
                DataValue::from(rank_to_output(vec_rank)),
                DataValue::from(rank_to_output(graph_rank)),
            ]);
            poison.check()?;
        }
        Ok(())
    }
}

fn signal_contribution(rank: usize) -> f64 {
    if rank == 0 {
        0.0
    } else {
        1.0 / (RRF_K + rank as f64)
    }
}

fn collect_signal_scores(
    input: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
) -> Result<FxHashMap<SmartString<LazyCompact>, f64>> {
    let mut scores: FxHashMap<SmartString<LazyCompact>, f64> = FxHashMap::default();
    for row in input.iter()? {
        let row = row?;
        if let (Some(id_val), Some(score_val)) = (row.first(), row.get(1)) {
            if let Some(id_str) = id_val.get_str() {
                let score = score_val.get_float().unwrap_or(0.0);
                scores.insert(SmartString::from(id_str), score);
            }
        }
    }
    Ok(scores)
}

fn assign_ranks(
    scores: &FxHashMap<SmartString<LazyCompact>, f64>,
) -> FxHashMap<SmartString<LazyCompact>, usize> {
    let mut sorted: Vec<_> = scores.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted
        .iter()
        .enumerate()
        .map(|(rank, (id, _))| ((*id).clone(), rank + 1))
        .collect()
}

fn rank_to_output(rank: usize) -> i64 {
    if rank == 0 { -1 } else { rank as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_contribution_absent_is_zero() {
        assert_eq!(signal_contribution(0), 0.0);
    }

    #[test]
    fn signal_contribution_rank_one() {
        let expected = 1.0 / (60.0 + 1.0);
        assert!((signal_contribution(1) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn assign_ranks_sorted_descending() {
        let mut scores = FxHashMap::default();
        scores.insert(SmartString::from("low"), 1.0);
        scores.insert(SmartString::from("high"), 9.0);
        scores.insert(SmartString::from("mid"), 5.0);

        let ranks = assign_ranks(&scores);
        assert_eq!(ranks[&SmartString::from("high")], 1);
        assert_eq!(ranks[&SmartString::from("mid")], 2);
        assert_eq!(ranks[&SmartString::from("low")], 3);
    }

    #[test]
    fn assign_ranks_empty() {
        let scores = FxHashMap::default();
        let ranks = assign_ranks(&scores);
        assert!(ranks.is_empty());
    }

    #[test]
    fn rank_to_output_absent() {
        assert_eq!(rank_to_output(0), -1);
    }

    #[test]
    fn rank_to_output_present() {
        assert_eq!(rank_to_output(5), 5);
    }
}
