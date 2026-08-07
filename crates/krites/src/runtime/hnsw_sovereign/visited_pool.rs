//! Pre-allocated pool of visited-node sets for lock-free HNSW search
//! traversal.
//!
//! A beam search needs a per-search "have I seen this node yet" set purely
//! for the duration of one traversal. Allocating a fresh [`FxHashSet`] per
//! search is wasted work under concurrent search load; this pool hands out
//! a cleared set on [`acquire`](VisitedPool::acquire) and reclaims it on
//! [`release`](VisitedPool::release), backed by a lock-free bounded MPMC
//! queue so acquiring and releasing never contend on a mutex.

use crossbeam::queue::ArrayQueue;
use rustc_hash::FxHashSet;

use super::types::CompoundKey;

/// Visited-set slots kept warm in the pool — sized for roughly one
/// in-flight search per slot under typical concurrent load.
const DEFAULT_POOL_CAPACITY: usize = 16;

/// Initial bucket count per pooled set, chosen to avoid rehashing on
/// small-to-medium graphs.
const DEFAULT_SET_CAPACITY: usize = 256;

/// A bounded pool of reusable [`FxHashSet`]s tracking visited HNSW nodes
/// during one search's traversal.
pub(crate) struct VisitedPool {
    slots: ArrayQueue<FxHashSet<CompoundKey>>,
    set_capacity: usize,
}

impl VisitedPool {
    /// Pre-allocate `pool_size` sets, each with initial capacity `set_capacity`.
    pub(crate) fn new(pool_size: usize, set_capacity: usize) -> Self {
        let slots = ArrayQueue::new(pool_size);
        for _ in 0..pool_size {
            // SAFETY: the queue's capacity is exactly `pool_size` and this
            // loop pushes at most `pool_size` times, so push never fails.
            let _ = slots.push(FxHashSet::with_capacity_and_hasher(set_capacity, Default::default()));
        }
        Self { slots, set_capacity }
    }

    pub(crate) fn with_defaults() -> Self {
        Self::new(DEFAULT_POOL_CAPACITY, DEFAULT_SET_CAPACITY)
    }

    /// Take a set from the pool — guaranteed empty. Falls back to a fresh
    /// allocation (graceful degradation, not a hard failure) if the pool is
    /// currently exhausted.
    pub(crate) fn acquire(&self) -> FxHashSet<CompoundKey> {
        self.slots
            .pop()
            .unwrap_or_else(|| FxHashSet::with_capacity_and_hasher(self.set_capacity, Default::default()))
    }

    /// Return a set to the pool, clearing it first. If the pool is already
    /// full the set is simply dropped — not an error.
    pub(crate) fn release(&self, mut set: FxHashSet<CompoundKey>) {
        set.clear();
        let _ = self.slots.push(set);
    }

    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        self.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::value::DataValue;

    #[test]
    fn acquired_set_is_empty() {
        let pool = VisitedPool::with_defaults();
        assert!(pool.acquire().is_empty());
    }

    #[test]
    fn release_clears_and_returns_to_pool() {
        let pool = VisitedPool::new(2, 64);
        assert_eq!(pool.available(), 2);

        let mut set = pool.acquire();
        assert_eq!(pool.available(), 1);
        set.insert((vec![DataValue::from(1_i64)], 0, -1));
        set.insert((vec![DataValue::from(2_i64)], 0, -1));

        pool.release(set);
        assert_eq!(pool.available(), 2);
        assert!(pool.acquire().is_empty());
    }

    #[test]
    fn exhausted_pool_falls_back_to_fresh_allocation() {
        let pool = VisitedPool::new(1, 64);
        let _held = pool.acquire();
        assert_eq!(pool.available(), 0);
        assert!(pool.acquire().is_empty(), "a second acquire beyond capacity must still succeed");
    }

    #[test]
    fn release_beyond_capacity_drops_the_set_silently() {
        let pool = VisitedPool::new(1, 64);
        pool.release(FxHashSet::default());
        assert_eq!(pool.available(), 1, "pool must not grow past its configured capacity");
    }
}
