//! SearchInput normalize_lsh and normalize_fts implementations.
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeSet;

use crate::engine::data::error::*;
use crate::engine::data::expr::Expr;
use crate::engine::error::{InternalError, InternalResult as Result};
use crate::engine::fts::FtsIndexManifest;
use crate::engine::query::logical::Disjunction;
use crate::engine::runtime::minhash_lsh::{LshSearch, MinHashLshIndexManifest};
use crate::engine::runtime::relation::RelationHandle;

use super::super::atoms::*;
use super::super::types::*;
use super::{FtsScoreKind, FtsSearch, SearchInput, Unification};

impl SearchInput {
    pub(crate) fn normalize_lsh(
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
    pub(crate) fn normalize_fts(
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
            filter,
            span: self.span,
        }));

        Ok(Disjunction::conj(conj))
    }
}
