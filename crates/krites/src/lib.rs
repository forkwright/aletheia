//! aletheia-krites: embedded Datalog engine with HNSW and graph support
//!
//! Krites (Κριτής): "judge." Evaluates Datalog queries against a graph store
//! with HNSW vector search and graph algorithms.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use crossbeam::channel::{Receiver, Sender, bounded};

pub mod error;
#[expect(
    clippy::expect_used,
    reason = "vendored CozoDB engine — Mutex poisoning is unrecoverable in cache internals"
)]
pub mod query_cache;
pub use error::{Error, Result};
pub use query_cache::{QueryCache, QueryCacheStats};

pub use crate::data::value::{DataValue, ValidityTs, Vector};
pub use crate::fixed_rule::{FixedRule, FixedRuleInputRelation, FixedRulePayload};
pub use crate::runtime::callback::CallbackOp;
pub use crate::runtime::db::{NamedRows, ScriptMutability, TransactionPayload};
#[cfg(feature = "storage-fjall")]
pub use crate::storage::fjall_backend::FjallStorage;
pub use crate::storage::mem::MemStorage;
pub use ndarray::Array1;

pub(crate) use crate::data::expr::Expr;
pub(crate) use crate::data::symb::Symbol;
pub(crate) use crate::parse::SourceSpan;
pub(crate) use crate::runtime::db::Db as DbCore;
pub(crate) use crate::runtime::relation::decode_tuple_from_kv;
pub(crate) use crate::storage::{Storage, StoreTx};
#[cfg(test)]
pub(crate) type DbInstance = crate::runtime::db::Db<crate::storage::mem::MemStorage>;

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
    dead_code,
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
    dead_code,
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
fn convert_internal(e: crate::error::InternalError) -> Error {
    use crate::error::InternalError;
    use snafu::IntoError;
    match e {
        InternalError::Runtime {
            source: crate::runtime::error::RuntimeError::QueryKilled { .. },
        } => error::QueryKilledSnafu.build(),
        InternalError::Parse { source } => error::ParseSnafu.into_error(source),
        InternalError::Storage { source } => error::StorageSnafu.into_error(source),
        other => error::EngineSnafu {
            message: other.to_string(),
        }
        .build(),
    }
}

/// Internal dispatch enum — one variant per storage backend.
enum DbInner {
    /// In-memory storage backend.
    Mem(crate::runtime::db::Db<MemStorage>),
    #[cfg(feature = "storage-fjall")]
    /// Persistent fjall storage backend.
    Fjall(crate::runtime::db::Db<FjallStorage>),
}

impl DbInner {
    fn run_multi_transaction_inner(
        self,
        write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<crate::error::InternalResult<NamedRows>>,
    ) {
        match self {
            DbInner::Mem(db) => db.run_multi_transaction(write, payloads, results),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.run_multi_transaction(write, payloads, results),
        }
    }
}

/// Public facade for the Datalog engine. Dispatches to a concrete storage backend.
///
/// Obtain an instance via [`Db::open_mem`] or [`Db::open_fjall`]. Attach an
/// optional LRU query cache with [`Db::with_cache`] to track hit/miss metrics
/// for repeated Datalog queries.
pub struct Db {
    inner: DbInner,
    /// Optional LRU cache that records whether each normalized query string has
    /// been seen before, exposing hit/miss metrics for observability.
    cache: Option<Arc<QueryCache>>,
}

#[expect(
    clippy::result_large_err,
    reason = "engine Error carries structured context — boxing deferred to avoid API churn"
)]
impl Db {
    fn new(inner: DbInner) -> Self {
        Self { inner, cache: None }
    }

    /// Open an in-memory database.
    pub fn open_mem() -> crate::Result<Self> {
        crate::storage::mem::new_mem_db()
            .map(|db| Self::new(DbInner::Mem(db)))
            .map_err(convert_internal)
    }

    /// Open a fjall-backed database at the given path.
    ///
    /// Primary production backend: pure Rust, LSM-tree, LZ4 compression,
    /// native read-your-own-writes.
    #[cfg(feature = "storage-fjall")]
    pub fn open_fjall(path: impl AsRef<Path>) -> crate::Result<Self> {
        crate::storage::fjall_backend::new_cozo_fjall(path)
            .map(|db| Self::new(DbInner::Fjall(db)))
            .map_err(convert_internal)
    }

