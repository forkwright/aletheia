#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::{Display, Formatter};

use compact_str::CompactString;

use crate::engine::data::aggr::Aggregation;
use crate::engine::data::error::*;
use crate::engine::data::expr::Expr;
use crate::engine::data::symb::{PROG_ENTRY, Symbol};
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::transact::SessionTx;

use super::search::{InputAtom, Unification};

use super::atoms::*;
use super::fixed_rule::*;
use super::magic::*;
use super::types::*;

#[derive(Debug, Clone)]
pub(crate) enum InputInlineRulesOrFixed {
    Rules { rules: Vec<InputInlineRule> },
    Fixed { fixed: FixedRuleApply },
}

impl InputInlineRulesOrFixed {
    pub(crate) fn first_span(&self) -> SourceSpan {
        match self {
            InputInlineRulesOrFixed::Rules { rules, .. } => rules[0].span,
            InputInlineRulesOrFixed::Fixed { fixed, .. } => fixed.span,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InputProgram {
    pub(crate) prog: BTreeMap<Symbol, InputInlineRulesOrFixed>,
    pub(crate) out_opts: QueryOutOptions,
    pub(crate) disable_magic_rewrite: bool,
}
impl Display for InputProgram {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (name, rules) in &self.prog {
            match rules {
                InputInlineRulesOrFixed::Rules { rules, .. } => {
                    for InputInlineRule {
                        head, aggr, body, ..
                    } in rules
                    {
                        write!(f, "{name}[")?;
                        for (i, (h, a)) in head.iter().zip(aggr).enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            if let Some((aggr, aggr_args)) = a {
                                write!(f, "{}({}", aggr.name, h)?;
                                for aga in aggr_args {
                                    write!(f, ", {aga}")?;
                                }
                                write!(f, ")")?;
                            } else {
                                write!(f, "{h}")?;
                            }
                        }
                        write!(f, "] := ")?;
                        for (i, atom) in body.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{atom}")?;
                        }
                        writeln!(f, ";")?;
                    }
                }
                InputInlineRulesOrFixed::Fixed {
                    fixed:
                        FixedRuleApply {
                            fixed_handle: handle,
                            rule_args,
                            options,
                            head,
                            ..
                        },
                } => {
                    write!(f, "{name}")?;
                    f.debug_list().entries(head).finish()?;
                    write!(f, " <~ ")?;
                    write!(f, "{}(", handle.name)?;
                    let mut first = true;
                    for rule_arg in rule_args {
                        if first {
                            first = false;
                        } else {
                            write!(f, ", ")?;
                        }
                        write!(f, "{rule_arg}")?;
                    }
                    for (k, v) in options.as_ref() {
                        if first {
                            first = false;
                        } else {
                            write!(f, ", ")?;
                        }
                        write!(f, "{k}: {v}")?;
                    }
                    writeln!(f, ");")?;
                }
            }
        }
        write!(f, "{}", self.out_opts)?;
        Ok(())
    }
}

