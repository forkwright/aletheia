//! Async surface for the blocking krites engine.
//!
//! Provides [`AsyncDb`], a tokio-native wrapper over the synchronous [`Db`].
//! CPU-bound query evaluation and blocking I/O operations are offloaded to
//! `tokio::task::spawn_blocking` so callers do not need to bridge manually.
//!
//! # Usage
//!
//! ```rust,no_run
//! use krites::{AsyncDb, ScriptMutability};
//! use std::collections::BTreeMap;
//!
//! # async fn example() -> Result<(), krites::Error> {
//! let db = AsyncDb::open_mem().await?;
//! let rows = db.run_read_only("?[x] := x = 1", BTreeMap::new()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! The synchronous [`Db`] API remains unchanged; the async surface is purely
//! additive.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
#[cfg(feature = "storage-fjall")]
use std::path::Path;
use std::sync::Arc;

use tracing::instrument;

#[cfg(not(target_arch = "wasm32"))]
use crate::runtime::callback::CallbackOp;
use crate::{
    DataValue, Db, DbConfig, FixedRule, NamedRows, QueryCache, QueryCacheStats, ScriptMutability,
};

/// Tokio-native async wrapper over the blocking [`Db`] core.
///
/// All mutating and query methods are mirrored as `async fn`. Methods that
/// perform genuine CPU work or blocking I/O use `tokio::task::spawn_blocking`
/// internally; lightweight getters return plain futures without a thread hop.
#[derive(Clone)]
#[non_exhaustive]
pub struct AsyncDb {
    inner: Arc<Db>,
}

#[expect(
    clippy::result_large_err,
    reason = "engine Error carries structured context — boxing deferred to avoid API churn"
)]
impl AsyncDb {
    /// Open an in-memory database.
    #[instrument]
    pub async fn open_mem() -> crate::Result<Self> {
        let db = Db::open_mem()?;
        Ok(Self {
            inner: Arc::new(db),
        })
    }

    /// Open a fjall-backed database at the given path.
    ///
    /// Primary production backend: pure Rust, LSM-tree, LZ4 compression,
    /// native read-your-own-writes.
    #[cfg(feature = "storage-fjall")]
    #[instrument(skip(path))]
    pub async fn open_fjall(path: impl AsRef<Path> + Send + 'static) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let db = tokio::task::spawn_blocking(move || Db::open_fjall(&path))
            .await
            .map_err(|e| map_join_err(&e))??;
        Ok(Self {
            inner: Arc::new(db),
        })
    }

    /// Attach an LRU query cache with the given capacity.
    ///
    /// Once attached, every [`AsyncDb::run`] call checks the normalized query
    /// against the cache and records a hit or miss. Retrieve statistics with
    /// [`AsyncDb::cache_stats`].
    #[must_use]
    pub fn with_cache(self, capacity: NonZeroUsize) -> Self {
        let mut db = match Arc::try_unwrap(self.inner) {
            Ok(db) => db,
            Err(arc) => Db {
                inner: arc.clone_inner(),
                cache: None,
            },
        };
        db.cache = Some(Arc::new(QueryCache::new(capacity)));
        Self {
            inner: Arc::new(db),
        }
    }

    /// Replace runtime limits for this database.
    #[must_use]
    pub fn with_config(self, config: DbConfig) -> Self {
        let db = match Arc::try_unwrap(self.inner) {
            Ok(db) => db,
            Err(arc) => Db {
                inner: arc.clone_inner(),
                cache: None,
            },
        }
        .with_config(config);
        Self {
            inner: Arc::new(db),
        }
    }

    /// Return a snapshot of query cache statistics, or `None` if no cache is attached.
    #[must_use]
    #[instrument(skip(self))]
    pub async fn cache_stats(&self) -> Option<QueryCacheStats> {
        self.inner.cache_stats()
    }

    /// Execute a Datalog script.
    ///
    /// If a query cache is attached, the normalized query string is checked
    /// before execution and the hit/miss counter is updated.
    #[instrument(skip(self))]
    pub async fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> crate::Result<NamedRows> {
        let inner = self.inner.clone();
        let script = script.to_string();
        tokio::task::spawn_blocking(move || inner.run(&script, params, mutability))
            .await
            .map_err(|e| map_join_err(&e))?
    }