    /// Attach an LRU query cache with the given capacity.
    ///
    /// Once attached, every [`Db::run`] call checks the normalized query
    /// against the cache and records a hit or miss. Retrieve statistics with
    /// [`Db::cache_stats`].
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero (use [`NonZeroUsize`]).
    #[must_use]
    pub fn with_cache(mut self, capacity: NonZeroUsize) -> Self {
        self.cache = Some(Arc::new(QueryCache::new(capacity)));
        self
    }

    /// Return a snapshot of query cache statistics, or `None` if no cache is attached.
    #[must_use]
    pub fn cache_stats(&self) -> Option<QueryCacheStats> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// Execute a Datalog script.
    ///
    /// If a query cache is attached, the normalized query string is checked
    /// before execution and the hit/miss counter is updated.
    pub fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> crate::Result<NamedRows> {
        if let Some(cache) = &self.cache {
            cache.check(script);
        }
        let result = match &self.inner {
            DbInner::Mem(db) => db.run_script(script, params, mutability),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.run_script(script, params, mutability),
        };
        result.map_err(convert_internal)
    }

    /// Execute a Datalog script in read-only mode.
    pub fn run_read_only(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::Result<NamedRows> {
        self.run(script, params, ScriptMutability::Immutable)
    }

    /// Backup the running database into an `SQLite` file.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    pub fn backup_db(&self, out_file: impl AsRef<Path>) -> crate::Result<()> {
        let path = out_file.as_ref();
        let result = match &self.inner {
            DbInner::Mem(db) => db.backup_db(path),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.backup_db(path),
        };
        result.map_err(convert_internal)
    }

    /// Restore from an `SQLite` backup.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    pub fn restore_backup(&self, in_file: impl AsRef<Path>) -> crate::Result<()> {
        let path = in_file.as_ref();
        let result = match &self.inner {
            DbInner::Mem(db) => db.restore_backup(path),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.restore_backup(path),
        };
        result.map_err(convert_internal)
    }

    /// Import data from relations in a backup file.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    pub fn import_from_backup(
        &self,
        in_file: impl AsRef<Path>,
        relations: &[String],
    ) -> crate::Result<()> {
        let path = in_file.as_ref();
        let result = match &self.inner {
            DbInner::Mem(db) => db.import_from_backup(path, relations),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.import_from_backup(path, relations),
        };
        result.map_err(convert_internal)
    }

    /// Export relations for backup.
    pub fn export_relations<I, T>(&self, relations: I) -> crate::Result<BTreeMap<String, NamedRows>>
    where
        I: Iterator<Item = T>,
        T: AsRef<str>,
    {
        let result = match &self.inner {
            DbInner::Mem(db) => db.export_relations(relations),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.export_relations(relations),
        };
        result.map_err(convert_internal)
    }

    /// Import relations from backup.
    pub fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> crate::Result<()> {
        let result = match &self.inner {
            DbInner::Mem(db) => db.import_relations(data),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.import_relations(data),
        };
        result.map_err(convert_internal)
    }

    /// Register a custom fixed rule (graph algorithm).
    pub fn register_fixed_rule<R: FixedRule + 'static>(
        &self,
        name: String,
        rule: R,
    ) -> crate::Result<()> {
        let result = match &self.inner {
            DbInner::Mem(db) => db.register_fixed_rule(name, rule),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.register_fixed_rule(name, rule),
        };
        result.map_err(convert_internal)
    }

    /// Register a callback for relation changes.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn register_callback(
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (
        u32,
        crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>,
    ) {
        match &self.inner {
            DbInner::Mem(db) => db.register_callback(relation, capacity),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => db.register_callback(relation, capacity),
        }
    }

    /// Begin a multi-relation transaction.
    #[must_use]
    pub fn multi_transaction(&self, write: bool) -> MultiTransaction {
        let (app2db_send, app2db_recv): (Sender<TransactionPayload>, Receiver<TransactionPayload>) =
            bounded(1);
        let (db2app_send, db2app_recv): (
            Sender<crate::error::InternalResult<NamedRows>>,
            Receiver<crate::error::InternalResult<NamedRows>>,
        ) = bounded(1);
        let db = self.clone_inner();
        rayon::spawn(move || db.run_multi_transaction_inner(write, app2db_recv, db2app_send));
        MultiTransaction {
            sender: app2db_send,
            receiver: db2app_recv,
        }
    }

    fn clone_inner(&self) -> DbInner {
        match &self.inner {
            DbInner::Mem(db) => DbInner::Mem(db.clone()),
            #[cfg(feature = "storage-fjall")]
            DbInner::Fjall(db) => DbInner::Fjall(db.clone()),
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
    pub receiver: Receiver<crate::error::InternalResult<NamedRows>>,
}

/// A poison token used to cancel an in-progress operation.
pub use crate::runtime::db::Poison;

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "test helpers — error size not critical"
)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
impl DbInstance {
    pub(crate) fn default() -> Self {
        crate::storage::mem::new_mem_db().unwrap()
    }

