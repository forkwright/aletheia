//! Core database instance and session management.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::default::Default;
use std::fmt::{Debug, Formatter};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::engine::error::InternalResult as Result;
use crate::engine::runtime::error::{InvalidOperationSnafu, QueryKilledSnafu, UnsupportedSnafu};
use compact_str::CompactString;
use crossbeam::channel::{Receiver, bounded, unbounded};
use crossbeam::sync::ShardedLock;
use itertools::Itertools;
use serde_json::json;
use snafu::Snafu;

use crate::engine::FixedRule;
use crate::engine::data::json::JsonValue;
use crate::engine::data::tuple::{Tuple, TupleT};
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::DEFAULT_FIXED_RULES;
use crate::engine::fts::TokenizerCache;
use crate::engine::runtime::callback::{CallbackDeclaration, CallbackOp, EventCallbackRegistry};
use crate::engine::runtime::relation::RelationId;
use crate::engine::storage::Storage;
use crate::engine::storage::temp::TempStorage;

pub(crate) struct RunningQueryHandle {
    pub(crate) started_at: f64,
    pub(crate) poison: Poison,
}

pub(crate) struct RunningQueryCleanup {
    pub(crate) id: u64,
    pub(crate) running_queries: Arc<Mutex<BTreeMap<u64, RunningQueryHandle>>>,
}

impl Drop for RunningQueryCleanup {
    fn drop(&mut self) {
        let mut map = self.running_queries.lock().expect("lock poisoned");
        if let Some(handle) = map.remove(&self.id) {
            handle.poison.0.store(true, Ordering::Relaxed);
        }
    }
}

/// Whether a script is mutable or immutable.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ScriptMutability {
    /// The script is mutable.
    Mutable,
    /// The script is immutable.
    Immutable,
}

/// The mneme engine database object.
#[derive(Clone)]
pub struct Db<S> {
    pub(crate) db: S,
    pub(crate) temp_db: TempStorage,
    pub(crate) relation_store_id: Arc<AtomicU64>,
    pub(crate) queries_count: Arc<AtomicU64>,
    pub(crate) running_queries: Arc<Mutex<BTreeMap<u64, RunningQueryHandle>>>,
    pub(crate) fixed_rules: Arc<ShardedLock<BTreeMap<String, Arc<Box<dyn FixedRule>>>>>,
    pub(crate) tokenizers: Arc<TokenizerCache>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) callback_count: Arc<AtomicU32>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) event_callbacks: Arc<ShardedLock<EventCallbackRegistry>>,
    pub(crate) relation_locks: Arc<ShardedLock<BTreeMap<CompactString, Arc<ShardedLock<()>>>>>,
}

impl<S> Debug for Db<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Db")
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
/// Rows in a relation, together with headers for the fields.
pub struct NamedRows {
    /// The headers
    pub headers: Vec<String>,
    /// The rows
    pub rows: Vec<Tuple>,
    /// Contains the next named rows, if exists
    pub next: Option<Box<NamedRows>>,
}

impl IntoIterator for NamedRows {
    type Item = Tuple;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl NamedRows {
    /// create a named rows with the given headers and rows
    pub fn new(headers: Vec<String>, rows: Vec<Tuple>) -> Self {
        Self {
            headers,
            rows,
            next: None,
        }
    }

    /// If there are more named rows after the current one
    pub fn has_more(&self) -> bool {
        self.next.is_some()
    }

    /// convert a chain of named rows to individual named rows
    pub fn flatten(self) -> Vec<Self> {
        let mut collected = vec![];
        let mut current = self;
        loop {
            let nxt = current.next.take();
            collected.push(current);
            if let Some(n) = nxt {
                current = *n;
            } else {
                break;
            }
        }
        collected
    }

    /// Convert to a JSON object
    pub fn into_json(self) -> JsonValue {
        let nxt = match self.next {
            None => json!(null),
            Some(more) => more.into_json(),
        };
        let rows = self
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(JsonValue::from).collect::<JsonValue>())
            .collect::<JsonValue>();
        json!({
            "headers": self.headers,
            "rows": rows,
            "next": nxt,
        })
    }
    /// Make named rows from JSON
    #[must_use]
    pub fn from_json(value: &JsonValue) -> Result<Self> {
        let headers =
            value
                .get("headers")
                .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "import",
                        reason: "NamedRows requires 'headers' field",
                    }
                    .build(),
                })?;
        let headers =
            headers
                .as_array()
                .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "import",
                        reason: "'headers' field must be an array",
                    }
                    .build(),
                })?;
        let headers = headers
            .iter()
            .map(|h| -> Result<String> {
                let h = h
                    .as_str()
                    .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                        source: InvalidOperationSnafu {
                            op: "import",
                            reason: "'headers' field must be an array of strings",
                        }
                        .build(),
                    })?;
                Ok(h.to_string())
            })
            .try_collect()?;
        let rows =
            value
                .get("rows")
                .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "import",
                        reason: "NamedRows requires 'rows' field",
                    }
                    .build(),
                })?;
        let rows = rows
            .as_array()
            .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "import",
                    reason: "'rows' field must be an array",
                }
                .build(),
            })?;
        let rows = rows
            .iter()
            .map(|row| -> Result<Vec<DataValue>> {
                let row =
                    row.as_array()
                        .ok_or_else(|| crate::engine::error::InternalError::Runtime {
                            source: InvalidOperationSnafu {
                                op: "import",
                                reason: "'rows' field must be an array of arrays",
                            }
                            .build(),
                        })?;
                Ok(row.iter().map(DataValue::from).collect_vec())
            })
            .try_collect()?;
        Ok(Self {
            headers,
            rows,
            next: None,
        })
    }

    /// Create a query and parameters to apply an operation (insert, put, delete, rm) to a stored
    /// relation with the named rows.
    pub fn into_payload(self, relation: &str, op: &str) -> Payload {
        let cols_str = self.headers.join(", ");
        let query = format!("?[{cols_str}] <- $data :{op} {relation} {{ {cols_str} }}");
        let data = DataValue::List(self.rows.into_iter().map(DataValue::List).collect());
        (query, [("data".to_string(), data)].into())
    }
}

