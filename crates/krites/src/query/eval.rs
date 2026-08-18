// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! Query plan evaluation.
#![expect(
    clippy::as_conversions,
    clippy::explicit_iter_loop,
    clippy::indexing_slicing,
    clippy::manual_let_else,
    clippy::match_wildcard_for_single_variants,
    clippy::mutable_key_type,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "engine-internal query evaluator — pass-by-value for Poison, indexing on validated bounds"
)]

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use itertools::Itertools;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use tracing::{debug, trace, warn};

use crate::data::aggr::Aggregation;
use crate::data::program::{MagicSymbol, NoEntryError};
use crate::data::symb::{PROG_ENTRY, Symbol};
use crate::data::tuple::Tuple;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::FixedRulePayload;
use crate::parse::SourceSpan;
use crate::query::compile::{
    AggrKind, CompiledProgram, CompiledRule, CompiledRuleSet, ContainedRuleMultiplicity,
};
use crate::query::context::QueryContext;
use crate::query::error::*;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::{EpochStore, MeetAggrStore, RegularTempStore, TempStore};

pub(crate) struct QueryLimiter {
    total: Option<usize>,
    skip: Option<usize>,
    counter: AtomicUsize,
}

impl QueryLimiter {
    pub(crate) fn incr_and_should_stop(&self) -> bool {
        if let Some(limit) = self.total {
            let old_count = self.counter.fetch_add(1, Ordering::Relaxed);
            old_count + 1 >= limit
        } else {
            false
        }
    }
    pub(crate) fn is_stopped(&self) -> bool {
        if let Some(limit) = self.total {
            self.counter.load(Ordering::Acquire) >= limit
        } else {
            false
        }
    }
    pub(crate) fn should_skip_next(&self) -> bool {
        match self.skip {
            None => false,
            Some(i) => i > self.counter.load(Ordering::Relaxed),
        }
    }
}

/// Epoch, wall-clock/kill, and (optionally) derived-row/work-unit budget for
/// one stratified evaluation, threaded through [`stratified_magic_evaluate`]
/// and [`semi_naive_magic_evaluate`].
///
/// [`Poison`] remains the primitive every `FixedRule` impl, `SessionTx`'s
/// HNSW/FTS/LSH index-search entry points (`SessionTx::poison`), and every
/// other cancellation-aware caller takes directly -- `QueryBudget` is the
/// accounting layer for the code driving a full stratified Datalog
/// evaluation. [`QueryBudget::poison`] hands out the same underlying
/// `Poison`, so a fixed rule or index search invoked mid-evaluation still
/// observes an explicit kill or wall-clock timeout through this budget.
///
/// NOTE: HNSW, FTS, and LSH searches check `Poison` once at their own entry
/// point (killed/timeout), but are not threaded into the row/work-unit
/// accounting this type adds -- that accounting happens only at the
/// semi-naive merge point below, since index searches are already bounded
/// by their own `k`/`ef` parameters rather than facing the unbounded
/// recursive-derivation risk `max_derived_rows` exists for.
///
/// `max_derived_rows` is reachable from the crate's public surface via
/// [`DbConfig::with_max_derived_rows`](crate::runtime::db::DbConfig::with_max_derived_rows),
/// applied to every query the configured `Db` runs.
#[derive(Clone)]
pub(crate) struct QueryBudget {
    poison: Poison,
    max_epochs: u32,
    max_derived_rows: Option<u64>,
    // WHY: Arc so every clone (one per stratum call, per rule-set fan-out)
    // shares one running total -- accounting must span the whole
    // evaluation, not reset per clone.
    rows_seen: Arc<AtomicU64>,
}

