//! System operation and metadata query methods for the Db engine.

use std::iter;
use std::sync::atomic::Ordering;

use itertools::Itertools;
use serde_json::json;

use crate::data::json::JsonValue;
use crate::data::tuple::TupleT;
use crate::data::value::{DataValue, LARGEST_UTF_CHAR};
use crate::error::InternalResult as Result;
use crate::parse::sys::SysOp;
use crate::runtime::db::{Db, NamedRows, OK_STR, STATUS_STR};
use crate::runtime::error::ReadOnlyViolationSnafu;
use crate::runtime::relation::{RelationHandle, RelationId};
use crate::runtime::transact::SessionTx;
use crate::storage::Storage;

impl<'s, S: Storage<'s>> Db<S> {
    pub(crate) fn run_sys_op_with_tx(
        &'s self,
        tx: &mut SessionTx<'_>,
        op: &SysOp,
        read_only: bool,
        skip_locking: bool,
    ) -> Result<NamedRows> {
        match op {
            SysOp::Explain(prog) => {
                let (normalized_program, _) = prog.clone().into_normalized_program(tx)?;
                let (stratified_program, _) = normalized_program.into_stratified_program()?;
                let program = stratified_program.magic_sets_rewrite(tx)?;
                let compiled = tx.stratified_magic_compile(program)?;
                self.explain_compiled(&compiled)
            }
            SysOp::Compact => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "compact",
                    }
                    .fail()?;
                }
                self.compact_relation()?;
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::ListRelations => self.list_relations(tx),
            SysOp::ListFixedRules => {
                let rules = self.fixed_rules.read().unwrap_or_else(|e| e.into_inner());
                Ok(NamedRows::new(
                    vec!["rule".to_string()],
                    rules
                        .keys()
                        .map(|k| vec![DataValue::from(k as &str)])
                        .collect_vec(),
                ))
            }
            SysOp::RemoveRelation(rel_names) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "remove relations",
                    }
                    .fail()?;
                }
                let rel_name_strs = rel_names.iter().map(|n| &n.name);
                let locks = if skip_locking {
                    vec![]
                } else {
                    self.obtain_relation_locks(rel_name_strs)
                };
                let _guards = locks
                    .iter()
                    .map(|l| l.read().unwrap_or_else(|e| e.into_inner()))
                    .collect_vec();
                let mut bounds = vec![];
                for rs in rel_names {
                    let bound = tx.destroy_relation(rs)?;
                    if !rs.is_temp_store_name() {
                        bounds.extend(bound);
                    }
                }
                for (lower, upper) in bounds {
                    tx.store_tx.del_range_from_persisted(&lower, &upper)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::DescribeRelation(rel_name, description) => {
                tx.describe_relation(rel_name, description)?;
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::CreateIndex(rel_name, idx_name, cols) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "create index",
                    }
                    .fail()?;
                }
                if skip_locking {
                    tx.create_index(rel_name, idx_name, cols)?;
                } else {
                    let lock = self
                        .obtain_relation_locks(iter::once(&rel_name.name))
                        .pop()
                        .unwrap_or_else(|| unreachable!());
                    let _guard = lock.write().unwrap_or_else(|e| e.into_inner());
                    tx.create_index(rel_name, idx_name, cols)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::CreateVectorIndex(config) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "create vector index",
                    }
                    .fail()?;
                }
                if skip_locking {
                    tx.create_hnsw_index(config)?;
                } else {
                    let lock = self
                        .obtain_relation_locks(iter::once(&config.base_relation))
                        .pop()
                        .unwrap_or_else(|| unreachable!());
                    let _guard = lock.write().unwrap_or_else(|e| e.into_inner());
                    tx.create_hnsw_index(config)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::CreateFtsIndex(config) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "create FTS index",
                    }
                    .fail()?;
                }
                if skip_locking {
                    tx.create_fts_index(config)?;
                } else {
                    let lock = self
                        .obtain_relation_locks(iter::once(&config.base_relation))
                        .pop()
                        .unwrap_or_else(|| unreachable!());
                    let _guard = lock.write().unwrap_or_else(|e| e.into_inner());
                    tx.create_fts_index(config)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::CreateMinHashLshIndex(config) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "create MinHash LSH index",
                    }
                    .fail()?;
                }
                if skip_locking {
                    tx.create_minhash_lsh_index(config)?;
                } else {
                    let lock = self
                        .obtain_relation_locks(iter::once(&config.base_relation))
                        .pop()
                        .unwrap_or_else(|| unreachable!());
                    let _guard = lock.write().unwrap_or_else(|e| e.into_inner());
                    tx.create_minhash_lsh_index(config)?;
                }

                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::RemoveIndex(rel_name, idx_name) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "remove index",
                    }
                    .fail()?;
                }
                let bounds = if skip_locking {
                    tx.remove_index(rel_name, idx_name)?
                } else {
                    let lock = self
                        .obtain_relation_locks(iter::once(&rel_name.name))
                        .pop()
                        .unwrap_or_else(|| unreachable!());
                    let _guard = lock.read().unwrap_or_else(|e| e.into_inner());
                    tx.remove_index(rel_name, idx_name)?
                };

                for (lower, upper) in bounds {
                    tx.store_tx.del_range_from_persisted(&lower, &upper)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::ListColumns(rs) => self.list_columns(tx, rs),
            SysOp::ListIndices(rs) => self.list_indices(tx, rs),
            SysOp::RenameRelation(rename_pairs) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "rename relations",
                    }
                    .fail()?;
                }
                let rel_names = rename_pairs.iter().flat_map(|(f, t)| [&f.name, &t.name]);
                let locks = if skip_locking {
                    vec![]
                } else {
                    self.obtain_relation_locks(rel_names)
                };
                let _guards = locks
                    .iter()
                    .map(|l| l.read().unwrap_or_else(|e| e.into_inner()))
                    .collect_vec();
                for (old, new) in rename_pairs {
                    tx.rename_relation(old, new)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::ListRunning => self.list_running(),
            SysOp::KillRunning(id) => {
                let queries = self
                    .running_queries
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                Ok(match queries.get(id) {
                    None => NamedRows::new(
                        vec![STATUS_STR.to_string()],
                        vec![vec![DataValue::from("NOT_FOUND")]],
                    ),
                    Some(handle) => {
                        handle.poison.0.store(true, Ordering::Relaxed);
                        NamedRows::new(
                            vec![STATUS_STR.to_string()],
                            vec![vec![DataValue::from("KILLING")]],
                        )
                    }
                })
            }
            SysOp::ShowTrigger(name) => {
                let rel = tx.get_relation(name, false)?;
                let mut rows: Vec<Vec<JsonValue>> = vec![];
                for (i, trigger) in rel.put_triggers.iter().enumerate() {
                    rows.push(vec![json!("put"), json!(i), json!(trigger)])
                }
                for (i, trigger) in rel.rm_triggers.iter().enumerate() {
                    rows.push(vec![json!("rm"), json!(i), json!(trigger)])
                }
                for (i, trigger) in rel.replace_triggers.iter().enumerate() {
                    rows.push(vec![json!("replace"), json!(i), json!(trigger)])
                }
                let rows = rows
                    .into_iter()
                    .map(|row| row.into_iter().map(DataValue::from).collect_vec())
                    .collect_vec();
                Ok(NamedRows::new(
                    vec!["type".to_string(), "idx".to_string(), "trigger".to_string()],
                    rows,
                ))
            }
            SysOp::SetTriggers(name, puts, rms, replaces) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "set triggers",
                    }
                    .fail()?;
                }
                tx.set_relation_triggers(name, puts, rms, replaces)?;
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
            SysOp::SetAccessLevel(names, level) => {
                if read_only {
                    ReadOnlyViolationSnafu {
                        operation: "set access level",
                    }
                    .fail()?;
                }
                for name in names {
                    tx.set_access_level(name, *level)?;
                }
                Ok(NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ))
            }
        }
    }

    pub(crate) fn run_sys_op(&'s self, op: SysOp, read_only: bool) -> Result<NamedRows> {
        let mut tx = if read_only {
            self.transact()?
        } else {
            self.transact_write()?
        };
        let res = self.run_sys_op_with_tx(&mut tx, &op, read_only, false)?;
        tx.commit_tx()?;
        Ok(res)
    }

    pub(crate) fn list_running(&self) -> Result<NamedRows> {
        let rows = self
            .running_queries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| {
                vec![
                    DataValue::from(*k as i64),
                    DataValue::from(format!("{:?}", v.started_at)),
                ]
            })
            .collect_vec();
        Ok(NamedRows::new(
            vec!["id".to_string(), "started_at".to_string()],
            rows,
        ))
    }

    fn list_indices(&'s self, tx: &SessionTx<'_>, name: &str) -> Result<NamedRows> {
        let handle = tx.get_relation(name, false)?;
        let mut rows = vec![];
        for (name, (rel, cols)) in &handle.indices {
            rows.push(vec![
                json!(name),
                json!("normal"),
                json!([rel.name]),
                json!({ "indices": cols }),
            ]);
        }
        for (name, (rel, manifest)) in &handle.hnsw_indices {
            rows.push(vec![
                json!(name),
                json!("hnsw"),
                json!([rel.name]),
                json!({
                    "vec_dim": manifest.vec_dim,
                    "dtype": manifest.dtype,
                    "vec_fields": manifest.vec_fields,
                    "distance": manifest.distance,
                    "ef_construction": manifest.ef_construction,
                    "m_neighbours": manifest.m_neighbours,
                    "m_max": manifest.m_max,
                    "m_max0": manifest.m_max0,
                    "level_multiplier": manifest.level_multiplier,
                    "extend_candidates": manifest.extend_candidates,
                    "keep_pruned_connections": manifest.keep_pruned_connections,
                }),
            ]);
        }
        for (name, (rel, manifest)) in &handle.fts_indices {
            rows.push(vec![
                json!(name),
                json!("fts"),
                json!([rel.name]),
                json!({
                    "extractor": manifest.extractor,
                    "tokenizer": manifest.tokenizer,
                    "tokenizer_filters": manifest.filters,
                }),
            ]);
        }
        for (name, (rel, inv_rel, manifest)) in &handle.lsh_indices {
            rows.push(vec![
                json!(name),
                json!("lsh"),
                json!([rel.name, inv_rel.name]),
                json!({
                    "extractor": manifest.extractor,
                    "tokenizer": manifest.tokenizer,
                    "tokenizer_filters": manifest.filters,
                    "n_gram": manifest.n_gram,
                    "num_perm": manifest.num_perm,
                    "n_bands": manifest.n_bands,
                    "n_rows_in_band": manifest.n_rows_in_band,
                    "threshold": manifest.threshold,
                }),
            ]);
        }
        let rows = rows
            .into_iter()
            .map(|row| row.into_iter().map(DataValue::from).collect_vec())
            .collect_vec();
        Ok(NamedRows::new(
            vec![
                "name".to_string(),
                "type".to_string(),
                "relations".to_string(),
                "config".to_string(),
            ],
            rows,
        ))
    }

    fn list_columns(&'s self, tx: &SessionTx<'_>, name: &str) -> Result<NamedRows> {
        let handle = tx.get_relation(name, false)?;
        let mut rows = vec![];
        let mut idx = 0;
        for col in &handle.metadata.keys {
            let default_expr = col.default_gen.as_ref().map(|r#gen| r#gen.to_string());

            rows.push(vec![
                json!(col.name),
                json!(true),
                json!(idx),
                json!(col.typing.to_string()),
                json!(col.default_gen.is_some()),
                json!(default_expr),
            ]);
            idx += 1;
        }
        for col in &handle.metadata.non_keys {
            let default_expr = col.default_gen.as_ref().map(|r#gen| r#gen.to_string());

            rows.push(vec![
                json!(col.name),
                json!(false),
                json!(idx),
                json!(col.typing.to_string()),
                json!(col.default_gen.is_some()),
                json!(default_expr),
            ]);
            idx += 1;
        }
        let rows = rows
            .into_iter()
            .map(|row| row.into_iter().map(DataValue::from).collect_vec())
            .collect_vec();
        Ok(NamedRows::new(
            vec![
                "column".to_string(),
                "is_key".to_string(),
                "index".to_string(),
                "type".to_string(),
                "has_default".to_string(),
                "default_expr".to_string(),
            ],
            rows,
        ))
    }

    fn list_relations(&'s self, tx: &SessionTx<'_>) -> Result<NamedRows> {
        let lower = vec![DataValue::from("")].encode_as_key(RelationId::SYSTEM);
        let upper =
            vec![DataValue::from(String::from(LARGEST_UTF_CHAR))].encode_as_key(RelationId::SYSTEM);
        let mut rows: Vec<Vec<JsonValue>> = vec![];
        for kv_res in tx.store_tx.range_scan(&lower, &upper) {
            let (k_slice, v_slice) = kv_res?;
            if upper <= k_slice {
                break;
            }
            let meta = RelationHandle::decode(&v_slice)?;
            let n_keys = meta.metadata.keys.len();
            let n_dependents = meta.metadata.non_keys.len();
            let arity = n_keys + n_dependents;
            let name = meta.name;
            let access_level = if name.contains(':') {
                "index".to_string()
            } else {
                meta.access_level.to_string()
            };
            rows.push(vec![
                json!(name),
                json!(arity),
                json!(access_level),
                json!(n_keys),
                json!(n_dependents),
                json!(meta.put_triggers.len()),
                json!(meta.rm_triggers.len()),
                json!(meta.replace_triggers.len()),
                json!(meta.description),
            ]);
        }
        let rows = rows
            .into_iter()
            .map(|row| row.into_iter().map(DataValue::from).collect_vec())
            .collect_vec();
        Ok(NamedRows::new(
            vec![
                "name".to_string(),
                "arity".to_string(),
                "access_level".to_string(),
                "n_keys".to_string(),
                "n_non_keys".to_string(),
                "n_put_triggers".to_string(),
                "n_rm_triggers".to_string(),
                "n_replace_triggers".to_string(),
                "description".to_string(),
            ],
            rows,
        ))
    }
}
