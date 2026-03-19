//! Stored relation mutation: put, delete, and index maintenance.
//! SessionTx methods for stored relation mutation and FTS/HNSW/LSH index maintenance.
//! Stored relation access operators.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
use crate::engine::query::error::*;
use compact_str::CompactString;
use itertools::Itertools;
use pest::Parser;

use crate::engine::data::expr::Bytecode;
use crate::engine::data::program::RelationOp;
use crate::engine::data::relation::StoredRelationMetadata;
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::{ENCODED_KEY_MIN_LEN, Tuple};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fts::tokenizer::TextAnalyzer;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::{DatalogParser, Rule, parse_script};
use crate::engine::runtime::callback::CallbackCollector;
use crate::engine::runtime::minhash_lsh::HashPermutations;
use crate::engine::runtime::relation::{
    AccessLevel, InputRelationHandle, RelationHandle, extend_tuple_from_v,
};
use crate::engine::storage::Storage;
use crate::engine::{DbCore as Db, SourceSpan, StoreTx};

use super::extractors::{make_extractors, make_update_extractors};

impl<'a> crate::engine::runtime::transact::SessionTx<'a> {
    pub(crate) fn execute_relation<'s, S: Storage<'s>>(
        &mut self,
        db: &Db<S>,
        res_iter: impl Iterator<Item = Tuple>,
        op: RelationOp,
        meta: &InputRelationHandle,
        headers: &[Symbol],
        cur_vld: ValidityTs,
        callback_targets: &BTreeSet<CompactString>,
        callback_collector: &mut CallbackCollector,
        propagate_triggers: bool,
        force_collect: &str,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut to_clear = vec![];
        let mut replaced_old_triggers = None;
        if op == RelationOp::Replace {
            if !propagate_triggers {
                return Err(StoredRelationSnafu {
                    message: "replace op in trigger is not allowed",
                }
                .build()
                .into());
            }
            if let Ok(old_handle) = self.get_relation(&meta.name, true) {
                if !old_handle.indices.is_empty() {
                    return Err(StoredRelationSnafu {
                        message: "cannot replace relation since it has indices",
                    }
                    .build()
                    .into());
                }
                if old_handle.access_level < AccessLevel::Normal {
                    return Err(InsufficientAccessSnafu {
                        message: "Insufficient access level for this operation",
                    }
                    .build()
                    .into());
                }
                if old_handle.has_triggers() {
                    replaced_old_triggers = Some((old_handle.put_triggers, old_handle.rm_triggers))
                }
                for trigger in &old_handle.replace_triggers {
                    let program = parse_script(
                        trigger,
                        &Default::default(),
                        &db.fixed_rules
                            .read()
                            .expect("fixed_rules lock is not poisoned"),
                        cur_vld,
                    )?
                    .get_single_program()?;

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
                let destroy_res = self.destroy_relation(&meta.name)?;
                if !meta.name.is_temp_store_name() {
                    to_clear.extend(destroy_res);
                }
            }
        }
        let mut relation_store = if op == RelationOp::Replace || op == RelationOp::Create {
            self.create_relation(meta.clone())?
        } else {
            self.get_relation(&meta.name, false)?
        };
        if let Some((old_put, old_retract)) = replaced_old_triggers {
            relation_store.put_triggers = old_put;
            relation_store.rm_triggers = old_retract;
        }
        let InputRelationHandle {
            metadata,
            key_bindings,
            dep_bindings,
            span,
            ..
        } = meta;

        match op {
            RelationOp::Rm | RelationOp::Delete => self.remove_from_relation(
                db,
                res_iter,
                headers,
                cur_vld,
                callback_targets,
                callback_collector,
                propagate_triggers,
                &mut to_clear,
                &relation_store,
                metadata,
                key_bindings,
                op == RelationOp::Delete,
                force_collect,
                *span,
            )?,
            RelationOp::Ensure => self.ensure_in_relation(
                res_iter,
                headers,
                cur_vld,
                &relation_store,
                metadata,
                key_bindings,
                *span,
            )?,
            RelationOp::EnsureNot => self.ensure_not_in_relation(
                res_iter,
                headers,
                cur_vld,
                &relation_store,
                metadata,
                key_bindings,
                *span,
            )?,
            RelationOp::Update => self.update_in_relation(
                db,
                res_iter,
                headers,
                cur_vld,
                callback_targets,
                callback_collector,
                propagate_triggers,
                &mut to_clear,
                &relation_store,
                metadata,
                key_bindings,
                force_collect,
                *span,
            )?,
            RelationOp::Create | RelationOp::Replace | RelationOp::Put | RelationOp::Insert => self
                .put_into_relation(
                    db,
                    res_iter,
                    headers,
                    cur_vld,
                    callback_targets,
                    callback_collector,
                    propagate_triggers,
                    &mut to_clear,
                    &relation_store,
                    metadata,
                    key_bindings,
                    dep_bindings,
                    op == RelationOp::Insert,
                    force_collect,
                    *span,
                )?,
        };

        Ok(to_clear)
    }

    fn put_into_relation<'s, S: Storage<'s>>(
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
        dep_bindings: &[Symbol],
        is_insert: bool,
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

        let mut key_extractors = make_extractors(
            &relation_store.metadata.keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;

        let need_to_collect = !force_collect.is_empty()
            || (!relation_store.is_temp
                && (is_callback_target
                    || (propagate_triggers && !relation_store.put_triggers.is_empty())));
        let has_indices = !relation_store.indices.is_empty();
        let has_hnsw_indices = !relation_store.hnsw_indices.is_empty();
        let has_fts_indices = !relation_store.fts_indices.is_empty();
        let has_lsh_indices = !relation_store.lsh_indices.is_empty();
        let mut new_tuples: Vec<DataValue> = vec![];
        let mut old_tuples: Vec<DataValue> = vec![];

        let val_extractors = if metadata.non_keys.is_empty() {
            make_extractors(
                &relation_store.metadata.non_keys,
                &metadata.keys,
                key_bindings,
                headers,
            )?
        } else {
            make_extractors(
                &relation_store.metadata.non_keys,
                &metadata.non_keys,
                dep_bindings,
                headers,
            )?
        };
        key_extractors.extend(val_extractors);
        let mut stack = vec![];
        let hnsw_filters = Self::make_hnsw_filters(relation_store)?;
        let fts_lsh_processors = self.make_fts_lsh_processors(relation_store)?;
        let lsh_perms = self.make_lsh_hash_perms(relation_store)?;

        for tuple in res_iter {
            let extracted: Vec<DataValue> = key_extractors
                .iter()
                .map(|ex| ex.extract_data(&tuple, cur_vld))
                .try_collect()?;

            let key = relation_store.encode_key_for_store(&extracted, span)?;

            if is_insert {
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

            let val = relation_store.encode_val_for_store(&extracted, span)?;

            if need_to_collect
                || has_indices
                || has_hnsw_indices
                || has_fts_indices
                || has_lsh_indices
            {
                if let Some(existing) = self.store_tx.get(&key, false)? {
                    let mut tup = extracted[0..relation_store.metadata.keys.len()].to_vec();
                    extend_tuple_from_v(&mut tup, &existing);
                    if has_indices && extracted != tup {
                        self.update_in_index(relation_store, &extracted, &tup)?;
                        self.del_in_fts(relation_store, &mut stack, &fts_lsh_processors, &tup)?;
                        self.del_in_lsh(relation_store, &tup)?;
                    }

                    if need_to_collect {
                        old_tuples.push(DataValue::List(tup));
                    }
                } else if has_indices {
                    for (idx_rel, extractor) in relation_store.indices.values() {
                        let idx_tup_new = extractor
                            .iter()
                            .map(|i| extracted[*i].clone())
                            .collect_vec();
                        let encoded_new =
                            idx_rel.encode_key_for_store(&idx_tup_new, Default::default())?;
                        self.store_tx.put(&encoded_new, &[])?;
                    }
                }

                self.update_in_hnsw(relation_store, &mut stack, &hnsw_filters, &extracted)?;
                self.put_in_fts(relation_store, &mut stack, &fts_lsh_processors, &extracted)?;
                self.put_in_lsh(
                    relation_store,
                    &mut stack,
                    &fts_lsh_processors,
                    &extracted,
                    &lsh_perms,
                )?;

                if need_to_collect {
                    new_tuples.push(DataValue::List(extracted));
                }
            }

            if relation_store.is_temp {
                self.temp_store_tx.put(&key, &val)?;
            } else {
                self.store_tx.put(&key, &val)?;
            }
        }

        if need_to_collect && !new_tuples.is_empty() {
            self.collect_mutations(
                db,
                cur_vld,
                callback_targets,
                callback_collector,
                propagate_triggers,
                to_clear,
                relation_store,
                is_callback_target,
                new_tuples,
                old_tuples,
            )?;
        }
        Ok(())
    }

    pub(crate) fn put_in_fts(
        &mut self,
        rel_handle: &RelationHandle,
        stack: &mut Vec<DataValue>,
        processors: &BTreeMap<CompactString, (Arc<TextAnalyzer>, Vec<Bytecode>)>,
        new_kv: &[DataValue],
    ) -> Result<()> {
        for (k, (idx_handle, _)) in rel_handle.fts_indices.iter() {
            let (tokenizer, extractor) = processors
                .get(k)
                .expect("FTS processor always present: built from same fts_indices keys");
            self.put_fts_index_item(new_kv, extractor, stack, tokenizer, rel_handle, idx_handle)?;
        }
        Ok(())
    }

    pub(crate) fn del_in_fts(
        &mut self,
        rel_handle: &RelationHandle,
        stack: &mut Vec<DataValue>,
        processors: &BTreeMap<CompactString, (Arc<TextAnalyzer>, Vec<Bytecode>)>,
        old_kv: &[DataValue],
    ) -> Result<()> {
        for (k, (idx_handle, _)) in rel_handle.fts_indices.iter() {
            let (tokenizer, extractor) = processors
                .get(k)
                .expect("FTS processor always present: built from same fts_indices keys");
            self.del_fts_index_item(old_kv, extractor, stack, tokenizer, rel_handle, idx_handle)?;
        }
        Ok(())
    }

    pub(crate) fn put_in_lsh(
        &mut self,
        rel_handle: &RelationHandle,
        stack: &mut Vec<DataValue>,
        processors: &BTreeMap<CompactString, (Arc<TextAnalyzer>, Vec<Bytecode>)>,
        new_kv: &[DataValue],
        hash_perms_map: &BTreeMap<CompactString, HashPermutations>,
    ) -> Result<()> {
        for (k, (idx_handle, inv_idx_handle, manifest)) in rel_handle.lsh_indices.iter() {
            let (tokenizer, extractor) = processors
                .get(k)
                .expect("LSH processor always present: built from same lsh_indices keys");
            self.put_lsh_index_item(
                new_kv,
                extractor,
                stack,
                tokenizer,
                rel_handle,
                idx_handle,
                inv_idx_handle,
                manifest,
                hash_perms_map
                    .get(k)
                    .expect("hash_perms always present: built from same lsh_indices keys"),
            )?;
        }
        Ok(())
    }

    pub(crate) fn del_in_lsh(
        &mut self,
        rel_handle: &RelationHandle,
        old_kv: &[DataValue],
    ) -> Result<()> {
        for (idx_handle, inv_idx_handle, _) in rel_handle.lsh_indices.values() {
            self.del_lsh_index_item(old_kv, None, idx_handle, inv_idx_handle)?;
        }
        Ok(())
    }

    pub(crate) fn update_in_hnsw(
        &mut self,
        relation_store: &RelationHandle,
        stack: &mut Vec<DataValue>,
        hnsw_filters: &BTreeMap<CompactString, Vec<Bytecode>>,
        new_kv: &[DataValue],
    ) -> Result<()> {
        for (name, (idx_handle, idx_manifest)) in relation_store.hnsw_indices.iter() {
            let filter = hnsw_filters.get(name);
            self.hnsw_put(
                idx_manifest,
                relation_store,
                idx_handle,
                filter,
                stack,
                new_kv,
            )?;
        }
        Ok(())
    }

    pub(crate) fn make_lsh_hash_perms(
        &self,
        relation_store: &RelationHandle,
    ) -> Result<BTreeMap<CompactString, HashPermutations>> {
        let mut perms = BTreeMap::new();
        for (name, (_, _, manifest)) in relation_store.lsh_indices.iter() {
            perms.insert(name.clone(), manifest.get_hash_perms()?);
        }
        Ok(perms)
    }

    pub(crate) fn make_fts_lsh_processors(
        &self,
        relation_store: &RelationHandle,
    ) -> Result<BTreeMap<CompactString, (Arc<TextAnalyzer>, Vec<Bytecode>)>> {
        let mut processors = BTreeMap::new();
        for (name, (_, manifest)) in relation_store.fts_indices.iter() {
            let tokenizer = self
                .tokenizers
                .get(name, &manifest.tokenizer, &manifest.filters)?;

            let parsed = DatalogParser::parse(Rule::expr, &manifest.extractor)
                .map_err(|e| {
                    CompilationFailedSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?
                .next()
                .ok_or_else(|| {
                    crate::engine::error::InternalError::from(
                        CompilationFailedSnafu {
                            message: format!(
                                "FTS extractor expression for '{name}' parsed to empty iterator"
                            ),
                        }
                        .build(),
                    )
                })?;
            let mut code_expr = build_expr(parsed, &Default::default())?;
            let binding_map = relation_store.raw_binding_map();
            code_expr.fill_binding_indices(&binding_map)?;
            let extractor = code_expr.compile()?;
            processors.insert(name.clone(), (tokenizer, extractor));
        }
        for (name, (_, _, manifest)) in relation_store.lsh_indices.iter() {
            let tokenizer = self
                .tokenizers
                .get(name, &manifest.tokenizer, &manifest.filters)?;

            let parsed = DatalogParser::parse(Rule::expr, &manifest.extractor)
                .map_err(|e| {
                    CompilationFailedSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?
                .next()
                .ok_or_else(|| {
                    crate::engine::error::InternalError::from(
                        CompilationFailedSnafu {
                            message: format!(
                                "LSH extractor expression for '{name}' parsed to empty iterator"
                            ),
                        }
                        .build(),
                    )
                })?;
            let mut code_expr = build_expr(parsed, &Default::default())?;
            let binding_map = relation_store.raw_binding_map();
            code_expr.fill_binding_indices(&binding_map)?;
            let extractor = code_expr.compile()?;
            processors.insert(name.clone(), (tokenizer, extractor));
        }
        Ok(processors)
    }

    pub(crate) fn make_hnsw_filters(
        relation_store: &RelationHandle,
    ) -> Result<BTreeMap<CompactString, Vec<Bytecode>>> {
        let mut hnsw_filters = BTreeMap::new();
        for (name, (_, manifest)) in relation_store.hnsw_indices.iter() {
            if let Some(f_code) = &manifest.index_filter {
                let parsed = DatalogParser::parse(Rule::expr, f_code)
                    .map_err(|e| {
                        CompilationFailedSnafu {
                            message: e.to_string(),
                        }
                        .build()
                    })?
                    .next()
                    .ok_or_else(|| {
                        crate::engine::error::InternalError::from(
                            CompilationFailedSnafu {
                                message: format!(
                                    "HNSW index filter for '{name}' parsed to empty iterator"
                                ),
                            }
                            .build(),
                        )
                    })?;
                let mut code_expr = build_expr(parsed, &Default::default())?;
                let binding_map = relation_store.raw_binding_map();
                code_expr.fill_binding_indices(&binding_map)?;
                hnsw_filters.insert(name.clone(), code_expr.compile()?);
            }
        }
        Ok(hnsw_filters)
    }

    fn update_in_relation<'s, S: Storage<'s>>(
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
                    || (propagate_triggers && !relation_store.put_triggers.is_empty())));
        let has_indices = !relation_store.indices.is_empty();
        let has_hnsw_indices = !relation_store.hnsw_indices.is_empty();
        let has_fts_indices = !relation_store.fts_indices.is_empty();
        let has_lsh_indices = !relation_store.lsh_indices.is_empty();
        let mut new_tuples: Vec<DataValue> = vec![];
        let mut old_tuples: Vec<DataValue> = vec![];

        let val_extractors = make_update_extractors(
            &relation_store.metadata.non_keys,
            &metadata.keys,
            key_bindings,
            headers,
        )?;

        let mut stack = vec![];
        let hnsw_filters = Self::make_hnsw_filters(relation_store)?;
        let fts_lsh_processors = self.make_fts_lsh_processors(relation_store)?;
        let lsh_perms = self.make_lsh_hash_perms(relation_store)?;

        for tuple in res_iter {
            let mut new_kv: Vec<DataValue> = key_extractors
                .iter()
                .map(|ex| ex.extract_data(&tuple, cur_vld))
                .try_collect()?;

            let key = relation_store.encode_key_for_store(&new_kv, span)?;
            let original_val_bytes = if relation_store.is_temp {
                self.temp_store_tx.get(&key, true)?
            } else {
                self.store_tx.get(&key, true)?
            };
            let original_val: Tuple = match original_val_bytes {
                None => {
                    return Err(StoredRelationSnafu {
                        message: format!(
                            "assertion failure for {:?} of {}: key to update does not exist",
                            new_kv, relation_store.name
                        ),
                    }
                    .build()
                    .into());
                }
                Some(v) => rmp_serde::from_slice(&v[ENCODED_KEY_MIN_LEN..]).map_err(|e| {
                    EvalFailedSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?,
            };
            let mut old_kv = Vec::with_capacity(relation_store.arity());
            old_kv.extend_from_slice(&new_kv);
            old_kv.extend_from_slice(&original_val);
            new_kv.reserve_exact(relation_store.arity());
            for (i, extractor) in val_extractors.iter().enumerate() {
                match extractor {
                    None => {
                        new_kv.push(original_val[i].clone());
                    }
                    Some(ex) => {
                        let val = ex.extract_data(&tuple, cur_vld)?;
                        new_kv.push(val);
                    }
                }
            }
            let new_val = relation_store.encode_val_for_store(&new_kv, span)?;

            if need_to_collect
                || has_indices
                || has_hnsw_indices
                || has_fts_indices
                || has_lsh_indices
            {
                self.del_in_fts(relation_store, &mut stack, &fts_lsh_processors, &old_kv)?;
                self.del_in_lsh(relation_store, &old_kv)?;
                self.update_in_index(relation_store, &new_kv, &old_kv)?;

                if need_to_collect {
                    old_tuples.push(DataValue::List(old_kv));
                }

                self.update_in_hnsw(relation_store, &mut stack, &hnsw_filters, &new_kv)?;
                self.put_in_fts(relation_store, &mut stack, &fts_lsh_processors, &new_kv)?;
                self.put_in_lsh(
                    relation_store,
                    &mut stack,
                    &fts_lsh_processors,
                    &new_kv,
                    &lsh_perms,
                )?;

                if need_to_collect {
                    new_tuples.push(DataValue::List(new_kv));
                }
            }

            if relation_store.is_temp {
                self.temp_store_tx.put(&key, &new_val)?;
            } else {
                self.store_tx.put(&key, &new_val)?;
            }
        }

        if need_to_collect && !new_tuples.is_empty() {
            self.collect_mutations(
                db,
                cur_vld,
                callback_targets,
                callback_collector,
                propagate_triggers,
                to_clear,
                relation_store,
                is_callback_target,
                new_tuples,
                old_tuples,
            )?;
        }
        Ok(())
    }
}
