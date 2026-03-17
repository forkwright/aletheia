use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};

use compact_str::CompactString;

use crate::engine::data::error::*;
use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::error::{InternalError, InternalResult as Result};
use crate::engine::fts::FtsIndexManifest;
use crate::engine::parse::SourceSpan;
use crate::engine::query::logical::Disjunction;
use crate::engine::runtime::hnsw::HnswIndexManifest;
use crate::engine::runtime::minhash_lsh::{LshSearch, MinHashLshIndexManifest};
use crate::engine::runtime::relation::{AccessLevel, RelationHandle};
use crate::engine::runtime::transact::SessionTx;

use super::atoms::*;
use super::types::*;

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

impl SearchInput {
    fn normalize_lsh(
        mut self,
        base_handle: RelationHandle,
        idx_handle: RelationHandle,
        manifest: MinHashLshIndexManifest,
        r#gen: &mut TempSymbGen,
    ) -> Result<Disjunction> {
        let mut conj = Vec::with_capacity(self.bindings.len() + 8);
        let mut bindings = Vec::with_capacity(self.bindings.len());
        let mut seen_variables = BTreeSet::new();

        for col in base_handle
            .metadata
            .keys
            .iter()
            .chain(base_handle.metadata.non_keys.iter())
        {
            if let Some(arg) = self.bindings.remove(&col.name) {
                match arg {
                    Expr::Binding { var, .. } => {
                        if var.is_ignored_symbol() {
                            bindings.push(r#gen.next_ignored(var.span));
                        } else if seen_variables.insert(var.clone()) {
                            bindings.push(var);
                        } else {
                            let span = var.span;
                            let dup = r#gen.next(span);
                            let unif = NormalFormAtom::Unification(Unification {
                                binding: dup.clone(),
                                expr: Expr::Binding {
                                    var,
                                    tuple_pos: None,
                                },
                                one_many_unif: false,
                                span,
                            });
                            conj.push(unif);
                            bindings.push(dup);
                        }
                    }
                    expr => {
                        let span = expr.span();
                        let kw = r#gen.next(span);
                        bindings.push(kw.clone());
                        let unif = NormalFormAtom::Unification(Unification {
                            binding: kw,
                            expr,
                            one_many_unif: false,
                            span,
                        });
                        conj.push(unif)
                    }
                }
            } else {
                bindings.push(r#gen.next_ignored(self.span));
            }
        }

        if let Some((name, _)) = self.bindings.pop_first() {
            return Err(FieldNotFoundSnafu {
                message: format!(
                    "stored relation '{}' does not have field '{}'",
                    self.relation.name, name
                ),
            }
            .build()
            .into());
        }

        let query = match self.parameters.remove("query").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `query` is required for LSH search".to_string(),
                }
                .build(),
            )
        })? {
            Expr::Binding { var, .. } => var,
            expr => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                kw
            }
        };

        let k = match self.parameters.remove("k") {
            None => None,
            Some(k_expr) => {
                let k = k_expr.eval_to_const()?;
                let k = k.get_int().ok_or_else(|| {
                    InternalError::from(
                        InvalidValueSnafu {
                            message: "Expected positive integer for `k`".to_string(),
                        }
                        .build(),
                    )
                })?;
                if k <= 0 {
                    return Err(InvalidValueSnafu {
                        message: "Expected positive integer for `k`".to_string(),
                    }
                    .build()
                    .into());
                }
                Some(k as usize)
            }
        };

        let filter = self.parameters.remove("filter");

        if !self.parameters.is_empty() {
            return Err(InvalidValueSnafu {
                message: format!(
                    "Extra parameters for LSH search: {:?}",
                    self.parameters.keys().collect::<Vec<_>>()
                ),
            }
            .build()
            .into());
        }

        conj.push(NormalFormAtom::LshSearch(LshSearch {
            base_handle,
            idx_handle,
            manifest,
            bindings,
            k,
            query,
            span: self.span,
            filter,
        }));

        Ok(Disjunction::conj(conj))
    }
    fn normalize_fts(
        mut self,
        base_handle: RelationHandle,
        idx_handle: RelationHandle,
        manifest: FtsIndexManifest,
        r#gen: &mut TempSymbGen,
    ) -> Result<Disjunction> {
        let mut conj = Vec::with_capacity(self.bindings.len() + 8);
        let mut bindings = Vec::with_capacity(self.bindings.len());
        let mut seen_variables = BTreeSet::new();

        for col in base_handle
            .metadata
            .keys
            .iter()
            .chain(base_handle.metadata.non_keys.iter())
        {
            if let Some(arg) = self.bindings.remove(&col.name) {
                match arg {
                    Expr::Binding { var, .. } => {
                        if var.is_ignored_symbol() {
                            bindings.push(r#gen.next_ignored(var.span));
                        } else if seen_variables.insert(var.clone()) {
                            bindings.push(var);
                        } else {
                            let span = var.span;
                            let dup = r#gen.next(span);
                            let unif = NormalFormAtom::Unification(Unification {
                                binding: dup.clone(),
                                expr: Expr::Binding {
                                    var,
                                    tuple_pos: None,
                                },
                                one_many_unif: false,
                                span,
                            });
                            conj.push(unif);
                            bindings.push(dup);
                        }
                    }
                    expr => {
                        let span = expr.span();
                        let kw = r#gen.next(span);
                        bindings.push(kw.clone());
                        let unif = NormalFormAtom::Unification(Unification {
                            binding: kw,
                            expr,
                            one_many_unif: false,
                            span,
                        });
                        conj.push(unif)
                    }
                }
            } else {
                bindings.push(r#gen.next_ignored(self.span));
            }
        }

        if let Some((name, _)) = self.bindings.pop_first() {
            return Err(FieldNotFoundSnafu {
                message: format!(
                    "stored relation '{}' does not have field '{}'",
                    self.relation.name, name
                ),
            }
            .build()
            .into());
        }

        let query = match self.parameters.remove("query").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `query` is required for FTS search".to_string(),
                }
                .build(),
            )
        })? {
            Expr::Binding { var, .. } => var,
            expr => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                kw
            }
        };

        let k_expr = self.parameters.remove("k").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `k` is required for FTS search".to_string(),
                }
                .build(),
            )
        })?;
        let k = k_expr.eval_to_const()?;
        let k = k.get_int().ok_or_else(|| {
            InternalError::from(
                InvalidValueSnafu {
                    message: "Expected positive integer for `k`".to_string(),
                }
                .build(),
            )
        })?;
        if k <= 0 {
            return Err(InvalidValueSnafu {
                message: "Expected positive integer for `k`".to_string(),
            }
            .build()
            .into());
        }

        let score_kind_expr = self.parameters.remove("score_kind");
        let score_kind = match score_kind_expr {
            Some(expr) => {
                let r = expr.eval_to_const()?;
                let r = r.get_str().ok_or_else(|| {
                    InternalError::from(
                        InvalidValueSnafu {
                            message: "Score kind for FTS must be a string".to_string(),
                        }
                        .build(),
                    )
                })?;

                match r {
                    "tf_idf" => FtsScoreKind::TfIdf,
                    "tf" => FtsScoreKind::Tf,
                    "bm25" => FtsScoreKind::Bm25,
                    s => {
                        return Err(InvalidValueSnafu {
                            message: format!("Unknown score kind for FTS: {}", s),
                        }
                        .build()
                        .into());
                    }
                }
            }
            None => FtsScoreKind::TfIdf,
        };

        let filter = self.parameters.remove("filter");

        let bind_score = match self.parameters.remove("bind_score") {
            None => None,
            Some(Expr::Binding { var, .. }) => Some(var),
            Some(expr) => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                Some(kw)
            }
        };

        if !self.parameters.is_empty() {
            return Err(InvalidValueSnafu {
                message: format!(
                    "Unknown parameters for FTS: {:?}",
                    self.parameters.keys().collect::<Vec<_>>()
                ),
            }
            .build()
            .into());
        }

        conj.push(NormalFormAtom::FtsSearch(FtsSearch {
            base_handle,
            idx_handle,
            manifest,
            bindings,
            k: k as usize,
            query,
            score_kind,
            bind_score,
            // lax_mode,
            // k1,
            // b,
            filter,
            span: self.span,
        }));

        Ok(Disjunction::conj(conj))
    }
    fn normalize_hnsw(
        mut self,
        base_handle: RelationHandle,
        idx_handle: RelationHandle,
        manifest: HnswIndexManifest,
        r#gen: &mut TempSymbGen,
    ) -> Result<Disjunction> {
        let mut conj = Vec::with_capacity(self.bindings.len() + 8);
        let mut bindings = Vec::with_capacity(self.bindings.len());
        let mut seen_variables = BTreeSet::new();

        for col in base_handle
            .metadata
            .keys
            .iter()
            .chain(base_handle.metadata.non_keys.iter())
        {
            if let Some(arg) = self.bindings.remove(&col.name) {
                match arg {
                    Expr::Binding { var, .. } => {
                        if var.is_ignored_symbol() {
                            bindings.push(r#gen.next_ignored(var.span));
                        } else if seen_variables.insert(var.clone()) {
                            bindings.push(var);
                        } else {
                            let span = var.span;
                            let dup = r#gen.next(span);
                            let unif = NormalFormAtom::Unification(Unification {
                                binding: dup.clone(),
                                expr: Expr::Binding {
                                    var,
                                    tuple_pos: None,
                                },
                                one_many_unif: false,
                                span,
                            });
                            conj.push(unif);
                            bindings.push(dup);
                        }
                    }
                    expr => {
                        let span = expr.span();
                        let kw = r#gen.next(span);
                        bindings.push(kw.clone());
                        let unif = NormalFormAtom::Unification(Unification {
                            binding: kw,
                            expr,
                            one_many_unif: false,
                            span,
                        });
                        conj.push(unif)
                    }
                }
            } else {
                bindings.push(r#gen.next_ignored(self.span));
            }
        }

        if let Some((name, _)) = self.bindings.pop_first() {
            return Err(FieldNotFoundSnafu {
                message: format!(
                    "stored relation '{}' does not have field '{}'",
                    self.relation.name, name
                ),
            }
            .build()
            .into());
        }

        let query = match self.parameters.remove("query").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `query` is required for HNSW search".to_string(),
                }
                .build(),
            )
        })? {
            Expr::Binding { var, .. } => var,
            expr => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                kw
            }
        };

        let k_expr = self.parameters.remove("k").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `k` is required for HNSW search".to_string(),
                }
                .build(),
            )
        })?;
        let k = k_expr.eval_to_const()?;
        let k = k.get_int().ok_or_else(|| {
            InternalError::from(
                InvalidValueSnafu {
                    message: "Expected positive integer for `k`".to_string(),
                }
                .build(),
            )
        })?;
        if k <= 0 {
            return Err(InvalidValueSnafu {
                message: "Expected positive integer for `k`".to_string(),
            }
            .build()
            .into());
        }

        let ef_expr = self.parameters.remove("ef").ok_or_else(|| {
            InternalError::from(
                FieldNotFoundSnafu {
                    message: "Field `ef` is required for HNSW search".to_string(),
                }
                .build(),
            )
        })?;
        let ef = ef_expr.eval_to_const()?;
        let ef = ef.get_int().ok_or_else(|| {
            InternalError::from(
                InvalidValueSnafu {
                    message: "Expected positive integer for `ef`".to_string(),
                }
                .build(),
            )
        })?;
        if ef <= 0 {
            return Err(InvalidValueSnafu {
                message: "Expected positive integer for `ef`".to_string(),
            }
            .build()
            .into());
        }

        let radius_expr = self.parameters.remove("radius");
        let radius = match radius_expr {
            Some(expr) => {
                let r = expr.eval_to_const()?;
                let r = r.get_float().ok_or_else(|| {
                    InternalError::from(
                        InvalidValueSnafu {
                            message: "Expected positive float for `radius`".to_string(),
                        }
                        .build(),
                    )
                })?;
                if r <= 0.0 || r.is_nan() {
                    return Err(InvalidValueSnafu {
                        message: "Expected positive float for `radius`".to_string(),
                    }
                    .build()
                    .into());
                }
                Some(r)
            }
            None => None,
        };

        let filter = self.parameters.remove("filter");

        let bind_field = match self.parameters.remove("bind_field") {
            None => None,
            Some(Expr::Binding { var, .. }) => Some(var),
            Some(expr) => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                Some(kw)
            }
        };

        let bind_field_idx = match self.parameters.remove("bind_field_idx") {
            None => None,
            Some(Expr::Binding { var, .. }) => Some(var),
            Some(expr) => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                Some(kw)
            }
        };

        let bind_distance = match self.parameters.remove("bind_distance") {
            None => None,
            Some(Expr::Binding { var, .. }) => Some(var),
            Some(expr) => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                Some(kw)
            }
        };

        let bind_vector = match self.parameters.remove("bind_vector") {
            None => None,
            Some(Expr::Binding { var, .. }) => Some(var),
            Some(expr) => {
                let span = expr.span();
                let kw = r#gen.next(span);
                let unif = NormalFormAtom::Unification(Unification {
                    binding: kw.clone(),
                    expr,
                    one_many_unif: false,
                    span,
                });
                conj.push(unif);
                Some(kw)
            }
        };

        if !self.parameters.is_empty() {
            return Err(InvalidValueSnafu {
                message: format!("Unexpected parameters for HNSW: {:?}", self.parameters),
            }
            .build()
            .into());
        }

        conj.push(NormalFormAtom::HnswSearch(HnswSearch {
            base_handle,
            idx_handle,
            manifest,
            bindings,
            k: k as usize,
            ef: ef as usize,
            query,
            bind_field,
            bind_field_idx,
            bind_distance,
            bind_vector,
            radius,
            filter,
            span: self.span,
        }));

        Ok(Disjunction::conj(conj))
    }
    pub(crate) fn normalize(
        self,
        r#gen: &mut TempSymbGen,
        tx: &SessionTx<'_>,
    ) -> Result<Disjunction> {
        let base_handle = tx.get_relation(&self.relation, false)?;
        if base_handle.access_level < AccessLevel::ReadOnly {
            return Err(InsufficientAccessSnafu {
                message: format!(
                    "Cannot read rows from '{}': access level insufficient ({:?})",
                    base_handle.name, base_handle.access_level
                ),
            }
            .build()
            .into());
        }
        if let Some((idx_handle, manifest)) =
            base_handle.hnsw_indices.get(&self.index.name).cloned()
        {
            return self.normalize_hnsw(base_handle, idx_handle, manifest, r#gen);
        }
        if let Some((idx_handle, manifest)) = base_handle.fts_indices.get(&self.index.name).cloned()
        {
            return self.normalize_fts(base_handle, idx_handle, manifest, r#gen);
        }
        if let Some((idx_handle, _, manifest)) =
            base_handle.lsh_indices.get(&self.index.name).cloned()
        {
            return self.normalize_lsh(base_handle, idx_handle, manifest, r#gen);
        }
        Err(FieldNotFoundSnafu {
            message: format!(
                "Index '{}' not found on relation '{}'",
                self.index, self.relation
            ),
        }
        .build()
        .into())
    }
}

impl Debug for InputAtom {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for InputAtom {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            InputAtom::Rule {
                inner: InputRuleApplyAtom { name, args, .. },
            } => {
                write!(f, "{name}")?;
                f.debug_list().entries(args).finish()?;
            }
            InputAtom::NamedFieldRelation {
                inner: InputNamedFieldRelationApplyAtom { name, args, .. },
            } => {
                f.write_str("*")?;
                let mut sf = f.debug_struct(name);
                for (k, v) in args {
                    sf.field(k, v);
                }
                sf.finish()?;
            }
            InputAtom::Relation {
                inner: InputRelationApplyAtom { name, args, .. },
            } => {
                write!(f, ":{name}")?;
                f.debug_list().entries(args).finish()?;
            }
            InputAtom::Search { inner } => {
                write!(f, "~{}:{}{{", inner.relation, inner.index)?;
                for (binding, expr) in &inner.bindings {
                    write!(f, "{binding}: {expr}, ")?;
                }
                write!(f, "| ")?;
                for (k, v) in inner.parameters.iter() {
                    write!(f, "{k}: {v}, ")?;
                }
                write!(f, "}}")?;
            }
            InputAtom::Predicate { inner } => {
                write!(f, "{inner}")?;
            }
            InputAtom::Negation { inner, .. } => {
                write!(f, "not {inner}")?;
            }
            InputAtom::Conjunction { inner, .. } => {
                for (i, a) in inner.iter().enumerate() {
                    if i > 0 {
                        write!(f, " and ")?;
                    }
                    write!(f, "({a})")?;
                }
            }
            InputAtom::Disjunction { inner, .. } => {
                for (i, a) in inner.iter().enumerate() {
                    if i > 0 {
                        write!(f, " or ")?;
                    }
                    write!(f, "({a})")?;
                }
            }
            InputAtom::Unification {
                inner:
                    Unification {
                        binding,
                        expr,
                        one_many_unif,
                        ..
                    },
            } => {
                write!(f, "{binding}")?;
                if *one_many_unif {
                    write!(f, " in ")?;
                } else {
                    write!(f, " = ")?;
                }
                write!(f, "{expr}")?;
            }
        }
        Ok(())
    }
}

impl InputAtom {
    pub(crate) fn span(&self) -> SourceSpan {
        match self {
            InputAtom::Negation { span, .. }
            | InputAtom::Conjunction { span, .. }
            | InputAtom::Disjunction { span, .. } => *span,
            InputAtom::Rule { inner, .. } => inner.span,
            InputAtom::NamedFieldRelation { inner, .. } => inner.span,
            InputAtom::Relation { inner, .. } => inner.span,
            InputAtom::Predicate { inner, .. } => inner.span(),
            InputAtom::Unification { inner, .. } => inner.span,
            InputAtom::Search { inner, .. } => inner.span,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Unification {
    pub(crate) binding: Symbol,
    pub(crate) expr: Expr,
    pub(crate) one_many_unif: bool,
    pub(crate) span: SourceSpan,
}

impl Unification {
    pub(crate) fn is_const(&self) -> bool {
        matches!(self.expr, Expr::Const { .. })
    }
    pub(crate) fn bindings_in_expr(&self) -> Result<BTreeSet<Symbol>> {
        self.expr.bindings()
    }
}
