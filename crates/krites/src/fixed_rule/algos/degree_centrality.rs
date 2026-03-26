//! Degree centrality computation.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) struct DegreeCentrality;

impl FixedRule for DegreeCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let it = payload.get_input(0)?.ensure_min_len(2)?.iter()?;
        let mut counter: BTreeMap<DataValue, (usize, usize, usize)> = BTreeMap::new();
        for tuple in it {
            let tuple = tuple?;
            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let from = tuple[0].clone();
            let (from_total, from_out, _) = counter.entry(from).or_default();
            *from_total += 1;
            *from_out += 1;

            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let to = tuple[1].clone();
            let (to_total, _, to_in) = counter.entry(to).or_default();
            *to_total += 1;
            *to_in += 1;
            poison.check()?;
        }
        if let Ok(nodes) = payload.get_input(1) {
            for tuple in nodes.iter()? {
                let tuple = tuple?;
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let id = &tuple[0];
                if !counter.contains_key(id) {
                    counter.insert(id.clone(), (0, 0, 0));
                }
                poison.check()?;
            }
        }
        for (k, (total_d, out_d, in_d)) in counter.into_iter() {
            let tuple = vec![
                k,
                DataValue::from(total_d as i64),
                DataValue::from(out_d as i64),
                DataValue::from(in_d as i64),
            ];
            out.put(tuple);
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
