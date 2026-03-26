//! Imperative script execution.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use compact_str::CompactString;
use either::{Either, Left, Right};
use itertools::Itertools;
use tracing::debug;

use crate::data::program::RelationOp;
use crate::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::data::symb::Symbol;
use crate::error::InternalResult as Result;
use crate::parse::{
    ImperativeCondition, ImperativeProgram, ImperativeStmt, ImperativeStmtClause, SourceSpan,
};
use crate::runtime::callback::CallbackCollector;
use crate::runtime::db::{RunningQueryCleanup, RunningQueryHandle, seconds_since_the_epoch};
use crate::runtime::error::{InvalidOperationSnafu, ReadOnlyViolationSnafu};
use crate::runtime::relation::InputRelationHandle;
use crate::runtime::transact::SessionTx;
use crate::{DataValue, DbCore as Db, NamedRows, Poison, Storage, ValidityTs};

enum ControlCode {
    Termination(NamedRows),
    Break(Option<CompactString>, SourceSpan),
    Continue(Option<CompactString>, SourceSpan),
}

struct ImperativeCallbackCtx<'ctx> {
    cleanups: &'ctx mut Vec<(Vec<u8>, Vec<u8>)>,
    callback_targets: &'ctx BTreeSet<CompactString>,
    callback_collector: &'ctx mut CallbackCollector,
    poison: &'ctx Poison,
    readonly: bool,
}

fn execute_temp_swap_stmt(
    tx: &mut SessionTx<'_>,
    left: &CompactString,
    right: &CompactString,
) -> Result<()> {
    tx.rename_temp_relation(
        Symbol::new(left.clone(), Default::default()),
        Symbol::new(CompactString::from("_*temp*"), Default::default()),
    )?;
    tx.rename_temp_relation(
        Symbol::new(right.clone(), Default::default()),
        Symbol::new(left.clone(), Default::default()),
    )?;
    tx.rename_temp_relation(
        Symbol::new(CompactString::from("_*temp*"), Default::default()),
        Symbol::new(right.clone(), Default::default()),
    )?;
    Ok(())
}

fn execute_program_stmt<'s, S: Storage<'s>>(
    db: &'s Db<S>,
    prog: &ImperativeStmtClause,
    tx: &mut SessionTx<'_>,
    cur_vld: ValidityTs,
    ctx: &mut ImperativeCallbackCtx<'_>,
) -> Result<NamedRows> {
    let ret = db.execute_single_program(
        prog.prog.clone(),
        tx,
        ctx.cleanups,
        cur_vld,
        ctx.callback_targets,
        ctx.callback_collector,
    )?;
    if let Some(store_as) = &prog.store_as {
        tx.script_store_as_relation(db, store_as, &ret, cur_vld)?;
    }
    Ok(ret)
}

impl<'s, S: Storage<'s>> Db<S> {
    fn execute_imperative_condition(
        &'s self,
        p: &ImperativeCondition,
        tx: &mut SessionTx<'_>,
        cleanups: &mut Vec<(Vec<u8>, Vec<u8>)>,
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
    ) -> Result<bool> {
        let res = match p {
            Left(rel) => {
                let relation = tx.get_relation(rel, false)?;
                relation.as_named_rows(tx)?
            }
            Right(p) => self.execute_single_program(
                p.prog.clone(),
                tx,
                cleanups,
                cur_vld,
                callback_targets,
                callback_collector,
            )?,
        };
        if let Right(pg) = &p
            && let Some(store_as) = &pg.store_as
        {
            tx.script_store_as_relation(self, store_as, &res, cur_vld)?;
        }
        Ok(!res.rows.is_empty())
    }

    fn collect_return_rows(
        &'s self,
        returns: &[Either<ImperativeStmtClause, CompactString>],
        tx: &mut SessionTx<'_>,
        cur_vld: ValidityTs,
        ctx: &mut ImperativeCallbackCtx<'_>,
    ) -> Result<ControlCode> {
        if returns.is_empty() {
            return Ok(ControlCode::Termination(NamedRows::default()));
        }
        let mut current = None;
        for nxt in returns.iter().rev() {
            let mut nr = match nxt {
                Left(prog) => self.execute_single_program(
                    prog.prog.clone(),
                    tx,
                    ctx.cleanups,
                    cur_vld,
                    ctx.callback_targets,
                    ctx.callback_collector,
                )?,
                Right(rel) => {
                    let relation = tx.get_relation(rel, false)?;
                    relation.as_named_rows(tx)?
                }
            };
            if let Left(pg) = nxt
                && let Some(store_as) = &pg.store_as
            {
                tx.script_store_as_relation(self, store_as, &nr, cur_vld)?;
            }
            nr.next = current;
            current = Some(Box::new(nr))
        }
        Ok(ControlCode::Termination(
            *current.unwrap_or_else(|| unreachable!()),
        ))
    }