pub(crate) const STATUS_STR: &str = "status";
pub(crate) const OK_STR: &str = "OK";

/// The query and parameters.
pub type Payload = (String, BTreeMap<String, DataValue>);

/// Commands to be sent to a multi-transaction
#[derive(Eq, PartialEq, Debug)]
pub enum TransactionPayload {
    /// Commit the current transaction
    Commit,
    /// Abort the current transaction
    Abort,
    /// Run a query inside the transaction
    Query(Payload),
}

impl<'s, S: Storage<'s>> Db<S> {
    /// Create a new database object with the given storage.
    /// You must call [`initialize`](Self::initialize) immediately after creation.
    /// Due to lifetime restrictions we are not able to call that for you automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage backend fails during initial setup.
    #[must_use]
    pub fn new(storage: S) -> Result<Self> {
        let ret = Self {
            db: storage,
            temp_db: Default::default(),
            relation_store_id: Default::default(),
            queries_count: Default::default(),
            running_queries: Default::default(),
            fixed_rules: Arc::new(ShardedLock::new(DEFAULT_FIXED_RULES.clone())),
            tokenizers: Arc::new(Default::default()),
            #[cfg(not(target_arch = "wasm32"))]
            callback_count: Default::default(),
            // callback_receiver: Arc::new(receiver),
            #[cfg(not(target_arch = "wasm32"))]
            event_callbacks: Default::default(),
            relation_locks: Default::default(),
        };
        Ok(ret)
    }

    /// Must be called after creation of the database to initialize the runtime state.
    #[must_use]
    pub fn initialize(&'s self) -> Result<()> {
        self.load_last_ids()?;
        Ok(())
    }

    /// This returns the set of fixed rule implementations for this specific backend.
    pub fn get_fixed_rules(&'s self) -> BTreeMap<String, Arc<Box<dyn FixedRule>>> {
        return self.fixed_rules.read().expect("lock poisoned").clone();
    }

    /// Backup the running database into an Sqlite file.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    #[must_use]
    pub fn backup_db(&'s self, _out_file: impl AsRef<Path>) -> Result<()> {
        UnsupportedSnafu {
            operation: "backup",
            reason: "requires the removed 'storage-sqlite' feature",
        }
        .fail()?
    }
    /// Restore from an Sqlite backup.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    #[must_use]
    pub fn restore_backup(&'s self, _in_file: impl AsRef<Path>) -> Result<()> {
        UnsupportedSnafu {
            operation: "restore",
            reason: "requires the removed 'storage-sqlite' feature",
        }
        .fail()?
    }
    /// Import data from relations in a backup file.
    ///
    /// Not currently supported: requires the removed `storage-sqlite` feature.
    pub fn import_from_backup(
        &'s self,
        _in_file: impl AsRef<Path>,
        _relations: &[String],
    ) -> Result<()> {
        UnsupportedSnafu {
            operation: "import_from_backup",
            reason: "requires the removed 'storage-sqlite' feature",
        }
        .fail()?
    }
    /// Register a custom fixed rule implementation.
    #[must_use]
    pub fn register_fixed_rule<R>(&self, name: String, rule_impl: R) -> Result<()>
    where
        R: FixedRule + 'static,
    {
        match self.fixed_rules.write().expect("lock poisoned").entry(name) {
            Entry::Vacant(ent) => {
                ent.insert(Arc::new(Box::new(rule_impl)));
                Ok(())
            }
            Entry::Occupied(ent) => InvalidOperationSnafu {
                op: "register fixed rule",
                reason: format!("a rule with the name '{}' is already registered", ent.key()),
            }
            .fail()?,
        }
    }

