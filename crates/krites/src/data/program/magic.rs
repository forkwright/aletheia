use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::{Debug, Display, Formatter};

use smallvec::SmallVec;

use crate::data::aggr::Aggregation;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::query::compile::ContainedRuleMultiplicity;

use super::atoms::*;
use super::fixed_rule::*;

#[derive(Debug)]
pub(crate) struct StratifiedNormalFormProgram(pub(crate) Vec<NormalFormProgram>);

#[derive(Debug)]
pub(crate) enum NormalFormRulesOrFixed {
    Rules { rules: Vec<NormalFormInlineRule> },
    Fixed { fixed: FixedRuleApply },
}

impl NormalFormRulesOrFixed {
    pub(crate) fn rules(&self) -> Option<&[NormalFormInlineRule]> {
        match self {
            NormalFormRulesOrFixed::Rules { rules: r } => Some(r),
            NormalFormRulesOrFixed::Fixed { fixed: _ } => None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct NormalFormProgram {
    pub(crate) prog: BTreeMap<Symbol, NormalFormRulesOrFixed>,
    pub(crate) disable_magic_rewrite: bool,
}

#[derive(Debug)]
pub(crate) struct StratifiedMagicProgram(pub(crate) Vec<MagicProgram>);

#[derive(Debug)]
pub(crate) enum MagicRulesOrFixed {
    Rules { rules: Vec<MagicInlineRule> },
    Fixed { fixed: MagicFixedRuleApply },
}

impl Default for MagicRulesOrFixed {
    fn default() -> Self {
        Self::Rules { rules: vec![] }
    }
}

impl MagicRulesOrFixed {
    pub(crate) fn arity(&self) -> Result<usize> {
        Ok(match self {
            MagicRulesOrFixed::Rules { rules } => rules
                .first()
                .expect("rules vec always has at least one rule")
                .head
                .len(),
            MagicRulesOrFixed::Fixed { fixed } => fixed.arity,
        })
    }
    pub(crate) fn mut_rules(&mut self) -> Option<&mut Vec<MagicInlineRule>> {
        match self {
            MagicRulesOrFixed::Rules { rules } => Some(rules),
            MagicRulesOrFixed::Fixed { fixed: _ } => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct MagicProgram {
    pub(crate) prog: BTreeMap<MagicSymbol, MagicRulesOrFixed>,
}

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
pub(crate) enum MagicSymbol {
    Muggle {
        inner: Symbol,
    },
    Magic {
        inner: Symbol,
        adornment: SmallVec<[bool; 8]>,
    },
    Input {
        inner: Symbol,
        adornment: SmallVec<[bool; 8]>,
    },
    Sup {
        inner: Symbol,
        adornment: SmallVec<[bool; 8]>,
        rule_idx: u16,
        sup_idx: u16,
    },
}

impl MagicSymbol {
    pub(crate) fn symbol(&self) -> &Symbol {
        match self {
            MagicSymbol::Muggle { inner, .. }
            | MagicSymbol::Magic { inner, .. }
            | MagicSymbol::Input { inner, .. }
            | MagicSymbol::Sup { inner, .. } => inner,
        }
    }
}

impl Display for MagicSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Debug for MagicSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MagicSymbol::Muggle { inner } => write!(f, "{}", inner.name),
            MagicSymbol::Magic { inner, adornment } => {
                write!(f, "{}|M", inner.name)?;
                for b in adornment {
                    if *b { write!(f, "b")? } else { write!(f, "f")? }
                }
                Ok(())
            }
            MagicSymbol::Input { inner, adornment } => {
                write!(f, "{}|I", inner.name)?;
                for b in adornment {
                    if *b { write!(f, "b")? } else { write!(f, "f")? }
                }
                Ok(())
            }
            MagicSymbol::Sup {
                inner,
                adornment,
                rule_idx,
                sup_idx,
            } => {
                write!(f, "{}|S.{}.{}", inner.name, rule_idx, sup_idx)?;
                for b in adornment {
                    if *b { write!(f, "b")? } else { write!(f, "f")? }
                }
                Ok(())
            }
        }
    }
}

impl MagicSymbol {
    pub(crate) fn as_plain_symbol(&self) -> &Symbol {
        match self {
            MagicSymbol::Muggle { inner, .. }
            | MagicSymbol::Magic { inner, .. }
            | MagicSymbol::Input { inner, .. }
            | MagicSymbol::Sup { inner, .. } => inner,
        }
    }
    pub(crate) fn magic_adornment(&self) -> &[bool] {
        match self {
            MagicSymbol::Muggle { .. } => &[],
            MagicSymbol::Magic { adornment, .. }
            | MagicSymbol::Input { adornment, .. }
            | MagicSymbol::Sup { adornment, .. } => adornment,
        }
    }
    pub(crate) fn has_bound_adornment(&self) -> bool {
        self.magic_adornment().iter().any(|b| *b)
    }
    pub(crate) fn is_prog_entry(&self) -> bool {
        if let MagicSymbol::Muggle { inner } = self {
            inner.is_prog_entry()
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub(crate) struct NormalFormInlineRule {
    pub(crate) head: Vec<Symbol>,
    pub(crate) aggr: Vec<Option<(Aggregation, Vec<DataValue>)>>,
    pub(crate) body: Vec<NormalFormAtom>,
}

#[derive(Debug)]
pub(crate) struct MagicInlineRule {
    pub(crate) head: Vec<Symbol>,
    pub(crate) aggr: Vec<Option<(Aggregation, Vec<DataValue>)>>,
    pub(crate) body: Vec<MagicAtom>,
}

impl MagicInlineRule {
    pub(crate) fn contained_rules(&self) -> BTreeMap<MagicSymbol, ContainedRuleMultiplicity> {
        let mut coll = BTreeMap::new();
        for atom in self.body.iter() {
            match atom {
                MagicAtom::Rule(rule) | MagicAtom::NegatedRule(rule) => {
                    match coll.entry(rule.name.clone()) {
                        Entry::Vacant(ent) => {
                            ent.insert(ContainedRuleMultiplicity::One);
                        }
                        Entry::Occupied(mut ent) => {
                            *ent.get_mut() = ContainedRuleMultiplicity::Many;
                        }
                    }
                }
                _ => {
                    // NOTE: non-rule atoms don't contribute to rule containment
                }
            }
        }
        coll
    }
}
