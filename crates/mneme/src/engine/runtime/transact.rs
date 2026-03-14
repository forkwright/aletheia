//! Schema and relation transact operations.
#![expect(
    clippy::unwrap_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64};

use crate::engine::data::program::ReturnMutation;
use crate::engine::error::InternalResult as Result;
use crate::engine::runtime::error::StorageVersionSnafu;

use crate::engine::data::tuple::TupleT;
use crate::engine::data::value::DataValue;
use crate::engine::fts::TokenizerCache;
use crate::engine::runtime::callback::CallbackCollector;
use crate::engine::runtime::relation::RelationId;
use crate::engine::storage::StoreTx;
use crate::engine::storage::temp::TempTx;
use crate::engine::{CallbackOp, NamedRows};

pub struct SessionTx<'a> {
    pub(crate) store_tx: Box<dyn StoreTx<'a> + 'a>,
    pub(crate) temp_store_tx: TempTx,
    pub(crate) relation_store_id: Arc<AtomicU64>,
    pub(crate) temp_store_id: AtomicU32,
    pub(crate) tokenizers: Arc<TokenizerCache>,
}

pub const CURRENT_STORAGE_VERSION: [u8; 1] = [0x00];

fn storage_version_key() -> Vec<u8> {
    let storage_version_tuple = vec![DataValue::Null, DataValue::from("STORAGE_VERSION")];
    storage_version_tuple.encode_as_key(RelationId::SYSTEM)
}

const STATUS_STR: &str = "status";
const OK_STR: &str = "OK";

impl<'a> SessionTx<'a> {
    pub(crate) fn get_returning_rows(
        &self,
        callback_collector: &mut CallbackCollector,
        rel: &str,
        returning: &ReturnMutation,
    ) -> Result<NamedRows> {
        let returned_rows = {
            match returning {
                ReturnMutation::NotReturning => NamedRows::new(
                    vec![STATUS_STR.to_string()],
                    vec![vec![DataValue::from(OK_STR)]],
                ),
                ReturnMutation::Returning => {
                    let meta = self.get_relation(rel, false)?;
                    let target_len = meta.metadata.keys.len() + meta.metadata.non_keys.len();
                    let mut returned_rows = Vec::new();
                    if let Some(collected) = callback_collector.get(&meta.name) {
                        for (kind, insertions, deletions) in collected {
                            let (pos_key, neg_key) = match kind {
                                CallbackOp::Put => ("inserted", "replaced"),
                                CallbackOp::Rm => ("requested", "deleted"),
                            };
                            for row in &insertions.rows {
                                let mut v = Vec::with_capacity(target_len + 1);
                                v.push(DataValue::from(pos_key));
                                v.extend_from_slice(row);
                                while v.len() <= target_len {
                                    v.push(DataValue::Null);
                                }
                                returned_rows.push(v);
                            }
                            for row in &deletions.rows {
                                let mut v = Vec::with_capacity(target_len + 1);
                                v.push(DataValue::from(neg_key));
                                v.extend_from_slice(row);
                                while v.len() <= target_len {
                                    v.push(DataValue::Null);
                                }
                                returned_rows.push(v);
                            }
                        }
                    }
                    let mut header = vec!["_kind".to_string()];
                    header.extend(
                        meta.metadata
                            .keys
                            .iter()
                            .chain(meta.metadata.non_keys.iter())
                            .map(|s| s.name.to_string()),
                    );
                    NamedRows::new(header, returned_rows)
                }
            }
        };
        Ok(returned_rows)
    }

    pub(crate) fn init_storage(&mut self) -> Result<RelationId> {
        let tuple = vec![DataValue::Null];
        let t_encoded = tuple.encode_as_key(RelationId::SYSTEM);
        let found = self.store_tx.get(&t_encoded, false)?;
        let storage_version_key = storage_version_key();
        let ret = match found {
            None => {
                self.store_tx
                    .put(&storage_version_key, &CURRENT_STORAGE_VERSION)?;
                self.store_tx
                    .put(&t_encoded, &RelationId::new(0)?.raw_encode())?;
                RelationId::SYSTEM
            }
            Some(slice) => {
                let version_found = self.store_tx.get(&storage_version_key, false)?;
                match version_found {
                    None => {
                        StorageVersionSnafu {
                            message: "Storage is used but un-versioned, probably created by an incompatible version.",
                        }
                        .fail()?
                    }
                    Some(v) => {
                        if v != CURRENT_STORAGE_VERSION {
                            StorageVersionSnafu {
                                message: format!(
                                    "Version mismatch: expect storage version {:?}, got {:?}",
                                    CURRENT_STORAGE_VERSION, v
                                ),
                            }
                            .fail()?
                        }
                    }
                }
                RelationId::raw_decode(&slice)?
            }
        };
        Ok(ret)
    }

