//! Relational algebra operators.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Formatter};

use crate::engine::error::InternalResult as Result;
use crate::engine::query::error::*;
use itertools::Itertools;

use crate::engine::data::expr::{Bytecode, Expr, eval_bytecode_pred};
use crate::engine::data::program::{FtsSearch, HnswSearch, MagicSymbol};
use crate::engine::data::relation::{ColType, NullableColType};
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::TupleIter;
use crate::engine::data::value::ValidityTs;
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::minhash_lsh::LshSearch;
use crate::engine::runtime::relation::RelationHandle;
use crate::engine::runtime::temp_store::EpochStore;
use crate::engine::runtime::transact::SessionTx;

mod filter;
mod join;
mod project;
mod search;
mod sort;
mod sources;

pub(crate) use filter::FilteredRA;
pub(crate) use join::{InnerJoin, Joiner, NegJoin};
pub(crate) use project::UnificationRA;
pub(crate) use search::{FtsSearchRA, HnswSearchRA, LshSearchRA};
pub(crate) use sort::ReorderRA;
pub(crate) use sources::{InlineFixedRA, StoredRA, StoredWithValidityRA, TempStoreRA};

pub(crate) enum RelAlgebra {
    Fixed(InlineFixedRA),
    TempStore(TempStoreRA),
    Stored(StoredRA),
    StoredWithValidity(StoredWithValidityRA),
    Join(Box<InnerJoin>),
    NegJoin(Box<NegJoin>),
    Reorder(ReorderRA),
    Filter(FilteredRA),
    Unification(UnificationRA),
    HnswSearch(HnswSearchRA),
    FtsSearch(FtsSearchRA),
    LshSearch(LshSearchRA),
}

impl RelAlgebra {
    pub(crate) fn span(&self) -> SourceSpan {
        match self {
            RelAlgebra::Fixed(i) => i.span,
            RelAlgebra::TempStore(i) => i.span,
            RelAlgebra::Stored(i) => i.span,
            RelAlgebra::Join(i) => i.span,
            RelAlgebra::NegJoin(i) => i.span,
            RelAlgebra::Reorder(i) => i.relation.span(),
            RelAlgebra::Filter(i) => i.span,
            RelAlgebra::Unification(i) => i.span,
            RelAlgebra::StoredWithValidity(i) => i.span,
            RelAlgebra::HnswSearch(i) => i.hnsw_search.span,
            RelAlgebra::FtsSearch(i) => i.fts_search.span,
            RelAlgebra::LshSearch(i) => i.lsh_search.span,
        }
    }
}

pub(crate) fn flatten_err<T>(
    v: std::result::Result<Result<T>, crate::engine::error::InternalError>,
) -> Result<T> {
    match v {
        Err(e) => Err(e),
        Ok(Err(e)) => Err(e),
        Ok(Ok(v)) => Ok(v),
    }
}

pub(crate) fn invert_option_err<T>(v: Result<Option<T>>) -> Option<Result<T>> {
    match v {
        Err(e) => Some(Err(e)),
        Ok(None) => None,
        Ok(Some(v)) => Some(Ok(v)),
    }
}

pub(crate) fn filter_iter(
    filters_bytecodes: Vec<(Vec<Bytecode>, SourceSpan)>,
    it: impl Iterator<Item = Result<crate::engine::data::tuple::Tuple>>,
) -> impl Iterator<Item = Result<crate::engine::data::tuple::Tuple>> {
    use tracing::debug;
    let mut stack = vec![];
    it.filter_map_ok(
        move |t| -> Option<Result<crate::engine::data::tuple::Tuple>> {
            for (p, span) in filters_bytecodes.iter() {
                match eval_bytecode_pred(p, &t, &mut stack, *span) {
                    Ok(false) => return None,
                    Err(e) => {
                        debug!("{:?}", t);
                        return Some(Err(e));
                    }
                    // NOTE: filter passed, continue to next
                    Ok(true) => {}
                }
            }
            Some(Ok(t))
        },
    )
    .map(flatten_err)
}

