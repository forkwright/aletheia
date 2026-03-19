//! Query execution methods for the Db engine.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use compact_str::CompactString;
use either::{Left, Right};
use itertools::Itertools;
use serde_json::json;

use crate::engine::data::functions::current_validity;
use crate::engine::data::json::JsonValue;
use crate::engine::data::program::{InputProgram, QueryAssertion, RelationOp, ReturnMutation};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::error::InternalResult as Result;
use crate::engine::parse::{DatalogScript, parse_script};
use crate::engine::query::compile::{CompiledProgram, CompiledRule, CompiledRuleSet};
use crate::engine::query::ra::{
    FilteredRA, FtsSearchRA, HnswSearchRA, InnerJoin, LshSearchRA, NegJoin, RelAlgebra, ReorderRA,
    StoredRA, StoredWithValidityRA, TempStoreRA, UnificationRA,
};
use crate::engine::runtime::callback::CallbackCollector;
use crate::engine::runtime::db::{
    Db, NamedRows, Poison, RunningQueryCleanup, RunningQueryHandle, ScriptMutability,
    seconds_since_the_epoch,
};
use crate::engine::runtime::error::{
    AssertionFailedSnafu, InvalidOperationSnafu, ReadOnlyViolationSnafu,
    RelationAlreadyExistsSnafu, RelationNotFoundSnafu,
};
use crate::engine::runtime::transact::SessionTx;
use crate::engine::storage::Storage;