    fn execute_ignore_error_program(
        &'s self,
        prog: &ImperativeStmtClause,
        tx: &mut SessionTx<'_>,
        cur_vld: ValidityTs,
        ctx: &mut ImperativeCallbackCtx<'_>,
    ) -> Result<NamedRows> {
        match self.execute_single_program(
            prog.prog.clone(),
            tx,
            ctx.cleanups,
            cur_vld,
            ctx.callback_targets,
            ctx.callback_collector,
        ) {
            Ok(res) => {
                if let Some(store_as) = &prog.store_as {
                    tx.script_store_as_relation(self, store_as, &res, cur_vld)?;
                }
                Ok(res)
            }
            Err(_) => Ok(NamedRows::new(
                vec!["status".to_string()],
                vec![vec![DataValue::from("FAILED")]],
            )),
        }
    }

    fn execute_if_stmt(
        &'s self,
        condition: &ImperativeCondition,
        then_branch: &ImperativeProgram,
        else_branch: &ImperativeProgram,
        negated: bool,
        ret: &mut NamedRows,
        tx: &mut SessionTx<'_>,
        cur_vld: ValidityTs,
        ctx: &mut ImperativeCallbackCtx<'_>,
    ) -> Result<Option<ControlCode>> {
        let cond_val = self.execute_imperative_condition(
            condition,
            tx,
            ctx.cleanups,
            cur_vld,
            ctx.callback_targets,
            ctx.callback_collector,
        )?;
        let to_execute = if cond_val ^ negated {
            then_branch
        } else {
            else_branch
        };
        match self.execute_imperative_stmts(to_execute, tx, cur_vld, ctx)? {
            Left(rows) => {
                *ret = rows;
                Ok(None)
            }
            Right(ctrl) => Ok(Some(ctrl)),
        }
    }

    fn execute_loop_stmt(
        &'s self,
        label: &Option<CompactString>,
        body: &ImperativeProgram,
        ret: &mut NamedRows,
        tx: &mut SessionTx<'_>,
        cur_vld: ValidityTs,
        ctx: &mut ImperativeCallbackCtx<'_>,
    ) -> Result<Option<ControlCode>> {
        *ret = NamedRows::default();
        loop {
            ctx.poison.check()?;
            match self.execute_imperative_stmts(body, tx, cur_vld, ctx)? {
                // NOTE: body completed normally, continue loop iteration
                Left(_) => {}
                Right(ctrl) => match ctrl {
                    ControlCode::Termination(ret_val) => {
                        return Ok(Some(ControlCode::Termination(ret_val)));
                    }
                    ControlCode::Break(break_label, span) => {
                        if break_label.is_none() || break_label == *label {
                            break;
                        } else {
                            return Ok(Some(ControlCode::Break(break_label, span)));
                        }
                    }
                    ControlCode::Continue(cont_label, span) => {
                        if cont_label.is_none() || cont_label == *label {
                            continue;
                        } else {
                            return Ok(Some(ControlCode::Continue(cont_label, span)));
                        }
                    }
                },
            }
        }
        Ok(None)
    }