pub(crate) fn get_eliminate_indices(
    bindings: &[Symbol],
    eliminate: &BTreeSet<Symbol>,
) -> BTreeSet<usize> {
    bindings
        .iter()
        .enumerate()
        .filter_map(|(idx, kw)| {
            if eliminate.contains(kw) {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
}

pub(crate) fn eliminate_from_tuple(
    mut ret: crate::engine::data::tuple::Tuple,
    eliminate_indices: &BTreeSet<usize>,
) -> crate::engine::data::tuple::Tuple {
    if !eliminate_indices.is_empty() {
        ret = ret
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                if eliminate_indices.contains(&i) {
                    None
                } else {
                    Some(v)
                }
            })
            .collect_vec();
    }
    ret
}

pub(crate) fn join_is_prefix(right_join_indices: &[usize]) -> bool {
    // WHY: We do not consider partial index match to be "prefix", e.g. [a, u => c]
    // with a, c bound and u unbound is not "prefix", as it is not clear that
    // using prefix scanning in this case will really save us computation.
    let mut indices = right_join_indices.to_vec();
    indices.sort();
    let l = indices.len();
    indices.into_iter().eq(0..l)
}

struct BindingFormatter(Vec<Symbol>);

impl Debug for BindingFormatter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = self.0.iter().map(|f| f.to_string()).join(", ");
        write!(f, "[{s}]")
    }
}

impl Debug for RelAlgebra {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let bindings = BindingFormatter(self.bindings_after_eliminate());
        match self {
            RelAlgebra::Fixed(r) => {
                if r.bindings.is_empty() && r.data.len() == 1 {
                    f.write_str("Unit")
                } else if r.data.len() == 1 {
                    f.debug_tuple("Singlet")
                        .field(&bindings)
                        .field(r.data.first().expect("data.len() == 1 checked above"))
                        .finish()
                } else {
                    f.debug_tuple("Fixed")
                        .field(&bindings)
                        .field(&["..."])
                        .finish()
                }
            }
            RelAlgebra::TempStore(r) => f
                .debug_tuple("TempStore")
                .field(&bindings)
                .field(&r.storage_key)
                .field(&r.filters)
                .finish(),
            RelAlgebra::Stored(r) => f
                .debug_tuple("Stored")
                .field(&bindings)
                .field(&r.storage.name)
                .field(&r.filters)
                .finish(),
            RelAlgebra::HnswSearch(s) => f
                .debug_tuple("HnswSearch")
                .field(&bindings)
                .field(&s.hnsw_search.idx_handle.name)
                .finish(),
            RelAlgebra::FtsSearch(s) => f
                .debug_tuple("FtsSearch")
                .field(&bindings)
                .field(&s.fts_search.idx_handle.name)
                .finish(),
            RelAlgebra::LshSearch(s) => f
                .debug_tuple("LshSearch")
                .field(&bindings)
                .field(&s.lsh_search.idx_handle.name)
                .finish(),
            RelAlgebra::StoredWithValidity(r) => f
                .debug_tuple("StoredWithValidity")
                .field(&bindings)
                .field(&r.storage.name)
                .field(&r.filters)
                .field(&r.valid_at)
                .finish(),
            RelAlgebra::Join(r) => {
                if r.left.is_unit() {
                    r.right.fmt(f)
                } else {
                    f.debug_tuple("Join")
                        .field(&bindings)
                        .field(&r.joiner)
                        .field(&r.left)
                        .field(&r.right)
                        .finish()
                }
            }
            RelAlgebra::NegJoin(r) => f
                .debug_tuple("NegJoin")
                .field(&bindings)
                .field(&r.joiner)
                .field(&r.left)
                .field(&r.right)
                .finish(),
            RelAlgebra::Reorder(r) => f
                .debug_tuple("Reorder")
                .field(&r.new_order)
                .field(&r.relation)
                .finish(),
            RelAlgebra::Filter(r) => f
                .debug_tuple("Filter")
                .field(&bindings)
                .field(&r.filters)
                .field(&r.parent)
                .finish(),
            RelAlgebra::Unification(r) => f
                .debug_tuple("Unify")
                .field(&bindings)
                .field(&r.parent)
                .field(&r.binding)
                .field(&r.expr)
                .finish(),
        }
    }
}

