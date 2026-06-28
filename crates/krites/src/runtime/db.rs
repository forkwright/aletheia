//! Core database instance and session management.
//!
//! Contains the `Db<S>` type -- the engine's top-level handle parameterized
//! over a storage backend. Manages running query tracking, fixed rule
//! registration, relation locks, event callbacks, and provides the
//! `NamedRows` result type for query output.
//!
//! Also re-exports `Poison`, `QueryBudget`, and related types from
//! [`super::poison`] for cooperative query cancellation. Defines
//! `ScriptMutability` for read/write mode control and `TransactionPayload`
//! for multi-statement transaction channels.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::default::Default;
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use compact_str::CompactString;
use crossbeam::channel::{Receiver, bounded};
use crossbeam::sync::ShardedLock;
use itertools::Itertools;
use parking_lot::RwLock;
use serde_json::json;

use crate::FixedRule;
use crate::data::json::JsonValue;
use crate::data::tuple::{Tuple, TupleT};
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::DEFAULT_FIXED_RULES;
use crate::fts::TokenizerCache;
use crate::fts::indexing::FtsCache;
use crate::runtime::callback::{CallbackDeclaration, CallbackOp, EventCallbackRegistry};
use crate::runtime::error::InvalidOperationSnafu;
use crate::runtime::poison::saturating_secs_f64;
use crate::runtime::relation::RelationId;
use crate::storage::Storage;
use crate::storage::temp::TempStorage;

pub(crate) use crate::runtime::poison::ProcessKilled;
pub use crate::runtime::poison::{
    DEFAULT_MAX_EVALUATION_EPOCHS, Poison, QueryBudget, QueryCancellationReason,
};

/// Durability level for fjall-backed storage.
///
/// WHY: Memory workloads accumulate many small fact writes per turn. Using
/// `Buffer` for routine writes avoids an fsync per transaction; callers that
/// need durability must opt into `SyncAll` or call an explicit checkpoint.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PersistMode {
    /// fsync the WAL after every write transaction (highest durability).
    SyncAll,
    /// fsync only file data, not metadata.
    SyncData,
    /// Rely on the OS page cache; no fsync on commit (fastest, bounded loss
    /// on crash).
    #[default]
    Buffer,
}

/// Runtime limits for a database instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DbConfig {
    /// Maximum semi-naive evaluation epochs per stratum.
    pub max_evaluation_epochs: u32,
    /// Optional maximum number of newly derived rows/facts per query.
    pub max_derived_rows: Option<u64>,
    /// Optional maximum evaluator work-unit count per query.
    pub max_work_units: Option<u64>,
    /// Durability level for fjall-backed storage.
    ///
    /// NOTE: This field is ignored by the in-memory backend.
    pub persist_mode: PersistMode,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            max_evaluation_epochs: DEFAULT_MAX_EVALUATION_EPOCHS,
            max_derived_rows: None,
            max_work_units: None,
            persist_mode: PersistMode::default(),
        }
    }
}

impl DbConfig {
    /// Create runtime limits with the given semi-naive epoch cap.
    #[must_use]
    pub fn new(max_evaluation_epochs: u32) -> Self {
        Self {
            max_evaluation_epochs,
            max_derived_rows: None,
            max_work_units: None,
            persist_mode: PersistMode::default(),
        }
    }

    /// Set the maximum number of newly derived rows/facts per query.
    #[must_use]
    pub fn with_max_derived_rows(mut self, max_derived_rows: u64) -> Self {
        self.max_derived_rows = Some(max_derived_rows);
        self
    }

    /// Set the maximum evaluator work-unit count per query.
    #[must_use]
    pub fn with_max_work_units(mut self, max_work_units: u64) -> Self {
        self.max_work_units = Some(max_work_units);
        self
    }

    /// Set the fjall persist mode.
    #[must_use]
    pub fn with_persist_mode(mut self, persist_mode: PersistMode) -> Self {
        self.persist_mode = persist_mode;
        self
    }

    pub(crate) fn query_budget(self, timeout_secs: Option<f64>) -> QueryBudget {
        QueryBudget {
            wall_clock_timeout: timeout_secs.map(saturating_secs_f64),
            max_epochs: self.max_evaluation_epochs,
            max_derived_rows: self.max_derived_rows,
            max_work_units: self.max_work_units,
        }
    }
}

pub(crate) struct RunningQueryHandle {
    pub(crate) started_at: f64,
    pub(crate) poison: Poison,
}

pub(crate) struct RunningQueryCleanup {
    pub(crate) id: u64,
    /// Guards the set of in-flight queries so concurrent cancellations and
    /// completions do not corrupt the map. Held briefly on drop to remove
    /// this query's entry and poison its handle.
    pub(crate) running_queries: Arc<Mutex<BTreeMap<u64, RunningQueryHandle>>>,
}