    fn execute_imperative_stmts(
        &'s self,
        ps: &ImperativeProgram,
        tx: &mut SessionTx<'_>,
        cur_vld: ValidityTs,
        ctx: &mut ImperativeCallbackCtx<'_>,
    ) -> Result<Either<NamedRows, ControlCode>> {
        let mut ret = NamedRows::default();
        for p in ps {
            ctx.poison.check()?;
            match p {
                ImperativeStmt::Break { target, span, .. } => {
                    return Ok(Right(ControlCode::Break(target.clone(), *span)));
                }
                ImperativeStmt::Continue { target, span, .. } => {
                    return Ok(Right(ControlCode::Continue(target.clone(), *span)));
                }
                ImperativeStmt::Return { returns } => {
                    return Ok(Right(self.collect_return_rows(returns, tx, cur_vld, ctx)?));
                }
                ImperativeStmt::TempDebug { temp, .. } => {
                    let relation = tx.get_relation(temp, false)?;
                    debug!(relation = %temp, rows = ?relation.as_named_rows(tx)?, "temp debug");
                    ret = NamedRows::default();
                }
                ImperativeStmt::SysOp { sysop, .. } => {
                    ret = self.run_sys_op_with_tx(tx, &sysop.sysop, ctx.readonly, true)?;
                    if let Some(store_as) = &sysop.store_as {
                        tx.script_store_as_relation(self, store_as, &ret, cur_vld)?;
                    }
                }
                ImperativeStmt::Program { prog, .. } => {
                    ret = execute_program_stmt(self, prog, tx, cur_vld, ctx)?;
                }
                ImperativeStmt::IgnoreErrorProgram { prog, .. } => {
                    ret = self.execute_ignore_error_program(prog, tx, cur_vld, ctx)?;
                }
                ImperativeStmt::If {
                    condition,
                    then_branch,
                    else_branch,
                    negated,
                    ..
                } => {
                    if let Some(ctrl) = self.execute_if_stmt(
                        condition,
                        then_branch,
                        else_branch,
                        *negated,
                        &mut ret,
                        tx,
                        cur_vld,
                        ctx,
                    )? {
                        return Ok(Right(ctrl));
                    }
                }
                ImperativeStmt::Loop { label, body, .. } => {
                    if let Some(ctrl) =
                        self.execute_loop_stmt(label, body, &mut ret, tx, cur_vld, ctx)?
                    {
                        return Ok(Right(ctrl));
                    }
                }
                ImperativeStmt::TempSwap { left, right, .. } => {
                    execute_temp_swap_stmt(tx, left, right)?;
                    ret = NamedRows::default();
                    break;
                }
            }
        }
        Ok(Left(ret))
    }
    pub(crate) fn execute_imperative(
        &'s self,
        cur_vld: ValidityTs,
        ps: &ImperativeProgram,
        readonly: bool,
    ) -> Result<NamedRows> {
        let mut callback_collector = BTreeMap::new();
        let mut write_lock_names = BTreeSet::new();
        for p in ps {
            p.needs_write_locks(&mut write_lock_names);
        }
        if readonly && !write_lock_names.is_empty() {
            ReadOnlyViolationSnafu {
                operation: "imperative program with write locks",
            }
            .fail()?;
        }
        let is_write = !write_lock_names.is_empty();
        let write_lock = self.obtain_relation_locks(write_lock_names.iter());
        let _write_lock_guards = write_lock
            .iter()
            .map(|l| {
                l.read().map_err(|_poison| {
                    InvalidOperationSnafu {
                        op: "imperative program",
                        reason: "relation lock poisoned by prior panic",
                    }
                    .build()
                    .into()
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let callback_targets = if is_write {
            self.current_callback_targets()
        } else {
            Default::default()
        };
        let mut cleanups: Vec<(Vec<u8>, Vec<u8>)> = vec![];
        let ret;
        {
            let mut tx = if is_write {
                self.transact_write()?
            } else {
                self.transact()?
            };

            let poison = Poison::default();
            let qid = self.queries_count.fetch_add(1, Ordering::AcqRel);
            let since_the_epoch = seconds_since_the_epoch()?;

            let q_handle = RunningQueryHandle {
                started_at: since_the_epoch,
                poison: poison.clone(),
            };
            self.running_queries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(qid, q_handle);
            let _guard = RunningQueryCleanup {
                id: qid,
                running_queries: self.running_queries.clone(),
            };

            let mut ctx = ImperativeCallbackCtx {
                cleanups: &mut cleanups,
                callback_targets: &callback_targets,
                callback_collector: &mut callback_collector,
                poison: &poison,
                readonly,
            };
            match self.execute_imperative_stmts(ps, &mut tx, cur_vld, &mut ctx)? {
                Left(res) => ret = res,
                Right(ctrl) => match ctrl {
                    ControlCode::Termination(res) => {
                        ret = res;
                    }
                    ControlCode::Break(_, _span) | ControlCode::Continue(_, _span) => {
                        return InvalidOperationSnafu {
                            op: "imperative execution",
                            reason: "control flow has nowhere to go",
                        }
                        .fail()
                        .map_err(Into::into);
                    }
                },
            }

            for (lower, upper) in cleanups {
                tx.store_tx.del_range_from_persisted(&lower, &upper)?;
            }

            tx.commit_tx()?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !callback_collector.is_empty() {
            self.send_callbacks(callback_collector)
        }

        Ok(ret)
    }
}

impl SessionTx<'_> {
    fn script_store_as_relation<'s, S: Storage<'s>>(
        &mut self,
        db: &Db<S>,
        name: &str,
        rels: &NamedRows,
        cur_vld: ValidityTs,
    ) -> Result<()> {
        let mut key_bindings = vec![];
        for k in rels.headers.iter() {
            let k = k.replace('(', "_").replace(')', "");
            let k = Symbol::new(k.clone(), Default::default());
            if key_bindings.contains(&k) {
                InvalidOperationSnafu {
                    op: "store as relation",
                    reason: format!(
                        "Duplicate variable name {k}, please use distinct variables in `as` construct."
                    ),
                }
                .fail()?;
            }
            key_bindings.push(k);
        }
        let keys = key_bindings
            .iter()
            .map(|s| ColumnDef {
                name: s.name.clone(),
                typing: NullableColType {
                    coltype: ColType::Any,
                    nullable: true,
                },
                default_gen: None,
            })
            .collect_vec();

        let meta = InputRelationHandle {
            name: Symbol::new(name, Default::default()),
            metadata: StoredRelationMetadata {
                keys,
                non_keys: vec![],
            },
            key_bindings,
            dep_bindings: vec![],
            span: Default::default(),
        };
        let headers = meta.key_bindings.clone();
        self.execute_relation(
            db,
            rels.rows.iter().cloned(),
            RelationOp::Replace,
            &meta,
            &headers,
            cur_vld,
            &Default::default(),
            &mut Default::default(),
            true,
            "",
        )?;
        Ok(())
    }
}