    /// Execute a Datalog script in read-only mode.
    #[instrument(skip(self))]
    pub async fn run_read_only(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::Result<NamedRows> {
        let inner = self.inner.clone();
        let script = script.to_string();
        tokio::task::spawn_blocking(move || inner.run_read_only(&script, params))
            .await
            .map_err(|e| map_join_err(&e))?
    }

    /// Export named relations as an engine-level relation snapshot.
    #[instrument(skip(self, relations))]
    pub async fn export_relations<I, T>(
        &self,
        relations: I,
    ) -> crate::Result<BTreeMap<String, NamedRows>>
    where
        I: Iterator<Item = T> + Send,
        T: AsRef<str> + Send,
    {
        let inner = self.inner.clone();
        let relations: Vec<String> = relations.map(|s| s.as_ref().to_string()).collect();
        tokio::task::spawn_blocking(move || {
            inner.export_relations(relations.iter().map(String::as_str))
        })
        .await
        .map_err(|e| map_join_err(&e))?
    }

    /// Import relations from an engine-level relation snapshot.
    #[instrument(skip(self))]
    pub async fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> crate::Result<()> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || inner.import_relations(data))
            .await
            .map_err(|e| map_join_err(&e))?
    }

    /// Register a custom fixed rule (graph algorithm).
    #[instrument(skip(self, rule))]
    pub async fn register_fixed_rule<R: FixedRule + 'static>(
        &self,
        name: String,
        rule: R,
    ) -> crate::Result<()> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || inner.register_fixed_rule(name, rule))
            .await
            .map_err(|e| map_join_err(&e))?
    }

    /// Register a callback for relation changes.
    ///
    /// `capacity` bounds the channel; when it is full, new events are dropped so a slow
    /// consumer cannot cause unbounded memory growth. Consumers can recover missed
    /// notifications by re-reading the relation.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    #[instrument(skip(self))]
    pub async fn register_callback(
        &self,
        relation: &str,
        capacity: usize,
    ) -> (
        u32,
        crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>,
    ) {
        self.inner.register_callback(relation, capacity)
    }

    /// Begin a multi-relation transaction.
    #[must_use]
    #[instrument(skip(self))]
    pub async fn multi_transaction(&self, write: bool) -> crate::MultiTransaction {
        self.inner.multi_transaction(write)
    }
}

#[inline]
fn map_join_err(err: &tokio::task::JoinError) -> crate::Error {
    crate::error::EngineSnafu {
        message: format!("blocking task panicked or was aborted: {err}"),
    }
    .build()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    use super::*;

    #[tokio::test]
    async fn async_run_matches_blocking_run() {
        let sync_db = Db::open_mem().unwrap();
        let async_db = AsyncDb::open_mem().await.unwrap();

        let script = "?[x] := x = 1";
        let params = BTreeMap::new();

        let sync_res = sync_db.run_read_only(script, params.clone()).unwrap();
        let async_res = async_db.run_read_only(script, params).await.unwrap();

        assert_eq!(sync_res.headers, async_res.headers);
        assert_eq!(sync_res.rows, async_res.rows);
        assert!(sync_res.next.is_none());
        assert!(async_res.next.is_none());
    }

    #[tokio::test]
    async fn async_run_does_not_block_runtime() {
        let async_db = AsyncDb::open_mem().await.unwrap();

        let script = "?[x] := x = 1";
        let params = BTreeMap::new();

        let start = Instant::now();
        let run_fut = async_db.run_read_only(script, params);
        let timer_fut = tokio::time::sleep(Duration::from_millis(10));

        let (run_res, ()) = tokio::join!(run_fut, timer_fut);
        assert!(run_res.is_ok());
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "timer should fire promptly, proving run did not block the async thread"
        );
    }

    #[tokio::test]
    async fn async_run_read_only_returns_on_completion() {
        let async_db = AsyncDb::open_mem().await.unwrap();
        let script = "?[x] := x = 1";
        let params = BTreeMap::new();

        let result = async_db.run_read_only(script, params).await;
        assert!(result.is_ok());
    }
}