impl InputProgram {
    pub(crate) fn needs_write_lock(&self) -> Option<CompactString> {
        if let Some((h, _, _)) = &self.out_opts.store_relation {
            if !h.name.name.starts_with('_') {
                Some(h.name.name.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn get_entry_arity(&self) -> Result<usize> {
        if let Some(entry) = self.prog.get(&Symbol::new(PROG_ENTRY, SourceSpan(0, 0))) {
            return match entry {
                InputInlineRulesOrFixed::Rules { rules } => Ok(rules
                    .last()
                    .expect("rules vec always has at least one rule")
                    .head
                    .len()),
                InputInlineRulesOrFixed::Fixed { fixed } => fixed.arity(),
            };
        }

        Err(NoEntryError.into())
    }
    pub(crate) fn get_entry_out_head_or_default(&self) -> Result<Vec<Symbol>> {
        match self.get_entry_out_head() {
            Ok(r) => Ok(r),
            Err(_) => {
                let arity = self.get_entry_arity()?;
                Ok((0..arity)
                    .map(|i| Symbol::new(format!("_{i}"), SourceSpan(0, 0)))
                    .collect())
            }
        }
    }
    pub(crate) fn get_entry_out_head(&self) -> Result<Vec<Symbol>> {
        if let Some(entry) = self.prog.get(&Symbol::new(PROG_ENTRY, SourceSpan(0, 0))) {
            return match entry {
                InputInlineRulesOrFixed::Rules { rules } => {
                    let last_rule = rules
                        .last()
                        .expect("rules vec always has at least one rule");
                    let head = &last_rule.head;
                    let mut ret = Vec::with_capacity(head.len());
                    let aggrs = &last_rule.aggr;
                    for (symb, aggr) in head.iter().zip(aggrs.iter()) {
                        if let Some((aggr, _)) = aggr {
                            ret.push(Symbol::new(
                                format!(
                                    "{}({})",
                                    aggr.name
                                        .strip_prefix("AGGR_")
                                        .expect("all aggregator names are prefixed with AGGR_")
                                        .to_ascii_lowercase(),
                                    symb
                                ),
                                symb.span,
                            ))
                        } else {
                            ret.push(symb.clone())
                        }
                    }
                    Ok(ret)
                }
                InputInlineRulesOrFixed::Fixed { fixed } => {
                    if fixed.head.is_empty() {
                        Err(ProgramConstraintSnafu {
                            message: format!(
                                "entry head is not explicitly defined at {:?}",
                                entry.first_span()
                            ),
                        }
                        .build()
                        .into())
                    } else {
                        Ok(fixed.head.to_vec())
                    }
                }
            };
        }

        Err(NoEntryError.into())
    }
    pub(crate) fn into_normalized_program(
        self,
        tx: &SessionTx<'_>,
    ) -> Result<(NormalFormProgram, QueryOutOptions)> {
        let mut prog: BTreeMap<Symbol, _> = Default::default();
        for (k, rules_or_fixed) in self.prog {
            match rules_or_fixed {
                InputInlineRulesOrFixed::Rules { rules } => {
                    let mut collected_rules = vec![];
                    for rule in rules {
                        let mut counter = -1;
                        let mut gen_symb = |span| {
                            counter += 1;
                            Symbol::new(&format!("***{counter}") as &str, span)
                        };
                        let normalized_body = InputAtom::Conjunction {
                            inner: rule.body,
                            span: rule.span,
                        }
                        .disjunctive_normal_form(tx)?;
                        let mut new_head = Vec::with_capacity(rule.head.len());
                        let mut seen: BTreeMap<&Symbol, Vec<Symbol>> = BTreeMap::default();
                        for symb in rule.head.iter() {
                            match seen.entry(symb) {
                                Entry::Vacant(e) => {
                                    e.insert(vec![]);
                                    new_head.push(symb.clone());
                                }
                                Entry::Occupied(mut e) => {
                                    let new_symb = gen_symb(symb.span);
                                    e.get_mut().push(new_symb.clone());
                                    new_head.push(new_symb);
                                }
                            }
                        }
                        for conj in normalized_body.inner {
                            let mut body = conj.0;
                            for (old_symb, new_symbs) in seen.iter() {
                                for new_symb in new_symbs.iter() {
                                    body.push(NormalFormAtom::Unification(Unification {
                                        binding: new_symb.clone(),
                                        expr: Expr::Binding {
                                            var: (*old_symb).clone(),
                                            tuple_pos: None,
                                        },
                                        one_many_unif: false,
                                        span: new_symb.span,
                                    }))
                                }
                            }
                            let normalized_rule = NormalFormInlineRule {
                                head: new_head.clone(),
                                aggr: rule.aggr.clone(),
                                body,
                            };
                            collected_rules.push(normalized_rule.convert_to_well_ordered_rule()?);
                        }
                    }
                    prog.insert(
                        k.clone(),
                        NormalFormRulesOrFixed::Rules {
                            rules: collected_rules,
                        },
                    );
                }
                InputInlineRulesOrFixed::Fixed { fixed } => {
                    prog.insert(k.clone(), NormalFormRulesOrFixed::Fixed { fixed });
                }
            }
        }
        Ok((
            NormalFormProgram {
                prog,
                disable_magic_rewrite: self.disable_magic_rewrite,
            },
            self.out_opts,
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InputInlineRule {
    pub(crate) head: Vec<Symbol>,
    pub(crate) aggr: Vec<Option<(Aggregation, Vec<DataValue>)>>,
    pub(crate) body: Vec<InputAtom>,
    pub(crate) span: SourceSpan,
}
