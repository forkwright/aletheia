//! Stored relation validation: ensure constraints and removal operations.
//! SessionTx methods for stored relation mutation and FTS/HNSW/LSH index maintenance.
//! Stored relation access operators.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::collections::BTreeSet;

use compact_str::CompactString;
use itertools::Itertools;

use super::extractors::{make_const_rule, make_extractors};
use crate::data::relation::StoredRelationMetadata;
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::parse_script;
use crate::query::error::*;
use crate::runtime::callback::{CallbackCollector, CallbackOp};
use crate::runtime::relation::{AccessLevel, RelationHandle, extend_tuple_from_v};
use crate::storage::Storage;
use crate::{DbCore as Db, NamedRows, SourceSpan, StoreTx};

impl<'a> crate::runtime::transact::SessionTx<'a> {
    pub(crate) fn collect_mutations<'s, S: Storage<'s>>(
        &mut self,
        db: &Db<S>,
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
        propagate_triggers: bool,
        to_clear: &mut Vec<(Vec<u8>, Vec<u8>)>,
        relation_store: &RelationHandle,
        is_callback_target: bool,
        new_tuples: Vec<DataValue>,
        old_tuples: Vec<DataValue>,
    ) -> Result<()> {
        let mut bindings = relation_store
            .metadata
            .keys
            .iter()
            .map(|k| Symbol::new(k.name.clone(), Default::default()))
            .collect_vec();
        let v_bindings = relation_store
            .metadata
            .non_keys
            .iter()
            .map(|k| Symbol::new(k.name.clone(), Default::default()));
        bindings.extend(v_bindings);

        let kv_bindings = bindings;
        if propagate_triggers {
            for trigger in &relation_store.put_triggers {
                let mut program = parse_script(
                    trigger,
                    &Default::default(),
                    &db.fixed_rules
                        .read()
                        .expect("fixed_rules lock is not poisoned"),
                    cur_vld,
                )?
                .get_single_program()?;

                make_const_rule(
                    &mut program,
                    "_new",
                    kv_bindings.clone(),
                    new_tuples.to_vec(),
                );
                make_const_rule(
                    &mut program,
                    "_old",
                    kv_bindings.clone(),
                    old_tuples.to_vec(),
                );

                let (_, cleanups) = db.run_query(
                    self,
                    program,
                    cur_vld,
                    callback_targets,
                    callback_collector,
                    false,
                )?;
                to_clear.extend(cleanups);
            }
        }

        if is_callback_target {
            let target_collector = callback_collector
                .entry(relation_store.name.clone())
                .or_default();
            let headers = kv_bindings
                .into_iter()
                .map(|k| k.name.to_string())
                .collect_vec();
            target_collector.push((
                CallbackOp::Put,
                NamedRows::new(
                    headers.clone(),
                    new_tuples
                        .into_iter()
                        .map(|v| match v {
                            DataValue::List(l) => l,
                            _ => unreachable!(),
                        })
                        .collect_vec(),
                ),
                NamedRows::new(
                    headers,
                    old_tuples
                        .into_iter()
                        .map(|v| match v {
                            DataValue::List(l) => l,
                            _ => unreachable!(),
                        })
                        .collect_vec(),
                ),
            ))
        }
        Ok(())
    }

    pub(crate) fn update_in_index(
        &mut self,
        relation_store: &RelationHandle,
        new_kv: &[DataValue],
        old_kv: &[DataValue],
    ) -> Result<()> {
        for (idx_rel, idx_extractor) in relation_store.indices.values() {
            let idx_tup_old = idx_extractor
                .iter()
                .map(|i| old_kv[*i].clone())
                .collect_vec();
            let encoded_old = idx_rel.encode_key_for_store(&idx_tup_old, Default::default())?;
            self.store_tx.del(&encoded_old)?;

            let idx_tup_new = idx_extractor
                .iter()
                .map(|i| new_kv[*i].clone())
                .collect_vec();
            let encoded_new = idx_rel.encode_key_for_store(&idx_tup_new, Default::default())?;
            self.store_tx.put(&encoded_new, &[])?;
        }
        Ok(())
    }