    pub fn commit_tx(&mut self) -> Result<()> {
        self.store_tx.commit()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Db transaction methods
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::iter;
use std::sync::atomic::Ordering;

use compact_str::CompactString;
use crossbeam::channel::{Receiver, Sender};
use itertools::Itertools;

use crate::engine::data::functions::current_validity;
use crate::engine::data::relation::ColumnDef;
use crate::engine::data::tuple::Tuple;
use crate::engine::decode_tuple_from_kv;
use crate::engine::parse::parse_script;
use crate::engine::runtime::db::{Db, TransactionPayload};
use crate::engine::runtime::error::{InsufficientAccessSnafu, InvalidOperationSnafu};
use crate::engine::runtime::relation::{AccessLevel, extend_tuple_from_v};
use crate::engine::storage::Storage;

impl<'s, S: Storage<'s>> Db<S> {
    pub(crate) fn load_last_ids(&'s self) -> Result<()> {
        let mut tx = self.transact_write()?;
        self.relation_store_id
            .store(tx.init_storage()?.0, Ordering::Release);
        tx.commit_tx()?;
        Ok(())
    }
    pub(crate) fn transact(&'s self) -> Result<SessionTx<'s>> {
        let ret = SessionTx {
            store_tx: Box::new(self.db.transact(false)?),
            temp_store_tx: self.temp_db.transact(true)?,
            relation_store_id: self.relation_store_id.clone(),
            temp_store_id: Default::default(),
            tokenizers: self.tokenizers.clone(),
        };
        Ok(ret)
    }
    pub(crate) fn transact_write(&'s self) -> Result<SessionTx<'s>> {
        let ret = SessionTx {
            store_tx: Box::new(self.db.transact(true)?),
            temp_store_tx: self.temp_db.transact(true)?,
            relation_store_id: self.relation_store_id.clone(),
            temp_store_id: Default::default(),
            tokenizers: self.tokenizers.clone(),
        };
        Ok(ret)
    }

    /// Run a multi-transaction. A command should be sent to `payloads`, and the result should be
    /// retrieved from `results`. A transaction ends when it receives a `Commit` or `Abort`,
    /// or when a query is not successful. After a transaction ends, sending / receiving from
    /// the channels will fail.
    ///
    /// Write transactions _may_ block other reads, but we guarantee that this does not happen
    /// for the RocksDB backend.
    pub fn run_multi_transaction(
        &'s self,
        is_write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<Result<NamedRows>>,
    ) {
        let tx = if is_write {
            self.transact_write()
        } else {
            self.transact()
        };
        let mut cleanups: Vec<(Vec<u8>, Vec<u8>)> = vec![];
        let mut tx = match tx {
            Ok(tx) => tx,
            Err(err) => {
                let _ = results.send(Err(err));
                return;
            }
        };

        let ts = current_validity();
        let callback_targets = self.current_callback_targets();
        let mut callback_collector = BTreeMap::new();
        let mut write_locks = BTreeMap::new();

        for payload in payloads {
            match payload {
                TransactionPayload::Commit => {
                    for (lower, upper) in cleanups {
                        if let Err(err) = tx.store_tx.del_range_from_persisted(&lower, &upper) {
                            eprintln!("{err:?}")
                        }
                    }

                    let _ = results.send(tx.commit_tx().map(|_| NamedRows::default()));
                    #[cfg(not(target_arch = "wasm32"))]
                    if !callback_collector.is_empty() {
                        self.send_callbacks(callback_collector)
                    }

                    break;
                }
                TransactionPayload::Abort => {
                    let _ = results.send(Ok(NamedRows::default()));
                    break;
                }
                TransactionPayload::Query((script, params)) => {
                    let p = match parse_script(&script, &params, &self.fixed_rules.read().unwrap(), ts) // INVARIANT: lock is not poisoned
                        {
                            Ok(p) => p,
                            Err(err) => {
                                if results.send(Err(err)).is_err() {
                                    break;
                                } else {
                                    continue;
                                }
                            }
                        };

                    let p = match p.get_single_program() {
                        Ok(p) => p,
                        Err(err) => {
                            if results.send(Err(err)).is_err() {
                                break;
                            } else {
                                continue;
                            }
                        }
                    };
                    if let Some(write_lock_name) = p.needs_write_lock() {
                        match write_locks.entry(write_lock_name) {
                            Entry::Vacant(e) => {
                                let lock = self
                                    .obtain_relation_locks(iter::once(e.key()))
                                    .pop()
                                    .unwrap();
                                e.insert(lock);
                            }
                            Entry::Occupied(_) => {}
                        }
                    }

                    let res = self.execute_single_program(
                        p,
                        &mut tx,
                        &mut cleanups,
                        ts,
                        &callback_targets,
                        &mut callback_collector,
                    );
                    if results.send(res).is_err() {
                        break;
                    }
                }
            }
        }
    }

    /// Export relations to JSON data.
    ///
    /// `relations` contains names of the stored relations to export.
    pub fn export_relations<I, T>(&'s self, relations: I) -> Result<BTreeMap<String, NamedRows>>
    where
        T: AsRef<str>,
        I: Iterator<Item = T>,
    {
        let tx = self.transact()?;
        let mut ret: BTreeMap<String, NamedRows> = BTreeMap::new();
        for rel in relations {
            let handle = tx.get_relation(rel.as_ref(), false)?;
            let size_hint = handle.metadata.keys.len() + handle.metadata.non_keys.len();

            if handle.access_level < AccessLevel::ReadOnly {
                InsufficientAccessSnafu {
                    operation: "export relation",
                }
                .fail()?;
            }

            let mut cols = handle
                .metadata
                .keys
                .iter()
                .map(|col| col.name.clone())
                .collect_vec();
            cols.extend(
                handle
                    .metadata
                    .non_keys
                    .iter()
                    .map(|col| col.name.clone())
                    .collect_vec(),
            );

            let start = Tuple::default().encode_as_key(handle.id);
            let end = Tuple::default().encode_as_key(handle.id.next()?);

            let mut rows = vec![];
            for data in tx.store_tx.range_scan(&start, &end) {
                let (k, v) = data?;
                let tuple = decode_tuple_from_kv(&k, &v, Some(size_hint));
                rows.push(tuple);
            }
            let headers = cols.iter().map(|col| col.to_string()).collect_vec();
            ret.insert(rel.as_ref().to_string(), NamedRows::new(headers, rows));
        }
        Ok(ret)
    }
    /// Import relations. The argument `data` accepts data in the shape of
    /// what was returned by [Self::export_relations].
    /// The target stored relations must already exist in the database.
    /// Any associated indices will be updated.
    ///
    /// Note that triggers and callbacks are _not_ run for the relations, if any exists.
    /// If you need to activate triggers or callbacks, use queries with parameters.
    pub fn import_relations(&'s self, data: BTreeMap<String, NamedRows>) -> Result<()> {
        let rel_names = data.keys().map(CompactString::from).collect_vec();
        let locks = self.obtain_relation_locks(rel_names.iter());
        let _guards = locks.iter().map(|l| l.read().unwrap()).collect_vec(); // INVARIANT: lock is not poisoned

        let cur_vld = current_validity();

        let mut tx = self.transact_write()?;

        for (relation_op, in_data) in data {
            let is_delete;
            let relation: &str = match relation_op.strip_prefix('-') {
                None => {
                    is_delete = false;
                    &relation_op
                }
                Some(s) => {
                    is_delete = true;
                    s
                }
            };
            if relation.contains(':') {
                InvalidOperationSnafu {
                    op: "import relations",
                    reason: "cannot import data into relation as it is an index",
                }
                .fail()?;
            }
            let handle = tx.get_relation(relation, false)?;
            let has_indices = !handle.indices.is_empty();

            if handle.access_level < AccessLevel::Protected {
                InsufficientAccessSnafu {
                    operation: "import into stored relation",
                }
                .fail()?;
            }

            let header2idx: BTreeMap<_, _> = in_data
                .headers
                .iter()
                .enumerate()
                .map(|(i, k)| -> Result<(&str, usize)> { Ok((k as &str, i)) })
                .try_collect()?;

            let key_indices: Vec<_> = handle
                .metadata
                .keys
                .iter()
                .map(|col| -> Result<(usize, &ColumnDef)> {
                    let idx = header2idx.get(&col.name as &str).ok_or_else(|| {
                        crate::engine::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "import",
                                reason: format!(
                                    "required header {} not found for relation {}",
                                    col.name, relation
                                ),
                            }
                            .build(),
                        }
                    })?;
                    Ok((*idx, col))
                })
                .try_collect()?;

            let val_indices: Vec<_> = if is_delete {
                vec![]
            } else {
                handle
                    .metadata
                    .non_keys
                    .iter()
                    .map(|col| -> Result<(usize, &ColumnDef)> {
                        let idx = header2idx.get(&col.name as &str).ok_or_else(|| {
                            crate::engine::error::InternalError::Runtime {
                                source: InvalidOperationSnafu {
                                    op: "import",
                                    reason: format!(
                                        "required header {} not found for relation {}",
                                        col.name, relation
                                    ),
                                }
                                .build(),
                            }
                        })?;
                        Ok((*idx, col))
                    })
                    .try_collect()?
            };

            for row in in_data.rows {
                let keys: Vec<_> = key_indices
                    .iter()
                    .map(|(i, col)| -> Result<DataValue> {
                        let v = row.get(*i).ok_or_else(|| {
                            crate::engine::error::InternalError::Runtime {
                                source: InvalidOperationSnafu {
                                    op: "import",
                                    reason: format!("row too short: {:?}", row),
                                }
                                .build(),
                            }
                        })?;
                        col.typing.coerce(v.clone(), cur_vld).map_err(|e| {
                            crate::engine::error::InternalError::Runtime {
                                source: InvalidOperationSnafu {
                                    op: "import",
                                    reason: e.to_string(),
                                }
                                .build(),
                            }
                        })
                    })
                    .try_collect()?;
                let k_store = handle.encode_key_for_store(&keys, Default::default())?;
                if has_indices && let Some(existing) = tx.store_tx.get(&k_store, false)? {
                    let mut old = keys.clone();
                    extend_tuple_from_v(&mut old, &existing);
                    if is_delete || old != row {
                        for (idx_rel, extractor) in handle.indices.values() {
                            let idx_tup = extractor.iter().map(|i| old[*i].clone()).collect_vec();
                            let encoded =
                                idx_rel.encode_key_for_store(&idx_tup, Default::default())?;
                            tx.store_tx.del(&encoded)?;
                        }
                    }
                }
                if is_delete {
                    tx.store_tx.del(&k_store)?;
                } else {
                    let vals: Vec<_> = val_indices
                        .iter()
                        .map(|(i, col)| -> Result<DataValue> {
                            let v = row.get(*i).ok_or_else(|| {
                                crate::engine::error::InternalError::Runtime {
                                    source: InvalidOperationSnafu {
                                        op: "import",
                                        reason: format!("row too short: {:?}", row),
                                    }
                                    .build(),
                                }
                            })?;
                            col.typing.coerce(v.clone(), cur_vld).map_err(|e| {
                                crate::engine::error::InternalError::Runtime {
                                    source: InvalidOperationSnafu {
                                        op: "import",
                                        reason: e.to_string(),
                                    }
                                    .build(),
                                }
                            })
                        })
                        .try_collect()?;
                    let v_store = handle.encode_val_only_for_store(&vals, Default::default())?;
                    tx.store_tx.put(&k_store, &v_store)?;
                    if has_indices {
                        let mut kv = keys;
                        kv.extend(vals);
                        for (idx_rel, extractor) in handle.indices.values() {
                            let idx_tup = extractor.iter().map(|i| kv[*i].clone()).collect_vec();
                            let encoded =
                                idx_rel.encode_key_for_store(&idx_tup, Default::default())?;
                            tx.store_tx.put(&encoded, &[])?;
                        }
                    }
                }
            }
        }
        tx.commit_tx()?;
        Ok(())
    }
}
