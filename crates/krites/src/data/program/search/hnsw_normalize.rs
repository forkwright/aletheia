//! SearchInput normalize_hnsw and normalize implementations.
use std::collections::BTreeSet;

use crate::data::error::*;
use crate::data::expr::Expr;
use crate::error::{InternalError, InternalResult as Result};
use crate::query::logical::Disjunction;
use crate::runtime::hnsw::HnswIndexManifest;
use crate::runtime::relation::{AccessLevel, RelationHandle};
use crate::runtime::transact::SessionTx;

use super::super::atoms::*;
use super::super::types::*;
use super::{HnswSearch, SearchInput, Unification};

impl SearchInput {
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
