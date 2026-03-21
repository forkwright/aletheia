use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::ValidityTs;
use crate::parse::SourceSpan;
use crate::runtime::minhash_lsh::LshSearch;

use super::magic::MagicSymbol;
use super::search::*;

#[derive(Debug, Clone)]
pub(crate) enum NormalFormAtom {
    Rule(NormalFormRuleApplyAtom),
    Relation(NormalFormRelationApplyAtom),
    NegatedRule(NormalFormRuleApplyAtom),
    NegatedRelation(NormalFormRelationApplyAtom),
    Predicate(Expr),
    Unification(Unification),
    HnswSearch(HnswSearch),
    FtsSearch(FtsSearch),
    LshSearch(LshSearch),
}

#[derive(Debug, Clone)]
pub(crate) enum MagicAtom {
    Rule(MagicRuleApplyAtom),
    Relation(MagicRelationApplyAtom),
    Predicate(Expr),
    NegatedRule(MagicRuleApplyAtom),
    NegatedRelation(MagicRelationApplyAtom),
    Unification(Unification),
    HnswSearch(HnswSearch),
    FtsSearch(FtsSearch),
    LshSearch(LshSearch),
}

#[derive(Clone, Debug)]
pub(crate) struct InputRuleApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: Vec<Expr>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct InputNamedFieldRelationApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: BTreeMap<CompactString, Expr>,
    pub(crate) valid_at: Option<ValidityTs>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct InputRelationApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: Vec<Expr>,
    pub(crate) valid_at: Option<ValidityTs>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalFormRuleApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: Vec<Symbol>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalFormRelationApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: Vec<Symbol>,
    pub(crate) valid_at: Option<ValidityTs>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct MagicRuleApplyAtom {
    pub(crate) name: MagicSymbol,
    pub(crate) args: Vec<Symbol>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct MagicRelationApplyAtom {
    pub(crate) name: Symbol,
    pub(crate) args: Vec<Symbol>,
    pub(crate) valid_at: Option<ValidityTs>,
    pub(crate) span: SourceSpan,
}