impl Drop for RunningQueryCleanup {
    fn drop(&mut self) {
        let mut map = self
            .running_queries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = map.remove(&self.id) {
            handle.poison.set_killed();
        }
    }
}

/// Whether a script is mutable or immutable.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScriptMutability {
    /// The script is mutable.
    Mutable,
    /// The script is immutable.
    Immutable,
}

/// Paired counter and registry for event callbacks, gated off on wasm32 where
/// threading is unavailable.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub(crate) struct EventCallbacks {
    pub(crate) count: Arc<AtomicU32>,
    pub(crate) registry: Arc<ShardedLock<EventCallbackRegistry>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for EventCallbacks {
    fn default() -> Self {
        Self {
            count: Default::default(),
            registry: Default::default(),
        }
    }
}

/// The krites engine database object.
#[derive(Clone)]
pub struct Db<S> {
    pub(crate) db: S,
    pub(crate) config: DbConfig,
    pub(crate) temp_db: TempStorage,
    pub(crate) relation_store_id: Arc<AtomicU64>,
    pub(crate) queries_count: Arc<AtomicU64>,
    /// Guards the set of in-flight queries. Invariant: each running query has
    /// exactly one entry keyed by its monotonic id; the entry is removed on
    /// completion or cancellation. Held briefly during query start, kill, and cleanup.
    pub(crate) running_queries: Arc<Mutex<BTreeMap<u64, RunningQueryHandle>>>,
    pub(crate) fixed_rules: Arc<ShardedLock<BTreeMap<String, Arc<Box<dyn FixedRule>>>>>,
    pub(crate) tokenizers: Arc<TokenizerCache>,
    pub(crate) fts_cache: Arc<RwLock<FtsCache>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) callbacks: EventCallbacks,
    pub(crate) relation_locks: Arc<ShardedLock<BTreeMap<CompactString, Arc<RwLock<()>>>>>,
    #[cfg(feature = "hot-reload")]
    pub(crate) rule_store: Option<Arc<arc_swap::ArcSwap<crate::hot_reload::RuleSet>>>,
}

impl<S> Debug for Db<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Db")
    }
}