impl RelAlgebra {
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        match self {
            // NOTE: fixed relations have no binding indices to fill
            RelAlgebra::Fixed(_) => {}
            RelAlgebra::TempStore(d) => {
                d.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::Stored(v) => {
                v.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::HnswSearch(s) => {
                s.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::FtsSearch(s) => {
                s.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::LshSearch(s) => {
                s.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::StoredWithValidity(v) => {
                v.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::Reorder(r) => {
                r.relation.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::Filter(f) => {
                f.parent.fill_binding_indices_and_compile()?;
                f.fill_binding_indices_and_compile()?
            }
            RelAlgebra::NegJoin(r) => {
                r.left.fill_binding_indices_and_compile()?;
            }
            RelAlgebra::Unification(u) => {
                u.parent.fill_binding_indices_and_compile()?;
                u.fill_binding_indices_and_compile()?
            }
            RelAlgebra::Join(r) => {
                r.left.fill_binding_indices_and_compile()?;
                r.right.fill_binding_indices_and_compile()?;
            }
        }
        Ok(())
    }
    pub(crate) fn unit(span: SourceSpan) -> Self {
        Self::Fixed(InlineFixedRA::unit(span))
    }
    pub(crate) fn is_unit(&self) -> bool {
        if let RelAlgebra::Fixed(r) = self {
            r.bindings.is_empty() && r.data.len() == 1
        } else {
            false
        }
    }
    pub(crate) fn cartesian_join(self, right: RelAlgebra, span: SourceSpan) -> Self {
        self.join(right, vec![], vec![], span)
    }
    pub(crate) fn derived(
        bindings: Vec<Symbol>,
        storage_key: MagicSymbol,
        span: SourceSpan,
    ) -> Self {
        Self::TempStore(TempStoreRA {
            bindings,
            storage_key,
            filters: vec![],
            filters_bytecodes: vec![],
            span,
        })
    }
    pub(crate) fn relation(
        bindings: Vec<Symbol>,
        storage: RelationHandle,
        span: SourceSpan,
        validity: Option<ValidityTs>,
    ) -> Result<Self> {
        match validity {
            None => Ok(Self::Stored(StoredRA {
                bindings,
                storage,
                filters: vec![],
                filters_bytecodes: vec![],
                span,
            })),
            Some(vld) => {
                let last_key = storage.metadata.keys.last().ok_or_else(|| {
                    InvalidTimeTravelSnafu {
                        message: "relation has no key columns",
                    }
                    .build()
                })?;
                if last_key.typing
                    != (NullableColType {
                        coltype: ColType::Validity,
                        nullable: false,
                    })
                {
                    return Err(InvalidTimeTravelSnafu {
                        message: "Invalid time travel on relation",
                    }
                    .build()
                    .into());
                };
                Ok(Self::StoredWithValidity(StoredWithValidityRA {
                    bindings,
                    storage,
                    filters: vec![],
                    filters_bytecodes: vec![],
                    valid_at: vld,
                    span,
                }))
            }
        }
    }
    pub(crate) fn reorder(self, new_order: Vec<Symbol>) -> Self {
        Self::Reorder(ReorderRA {
            relation: Box::new(self),
            new_order,
        })
    }
    pub(crate) fn filter(self, filter: Expr) -> Result<Self> {
        Ok(match self {
            s @ (RelAlgebra::Fixed(_)
            | RelAlgebra::Reorder(_)
            | RelAlgebra::NegJoin(_)
            | RelAlgebra::Unification(_)
            | RelAlgebra::HnswSearch(_)
            | RelAlgebra::FtsSearch(_)
            | RelAlgebra::LshSearch(_)) => {
                let span = filter.span();
                RelAlgebra::Filter(FilteredRA {
                    parent: Box::new(s),
                    filters: vec![filter],
                    filters_bytecodes: vec![],
                    to_eliminate: Default::default(),
                    span,
                })
            }
            RelAlgebra::Filter(FilteredRA {
                parent,
                filters: mut pred,
                filters_bytecodes,
                to_eliminate,
                span,
            }) => {
                pred.push(filter);
                RelAlgebra::Filter(FilteredRA {
                    parent,
                    filters: pred,
                    filters_bytecodes,
                    to_eliminate,
                    span,
                })
            }
            RelAlgebra::TempStore(TempStoreRA {
                bindings,
                storage_key,
                mut filters,
                filters_bytecodes: filters_asm,
                span,
            }) => {
                filters.push(filter);
                RelAlgebra::TempStore(TempStoreRA {
                    bindings,
                    storage_key,
                    filters,
                    filters_bytecodes: filters_asm,
                    span,
                })
            }
            RelAlgebra::Stored(StoredRA {
                bindings,
                storage,
                mut filters,
                filters_bytecodes,
                span,
            }) => {
                filters.push(filter);
                RelAlgebra::Stored(StoredRA {
                    bindings,
                    storage,
                    filters,
                    filters_bytecodes,
                    span,
                })
            }
            RelAlgebra::StoredWithValidity(StoredWithValidityRA {
                bindings,
                storage,
                mut filters,
                filters_bytecodes: filter_bytecodes,
                span,
                valid_at,
            }) => {
                filters.push(filter);
                RelAlgebra::StoredWithValidity(StoredWithValidityRA {
                    bindings,
                    storage,
                    filters,
                    span,
                    valid_at,
                    filters_bytecodes: filter_bytecodes,
                })
            }
            RelAlgebra::Join(inner) => {
                let filters = filter.to_conjunction();
                let left_bindings: BTreeSet<Symbol> =
                    inner.left.bindings_before_eliminate().into_iter().collect();
                let right_bindings: BTreeSet<Symbol> = inner
                    .right
                    .bindings_before_eliminate()
                    .into_iter()
                    .collect();
                let mut remaining = vec![];
                let InnerJoin {
                    mut left,
                    mut right,
                    joiner,
                    to_eliminate,
                    span,
                    ..
                } = *inner;
                for filter in filters {
                    let f_bindings = filter.bindings()?;
                    if f_bindings.is_subset(&left_bindings) {
                        left = left.filter(filter)?;
                    } else if f_bindings.is_subset(&right_bindings) {
                        right = right.filter(filter)?;
                    } else {
                        remaining.push(filter);
                    }
                }
                let mut joined = RelAlgebra::Join(Box::new(InnerJoin {
                    left,
                    right,
                    joiner,
                    to_eliminate,
                    span,
                }));
                if !remaining.is_empty() {
                    joined = RelAlgebra::Filter(FilteredRA {
                        parent: Box::new(joined),
                        filters: remaining,
                        filters_bytecodes: vec![],
                        to_eliminate: Default::default(),
                        span,
                    });
                }
                joined
            }
        })
    }
    pub(crate) fn unify(
        self,
        binding: Symbol,
        expr: Expr,
        is_multi: bool,
        span: SourceSpan,
    ) -> Self {
        RelAlgebra::Unification(UnificationRA {
            parent: Box::new(self),
            binding,
            expr,
            expr_bytecode: vec![],
            is_multi,
            to_eliminate: Default::default(),
            span,
        })
    }
    pub(crate) fn hnsw_search(
        self,
        hnsw_search: HnswSearch,
        own_bindings: Vec<Symbol>,
    ) -> Result<Self> {
        Ok(Self::HnswSearch(HnswSearchRA {
            parent: Box::new(self),
            hnsw_search,
            filter_bytecode: None,
            own_bindings,
        }))
    }
    pub(crate) fn fts_search(
        self,
        fts_search: FtsSearch,
        own_bindings: Vec<Symbol>,
    ) -> Result<Self> {
        Ok(Self::FtsSearch(FtsSearchRA {
            parent: Box::new(self),
            fts_search,
            filter_bytecode: None,
            own_bindings,
        }))
    }
    pub(crate) fn lsh_search(
        self,
        fts_search: LshSearch,
        own_bindings: Vec<Symbol>,
    ) -> Result<Self> {
        Ok(Self::LshSearch(LshSearchRA {
            parent: Box::new(self),
            lsh_search: fts_search,
            filter_bytecode: None,
            own_bindings,
        }))
    }
    pub(crate) fn join(
        self,
        right: RelAlgebra,
        left_keys: Vec<Symbol>,
        right_keys: Vec<Symbol>,
        span: SourceSpan,
    ) -> Self {
        RelAlgebra::Join(Box::new(InnerJoin {
            left: self,
            right,
            joiner: Joiner {
                left_keys,
                right_keys,
            },
            to_eliminate: Default::default(),
            span,
        }))
    }
    pub(crate) fn neg_join(
        self,
        right: RelAlgebra,
        left_keys: Vec<Symbol>,
        right_keys: Vec<Symbol>,
        span: SourceSpan,
    ) -> Self {
        RelAlgebra::NegJoin(Box::new(NegJoin {
            left: self,
            right,
            joiner: Joiner {
                left_keys,
                right_keys,
            },
            to_eliminate: Default::default(),
            span,
        }))
    }
}

impl RelAlgebra {
    pub(crate) fn eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        match self {
            RelAlgebra::Fixed(r) => r.do_eliminate_temp_vars(used),
            RelAlgebra::TempStore(_r) => Ok(()),
            RelAlgebra::Stored(_v) => Ok(()),
            RelAlgebra::StoredWithValidity(_v) => Ok(()),
            RelAlgebra::Join(r) => r.do_eliminate_temp_vars(used),
            RelAlgebra::Reorder(r) => r.relation.eliminate_temp_vars(used),
            RelAlgebra::Filter(r) => r.do_eliminate_temp_vars(used),
            RelAlgebra::NegJoin(r) => r.do_eliminate_temp_vars(used),
            RelAlgebra::Unification(r) => r.do_eliminate_temp_vars(used),
            RelAlgebra::HnswSearch(_) => Ok(()),
            RelAlgebra::FtsSearch(_) => Ok(()),
            RelAlgebra::LshSearch(_) => Ok(()),
        }
    }

    fn eliminate_set(&self) -> Option<&BTreeSet<Symbol>> {
        match self {
            RelAlgebra::Fixed(r) => Some(&r.to_eliminate),
            RelAlgebra::TempStore(_) => None,
            RelAlgebra::Stored(_) => None,
            RelAlgebra::StoredWithValidity(_) => None,
            RelAlgebra::Join(r) => Some(&r.to_eliminate),
            RelAlgebra::Reorder(_) => None,
            RelAlgebra::Filter(r) => Some(&r.to_eliminate),
            RelAlgebra::NegJoin(r) => Some(&r.to_eliminate),
            RelAlgebra::Unification(u) => Some(&u.to_eliminate),
            RelAlgebra::HnswSearch(_) => None,
            RelAlgebra::FtsSearch(_) => None,
            RelAlgebra::LshSearch(_) => None,
        }
    }

    pub(crate) fn bindings_after_eliminate(&self) -> Vec<Symbol> {
        let ret = self.bindings_before_eliminate();
        if let Some(to_eliminate) = self.eliminate_set() {
            ret.into_iter()
                .filter(|kw| !to_eliminate.contains(kw))
                .collect()
        } else {
            ret
        }
    }

    pub(crate) fn bindings_before_eliminate(&self) -> Vec<Symbol> {
        match self {
            RelAlgebra::Fixed(f) => f.bindings.clone(),
            RelAlgebra::TempStore(d) => d.bindings.clone(),
            RelAlgebra::Stored(v) => v.bindings.clone(),
            RelAlgebra::StoredWithValidity(v) => v.bindings.clone(),
            RelAlgebra::Join(j) => j.bindings(),
            RelAlgebra::Reorder(r) => r.bindings(),
            RelAlgebra::Filter(r) => r.parent.bindings_after_eliminate(),
            RelAlgebra::NegJoin(j) => j.left.bindings_after_eliminate(),
            RelAlgebra::Unification(u) => {
                let mut bindings = u.parent.bindings_after_eliminate();
                bindings.push(u.binding.clone());
                bindings
            }
            RelAlgebra::HnswSearch(s) => {
                let mut bindings = s.parent.bindings_after_eliminate();
                bindings.extend_from_slice(&s.own_bindings);
                bindings
            }
            RelAlgebra::FtsSearch(s) => {
                let mut bindings = s.parent.bindings_after_eliminate();
                bindings.extend_from_slice(&s.own_bindings);
                bindings
            }
            RelAlgebra::LshSearch(s) => {
                let mut bindings = s.parent.bindings_after_eliminate();
                bindings.extend_from_slice(&s.own_bindings);
                bindings
            }
        }
    }
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        match self {
            RelAlgebra::Fixed(f) => Ok(Box::new(f.data.iter().map(|t| Ok(t.clone())))),
            RelAlgebra::TempStore(r) => r.iter(delta_rule, stores),
            RelAlgebra::Stored(v) => v.iter(tx),
            RelAlgebra::StoredWithValidity(v) => v.iter(tx),
            RelAlgebra::Join(j) => j.iter(tx, delta_rule, stores),
            RelAlgebra::Reorder(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::Filter(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::NegJoin(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::Unification(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::HnswSearch(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::FtsSearch(r) => r.iter(tx, delta_rule, stores),
            RelAlgebra::LshSearch(r) => r.iter(tx, delta_rule, stores),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::DbInstance;
    use crate::engine::data::value::DataValue;

    #[test]
    fn test_mat_join() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
        data[a, b] <- [[1, 2], [1, 3], [2, 3]]
        ?[x] := a = 3, data[x, a]
        "#,
            )
            .expect("RA query must succeed in test")
            .rows;
        assert_eq!(
            res,
            vec![vec![DataValue::from(1)], vec![DataValue::from(2)]]
        )
    }
}