impl<'s, S: Storage<'s>> Db<S> {
    /// Run the DatalogScript passed in. The `params` argument is a map of parameters.
    pub fn run_script(
        &'s self,
        payload: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> Result<NamedRows> {
        self.run_script_ast(
            parse_script(
                payload,
                &params,
                &self.get_fixed_rules(),
                current_validity(),
            )?,
            current_validity(),
            mutability,
        )
    }

    /// Run the DatalogScript passed in. The `params` argument is a map of parameters.
    pub fn run_script_read_only(
        &'s self,
        payload: &str,
        params: BTreeMap<String, DataValue>,
    ) -> Result<NamedRows> {
        self.run_script(payload, params, ScriptMutability::Immutable)
    }

    /// Run the AST DatalogScript passed in.
    pub fn run_script_ast(
        &'s self,
        payload: DatalogScript,
        cur_vld: ValidityTs,
        mutability: ScriptMutability,
    ) -> Result<NamedRows> {
        let read_only = mutability == ScriptMutability::Immutable;
        match payload {
            DatalogScript::Single(p) => self.execute_single(cur_vld, p, read_only),
            DatalogScript::Imperative(ps) => self.execute_imperative(cur_vld, &ps, read_only),
            DatalogScript::Sys(op) => self.run_sys_op(op, read_only),
        }
    }

    pub(crate) fn execute_single_program(
        &'s self,
        p: InputProgram,
        tx: &mut SessionTx<'_>,
        cleanups: &mut Vec<(Vec<u8>, Vec<u8>)>,
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
    ) -> Result<NamedRows> {
        let sleep_opt = p.out_opts.sleep;
        let (q_res, q_cleanups) =
            self.run_query(tx, p, cur_vld, callback_targets, callback_collector, true)?;
        cleanups.extend(q_cleanups);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(secs) = sleep_opt {
            thread::sleep(Duration::from_micros((secs * 1000000.) as u64));
        }
        Ok(q_res)
    }

    fn execute_single(
        &'s self,
        cur_vld: ValidityTs,
        p: InputProgram,
        read_only: bool,
    ) -> Result<NamedRows> {
        let mut callback_collector = BTreeMap::new();
        let write_lock_names = p.needs_write_lock();
        let is_write = write_lock_names.is_some();
        if read_only && is_write {
            ReadOnlyViolationSnafu {
                operation: "query requiring write lock",
            }
            .fail()?;
        }
        let write_lock = self.obtain_relation_locks(write_lock_names.iter());
        let _write_lock_guards = if is_write {
            Some(write_lock[0].read().expect("lock poisoned"))
        } else {
            None
        };
        let callback_targets = if is_write {
            self.current_callback_targets()
        } else {
            Default::default()
        };
        let mut cleanups = vec![];
        let res;
        {
            let mut tx = if is_write {
                self.transact_write()?
            } else {
                self.transact()?
            };

            res = self.execute_single_program(
                p,
                &mut tx,
                &mut cleanups,
                cur_vld,
                &callback_targets,
                &mut callback_collector,
            )?;

            for (lower, upper) in cleanups {
                tx.store_tx.del_range_from_persisted(&lower, &upper)?;
            }

            tx.commit_tx()?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !callback_collector.is_empty() {
            self.send_callbacks(callback_collector)
        }

        Ok(res)
    }
    pub(crate) fn explain_compiled(&self, strata: &[CompiledProgram]) -> Result<NamedRows> {
        let mut ret: Vec<JsonValue> = vec![];
        const STRATUM: &str = "stratum";
        const ATOM_IDX: &str = "atom_idx";
        const OP: &str = "op";
        const RULE_IDX: &str = "rule_idx";
        const RULE_NAME: &str = "rule";
        const REF_NAME: &str = "ref";
        const OUT_BINDINGS: &str = "out_relation";
        const JOINS_ON: &str = "joins_on";
        const FILTERS: &str = "filters/expr";

        let headers = vec![
            STRATUM.to_string(),
            RULE_IDX.to_string(),
            RULE_NAME.to_string(),
            ATOM_IDX.to_string(),
            OP.to_string(),
            REF_NAME.to_string(),
            JOINS_ON.to_string(),
            FILTERS.to_string(),
            OUT_BINDINGS.to_string(),
        ];

        for (stratum, p) in strata.iter().enumerate() {
            let mut clause_idx = -1;
            for (rule_name, v) in p {
                match v {
                    CompiledRuleSet::Rules(rules) => {
                        for CompiledRule { aggr, relation, .. } in rules.iter() {
                            clause_idx += 1;
                            let mut ret_for_relation = vec![];
                            let mut rel_stack = vec![relation];
                            let mut idx = 0;
                            let mut atom_type = "out";
                            for (a, _) in aggr.iter().flatten() {
                                if a.is_meet {
                                    if atom_type == "out" {
                                        atom_type = "meet_aggr_out";
                                    }
                                } else {
                                    atom_type = "aggr_out";
                                }
                            }

                            ret_for_relation.push(json!({
                                STRATUM: stratum,
                                ATOM_IDX: idx,
                                OP: atom_type,
                                RULE_IDX: clause_idx,
                                RULE_NAME: rule_name.to_string(),
                                OUT_BINDINGS: relation.bindings_after_eliminate().into_iter().map(|v| v.to_string()).collect_vec()
                            }));
                            idx += 1;

                            while let Some(rel) = rel_stack.pop() {
                                let (atom_type, ref_name, joins_on, filters) = match rel {
                                    r @ RelAlgebra::Fixed(..) => {
                                        if r.is_unit() {
                                            continue;
                                        }
                                        ("fixed", json!(null), json!(null), json!(null))
                                    }
                                    RelAlgebra::TempStore(TempStoreRA {
                                        storage_key,
                                        filters,
                                        ..
                                    }) => (
                                        "load_mem",
                                        json!(storage_key.to_string()),
                                        json!(null),
                                        json!(filters.iter().map(|f| f.to_string()).collect_vec()),
                                    ),
                                    RelAlgebra::Stored(StoredRA {
                                        storage, filters, ..
                                    }) => (
                                        "load_stored",
                                        json!(format!(":{}", storage.name)),
                                        json!(null),
                                        json!(filters.iter().map(|f| f.to_string()).collect_vec()),
                                    ),
                                    RelAlgebra::StoredWithValidity(StoredWithValidityRA {
                                        storage,
                                        filters,
                                        ..
                                    }) => (
                                        "load_stored_with_validity",
                                        json!(format!(":{}", storage.name)),
                                        json!(null),
                                        json!(filters.iter().map(|f| f.to_string()).collect_vec()),
                                    ),
                                    RelAlgebra::Join(inner) => {
                                        if inner.left.is_unit() {
                                            rel_stack.push(&inner.right);
                                            continue;
                                        }
                                        let t = inner.join_type();
                                        let InnerJoin {
                                            left,
                                            right,
                                            joiner,
                                            ..
                                        } = inner.as_ref();
                                        rel_stack.push(left);
                                        rel_stack.push(right);
                                        (t, json!(null), json!(joiner.as_map()), json!(null))
                                    }
                                    RelAlgebra::NegJoin(inner) => {
                                        let t = inner.join_type();
                                        let NegJoin {
                                            left,
                                            right,
                                            joiner,
                                            ..
                                        } = inner.as_ref();
                                        rel_stack.push(left);
                                        rel_stack.push(right);
                                        (t, json!(null), json!(joiner.as_map()), json!(null))
                                    }
                                    RelAlgebra::Reorder(ReorderRA { relation, .. }) => {
                                        rel_stack.push(relation);
                                        ("reorder", json!(null), json!(null), json!(null))
                                    }
                                    RelAlgebra::Filter(FilteredRA {
                                        parent,
                                        filters: pred,
                                        ..
                                    }) => {
                                        rel_stack.push(parent);
                                        (
                                            "filter",
                                            json!(null),
                                            json!(null),
                                            json!(pred.iter().map(|f| f.to_string()).collect_vec()),
                                        )
                                    }
                                    RelAlgebra::Unification(UnificationRA {
                                        parent,
                                        binding,
                                        expr,
                                        is_multi,
                                        ..
                                    }) => {
                                        rel_stack.push(parent);
                                        (
                                            if *is_multi { "multi-unify" } else { "unify" },
                                            json!(binding.name),
                                            json!(null),
                                            json!(expr.to_string()),
                                        )
                                    }
                                    RelAlgebra::HnswSearch(HnswSearchRA {
                                        hnsw_search, ..
                                    }) => (
                                        "hnsw_index",
                                        json!(format!(":{}", hnsw_search.query.name)),
                                        json!(hnsw_search.query.name),
                                        json!(
                                            hnsw_search
                                                .filter
                                                .iter()
                                                .map(|f| f.to_string())
                                                .collect_vec()
                                        ),
                                    ),
                                    RelAlgebra::FtsSearch(FtsSearchRA { fts_search, .. }) => (
                                        "fts_index",
                                        json!(format!(":{}", fts_search.query.name)),
                                        json!(fts_search.query.name),
                                        json!(
                                            fts_search
                                                .filter
                                                .iter()
                                                .map(|f| f.to_string())
                                                .collect_vec()
                                        ),
                                    ),
                                    RelAlgebra::LshSearch(LshSearchRA { lsh_search, .. }) => (
                                        "lsh_index",
                                        json!(format!(":{}", lsh_search.query.name)),
                                        json!(lsh_search.query.name),
                                        json!(
                                            lsh_search
                                                .filter
                                                .iter()
                                                .map(|f| f.to_string())
                                                .collect_vec()
                                        ),
                                    ),
                                };
                                ret_for_relation.push(json!({
                                    STRATUM: stratum,
                                    ATOM_IDX: idx,
                                    OP: atom_type,
                                    RULE_IDX: clause_idx,
                                    RULE_NAME: rule_name.to_string(),
                                    REF_NAME: ref_name,
                                    OUT_BINDINGS: rel.bindings_after_eliminate().into_iter().map(|v| v.to_string()).collect_vec(),
                                    JOINS_ON: joins_on,
                                    FILTERS: filters,
                                }));
                                idx += 1;
                            }
                            ret_for_relation.reverse();
                            ret.extend(ret_for_relation)
                        }
                    }
                    CompiledRuleSet::Fixed(_) => ret.push(json!({
                        STRATUM: stratum,
                        ATOM_IDX: 0,
                        OP: "algo",
                        RULE_IDX: 0,
                        RULE_NAME: rule_name.to_string(),
                    })),
                }
            }
        }

        let rows = ret
            .into_iter()
            .map(|m: JsonValue| {
                headers
                    .iter()
                    .map(|i| DataValue::from(m.get(i).unwrap_or(&JsonValue::Null)))
                    .collect_vec()
            })
            .collect_vec();

        Ok(NamedRows::new(headers, rows))
    }
    /// This is the entry to query evaluation
    pub(crate) fn run_query(
        &self,
        tx: &mut SessionTx<'_>,
        input_program: InputProgram,
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
        top_level: bool,
    ) -> Result<(NamedRows, Vec<(Vec<u8>, Vec<u8>)>)> {
        let mut clean_ups = vec![];

        if let Some((meta, op, _)) = &input_program.out_opts.store_relation {
            if *op == RelationOp::Create {
                if tx.relation_exists(&meta.name)? {
                    RelationAlreadyExistsSnafu {
                        name: meta.name.name.to_string(),
                    }
                    .fail()?;
                }
            } else if *op != RelationOp::Replace {
                let existing = tx.get_relation(&meta.name, false)?;

                if !tx.relation_exists(&meta.name)? {
                    RelationNotFoundSnafu {
                        name: meta.name.name.to_string(),
                    }
                    .fail()?;
                }

                existing.ensure_compatible(
                    meta,
                    *op == RelationOp::Rm || *op == RelationOp::Delete || *op == RelationOp::Update,
                )?;
            }
        };

        let entry_head_or_default = input_program.get_entry_out_head_or_default()?;
        let (normalized_program, out_opts) = input_program.into_normalized_program(tx)?;
        let (stratified_program, store_lifetimes) = normalized_program.into_stratified_program()?;
        let program = stratified_program.magic_sets_rewrite(tx)?;
        let compiled = tx.stratified_magic_compile(program)?;

        let poison = Poison::default();
        if let Some(secs) = out_opts.timeout {
            poison.set_timeout(secs)?;
        }
        let id = self.queries_count.fetch_add(1, Ordering::AcqRel);

        let since_the_epoch = seconds_since_the_epoch()?;

        let handle = RunningQueryHandle {
            started_at: since_the_epoch,
            poison: poison.clone(),
        };
        self.running_queries
            .lock()
            .expect("lock poisoned")
            .insert(id, handle);

        let _guard = RunningQueryCleanup {
            id,
            running_queries: self.running_queries.clone(),
        };

        let total_num_to_take = if out_opts.sorters.is_empty() {
            out_opts.num_to_take()
        } else {
            None
        };

        let num_to_skip = if out_opts.sorters.is_empty() {
            out_opts.offset
        } else {
            None
        };

        let (result_store, early_return) = tx.stratified_magic_evaluate(
            &compiled,
            store_lifetimes,
            total_num_to_take,
            num_to_skip,
            poison,
        )?;

        if let Some(assertion) = &out_opts.assertion {
            match assertion {
                QueryAssertion::AssertNone(_span) => {
                    if result_store.all_iter().next().is_some() {
                        AssertionFailedSnafu {
                            message: "The query is asserted to return no result, but a tuple was found",
                        }
                        .fail()?;
                    }
                }
                QueryAssertion::AssertSome(_span) => {
                    if result_store.all_iter().next().is_none() {
                        AssertionFailedSnafu {
                            message: "The query is asserted to return some results, but returned none",
                        }
                        .fail()?;
                    }
                }
            }
        }

        if !out_opts.sorters.is_empty() {
            let sorted_result =
                tx.sort_and_collect(result_store, &out_opts.sorters, &entry_head_or_default)?;
            let sorted_iter = if let Some(offset) = out_opts.offset {
                Left(sorted_result.into_iter().skip(offset))
            } else {
                Right(sorted_result.into_iter())
            };
            let sorted_iter = if let Some(limit) = out_opts.limit {
                Left(sorted_iter.take(limit))
            } else {
                Right(sorted_iter)
            };
            if let Some((meta, relation_op, returning)) = &out_opts.store_relation {
                let to_clear = tx
                    .execute_relation(
                        self,
                        sorted_iter,
                        *relation_op,
                        meta,
                        &entry_head_or_default,
                        cur_vld,
                        callback_targets,
                        callback_collector,
                        top_level,
                        if *returning == ReturnMutation::Returning {
                            &meta.name.name
                        } else {
                            ""
                        },
                    )
                    .map_err(|e| crate::engine::error::InternalError::Runtime {
                        source: InvalidOperationSnafu {
                            op: "transaction",
                            reason: format!("{e}: when executing against relation '{}'", meta.name),
                        }
                        .build(),
                    })?;
                clean_ups.extend(to_clear);
                let returned_rows =
                    tx.get_returning_rows(callback_collector, &meta.name, returning)?;
                Ok((returned_rows, clean_ups))
            } else {
                let rows: Vec<_> = sorted_iter.collect_vec();
                Ok((
                    NamedRows::new(
                        entry_head_or_default
                            .iter()
                            .map(|s| s.to_string())
                            .collect_vec(),
                        rows,
                    ),
                    clean_ups,
                ))
            }
        } else {
            let scan = if early_return {
                Right(Left(
                    result_store.early_returned_iter().map(|t| t.into_tuple()),
                ))
            } else if out_opts.limit.is_some() || out_opts.offset.is_some() {
                let limit = out_opts.limit.unwrap_or(usize::MAX);
                let offset = out_opts.offset.unwrap_or(0);
                Right(Right(
                    result_store
                        .all_iter()
                        .skip(offset)
                        .take(limit)
                        .map(|t| t.into_tuple()),
                ))
            } else {
                Left(result_store.all_iter().map(|t| t.into_tuple()))
            };

            if let Some((meta, relation_op, returning)) = &out_opts.store_relation {
                let to_clear = tx
                    .execute_relation(
                        self,
                        scan,
                        *relation_op,
                        meta,
                        &entry_head_or_default,
                        cur_vld,
                        callback_targets,
                        callback_collector,
                        top_level,
                        if *returning == ReturnMutation::Returning {
                            &meta.name.name
                        } else {
                            ""
                        },
                    )
                    .map_err(|e| crate::engine::error::InternalError::Runtime {
                        source: InvalidOperationSnafu {
                            op: "transaction",
                            reason: format!("{e}: when executing against relation '{}'", meta.name),
                        }
                        .build(),
                    })?;
                clean_ups.extend(to_clear);
                let returned_rows =
                    tx.get_returning_rows(callback_collector, &meta.name, returning)?;

                Ok((returned_rows, clean_ups))
            } else {
                let rows: Vec<_> = scan.collect_vec();

                Ok((
                    NamedRows::new(
                        entry_head_or_default
                            .iter()
                            .map(|s| s.to_string())
                            .collect_vec(),
                        rows,
                    ),
                    clean_ups,
                ))
            }
        }
    }
}
