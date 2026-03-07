// aletheia-mneme-engine -- embedded Datalog + HNSW + graph engine for Aletheia

use std::collections::BTreeMap;
use std::path::Path;

use crossbeam::channel::{Receiver, Sender, bounded};

pub mod error;
pub use error::{Error, Result};

// Public type re-exports
pub use crate::engine::data::value::{DataValue, ValidityTs, Vector};
pub use crate::engine::fixed_rule::{FixedRule, FixedRuleInputRelation, FixedRulePayload};
pub use crate::engine::runtime::callback::CallbackOp;
pub use crate::engine::runtime::db::{NamedRows, ScriptMutability, TransactionPayload};
pub use crate::engine::storage::mem::MemStorage;
#[cfg(feature = "storage-redb")]
pub use crate::engine::storage::redb::RedbStorage;
#[cfg(feature = "storage-new-rocksdb")]
pub use crate::engine::storage::newrocks::NewRocksDbStorage;
pub use ndarray::Array1;

// Internal re-exports needed by submodules (not part of the public API)
pub(crate) use crate::engine::data::expr::Expr;
pub(crate) use crate::engine::data::symb::Symbol;
pub(crate) use crate::engine::parse::SourceSpan;
pub(crate) use crate::engine::runtime::db::Db as DbCore;
pub(crate) use crate::engine::runtime::relation::decode_tuple_from_kv;
pub(crate) use crate::engine::storage::{Storage, StoreTx};
// Test-only type alias — matches original `DbInstance` used in internal test modules
#[cfg(test)]
pub(crate) type DbInstance =
    crate::engine::runtime::db::Db<crate::engine::storage::mem::MemStorage>;

// All internal modules — pub(crate) only
pub(crate) mod data;
pub(crate) mod fixed_rule;
pub(crate) mod fts;
#[allow(clippy::all, clippy::restriction, unused_assignments)]
pub(crate) mod parse;
#[allow(clippy::all, clippy::restriction, unused_assignments)]
pub(crate) mod query;
#[allow(clippy::all, clippy::restriction, unused_assignments)]
pub(crate) mod runtime;
pub(crate) mod storage;
pub(crate) mod utils;

/// Convert an internal BoxErr to the public Error type, detecting ProcessKilled for typed matching.
fn convert_err(e: crate::engine::error::BoxErr) -> Error {
    if e.downcast_ref::<crate::engine::runtime::db::ProcessKilled>()
        .is_some()
    {
        return error::QueryKilledSnafu.build();
    }
    error::EngineSnafu {
        message: e.to_string(),
    }
    .build()
}

/// Public facade replacing DbInstance. Dispatches to concrete storage implementations.
pub enum Db {
    Mem(crate::engine::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-redb")]
    Redb(crate::engine::runtime::db::Db<RedbStorage>),
    #[cfg(feature = "storage-new-rocksdb")]
    RocksDb(crate::engine::runtime::db::Db<NewRocksDbStorage>),
}

impl Db {
    /// Open an in-memory database.
    pub fn open_mem() -> crate::engine::Result<Self> {
        crate::engine::storage::mem::new_mem_db()
            .map(Db::Mem)
            .map_err(convert_err)
    }

    /// Open a redb-backed database at the given path.
    #[cfg(feature = "storage-redb")]
    pub fn open_redb(path: impl AsRef<Path>) -> crate::engine::Result<Self> {
        crate::engine::storage::redb::new_cozo_redb(path)
            .map(Db::Redb)
            .map_err(convert_err)
    }

    /// Open a RocksDB-backed database at the given path.
    #[cfg(feature = "storage-new-rocksdb")]
    pub fn open_rocksdb(path: impl AsRef<Path>) -> crate::engine::Result<Self> {
        crate::engine::storage::newrocks::new_rocksdb_db(path)
            .map(Db::RocksDb)
            .map_err(convert_err)
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
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.run_script(script, params, mutability),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.run_script(script, params, mutability),
        };
        result.map_err(convert_err)
    }

