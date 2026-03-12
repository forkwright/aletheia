//! Embedded Datalog engine with HNSW and graph support.

use std::collections::BTreeMap;
use std::path::Path;

use crossbeam::channel::{Receiver, Sender, bounded};

pub mod error;
pub use error::{Error, Result};

pub use crate::engine::data::value::{DataValue, ValidityTs, Vector};
pub use crate::engine::fixed_rule::{FixedRule, FixedRuleInputRelation, FixedRulePayload};
pub use crate::engine::runtime::callback::CallbackOp;
pub use crate::engine::runtime::db::{NamedRows, ScriptMutability, TransactionPayload};
#[cfg(feature = "storage-fjall")]
pub use crate::engine::storage::fjall_backend::FjallStorage;
pub use crate::engine::storage::mem::MemStorage;
pub use ndarray::Array1;

pub(crate) use crate::engine::data::expr::Expr;
pub(crate) use crate::engine::data::symb::Symbol;
pub(crate) use crate::engine::parse::SourceSpan;
pub(crate) use crate::engine::runtime::db::Db as DbCore;
pub(crate) use crate::engine::runtime::relation::decode_tuple_from_kv;
pub(crate) use crate::engine::storage::{Storage, StoreTx};
#[cfg(test)]
pub(crate) type DbInstance =
    crate::engine::runtime::db::Db<crate::engine::storage::mem::MemStorage>;

#[expect(
    unsafe_code,
    private_interfaces,
    clippy::pedantic,
    clippy::float_cmp,
    clippy::mutable_key_type,
    clippy::result_large_err,
    reason = "vendored CozoDB engine — unsafe for DataValue layout, pedantic lints deferred"
)]
pub(crate) mod data;
#[expect(
    private_interfaces,
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::result_large_err,
    clippy::type_complexity,
    reason = "vendored CozoDB engine — graph algorithm signatures are domain-inherent"
)]
pub(crate) mod fixed_rule;
#[expect(
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::result_large_err,
    clippy::too_many_arguments,
    reason = "vendored CozoDB engine — FTS tokenizer data files and Unicode tables"
)]
pub(crate) mod fts;
#[expect(
    private_interfaces,
    clippy::pedantic,
    clippy::needless_return,
    clippy::result_large_err,
    clippy::type_complexity,
    reason = "vendored CozoDB engine — parser signatures are domain-inherent"
)]
pub(crate) mod parse;
#[expect(
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "vendored CozoDB engine — query planner complexity is inherent"
)]
pub(crate) mod query;
#[expect(
    private_interfaces,
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::needless_return,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "vendored CozoDB engine — runtime DB core with unsafe storage layer"
)]
pub(crate) mod runtime;
#[expect(
    clippy::pedantic,
    clippy::result_large_err,
    clippy::type_complexity,
    reason = "vendored CozoDB engine — storage backend trait implementations"
)]
pub(crate) mod storage;
#[expect(
    clippy::pedantic,
    reason = "vendored CozoDB engine — utility functions"
)]
pub(crate) mod utils;

/// Convert an `InternalError` to the public `Error` type.
///
/// Specific internal error types map to typed public variants where possible.
/// Everything else falls back to `Error::Engine { message }`.
fn convert_internal(e: crate::engine::error::InternalError) -> Error {
    use crate::engine::error::InternalError;
    use snafu::IntoError;
    match e {
        InternalError::Runtime {
            source: crate::engine::runtime::error::RuntimeError::QueryKilled { .. },
        } => error::QueryKilledSnafu.build(),
        InternalError::Parse { source } => error::ParseSnafu.into_error(source),
        InternalError::Storage { source } => error::StorageSnafu.into_error(source),
        other => error::EngineSnafu {
            message: other.to_string(),
        }
        .build(),
    }
}

