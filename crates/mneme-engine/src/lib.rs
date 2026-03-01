// aletheia-mneme-engine -- embedded Datalog + HNSW + graph engine for Aletheia

use std::collections::BTreeMap;
#[cfg(feature = "storage-new-rocksdb")]
use std::path::Path;

use crossbeam::channel::{bounded, Receiver, Sender};

// Public type re-exports
pub use crate::data::value::{DataValue, ValidityTs};
pub use crate::fixed_rule::{FixedRule, FixedRuleInputRelation, FixedRulePayload};
pub use crate::runtime::callback::CallbackOp;
pub use crate::runtime::db::{NamedRows, ScriptMutability, TransactionPayload};
pub use crate::storage::mem::MemStorage;
#[cfg(feature = "storage-new-rocksdb")]
pub use crate::storage::newrocks::NewRocksDbStorage;

// Internal re-exports needed by submodules (not part of the public API)
pub(crate) use crate::data::symb::Symbol;
pub(crate) use crate::data::expr::Expr;
pub(crate) use crate::parse::SourceSpan;
pub(crate) use crate::runtime::db::Db as DbCore;
pub(crate) use crate::runtime::relation::decode_tuple_from_kv;
pub(crate) use crate::storage::{Storage, StoreTx};
// Test-only type alias — matches original `DbInstance` used in internal test modules
#[cfg(test)]
pub(crate) type DbInstance = crate::runtime::db::Db<crate::storage::mem::MemStorage>;

// All internal modules — pub(crate) only
pub(crate) mod data;
pub(crate) mod fixed_rule;
pub(crate) mod fts;
pub(crate) mod parse;
pub(crate) mod query;
pub(crate) mod runtime;
pub(crate) mod storage;
pub(crate) mod utils;

#[cfg(test)]
mod safety_assertions {
    use static_assertions::assert_impl_all;
    assert_impl_all!(crate::runtime::db::Db<crate::storage::mem::MemStorage>: Send, Sync);
    #[cfg(feature = "storage-new-rocksdb")]
    assert_impl_all!(crate::runtime::db::Db<crate::storage::newrocks::NewRocksDbStorage>: Send, Sync);
}

/// Public facade replacing DbInstance. Dispatches to concrete storage implementations.
pub enum Db {
    Mem(crate::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-new-rocksdb")]
    RocksDb(crate::runtime::db::Db<NewRocksDbStorage>),
}

impl Db {
    /// Open an in-memory database.
    pub fn open_mem() -> miette::Result<Self> {
        crate::storage::mem::new_cozo_mem().map(Db::Mem)
    }

    /// Open a RocksDB-backed database at the given path.
    #[cfg(feature = "storage-new-rocksdb")]
    pub fn open_rocksdb(path: impl AsRef<Path>) -> miette::Result<Self> {
        crate::storage::newrocks::new_cozo_newrocksdb(path).map(Db::RocksDb)
    }

    /// Execute a Datalog script.
    pub fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> miette::Result<NamedRows> {
        match self {
            Db::Mem(db) => db.run_script(script, params, mutability),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.run_script(script, params, mutability),
        }
    }

    /// Export relations for backup.
    pub fn export_relations<I, T>(&self, relations: I) -> miette::Result<BTreeMap<String, NamedRows>>
    where
        I: Iterator<Item = T>,
        T: AsRef<str>,
    {
        match self {
            Db::Mem(db) => db.export_relations(relations),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.export_relations(relations),
        }
    }

    /// Import relations from backup.
    pub fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> miette::Result<()> {
        match self {
            Db::Mem(db) => db.import_relations(data),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.import_relations(data),
        }
    }

    /// Register a custom fixed rule (graph algorithm).
    pub fn register_fixed_rule<R: FixedRule + 'static>(
        &self,
        name: String,
        rule: R,
    ) -> miette::Result<()> {
        match self {
            Db::Mem(db) => db.register_fixed_rule(name, rule),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.register_fixed_rule(name, rule),
        }
    }

    /// Register a callback for relation changes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_callback(
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (u32, crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>) {
        match self {
            Db::Mem(db) => db.register_callback(relation, capacity),
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => db.register_callback(relation, capacity),
        }
    }

    /// Begin a multi-relation transaction.
    pub fn multi_transaction(&self, write: bool) -> MultiTransaction {
        let (app2db_send, app2db_recv): (Sender<TransactionPayload>, Receiver<TransactionPayload>) =
            bounded(1);
        let (db2app_send, db2app_recv): (
            Sender<miette::Result<NamedRows>>,
            Receiver<miette::Result<NamedRows>>,
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
            #[cfg(feature = "storage-new-rocksdb")]
            Db::RocksDb(db) => DbInner::RocksDb(db.clone()),
        }
    }
}

/// Internal enum for owned clones used in spawned tasks.
enum DbInner {
    Mem(crate::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-new-rocksdb")]
    RocksDb(crate::runtime::db::Db<NewRocksDbStorage>),
}

impl DbInner {
    fn run_multi_transaction_inner(
        self,
        write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<miette::Result<NamedRows>>,
    ) {
        match self {
            DbInner::Mem(db) => db.run_multi_transaction(write, payloads, results),
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
    pub receiver: Receiver<miette::Result<NamedRows>>,
}

/// A poison token used to cancel an in-progress operation.
pub use crate::runtime::db::Poison;

#[cfg(test)]
impl DbInstance {
    pub(crate) fn default() -> Self {
        crate::storage::mem::new_cozo_mem().unwrap()
    }

    pub(crate) fn run_default(&self, script: &str) -> miette::Result<NamedRows> {
        use crate::runtime::db::ScriptMutability;
        self.run_script(script, Default::default(), ScriptMutability::Mutable)
    }

    pub(crate) fn multi_transaction_test(&self, write: bool) -> TestMultiTx {
        let (app_tx, app_rx) = bounded::<TransactionPayload>(1);
        let (db_tx, db_rx) = bounded::<miette::Result<NamedRows>>(1);
        let db = self.clone();
        rayon::spawn(move || db.run_multi_transaction(write, app_rx, db_tx));
        TestMultiTx { sender: app_tx, receiver: db_rx }
    }
}

#[cfg(test)]
pub(crate) struct TestMultiTx {
    pub(crate) sender: Sender<TransactionPayload>,
    pub(crate) receiver: Receiver<miette::Result<NamedRows>>,
}

#[cfg(test)]
impl TestMultiTx {
    pub(crate) fn run_script(&self, script: &str, params: BTreeMap<String, DataValue>) -> miette::Result<NamedRows> {
        self.sender.send(TransactionPayload::Query((script.to_string(), params))).unwrap();
        self.receiver.recv().unwrap()
    }

    pub(crate) fn commit(self) -> miette::Result<()> {
        self.sender.send(TransactionPayload::Commit).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }

    pub(crate) fn abort(self) -> miette::Result<()> {
        self.sender.send(TransactionPayload::Abort).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }
}