    /// Unregister a custom fixed rule implementation.
    #[must_use]
    pub fn unregister_fixed_rule(&self, name: &str) -> Result<bool> {
        if DEFAULT_FIXED_RULES.contains_key(name) {
            InvalidOperationSnafu {
                op: "unregister fixed rule",
                reason: format!("cannot unregister builtin fixed rule '{name}'"),
            }
            .fail()?;
        }
        Ok(self
            .fixed_rules
            .write()
            .expect("lock poisoned")
            .remove(name)
            .is_some())
    }

    /// Register callback channel to receive changes when the requested relation are successfully committed.
    /// The returned ID can be used to unregister the callback channel.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_callback(
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (u32, Receiver<(CallbackOp, NamedRows, NamedRows)>) {
        let (sender, receiver) = if let Some(c) = capacity {
            bounded(c)
        } else {
            unbounded()
        };
        let cb = CallbackDeclaration {
            dependent: CompactString::from(relation),
            sender,
        };

        let mut guard = self.event_callbacks.write().expect("lock poisoned");
        let new_id = self.callback_count.fetch_add(1, Ordering::SeqCst);
        guard
            .1
            .entry(CompactString::from(relation))
            .or_default()
            .insert(new_id);

        guard.0.insert(new_id, cb);
        (new_id, receiver)
    }

    /// Unregister callbacks/channels to run when changes to relations are committed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn unregister_callback(&self, id: u32) -> bool {
        let mut guard = self.event_callbacks.write().expect("lock poisoned");
        let ret = guard.0.remove(&id);
        if let Some(cb) = &ret {
            guard
                .1
                .get_mut(&cb.dependent)
                .expect("callback dependency entry exists")
                .remove(&id);

            if guard
                .1
                .get(&cb.dependent)
                .expect("callback dependency entry exists")
                .is_empty()
            {
                guard.1.remove(&cb.dependent);
            }
        }
        ret.is_some()
    }

    pub(crate) fn obtain_relation_locks<'a, T: Iterator<Item = &'a CompactString>>(
        &'s self,
        rels: T,
    ) -> Vec<Arc<ShardedLock<()>>> {
        let mut collected = vec![];
        let mut pending = vec![];
        {
            let locks = self.relation_locks.read().expect("lock poisoned");
            for rel in rels {
                match locks.get(rel) {
                    None => {
                        pending.push(rel);
                    }
                    Some(lock) => collected.push(lock.clone()),
                }
            }
        }
        if !pending.is_empty() {
            let mut locks = self.relation_locks.write().expect("lock poisoned");
            for rel in pending {
                let lock = locks.entry(rel.clone()).or_default().clone();
                collected.push(lock);
            }
        }
        collected
    }

    pub(crate) fn compact_relation(&'s self) -> Result<()> {
        let l = Tuple::default().encode_as_key(RelationId(0));
        let u = vec![DataValue::Bot].encode_as_key(RelationId(u64::MAX));
        self.db.range_compact(&l, &u)?;
        Ok(())
    }
}

/// Used for user-initiated termination of running queries
#[derive(Clone, Default)]
pub struct Poison(pub(crate) Arc<AtomicBool>);

/// Typed error for query cancellation: enables downstream matching without string parsing.
#[derive(Debug, Snafu)]
#[snafu(display("Running query is killed before completion"))]
pub(crate) struct ProcessKilled;

impl Poison {
    /// Check whether the query has been cancelled.
    ///
    /// # Errors
    ///
    /// Returns a query-killed error if the user initiated termination.
    #[inline(always)]
    #[must_use]
    pub fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Relaxed) {
            QueryKilledSnafu.fail()?;
        }
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn set_timeout(&self, _secs: f64) -> Result<()> {
        UnsupportedSnafu {
            operation: "set timeout",
            reason: "threading is disallowed on this platform",
        }
        .fail()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_timeout(&self, secs: f64) -> Result<()> {
        let pill = self.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_micros((secs * 1000000.) as u64));
            pill.0.store(true, Ordering::Relaxed);
        });
        Ok(())
    }
}

pub(crate) fn seconds_since_the_epoch() -> Result<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    let now = SystemTime::now();
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(now
        .duration_since(UNIX_EPOCH)
        .map_err(|e| crate::engine::error::InternalError::Runtime {
            source: InvalidOperationSnafu {
                op: "timestamp",
                reason: e.to_string(),
            }
            .build(),
        })?
        .as_secs_f64());

    #[cfg(target_arch = "wasm32")]
    Ok(js_sys::Date::now())
}