/// Public facade replacing `DbInstance`. Dispatches to concrete storage implementations.
pub enum Db {
    Mem(crate::engine::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-fjall")]
    Fjall(crate::engine::runtime::db::Db<FjallStorage>),
}

#[expect(
    clippy::result_large_err,
    reason = "engine Error carries structured context — boxing deferred to avoid API churn"
)]
impl Db {
    /// Open an in-memory database.
    pub fn open_mem() -> crate::engine::Result<Self> {
        crate::engine::storage::mem::new_mem_db()
            .map(Db::Mem)
            .map_err(convert_internal)
    }

    /// Open a fjall-backed database at the given path.
    ///
    /// Primary production backend: pure Rust, LSM-tree, LZ4 compression,
    /// native read-your-own-writes.
    #[cfg(feature = "storage-fjall")]
    pub fn open_fjall(path: impl AsRef<Path>) -> crate::engine::Result<Self> {
        crate::engine::storage::fjall_backend::new_cozo_fjall(path)
            .map(Db::Fjall)
            .map_err(convert_internal)
    }

    /// Execute a Datalog script.
    pub fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> crate::engine::Result<NamedRows> {
        let result = match self {
            Db::Mem(db) => db.run_script(script, params, mutability),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.run_script(script, params, mutability),
        };
        result.map_err(convert_internal)
    }

    /// Execute a Datalog script in read-only mode.
    pub fn run_read_only(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::engine::Result<NamedRows> {
        self.run(script, params, ScriptMutability::Immutable)
    }

    /// Backup the running database into an `SQLite` file.
    ///
    /// Not currently supported — requires the removed `storage-sqlite` feature.
    pub fn backup_db(&self, out_file: impl AsRef<Path>) -> crate::engine::Result<()> {
        let path = out_file.as_ref();
        let result = match self {
            Db::Mem(db) => db.backup_db(path),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.backup_db(path),
        };
        result.map_err(convert_internal)
    }

    /// Restore from an `SQLite` backup.
    ///
    /// Not currently supported — requires the removed `storage-sqlite` feature.
    pub fn restore_backup(&self, in_file: impl AsRef<Path>) -> crate::engine::Result<()> {
        let path = in_file.as_ref();
        let result = match self {
            Db::Mem(db) => db.restore_backup(path),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.restore_backup(path),
        };
        result.map_err(convert_internal)
    }

    /// Import data from relations in a backup file.
    ///
    /// Not currently supported — requires the removed `storage-sqlite` feature.
    pub fn import_from_backup(
        &self,
        in_file: impl AsRef<Path>,
        relations: &[String],
    ) -> crate::engine::Result<()> {
        let path = in_file.as_ref();
        let result = match self {
            Db::Mem(db) => db.import_from_backup(path, relations),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.import_from_backup(path, relations),
        };
        result.map_err(convert_internal)
    }

    /// Export relations for backup.
    pub fn export_relations<I, T>(
        &self,
        relations: I,
    ) -> crate::engine::Result<BTreeMap<String, NamedRows>>
    where
        I: Iterator<Item = T>,
        T: AsRef<str>,
    {
        let result = match self {
            Db::Mem(db) => db.export_relations(relations),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.export_relations(relations),
        };
        result.map_err(convert_internal)
    }

    /// Import relations from backup.
    pub fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> crate::engine::Result<()> {
        let result = match self {
            Db::Mem(db) => db.import_relations(data),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.import_relations(data),
        };
        result.map_err(convert_internal)
    }

    /// Register a custom fixed rule (graph algorithm).
    pub fn register_fixed_rule<R: FixedRule + 'static>(
        &self,
        name: String,
        rule: R,
    ) -> crate::engine::Result<()> {
        let result = match self {
            Db::Mem(db) => db.register_fixed_rule(name, rule),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.register_fixed_rule(name, rule),
        };
        result.map_err(convert_internal)
    }

    /// Register a callback for relation changes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_callback(
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (
        u32,
        crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>,
    ) {
        match self {
            Db::Mem(db) => db.register_callback(relation, capacity),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => db.register_callback(relation, capacity),
        }
    }