impl QueryBudget {
    /// Build a budget with no row/work-unit cap (unbounded, matching
    /// behavior from before this cap existed).
    pub(crate) fn new(poison: Poison, max_epochs: u32) -> Self {
        Self {
            poison,
            max_epochs,
            max_derived_rows: None,
            rows_seen: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Set a derived-row / work-unit cap for this evaluation.
    pub(crate) fn with_max_derived_rows(mut self, max_derived_rows: u64) -> Self {
        self.max_derived_rows = Some(max_derived_rows);
        self
    }

    /// The cooperative kill-switch/timeout handle, cloned for handing to
    /// code that only understands `Poison` (fixed rules, non-Datalog scans).
    pub(crate) fn poison(&self) -> Poison {
        self.poison.clone()
    }

    /// Maximum semi-naive evaluation epochs before `EpochLimitExceeded`.
    pub(crate) fn max_epochs(&self) -> u32 {
        self.max_epochs
    }

    /// Record newly-derived rows against the work-unit budget.
    ///
    /// A no-op (always `Ok`) when no cap is configured, so the default
    /// (unbounded) path pays no accounting cost.
    pub(crate) fn record_rows(&self, n: u64, stratum: usize) -> Result<()> {
        let Some(cap) = self.max_derived_rows else {
            return Ok(());
        };
        let total = self.rows_seen.fetch_add(n, Ordering::Relaxed) + n;
        if total > cap {
            warn!(
                derived_rows = total,
                max_derived_rows = cap,
                stratum,
                "evaluation exceeded row/work-unit budget"
            );
            RowLimitExceededSnafu {
                derived_rows: total,
                max_derived_rows: cap,
                stratum,
            }
            .fail()?;
        }
        Ok(())
    }
}

/// Evaluate a stratified Datalog program with magic sets optimization.
///
/// # Complexity
///
/// O(S * R * T) where S is the number of strata, R is the maximum rule count
/// per stratum, and T is the average tuple count processed. Semi-naive evaluation
/// converges in O(d) epochs where d is the data depth (typically small).
pub(crate) fn stratified_magic_evaluate(
    tx: &dyn QueryContext,
    strata: &[CompiledProgram],
    store_lifetimes: BTreeMap<MagicSymbol, usize>,
    total_num_to_take: Option<usize>,
    num_to_skip: Option<usize>,
    budget: QueryBudget,
) -> Result<(EpochStore, bool)> {
    let mut stores: BTreeMap<MagicSymbol, EpochStore> = BTreeMap::new();
    let mut early_return = false;
    for (stratum, cur_prog) in strata.iter().enumerate() {
        if stratum > 0 {
            stores.retain(|name, _| match store_lifetimes.get(name) {
                None => false,
                Some(n) => *n >= stratum,
            });
            trace!("{:?}", stores);
        }
        for (rule_name, rule_set) in cur_prog {
            let store =
                match rule_set.aggr_kind() {
                    AggrKind::None | AggrKind::Normal => EpochStore::new_normal(rule_set.arity()),
                    AggrKind::Meet => {
                        let rs = match rule_set {
                        CompiledRuleSet::Rules(rs) => rs,
                        _ => return Err(EvalFailedSnafu {
                            message: "meet aggregation requires compiled rules, not fixed rules",
                        }.build().into()),
                    };
                        EpochStore::new_meet(&rs[0].aggr)?
                    }
                };
            stores.insert(rule_name.clone(), store);
        }
        debug!("stratum {}", stratum);
        early_return = semi_naive_magic_evaluate(
            tx,
            cur_prog,
            &mut stores,
            total_num_to_take,
            num_to_skip,
            stratum,
            budget.clone(),
        )?;
    }
    let entry_symbol = MagicSymbol::Muggle {
        inner: Symbol::new(PROG_ENTRY, SourceSpan(0, 0)),
    };
    let ret_area = stores.remove(&entry_symbol).ok_or(NoEntryError)?;
    Ok((ret_area, early_return))
}

#[expect(
    clippy::needless_borrow,
    reason = "closure borrows &ruleset from pattern match binding"
)]
fn eval_compiled_ruleset_epoch_zero<'b>(
    tx: &dyn QueryContext,
    k: &'b MagicSymbol,
    compiled_ruleset: &CompiledRuleSet,
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    used_limiter: &AtomicBool,
    poison: Poison,
) -> Result<(&'b MagicSymbol, TempStore)> {
    let new_store = match compiled_ruleset {
        CompiledRuleSet::Rules(ruleset) => match compiled_ruleset.aggr_kind() {
            AggrKind::None => {
                let res =
                    initial_rule_non_aggr_eval(tx, k, &ruleset, stores, limiter, poison.clone())?;
                used_limiter.fetch_or(res.0, Ordering::Relaxed);
                res.1.wrap()
            }
            AggrKind::Normal => {
                let res = initial_rule_aggr_eval(tx, k, &ruleset, stores, limiter, poison.clone())?;
                used_limiter.fetch_or(res.0, Ordering::Relaxed);
                res.1.wrap()
            }
            AggrKind::Meet => {
                let new = initial_rule_meet_eval(tx, k, &ruleset, stores, poison.clone())?;
                new.wrap()
            }
        },
        CompiledRuleSet::Fixed(fixed) => {
            let fixed_impl = fixed.fixed_impl.as_ref();
            let mut out = RegularTempStore::default();
            let payload = FixedRulePayload {
                manifest: &fixed,
                stores,
                tx,
            };
            fixed_impl.run(payload, &mut out, poison.clone())?;
            out.wrap()
        }
    };
    Ok((k, new_store))
}