    pub(crate) fn run_default(&self, script: &str) -> crate::error::InternalResult<NamedRows> {
        use crate::runtime::db::ScriptMutability;
        self.run_script(script, BTreeMap::new(), ScriptMutability::Mutable)
    }

    pub(crate) fn multi_transaction_test(&self, write: bool) -> TestMultiTx {
        let (app_tx, app_rx) = bounded::<TransactionPayload>(1);
        let (db_tx, db_rx) = bounded::<crate::error::InternalResult<NamedRows>>(1);
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
    pub(crate) receiver: Receiver<crate::error::InternalResult<NamedRows>>,
}

#[cfg(test)]
#[expect(
    clippy::result_large_err,
    reason = "test helpers — error size not critical"
)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
impl TestMultiTx {
    pub(crate) fn run_script(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::error::InternalResult<NamedRows> {
        self.sender
            .send(TransactionPayload::Query((script.to_string(), params)))
            .unwrap();
        self.receiver.recv().unwrap()
    }

    pub(crate) fn commit(self) -> crate::error::InternalResult<()> {
        self.sender.send(TransactionPayload::Commit).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }

    pub(crate) fn abort(self) -> crate::error::InternalResult<()> {
        self.sender.send(TransactionPayload::Abort).unwrap();
        self.receiver.recv().unwrap().map(|_| ())
    }
}

#[cfg(test)]
mod safety_assertions {
    use static_assertions::assert_impl_all;
    assert_impl_all!(crate::runtime::db::Db<crate::storage::mem::MemStorage>: Send, Sync);
    #[cfg(feature = "storage-fjall")]
    assert_impl_all!(crate::runtime::db::Db<crate::storage::fjall_backend::FjallStorage>: Send, Sync);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod db_cache_tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use super::Db;
    use crate::runtime::db::ScriptMutability;

    #[test]
    fn cache_stats_none_without_cache() {
        let db = Db::open_mem().expect("open_mem should succeed");
        assert!(
            db.cache_stats().is_none(),
            "cache_stats should be None when no cache is attached"
        );
    }

    #[test]
    fn cache_tracks_misses_and_hits() {
        let db = Db::open_mem()
            .expect("open_mem should succeed")
            .with_cache(NonZeroUsize::new(16).expect("16 is non-zero"));

        let script = "?[x] := x = 1";
        let _ = db.run(script, BTreeMap::new(), ScriptMutability::Immutable);
        let _ = db.run(script, BTreeMap::new(), ScriptMutability::Immutable);

        let stats = db
            .cache_stats()
            .expect("cache_stats should be Some after with_cache");
        assert_eq!(stats.misses, 1, "first run should be a cache miss");
        assert_eq!(stats.hits, 1, "second identical run should be a cache hit");
    }

    #[test]
    fn cache_normalizes_whitespace() {
        let db = Db::open_mem()
            .expect("open_mem should succeed")
            .with_cache(NonZeroUsize::new(16).expect("16 is non-zero"));

        let _ = db.run(
            "?[x] := x = 1",
            BTreeMap::new(),
            ScriptMutability::Immutable,
        );
        // Same query with extra whitespace — should hit.
        let _ = db.run(
            "  ?[x]   :=  x  =  1  ",
            BTreeMap::new(),
            ScriptMutability::Immutable,
        );

        let stats = db.cache_stats().expect("cache_stats should be Some");
        assert_eq!(
            stats.hits, 1,
            "whitespace-normalized query should be a cache hit"
        );
    }
}