    pub(crate) fn ensure_not_in_relation(
        &mut self,
        res_iter: impl Iterator<Item = Tuple>,
        headers: &[Symbol],
        cur_vld: ValidityTs,
        relation_store: &RelationHandle,
        metadata: &StoredRelationMetadata,
        key_bindings: &[Symbol],
        span: SourceSpan,
    ) -> Result<()> {
        if relation_store.access_level < AccessLevel::ReadOnly {
            return Err(InsufficientAccessSnafu {
                message: "Insufficient access level for this operation",
            }
            .build()
            .into());
        }

        let key_extractors = make_extractors(
            &relation_store.metadata.keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;

        for tuple in res_iter {
            let extracted: Vec<DataValue> = key_extractors
                .iter()
                .map(|ex| ex.extract_data(&tuple, cur_vld))
                .try_collect()?;
            let key = relation_store.encode_key_for_store(&extracted, span)?;
            let already_exists = if relation_store.is_temp {
                self.temp_store_tx.exists(&key, true)?
            } else {
                self.store_tx.exists(&key, true)?
            };
            if already_exists {
                return Err(StoredRelationSnafu {
                    message: format!(
                        "assertion failure for {:?} of {}: key exists in database",
                        extracted, relation_store.name
                    ),
                }
                .build()
                .into());
            }
        }
        Ok(())
    }

    pub(crate) fn ensure_in_relation(
        &mut self,
        res_iter: impl Iterator<Item = Tuple>,
        headers: &[Symbol],
        cur_vld: ValidityTs,
        relation_store: &RelationHandle,
        metadata: &StoredRelationMetadata,
        key_bindings: &[Symbol],
        span: SourceSpan,
    ) -> Result<()> {
        if relation_store.access_level < AccessLevel::ReadOnly {
            return Err(InsufficientAccessSnafu {
                message: "Insufficient access level for this operation",
            }
            .build()
            .into());
        }

        let mut key_extractors = make_extractors(
            &relation_store.metadata.keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;

        let val_extractors = make_extractors(
            &relation_store.metadata.non_keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;
        key_extractors.extend(val_extractors);

        for tuple in res_iter {
            let extracted: Vec<DataValue> = key_extractors
                .iter()
                .map(|ex| ex.extract_data(&tuple, cur_vld))
                .try_collect()?;

            let key = relation_store.encode_key_for_store(&extracted, span)?;
            let val = relation_store.encode_val_for_store(&extracted, span)?;

            let existing = if relation_store.is_temp {
                self.temp_store_tx.get(&key, true)?
            } else {
                self.store_tx.get(&key, true)?
            };
            match existing {
                None => {
                    return Err(StoredRelationSnafu {
                        message: format!(
                            "assertion failure for {:?} of {}: key does not exist in database",
                            extracted, relation_store.name
                        ),
                    }
                    .build()
                    .into());
                }
                Some(v) => {
                    if &v as &[u8] != &val as &[u8] {
                        return Err(StoredRelationSnafu {
                            message: format!(
                                "assertion failure for {:?} of {}: key exists in database, but value does not match",
                                extracted, relation_store.name
                            ),
                        }
                        .build()
                        .into());
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn remove_from_relation<'s, S: Storage<'s>>(
        &mut self,
        db: &Db<S>,
        res_iter: impl Iterator<Item = Tuple>,
        headers: &[Symbol],
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
        propagate_triggers: bool,
        to_clear: &mut Vec<(Vec<u8>, Vec<u8>)>,
        relation_store: &RelationHandle,
        metadata: &StoredRelationMetadata,
        key_bindings: &[Symbol],
        check_exists: bool,
        force_collect: &str,
        span: SourceSpan,
    ) -> Result<()> {
        let is_callback_target =
            callback_targets.contains(&relation_store.name) || force_collect == relation_store.name;

        if relation_store.access_level < AccessLevel::Protected {
            return Err(InsufficientAccessSnafu {
                message: "Insufficient access level for this operation",
            }
            .build()
            .into());
        }
        let key_extractors = make_extractors(
            &relation_store.metadata.keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;

        let need_to_collect = !force_collect.is_empty()
            || (!relation_store.is_temp
                && (is_callback_target
                    || (propagate_triggers && !relation_store.rm_triggers.is_empty())));
        let has_indices = !relation_store.indices.is_empty();
        let has_hnsw_indices = !relation_store.hnsw_indices.is_empty();
        let has_fts_indices = !relation_store.fts_indices.is_empty();
        let has_lsh_indices = !relation_store.lsh_indices.is_empty();
        let fts_processors = self.make_fts_lsh_processors(relation_store)?;
        let mut new_tuples: Vec<DataValue> = vec![];
        let mut old_tuples: Vec<DataValue> = vec![];
        let mut stack = vec![];

        for tuple in res_iter {
            let extracted: Vec<DataValue> = key_extractors
                .iter()
                .map(|ex| ex.extract_data(&tuple, cur_vld))
                .try_collect()?;
            let key = relation_store.encode_key_for_store(&extracted, span)?;
            if check_exists {
                let exists = if relation_store.is_temp {
                    self.temp_store_tx.exists(&key, false)?
                } else {
                    self.store_tx.exists(&key, false)?
                };
                if !exists {
                    return Err(StoredRelationSnafu {
                        message: format!(
                            "assertion failure for {:?} of {}: key does not exists in database",
                            extracted, relation_store.name
                        ),
                    }
                    .build()
                    .into());
                }
            }
            if need_to_collect
                || has_indices
                || has_hnsw_indices
                || has_fts_indices
                || has_lsh_indices
            {
                if let Some(existing) = self.store_tx.get(&key, false)? {
                    let mut tup = extracted.clone();
                    extend_tuple_from_v(&mut tup, &existing);
                    self.del_in_fts(relation_store, &mut stack, &fts_processors, &tup)?;
                    self.del_in_lsh(relation_store, &tup)?;
                    if has_indices {
                        for (idx_rel, extractor) in relation_store.indices.values() {
                            let idx_tup = extractor.iter().map(|i| tup[*i].clone()).collect_vec();
                            let encoded =
                                idx_rel.encode_key_for_store(&idx_tup, Default::default())?;
                            self.store_tx.del(&encoded)?;
                        }
                    }
                    if has_hnsw_indices {
                        for (idx_handle, _) in relation_store.hnsw_indices.values() {
                            self.hnsw_remove(relation_store, idx_handle, &extracted)?;
                        }
                    }
                    if need_to_collect {
                        old_tuples.push(DataValue::List(tup));
                    }
                }
                if need_to_collect {
                    new_tuples.push(DataValue::List(extracted.clone()));
                }
            }
            if relation_store.is_temp {
                self.temp_store_tx.del(&key)?;
            } else {
                self.store_tx.del(&key)?;
            }
        }

        if need_to_collect && !new_tuples.is_empty() {
            let k_bindings = relation_store
                .metadata
                .keys
                .iter()
                .map(|k| Symbol::new(k.name.clone(), Default::default()))
                .collect_vec();

            let v_bindings = relation_store
                .metadata
                .non_keys
                .iter()
                .map(|k| Symbol::new(k.name.clone(), Default::default()));
            let mut kv_bindings = k_bindings.clone();
            kv_bindings.extend(v_bindings);
            let kv_bindings = kv_bindings;

            if propagate_triggers {
                for trigger in &relation_store.rm_triggers {
                    let mut program = parse_script(
                        trigger,
                        &Default::default(),
                        &db.fixed_rules
                            .read()
                            .expect("fixed_rules lock is not poisoned"),
                        cur_vld,
                    )?
                    .get_single_program()?;

                    make_const_rule(&mut program, "_new", k_bindings.clone(), new_tuples.clone());

                    make_const_rule(
                        &mut program,
                        "_old",
                        kv_bindings.clone(),
                        old_tuples.clone(),
                    );

                    let (_, cleanups) = db.run_query(
                        self,
                        program,
                        cur_vld,
                        callback_targets,
                        callback_collector,
                        false,
                    )?;
                    to_clear.extend(cleanups);
                }
            }

            if is_callback_target {
                let target_collector = callback_collector
                    .entry(relation_store.name.clone())
                    .or_default();
                target_collector.push((
                    CallbackOp::Rm,
                    NamedRows::new(
                        k_bindings
                            .into_iter()
                            .map(|k| k.name.to_string())
                            .collect_vec(),
                        new_tuples
                            .into_iter()
                            .map(|v| match v {
                                DataValue::List(l) => l,
                                _ => unreachable!(),
                            })
                            .collect_vec(),
                    ),
                    NamedRows::new(
                        kv_bindings
                            .into_iter()
                            .map(|k| k.name.to_string())
                            .collect_vec(),
                        old_tuples
                            .into_iter()
                            .map(|v| match v {
                                DataValue::List(l) => l,
                                _ => unreachable!(),
                            })
                            .collect_vec(),
                    ),
                ))
            }
        }
        Ok(())
    }
}
