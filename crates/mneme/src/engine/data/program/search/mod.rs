use std::collections::BTreeMap;
use std::fmt::Debug;

use compact_str::CompactString;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::fts::FtsIndexManifest;
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::hnsw::HnswIndexManifest;
use crate::engine::runtime::relation::RelationHandle;

use super::atoms::*;

#[derive(Clone)]
pub(crate) enum InputAtom {
    Rule {
        inner: InputRuleApplyAtom,
    },
    NamedFieldRelation {
        inner: InputNamedFieldRelationApplyAtom,
    },
    Relation {
        inner: InputRelationApplyAtom,
    },
    Predicate {
        inner: Expr,
    },
    Negation {
        inner: Box<InputAtom>,
        span: SourceSpan,
    },
    Conjunction {
        inner: Vec<InputAtom>,
        span: SourceSpan,
    },
    Disjunction {
        inner: Vec<InputAtom>,
        span: SourceSpan,
    },
    Unification {
        inner: Unification,
    },
    Search {
        inner: SearchInput,
    },
}

#[derive(Clone)]
pub(crate) struct SearchInput {
    pub(crate) relation: Symbol,
    pub(crate) index: Symbol,
    pub(crate) bindings: BTreeMap<CompactString, Expr>,
    pub(crate) parameters: BTreeMap<CompactString, Expr>,
    pub(crate) span: SourceSpan,
}

#[derive(Clone, Debug)]
pub(crate) struct HnswSearch {
    pub(crate) base_handle: RelationHandle,
    pub(crate) idx_handle: RelationHandle,
    pub(crate) manifest: HnswIndexManifest,
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) k: usize,
    pub(crate) ef: usize,
    pub(crate) query: Symbol,
    pub(crate) bind_field: Option<Symbol>,
    pub(crate) bind_field_idx: Option<Symbol>,
    pub(crate) bind_distance: Option<Symbol>,
    pub(crate) bind_vector: Option<Symbol>,
    pub(crate) radius: Option<f64>,
    pub(crate) filter: Option<Expr>,
    pub(crate) span: SourceSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum FtsScoreKind {
    TfIdf,
    Tf,
    Bm25,
}

#[derive(Clone, Debug)]
pub(crate) struct FtsSearch {
    pub(crate) base_handle: RelationHandle,
    pub(crate) idx_handle: RelationHandle,
    pub(crate) manifest: FtsIndexManifest,
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) k: usize,
    pub(crate) query: Symbol,
    pub(crate) score_kind: FtsScoreKind,
    pub(crate) bind_score: Option<Symbol>,
    pub(crate) filter: Option<Expr>,
    pub(crate) span: SourceSpan,
}

impl HnswSearch {
    pub(crate) fn all_bindings(&self) -> impl Iterator<Item = &Symbol> {
        self.bindings
            .iter()
            .chain(self.bind_field.iter())
            .chain(self.bind_field_idx.iter())
            .chain(self.bind_distance.iter())
            .chain(self.bind_vector.iter())
    }
}

impl FtsSearch {
    pub(crate) fn all_bindings(&self) -> impl Iterator<Item = &Symbol> {
        self.bindings.iter().chain(self.bind_score.iter())
    }
}

mod atom_impl;
mod hnsw_normalize;
mod lsh_fts;

pub(crate) use atom_impl::Unification;