fn run_epoch_zero_rules<'b>(
    tx: &dyn QueryContext,
    prog: &'b CompiledProgram,
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    used_limiter: &AtomicBool,
    poison: Poison,
) -> Result<BTreeMap<&'b MagicSymbol, TempStore>> {
    let execution = |(k, compiled_ruleset): (_, &CompiledRuleSet)| {
        eval_compiled_ruleset_epoch_zero(
            tx,
            k,
            compiled_ruleset,
            stores,
            limiter,
            used_limiter,
            poison.clone(),
        )
    };

    let mut to_merge = BTreeMap::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let limiter_enabled = limiter.total.is_some();
        for res in prog
            .iter()
            .filter(|(symb, _)| limiter_enabled && symb.is_prog_entry())
            .map(execution)
        {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
            if limiter.is_stopped() {
                break;
            }
        }
        let execs = prog
            .par_iter()
            .filter(|(symb, _)| !(limiter_enabled && symb.is_prog_entry()))
            .map(execution);
        for res in execs.collect::<Vec<_>>() {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        for res in prog.iter().map(execution) {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
        }
    }
    Ok(to_merge)
}

#[expect(
    clippy::needless_borrow,
    reason = "closure borrows &ruleset from pattern match binding"
)]
fn eval_compiled_ruleset_subsequent_epoch<'b>(
    tx: &dyn QueryContext,
    k: &'b MagicSymbol,
    compiled_ruleset: &CompiledRuleSet,
    epoch: u32,
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    used_limiter: &AtomicBool,
    poison: Poison,
) -> Result<(&'b MagicSymbol, TempStore)> {
    let new_store = match compiled_ruleset {
        CompiledRuleSet::Rules(ruleset) => match compiled_ruleset.aggr_kind() {
            AggrKind::None => {
                let res = incremental_rule_non_aggr_eval(
                    tx,
                    k,
                    &ruleset,
                    epoch,
                    stores,
                    limiter,
                    poison.clone(),
                )?;
                used_limiter.fetch_or(res.0, Ordering::Relaxed);
                res.1.wrap()
            }
            AggrKind::Meet => {
                let new = incremental_rule_meet_eval(tx, k, &ruleset, stores, poison.clone())?;
                new.wrap()
            }
            AggrKind::Normal => RegularTempStore::default().wrap(),
        },
        CompiledRuleSet::Fixed(_) => RegularTempStore::default().wrap(),
    };
    Ok((k, new_store))
}

fn run_subsequent_epoch_rules<'b>(
    tx: &dyn QueryContext,
    prog: &'b CompiledProgram,
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    epoch: u32,
    limiter: &QueryLimiter,
    used_limiter: &AtomicBool,
    poison: Poison,
) -> Result<BTreeMap<&'b MagicSymbol, TempStore>> {
    let execution = |(k, compiled_ruleset): (_, &CompiledRuleSet)| {
        eval_compiled_ruleset_subsequent_epoch(
            tx,
            k,
            compiled_ruleset,
            epoch,
            stores,
            limiter,
            used_limiter,
            poison.clone(),
        )
    };

    let mut to_merge = BTreeMap::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let limiter_enabled = limiter.total.is_some();
        for res in prog
            .iter()
            .filter(|(symb, _)| limiter_enabled && symb.is_prog_entry())
            .map(execution)
        {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
            if limiter.is_stopped() {
                break;
            }
        }
        let execs = prog
            .par_iter()
            .filter(|(symb, _)| !(limiter_enabled && symb.is_prog_entry()))
            .map(execution);
        for res in execs.collect::<Vec<_>>() {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        for res in prog.iter().map(execution) {
            let (k, new_store) = res?;
            to_merge.insert(k, new_store);
        }
    }
    Ok(to_merge)
}

