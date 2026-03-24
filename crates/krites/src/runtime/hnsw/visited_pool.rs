//! Pre-allocated visited-list pool for lock-free HNSW search traversal.
//!
//! Eliminates per-search allocation by pooling [`FxHashSet`] instances. Each
//! search acquires a set from the pool, uses it for duplicate detection during
//! graph traversal, and releases it back when done. The set is cleared on
//! release, not on acquire, so the `acquire` path is allocation-free when a
//! pooled set is available.
//!
//! Thread-safety is provided by `crossbeam::queue::ArrayQueue` (lock-free
//! bounded MPMC queue).

use crossbeam::queue::ArrayQueue;
use rustc_hash::FxHashSet;

use super::types::CompoundKey;

/// Default number of visited lists kept in the pool.
///
/// Sized for typical concurrent search load: one list per in-flight search.
const DEFAULT_POOL_CAPACITY: usize = 16;

/// Default initial capacity for each hash set in the pool.
///
/// Pre-allocates enough buckets to avoid rehashing for small-to-medium graphs.
const DEFAULT_SET_CAPACITY: usize = 256;

/// A pool of reusable visited-node sets for HNSW search traversal.
///
/// Each set tracks which graph nodes have been visited during a single search
/// operation. Instead of allocating a fresh `FxHashSet` per search, callers
/// [`acquire`](VisitedPool::acquire) a set from this pool and
/// [`release`](VisitedPool::release) it when done.
///
/// When the pool is exhausted, `acquire` creates a fresh set (graceful
/// degradation, not a hard failure).
pub(crate) struct VisitedPool {
    pool: ArrayQueue<FxHashSet<CompoundKey>>,
    set_capacity: usize,
}

impl VisitedPool {
    /// Create a pool with `pool_size` pre-allocated visited sets, each with
    /// initial hash-map capacity `set_capacity`.
    pub(crate) fn new(pool_size: usize, set_capacity: usize) -> Self {
        let pool = ArrayQueue::new(pool_size);
        for _ in 0..pool_size {
            // SAFETY: pool_size == queue capacity, so push always succeeds here.
            let _ = pool.push(FxHashSet::with_capacity_and_hasher(
                set_capacity,
                Default::default(),
            ));
        }
        Self { pool, set_capacity }
    }

    /// Create a pool with default sizing.
    pub(crate) fn with_defaults() -> Self {
        Self::new(DEFAULT_POOL_CAPACITY, DEFAULT_SET_CAPACITY)
    }

    /// Acquire a visited set from the pool.
    ///
    /// Returns a pooled set if one is available, otherwise allocates a new one.
    /// The returned set is guaranteed to be empty.
    pub(crate) fn acquire(&self) -> FxHashSet<CompoundKey> {
        self.pool.pop().unwrap_or_else(|| {
            FxHashSet::with_capacity_and_hasher(self.set_capacity, Default::default())
        })
    }

    /// Release a visited set back to the pool.
    ///
    /// The set is cleared before being returned to the pool. If the pool is
    /// full the set is simply dropped.
    pub(crate) fn release(&self, mut set: FxHashSet<CompoundKey>) {
        set.clear();
        // If the pool is full, the set is dropped -- no error.
        let _ = self.pool.push(set);
    }

    /// Number of sets currently available in the pool.
    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        self.pool.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::value::DataValue;

    #[test]
    fn acquire_returns_empty_set() {
        let pool = VisitedPool::with_defaults();
        let set = pool.acquire();
        assert!(set.is_empty(), "acquired set must be empty");
    }

    #[test]
    fn release_clears_and_returns_to_pool() {
        let pool = VisitedPool::new(2, 64);
        assert_eq!(pool.available(), 2, "pool starts full");

        let mut set = pool.acquire();
        assert_eq!(pool.available(), 1, "one set consumed");

        // Insert some data.
        set.insert((vec![DataValue::from(1_i64)], 0, -1));
        set.insert((vec![DataValue::from(2_i64)], 0, -1));

        pool.release(set);
        assert_eq!(pool.available(), 2, "set returned to pool");

        // Re-acquire -- should be empty.
        let reused = pool.acquire();
        assert!(reused.is_empty(), "released set must be cleared");
    }

    #[test]
    fn pool_exhaustion_creates_fresh_set() {
        let pool = VisitedPool::new(1, 64);
        let _s1 = pool.acquire();
        assert_eq!(pool.available(), 0, "pool is empty");

        // Second acquire should still succeed (fallback allocation).
        let s2 = pool.acquire();
        assert!(s2.is_empty(), "fallback set must be empty");
    }

    #[test]
    fn release_to_full_pool_drops_set() {
        let pool = VisitedPool::new(1, 64);
        // Pool is full (1/1).
        let extra = FxHashSet::default();
        pool.release(extra);
        // Pool is still 1 -- the extra set was dropped, not enqueued.
        assert_eq!(pool.available(), 1, "pool should not exceed capacity");
    }
}