    /// Begin a multi-relation transaction.
    pub fn multi_transaction(&self, write: bool) -> MultiTransaction {
        let (app2db_send, app2db_recv): (Sender<TransactionPayload>, Receiver<TransactionPayload>) =
            bounded(1);
        let (db2app_send, db2app_recv): (
            Sender<crate::engine::error::InternalResult<NamedRows>>,
            Receiver<crate::engine::error::InternalResult<NamedRows>>,
        ) = bounded(1);
        let db = self.clone_inner();
        rayon::spawn(move || db.run_multi_transaction_inner(write, app2db_recv, db2app_send));
        MultiTransaction {
            sender: app2db_send,
            receiver: db2app_recv,
        }
    }

    fn clone_inner(&self) -> DbInner {
        match self {
            Db::Mem(db) => DbInner::Mem(db.clone()),
            #[cfg(feature = "storage-fjall")]
            Db::Fjall(db) => DbInner::Fjall(db.clone()),
        }
    }
}

/// Internal enum for owned clones used in spawned tasks.
enum DbInner {
    Mem(crate::engine::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-fjall")]
    Fjall(crate::engine::runtime::db::Db<FjallStorage>),
}

impl DbInner {
    fn run_multi_transaction_inner(
        self,
        write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<crate::engine::error::InternalResult<NamedRows>>,
    ) {
        match self {
            DbInner::Mem(db) => db.run_multi_transaction(write, payloads, results),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.run_multi_transaction(write, payloads, results),
        }
    }
}

/// A multi-transaction handle.
#[expect(
    private_interfaces,
    reason = "InternalResult is pub(crate) — MultiTransaction is consumed within the crate"
)]
pub struct MultiTransaction {
    /// Commands can be sent into the transaction through this channel
    pub sender: Sender<TransactionPayload>,
    /// Results can be retrieved from the transaction from this channel
    pub receiver: Receiver<crate::engine::error::InternalResult<NamedRows>>,
}

/// A poison token used to cancel an in-progress operation.
pub use crate::engine::runtime::db::Poison;

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "test helpers — error size not critical"
)]
impl DbInstance {
    pub(crate) fn default() -> Self {
        crate::engine::storage::mem::new_mem_db().unwrap()
    }

    pub(crate) fn run_default(
        &self,
        script: &str,
    ) -> crate::engine::error::InternalResult<NamedRows> {
        use crate::engine::runtime::db::ScriptMutability;
        self.run_script(script, BTreeMap::new(), ScriptMutability::Mutable)
    }

    pub(crate) fn multi_transaction_test(&self, write: bool) -> TestMultiTx {
        let (app_tx, app_rx) = bounded::<TransactionPayload>(1);
        let (db_tx, db_rx) = bounded::<crate::engine::error::InternalResult<NamedRows>>(1);
        let db = self.clone();
        rayon::spawn(move || db.run_multi_transaction(write, app_rx, db_tx));
        TestMultiTx {
            sender: app_tx,
            receiver: db_rx,
        }
    }
}

#[cfg(test)]
pub(crate) struct TestMultiTx {
    pub(crate) sender: Sender<TransactionPayload>,
    pub(crate) receiver: Receiver<crate::engine::error::InternalResult<NamedRows>>,
}

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "test helpers — error size not critical"
)]
impl TestMultiTx {
    pub(crate) fn run_script(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::engine::error::InternalResult<NamedRows> {
        self.sender
            .send(TransactionPayload::Query((script.to_string(), params)))
            .unwrap();
        self.receiver.recv().unwrap()
    }

    pub(crate) fn commit(self) -> crate::engine::error::InternalResult<()> {
        self.sender.send(TransactionPayload::Commit).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }

    pub(crate) fn abort(self) -> crate::engine::error::InternalResult<()> {
        self.sender.send(TransactionPayload::Abort).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }
}

#[cfg(test)]
mod safety_assertions {
    use static_assertions::assert_impl_all;
    assert_impl_all!(crate::engine::runtime::db::Db<crate::engine::storage::mem::MemStorage>: Send, Sync);
    #[cfg(feature = "storage-fjall")]
    assert_impl_all!(crate::engine::runtime::db::Db<crate::engine::storage::fjall_backend::FjallStorage>: Send, Sync);
}
