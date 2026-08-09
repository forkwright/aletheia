//! Degree centrality: total, outgoing, and incoming edge counts per node.
//!
//! Single pass over the edge relation accumulates separate out/in counters
//! per endpoint; total degree is their sum. A secondary node relation may
//! contribute nodes with zero edges (isolated nodes) so they still receive
//! a row.
//!
//! Reference: Freeman, L.C. (1978). "Centrality in Social Networks:
//! Conceptual Clarification." *Social Networks*, 1(3), 215--239.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Degree centrality: total, out-, and in-degree per node.
///
/// **Complexity:** O(E) where E is the edge count — a single pass.
///
/// **When to use:** Quick hub/authority identification in directed
/// networks, or as a baseline before more expensive centrality measures.
pub(crate) struct DegreeCentrality;

impl FixedRule for DegreeCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;

        let mut out_degree: BTreeMap<DataValue, i64> = BTreeMap::new();
        let mut in_degree: BTreeMap<DataValue, i64> = BTreeMap::new();
        let mut seen: BTreeSet<DataValue> = BTreeSet::new();

        for row in edges.iter()? {
            let mut fields = row?.into_iter();
            let Some(source) = fields.next() else {
                continue;
            };
            let Some(target) = fields.next() else {
                continue;
            };
            seen.insert(source.clone());
            seen.insert(target.clone());
            *out_degree.entry(source).or_insert(0) += 1;
            *in_degree.entry(target).or_insert(0) += 1;
            poison.check()?;
        }

        if let Ok(extra_nodes) = payload.get_input(1) {
            for row in extra_nodes.iter()? {
                if let Some(node) = row?.into_iter().next() {
                    seen.insert(node);
                }
                poison.check()?;
            }
        }

        for node in seen {
            let out_d = out_degree.get(&node).copied().unwrap_or(0);
            let in_d = in_degree.get(&node).copied().unwrap_or(0);
            out.put(vec![
                node,
                DataValue::from(out_d + in_d),
                DataValue::from(out_d),
                DataValue::from(in_d),
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
        Ok(4)
    }
}