/// Returns `true` if early return is activated.
///
/// # Complexity
///
/// O(E * R * T) where E is epochs until fixpoint, R is rules, T is tuples.
/// Converges when no new facts are derived (monotonic).
fn semi_naive_magic_evaluate(
    tx: &dyn QueryContext,
    prog: &CompiledProgram,
    stores: &mut BTreeMap<MagicSymbol, EpochStore>,
    total_num_to_take: Option<usize>,
    num_to_skip: Option<usize>,
    stratum: usize,
    budget: QueryBudget,
) -> Result<bool> {
    let limiter = QueryLimiter {
        total: total_num_to_take,
        skip: num_to_skip,
        counter: 0.into(),
    };
    let used_limiter: AtomicBool = false.into();
    let max_epochs = budget.max_epochs();

    for epoch in 0..max_epochs {
        debug!("epoch {}", epoch);
        let borrowed_stores = stores as &BTreeMap<_, _>;
        let to_merge = if epoch == 0 {
            run_epoch_zero_rules(
                tx,
                prog,
                borrowed_stores,
                &limiter,
                &used_limiter,
                budget.poison(),
            )?
        } else {
            run_subsequent_epoch_rules(
                tx,
                prog,
                borrowed_stores,
                epoch,
                &limiter,
                &used_limiter,
                budget.poison(),
            )?
        };

        let mut changed = false;
        for (k, new_store) in to_merge {
            let old_store = stores.get_mut(k).ok_or_else(|| {
                crate::error::InternalError::from(
                    EvalFailedSnafu {
                        message: format!("epoch store not found for rule '{k}'"),
                    }
                    .build(),
                )
            })?;
            old_store.merge_in(new_store)?;
            let has_delta = old_store.has_delta();
            trace!("delta for {}: {}", k, has_delta);
            // WHY: only count rows when a cap is configured -- record_rows
            // is a no-op without one, so skip the delta_all_iter() walk
            // entirely on the default (unbounded) path.
            if has_delta && budget.max_derived_rows.is_some() {
                let delta_rows = old_store.delta_all_iter().count() as u64;
                budget.record_rows(delta_rows, stratum)?;
            }
            changed |= has_delta;
        }
        if !changed {
            return Ok(used_limiter.load(Ordering::Acquire));
        }
    }
    let rule_context = prog.keys().map(ToString::to_string).join(", ");
    warn!(
        epoch_count = max_epochs,
        max_epochs,
        stratum,
        rule_context = %rule_context,
        "semi-naive evaluation exceeded epoch limit"
    );
    EpochLimitExceededSnafu {
        epoch_count: max_epochs,
        max_epochs,
        stratum,
        rule_context,
    }
    .fail()?
}
/// Returns `true` if early return is activated.
///
/// # Complexity
///
/// O(R * T) where R is rules and T is tuples scanned. Aggregation-free rules
/// stream tuples directly without grouping overhead.
fn initial_rule_non_aggr_eval(
    tx: &dyn QueryContext,
    rule_symb: &MagicSymbol,
    ruleset: &[CompiledRule],
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    poison: Poison,
) -> Result<(bool, RegularTempStore)> {
    let mut out_store = RegularTempStore::default();
    let should_check_limit = limiter.total.is_some() && rule_symb.is_prog_entry();

    for (rule_n, rule) in ruleset.iter().enumerate() {
        debug!("initial calculation for rule {:?}.{}", rule_symb, rule_n);
        for item_res in rule.relation.iter(tx, None, stores)? {
            let item = item_res?;
            trace!("item for {:?}.{}: {:?} at {}", rule_symb, rule_n, item, 0);
            if should_check_limit {
                if !out_store.exists(&item) {
                    if limiter.should_skip_next() {
                        out_store.put_with_skip(item);
                    } else {
                        out_store.put(item);
                    }
                    if limiter.incr_and_should_stop() {
                        trace!("early stopping due to result count limit exceeded");
                        return Ok((true, out_store));
                    }
                }
            } else {
                out_store.put(item);
            }
        }
        poison.check()?;
    }

    Ok((should_check_limit, out_store))
}
/// Evaluate meet aggregation rules (initial epoch).
///
/// # Complexity
///
/// O(R * T * A) where R is rules, T is tuples, A is aggregation arity.
/// Meet aggregation combines partial results incrementally.
fn initial_rule_meet_eval(
    tx: &dyn QueryContext,
    rule_symb: &MagicSymbol,
    ruleset: &[CompiledRule],
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    poison: Poison,
) -> Result<MeetAggrStore> {
    // SAFETY: `ruleset` is guaranteed to have at least one element in this code path.
    let mut out_store = MeetAggrStore::new(ruleset[0].aggr.clone())?;

    for (rule_n, rule) in ruleset.iter().enumerate() {
        debug!("initial calculation for rule {:?}.{}", rule_symb, rule_n);
        let mut aggr = rule.aggr.clone();
        for (aggr, args) in aggr.iter_mut().flatten() {
            aggr.meet_init(args)?;
        }
        for item_res in rule.relation.iter(tx, None, stores)? {
            let item = item_res?;
            trace!("item for {:?}.{}: {:?} at {}", rule_symb, rule_n, item, 0);
            out_store.meet_put(item)?;
        }
        poison.check()?;
    }
    if out_store.is_empty() && ruleset[0].aggr.iter().all(|a| a.is_some()) {
        // SAFETY: `ruleset` is guaranteed to have at least one element in this code path.
        let mut aggr = ruleset[0].aggr.clone();
        for (aggr, args) in aggr.iter_mut().flatten() {
            aggr.meet_init(args)?;
        }
        let value: Vec<_> = aggr
            .iter()
            .map(|a| -> Result<DataValue> {
                let (aggr, _) = a.as_ref().ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: "aggregation entry missing in meet evaluation",
                        }
                        .build(),
                    )
                })?;
                let op = aggr.meet_op.as_ref().ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: "meet_op missing on aggregation",
                        }
                        .build(),
                    )
                })?;
                Ok(op.init_val())
            })
            .try_collect()?;
        out_store.meet_put(value)?;
    }
    Ok(out_store)
}
/// Evaluate normal aggregation rules (initial epoch).
///
/// # Complexity
///
/// O(R * T * log G) where R is rules, T is tuples, G is group count.
/// Groups tuples by key and applies aggregation functions.
fn initial_rule_aggr_eval(
    tx: &dyn QueryContext,
    rule_symb: &MagicSymbol,
    ruleset: &[CompiledRule],
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    poison: Poison,
) -> Result<(bool, RegularTempStore)> {
    let mut out_store = RegularTempStore::default();
    let should_check_limit = limiter.total.is_some() && rule_symb.is_prog_entry();
    let mut aggr_work: BTreeMap<Vec<DataValue>, Vec<Aggregation>> = BTreeMap::new();

    for (rule_n, rule) in ruleset.iter().enumerate() {
        debug!(
            "Calculation for normal aggr rule {:?}.{}",
            rule_symb, rule_n
        );
        trace!("{:?}", rule);

        let keys_indices = rule
            .aggr
            .iter()
            .enumerate()
            .filter_map(|(i, a)| if a.is_none() { Some(i) } else { None })
            .collect_vec();
        let extract_keys = |t: &Tuple| -> Vec<DataValue> {
            keys_indices.iter().map(|i| t[*i].clone()).collect_vec()
        };

        let val_indices_and_aggrs = rule
            .aggr
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.as_ref().map(|aggr| (i, aggr.clone())))
            .collect_vec();

        for item_res in rule.relation.iter(tx, None, stores)? {
            let item = item_res?;
            trace!("item for {:?}.{}: {:?} at {}", rule_symb, rule_n, item, 0);

            let keys = extract_keys(&item);

            match aggr_work.entry(keys) {
                Entry::Occupied(mut ent) => {
                    let aggr_ops = ent.get_mut();
                    for (aggr_idx, (tuple_idx, _)) in val_indices_and_aggrs.iter().enumerate() {
                        // SAFETY: `aggr_ops` and `val_indices_and_aggrs` have the same length,
                        // so `aggr_idx` is always valid.
                        aggr_ops[aggr_idx]
                            .normal_op
                            .as_mut()
                            .ok_or_else(|| {
                                crate::error::InternalError::from(
                                    EvalFailedSnafu {
                                        message: "normal_op missing on aggregation",
                                    }
                                    .build(),
                                )
                            })?
                            .set(&item[*tuple_idx])?;
                    }
                }
                Entry::Vacant(ent) => {
                    let mut aggr_ops = Vec::with_capacity(val_indices_and_aggrs.len());
                    for (i, (aggr, params)) in &val_indices_and_aggrs {
                        let mut cur_aggr = aggr.clone();
                        cur_aggr.normal_init(params)?;
                        cur_aggr
                            .normal_op
                            .as_mut()
                            .ok_or_else(|| {
                                crate::error::InternalError::from(
                                    EvalFailedSnafu {
                                        message: "normal_op missing on aggregation after init",
                                    }
                                    .build(),
                                )
                            })?
                            .set(&item[*i])?;
                        aggr_ops.push(cur_aggr)
                    }
                    ent.insert(aggr_ops);
                }
            }
        }
        poison.check()?;
    }

    // SAFETY: `ruleset` is guaranteed to have at least one element in this code path.
    let mut inv_indices = Vec::with_capacity(ruleset[0].aggr.len());
    let mut seen_keys = 0usize;
    let mut seen_aggrs = 0usize;
    for aggr in ruleset[0].aggr.iter() {
        if aggr.is_some() {
            inv_indices.push((true, seen_aggrs));
            seen_aggrs += 1;
        } else {
            inv_indices.push((false, seen_keys));
            seen_keys += 1;
        }
    }

    if aggr_work.is_empty() && ruleset[0].aggr.iter().all(|v| v.is_some()) {
        // SAFETY: `ruleset` is guaranteed to have at least one element in this code path.
        let empty_result: Vec<_> = ruleset[0]
            .aggr
            .iter()
            .map(|a| -> Result<DataValue> {
                let (aggr, args) = a.as_ref().ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: "aggregation entry missing in empty-result path",
                        }
                        .build(),
                    )
                })?;
                let mut aggr = aggr.clone();
                aggr.normal_init(args)?;
                let op = aggr.normal_op.ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: "normal_op missing on aggregation after init",
                        }
                        .build(),
                    )
                })?;
                Ok(op.get()?)
            })
            .try_collect()?;
        out_store.put(empty_result);
    }

    for (keys, aggrs) in aggr_work {
        let tuple_data: Vec<_> = inv_indices
            .iter()
            .map(|(is_aggr, idx)| -> Result<DataValue> {
                if *is_aggr {
                    Ok(aggrs[*idx]
                        .normal_op
                        .as_ref()
                        .ok_or_else(|| {
                            crate::error::InternalError::from(EvalFailedSnafu {
                                message: "normal_op missing on aggregation during result collection",
                            }.build())
                        })?
                        .get()?)
                } else {
                    Ok(keys[*idx].clone())
                }
            })
            .try_collect()?;
        let tuple = tuple_data;
        if should_check_limit {
            if !out_store.exists(&tuple) {
                if limiter.should_skip_next() {
                    out_store.put_with_skip(tuple);
                } else {
                    out_store.put(tuple);
                }
                if limiter.incr_and_should_stop() {
                    return Ok((true, out_store));
                }
            }
        } else {
            out_store.put(tuple);
        }
    }
    Ok((should_check_limit, out_store))
}
/// Evaluate non-aggregation rules incrementally (subsequent epochs).
///
/// # Complexity
///
/// O(R * D * T) where R is rules, D is delta tuples, T is derivation cost.
/// Only processes changed dependencies (semi-naive).
fn incremental_rule_non_aggr_eval(
    tx: &dyn QueryContext,
    rule_symb: &MagicSymbol,
    ruleset: &[CompiledRule],
    epoch: u32,
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    limiter: &QueryLimiter,
    poison: Poison,
) -> Result<(bool, RegularTempStore)> {
    let prev_store = stores.get(rule_symb).ok_or_else(|| {
        crate::error::InternalError::from(
            EvalFailedSnafu {
                message: format!("epoch store not found for rule '{rule_symb}'"),
            }
            .build(),
        )
    })?;
    let mut out_store = RegularTempStore::default();
    let should_check_limit = limiter.total.is_some() && rule_symb.is_prog_entry();
    for (rule_n, rule) in ruleset.iter().enumerate() {
        let mut need_complete_run = false;
        let mut dependencies_changed = false;

        for (symb, multiplicity) in rule.contained_rules.iter() {
            if stores
                .get(symb)
                .ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: format!("epoch store not found for dependency '{symb}'"),
                        }
                        .build(),
                    )
                })?
                .has_delta()
            {
                dependencies_changed = true;
                if *multiplicity == ContainedRuleMultiplicity::Many {
                    need_complete_run = true;
                    break;
                }
            }
        }

        if !dependencies_changed {
            continue;
        }

        if need_complete_run {
            debug!("complete rule for rule {:?}.{}", rule_symb, rule_n);
            for item_res in rule.relation.iter(tx, None, stores)? {
                let item = item_res?;
                if prev_store.exists(&item) {
                    trace!(
                        "item for {:?}.{}: {:?} at {}, rederived",
                        rule_symb, rule_n, item, epoch
                    );
                } else {
                    trace!(
                        "item for {:?}.{}: {:?} at {}",
                        rule_symb, rule_n, item, epoch
                    );
                    if limiter.should_skip_next() {
                        out_store.put_with_skip(item);
                    } else {
                        out_store.put(item);
                    }
                    if should_check_limit && limiter.incr_and_should_stop() {
                        trace!("early stopping due to result count limit exceeded");
                        return Ok((true, out_store));
                    }
                }
            }
            poison.check()?;
        } else {
            for delta_key in stores.keys() {
                if !rule.contained_rules.contains_key(delta_key) {
                    continue;
                }
                debug!(
                    "with delta {:?} for rule {:?}.{}",
                    delta_key, rule_symb, rule_n
                );
                for item_res in rule.relation.iter(tx, Some(delta_key), stores)? {
                    let item = item_res?;
                    if prev_store.exists(&item) {
                        trace!(
                            "item for {:?}.{}: {:?} at {}, rederived",
                            rule_symb, rule_n, item, epoch
                        );
                    } else {
                        trace!(
                            "item for {:?}.{}: {:?} at {}",
                            rule_symb, rule_n, item, epoch
                        );
                        if limiter.should_skip_next() {
                            out_store.put_with_skip(item);
                        } else {
                            out_store.put(item);
                        }
                        if should_check_limit && limiter.incr_and_should_stop() {
                            trace!("early stopping due to result count limit exceeded");
                            return Ok((true, out_store));
                        }
                    }
                }
                poison.check()?;
            }
        }
    }
    Ok((should_check_limit, out_store))
}
/// Evaluate meet aggregation rules incrementally.
///
/// # Complexity
///
/// O(R * D * A) where R is rules, D is delta tuples, A is aggregation arity.
fn incremental_rule_meet_eval(
    tx: &dyn QueryContext,
    rule_symb: &MagicSymbol,
    ruleset: &[CompiledRule],
    stores: &BTreeMap<MagicSymbol, EpochStore>,
    poison: Poison,
) -> Result<MeetAggrStore> {
    // SAFETY: `ruleset` is guaranteed to have at least one element in this code path.
    let mut out_store = MeetAggrStore::new(ruleset[0].aggr.clone())?;
    for (rule_n, rule) in ruleset.iter().enumerate() {
        let mut need_complete_run = false;
        let mut dependencies_changed = false;

        for (symb, multiplicity) in rule.contained_rules.iter() {
            if stores
                .get(symb)
                .ok_or_else(|| {
                    crate::error::InternalError::from(
                        EvalFailedSnafu {
                            message: format!("epoch store not found for dependency '{symb}'"),
                        }
                        .build(),
                    )
                })?
                .has_delta()
            {
                dependencies_changed = true;
                if *multiplicity == ContainedRuleMultiplicity::Many {
                    need_complete_run = true;
                    break;
                }
            }
        }

        if !dependencies_changed {
            continue;
        }

        let mut aggr = rule.aggr.clone();
        for (aggr, args) in aggr.iter_mut().flatten() {
            aggr.meet_init(args)?;
        }

        if need_complete_run {
            debug!("complete run for rule {:?}.{}", rule_symb, rule_n);
            for item_res in rule.relation.iter(tx, None, stores)? {
                out_store.meet_put(item_res?)?;
            }
            poison.check()?;
        } else {
            for delta_key in stores.keys() {
                if !rule.contained_rules.contains_key(delta_key) {
                    continue;
                }
                debug!(
                    "with delta {:?} for rule {:?}.{}",
                    delta_key, rule_symb, rule_n
                );
                for item_res in rule.relation.iter(tx, Some(delta_key), stores)? {
                    out_store.meet_put(item_res?)?;
                }
                poison.check()?;
            }
        }
    }
    Ok(out_store)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod query_budget_tests {
    use super::QueryBudget;
    use crate::DbInstance;
    use crate::runtime::db::{CancellationReason, DbConfig, Poison};

    #[test]
    fn epoch_limit_exceeded_is_reported_with_epoch_limit_reason() {
        let mut db: DbInstance = crate::storage::mem::new_mem_db().expect("open in-memory db");
        // A single allowed epoch can never observe "no change", so any
        // recursive rule that derives at least one fact on epoch 0 is
        // guaranteed to exhaust this cap -- deterministic, no data-shape
        // dependence beyond "the recursion produces something."
        db.config = DbConfig::new(1);

        let script = r"
edge[] <- [[1, 2], [2, 3], [3, 4]]
reachable[a, b] := edge[a, b]
reachable[a, c] := reachable[a, b], edge[b, c]
?[a, c] := reachable[a, c]
";
        let err = db
            .run_default(script)
            .expect_err("a 1-epoch cap must be exceeded by a self-recursive rule");
        let public = crate::convert_internal(err);
        assert_eq!(
            public.cancellation_reason(),
            Some(CancellationReason::EpochLimit)
        );
    }

    #[test]
    fn max_derived_rows_from_db_config_is_enforced() {
        let mut db: DbInstance = crate::storage::mem::new_mem_db().expect("open in-memory db");
        // Public-API path (#4511): DbConfig::with_max_derived_rows, not
        // QueryBudget directly -- proves the row cap is reachable from the
        // crate's public surface, not only from query::eval internals.
        db.config = DbConfig::default().with_max_derived_rows(3);

        // The base rule alone derives 5 rows (one per edge) on epoch 0,
        // exceeding the cap before the recursive term contributes anything
        // -- deterministic, no dependence on evaluation order.
        let script = r"
edge[] <- [[1, 2], [2, 3], [3, 4], [4, 5], [5, 6]]
reachable[a, b] := edge[a, b]
reachable[a, c] := reachable[a, b], edge[b, c]
?[a, c] := reachable[a, c]
";
        let err = db
            .run_default(script)
            .expect_err("a 3-row cap must be exceeded by 5 base-case rows");
        let public = crate::convert_internal(err);
        assert_eq!(
            public.cancellation_reason(),
            Some(CancellationReason::RowLimit)
        );
    }

    #[test]
    fn unbounded_budget_never_errors_on_record_rows() {
        let budget = QueryBudget::new(Poison::default(), 10);
        for _ in 0..5 {
            budget
                .record_rows(1_000_000, 0)
                .expect("no cap configured -- record_rows is a no-op");
        }
    }

    #[test]
    fn row_cap_is_cumulative_across_calls() {
        let budget = QueryBudget::new(Poison::default(), 10).with_max_derived_rows(5);
        budget.record_rows(3, 0).expect("3 <= 5, under cap");
        budget
            .record_rows(2, 0)
            .expect("5 <= 5, at cap but not over");
        let err = budget
            .record_rows(1, 0)
            .expect_err("6 > 5, one more row must exceed the cap");
        let public = crate::convert_internal(err);
        assert_eq!(
            public.cancellation_reason(),
            Some(CancellationReason::RowLimit)
        );
    }

    #[test]
    fn cloned_budget_shares_row_accounting() {
        let budget = QueryBudget::new(Poison::default(), 10).with_max_derived_rows(2);
        let clone = budget.clone();
        budget.record_rows(1, 0).expect("1 <= 2, under cap");
        let err = clone
            .record_rows(2, 0)
            .expect_err("1 + 2 = 3 > 2, clone must observe the original's accounting");
        let public = crate::convert_internal(err);
        assert_eq!(
            public.cancellation_reason(),
            Some(CancellationReason::RowLimit)
        );
    }
}