    /// Execute a Datalog script in read-only mode.
    pub fn run_read_only(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::engine::Result<NamedRows> {
        self.run(script, params, ScriptMutability::Immutable)
    }

    /// Backup the running database into an SQLite file.
    ///
    /// Not currently supported — requires the removed `storage-sqlite` feature.
    pub fn backup_db(&self, out_file: impl AsRef<Path>) -> crate::engine::Result<()> {
        let path = out_file.as_ref();
        let result = match self {
            Db::Mem(db) => db.backup_db(path),
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.backup_db(path),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.backup_db(path),
        };
        result.map_err(convert_err)
    }

    /// Restore from an SQLite backup.
    ///
    /// Not currently supported — requires the removed `storage-sqlite` feature.
    pub fn restore_backup(&self, in_file: impl AsRef<Path>) -> crate::engine::Result<()> {
        let path = in_file.as_ref();
        let result = match self {
            Db::Mem(db) => db.restore_backup(path),
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.restore_backup(path),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.restore_backup(path),
        };
        result.map_err(convert_err)
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
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.import_from_backup(path, relations),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.import_from_backup(path, relations),
        };
        result.map_err(convert_err)
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
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.export_relations(relations),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.export_relations(relations),
        };
        result.map_err(convert_err)
    }

    /// Import relations from backup.
    pub fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> crate::engine::Result<()> {
        let result = match self {
            Db::Mem(db) => db.import_relations(data),
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.import_relations(data),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.import_relations(data),
        };
        result.map_err(convert_err)
    }

    /// Register a custom fixed rule (graph algorithm).
    pub fn register_fixed_rule<R: FixedRule + 'static>(
        &self,
        name: String,
        rule: R,
    ) -> crate::engine::Result<()> {
        let result = match self {
            Db::Mem(db) => db.register_fixed_rule(name, rule),
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.register_fixed_rule(name, rule),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.register_fixed_rule(name, rule),
        };
        result.map_err(convert_err)
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
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => db.register_callback(relation, capacity),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.register_callback(relation, capacity),
        }
    }

    /// Begin a multi-relation transaction.
    pub fn multi_transaction(&self, write: bool) -> MultiTransaction {
        let (app2db_send, app2db_recv): (Sender<TransactionPayload>, Receiver<TransactionPayload>) =
            bounded(1);
        let (db2app_send, db2app_recv): (
            Sender<crate::engine::error::DbResult<NamedRows>>,
            Receiver<crate::engine::error::DbResult<NamedRows>>,
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
            #[cfg(feature = "storage-redb")]
            Db::Redb(db) => DbInner::Redb(db.clone()),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => DbInner::RocksDb(db.clone()),
        }
    }
}

/// Internal enum for owned clones used in spawned tasks.
enum DbInner {
    Mem(crate::engine::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-redb")]
    Redb(crate::engine::runtime::db::Db<RedbStorage>),
    #[cfg(feature = "storage-new-rocksdb")]
    RocksDb(crate::engine::runtime::db::Db<NewRocksDbStorage>),
}

impl DbInner {
    fn run_multi_transaction_inner(
        self,
        write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<crate::engine::error::DbResult<NamedRows>>,
    ) {
        match self {
            DbInner::Mem(db) => db.run_multi_transaction(write, payloads, results),
            #[cfg(feature = "storage-redb")]
            DbInner::Redb(db) => db.run_multi_transaction(write, payloads, results),
            #[cfg(feature = "storage-new-rocksdb")]
            DbInner::RocksDb(db) => db.run_multi_transaction(write, payloads, results),
        }
    }
}

/// A multi-transaction handle.
pub struct MultiTransaction {
    /// Commands can be sent into the transaction through this channel
    pub sender: Sender<TransactionPayload>,
    /// Results can be retrieved from the transaction from this channel
    pub receiver: Receiver<crate::engine::error::DbResult<NamedRows>>,
}

/// A poison token used to cancel an in-progress operation.
pub use crate::engine::runtime::db::Poison;

#[cfg(test)]
impl DbInstance {
    pub(crate) fn default() -> Self {
        crate::engine::storage::mem::new_mem_db().unwrap()
    }

    pub(crate) fn run_default(&self, script: &str) -> crate::engine::error::DbResult<NamedRows> {
        use crate::engine::runtime::db::ScriptMutability;
        self.run_script(script, Default::default(), ScriptMutability::Mutable)
    }

    pub(crate) fn multi_transaction_test(&self, write: bool) -> TestMultiTx {
        let (app_tx, app_rx) = bounded::<TransactionPayload>(1);
        let (db_tx, db_rx) = bounded::<crate::engine::error::DbResult<NamedRows>>(1);
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
    pub(crate) receiver: Receiver<crate::engine::error::DbResult<NamedRows>>,
}

#[cfg(test)]
impl TestMultiTx {
    pub(crate) fn run_script(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::engine::error::DbResult<NamedRows> {
        self.sender
            .send(TransactionPayload::Query((script.to_string(), params)))
            .unwrap();
        self.receiver.recv().unwrap()
    }

    pub(crate) fn commit(self) -> crate::engine::error::DbResult<()> {
        self.sender.send(TransactionPayload::Commit).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }

    pub(crate) fn abort(self) -> crate::engine::error::DbResult<()> {
        self.sender.send(TransactionPayload::Abort).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }
}

#[cfg(test)]
mod safety_assertions {
    use static_assertions::assert_impl_all;
    assert_impl_all!(crate::engine::runtime::db::Db<crate::engine::storage::mem::MemStorage>: Send, Sync);
    #[cfg(feature = "storage-redb")]
    assert_impl_all!(crate::engine::runtime::db::Db<crate::engine::storage::redb::RedbStorage>: Send, Sync);
    #[cfg(feature = "storage-new-rocksdb")]
    assert_impl_all!(crate::engine::runtime::db::Db<crate::engine::storage::newrocks::NewRocksDbStorage>: Send, Sync);
}