/// Raw deserialization type for [`NamedRows`].
#[derive(serde::Deserialize, Debug, Clone, Default)]
struct NamedRowsRaw {
    headers: Vec<String>,
    rows: Vec<Tuple>,
    next: Option<Box<NamedRows>>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
#[serde(from = "NamedRowsRaw")]
/// Rows in a relation, together with headers for the fields.
pub struct NamedRows {
    /// The headers
    pub headers: Vec<String>,
    /// The rows
    pub rows: Vec<Tuple>,
    /// Contains the next named rows, if exists
    pub next: Option<Box<NamedRows>>,
}

impl From<NamedRowsRaw> for NamedRows {
    fn from(raw: NamedRowsRaw) -> Self {
        Self::new(raw.headers, raw.rows)
    }
}

impl IntoIterator for NamedRows {
    type Item = Tuple;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl NamedRows {
    /// Create named rows with the given headers and rows.
    #[must_use]
    pub fn new(headers: Vec<String>, rows: Vec<Tuple>) -> Self {
        Self {
            headers,
            rows,
            next: None,
        }
    }

    /// If there are more named rows after the current one.
    #[must_use]
    pub fn has_more(&self) -> bool {
        self.next.is_some()
    }

    /// Convert a chain of named rows to individual named rows.
    #[must_use]
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

    /// Convert to a JSON object.
    #[must_use]
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
    /// Make named rows from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is missing required fields (`headers`, `rows`)
    /// or if the structure is invalid.
    #[must_use = "returns parsed rows or an error"]
    pub fn from_json(value: &JsonValue) -> Result<Self> {
        let headers = value
            .get("headers")
            .ok_or_else(|| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "import",
                    reason: "NamedRows requires 'headers' field",
                }
                .build(),
            })?;
        let headers = headers
            .as_array()
            .ok_or_else(|| crate::error::InternalError::Runtime {
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
                    .ok_or_else(|| crate::error::InternalError::Runtime {
                        source: InvalidOperationSnafu {
                            op: "import",
                            reason: "'headers' field must be an array of strings",
                        }
                        .build(),
                    })?;
                Ok(h.to_string())
            })
            .try_collect()?;
        let rows = value
            .get("rows")
            .ok_or_else(|| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "import",
                    reason: "NamedRows requires 'rows' field",
                }
                .build(),
            })?;
        let rows = rows
            .as_array()
            .ok_or_else(|| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "import",
                    reason: "'rows' field must be an array",
                }
                .build(),
            })?;
        let rows = rows
            .iter()
            .map(|row| -> Result<Vec<DataValue>> {
                let row = row
                    .as_array()
                    .ok_or_else(|| crate::error::InternalError::Runtime {
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
    #[must_use]
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
#[non_exhaustive]
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
    #[must_use = "returns the database instance or an error"]
    pub fn new(storage: S) -> Result<Self> {
        let ret = Self {
            db: storage,
            config: DbConfig::default(),
            temp_db: Default::default(),
            relation_store_id: Default::default(),
            queries_count: Default::default(),
            running_queries: Default::default(),
            fixed_rules: Arc::new(ShardedLock::new(DEFAULT_FIXED_RULES.clone())),
            tokenizers: Arc::new(Default::default()),
            fts_cache: Arc::new(RwLock::new(FtsCache::default())),
            #[cfg(not(target_arch = "wasm32"))]
            callbacks: EventCallbacks::default(),
            relation_locks: Default::default(),
            #[cfg(feature = "hot-reload")]
            rule_store: None,
        };
        Ok(ret)
    }

    /// Must be called after creation of the database to initialize the runtime state.
    ///
    /// # Errors
    ///
    /// Returns an error if storage initialization fails or version checks fail.
    #[must_use = "initialization can fail"]
    pub fn initialize(&'s self) -> Result<()> {
        self.load_last_ids()?;
        Ok(())
    }

    /// This returns the set of fixed rule implementations for this specific backend.
    pub fn get_fixed_rules(&'s self) -> BTreeMap<String, Arc<Box<dyn FixedRule>>> {
        return self
            .fixed_rules
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }

    /// Register a custom fixed rule implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if a rule with the same name is already registered.
    #[must_use = "registration can fail if the name is already taken"]
    pub fn register_fixed_rule<R>(&self, name: String, rule_impl: R) -> Result<()>
    where
        R: FixedRule + 'static,
    {
        match self
            .fixed_rules
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(name)
        {
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
    ///
    /// # Errors
    ///
    /// Returns an error if attempting to unregister a builtin fixed rule.
    #[must_use = "returns whether the rule existed or an error"]
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
            .unwrap_or_else(|e| e.into_inner())
            .remove(name)
            .is_some())
    }

    /// Register callback channel to receive changes when the requested relation are successfully committed.
    /// The returned ID can be used to unregister the callback channel.
    ///
    /// `capacity` bounds the channel; when it is full, new events are dropped so a slow
    /// consumer cannot cause unbounded memory growth. Consumers can recover missed
    /// notifications by re-reading the relation.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_callback(
        &self,
        relation: &str,
        capacity: usize,
    ) -> (u32, Receiver<(CallbackOp, NamedRows, NamedRows)>) {
        let (sender, receiver) = bounded(capacity);
        let cb = CallbackDeclaration {
            dependent: CompactString::from(relation),
            sender,
        };

        let mut guard = self
            .callbacks
            .registry
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let new_id = self.callbacks.count.fetch_add(1, Ordering::SeqCst);
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
        let mut guard = self
            .callbacks
            .registry
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let ret = guard.0.remove(&id);
        if let Some(cb) = &ret {
            guard
                .1
                // INVARIANT: register_callback inserts into both maps
                .get_mut(&cb.dependent)
                .unwrap_or_else(|| {
                    unreachable!("callback dependent entry missing from reverse index")
                })
                .remove(&id);

            if guard
                .1
                .get(&cb.dependent)
                .unwrap_or_else(|| {
                    unreachable!("callback dependent entry missing from reverse index")
                })
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
    ) -> Vec<Arc<RwLock<()>>> {
        // WHY: hold a single write lock for the entire classify-and-insert
        // operation. A read-then-write upgrade gap is a TOCTOU window where
        // concurrent callers could race on first creation and a future cleanup
        // path could re-insert a stale lock for a dropped relation.
        let mut locks = self
            .relation_locks
            .write()
            .unwrap_or_else(|e| e.into_inner());
        rels.map(|rel| locks.entry(rel.clone()).or_default().clone())
            .collect()
    }

    pub(crate) fn compact_relation(&'s self) -> Result<()> {
        let l = Tuple::default().encode_as_key(RelationId(0));
        let u = vec![DataValue::Bot].encode_as_key(RelationId(u64::MAX));
        self.db.range_compact(&l, &u)?;
        Ok(())
    }
}

pub(crate) fn seconds_since_the_epoch() -> Result<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    let now = SystemTime::now();
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(now
        .duration_since(UNIX_EPOCH)
        .map_err(|e| crate::error::InternalError::Runtime {
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
