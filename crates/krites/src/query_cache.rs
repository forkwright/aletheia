//! LRU-bounded prepared statement cache for Datalog queries.
//!
//! Caches normalized query strings to track repeated query execution.
//! Hit and miss counters are exposed for observability via [`QueryCacheStats`].
//!
//! Thread-safe via an internal `Mutex`.

use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use lru::LruCache;

/// Snapshot of [`QueryCache`] statistics.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct QueryCacheStats {
    /// Number of cache hits since the cache was created.
    pub hits: u64,
    /// Number of cache misses since the cache was created.
    pub misses: u64,
    /// Maximum number of distinct normalized queries the cache can hold.
    pub capacity: usize,
    /// Number of distinct normalized queries currently held in the cache.
    pub len: usize,
}

/// LRU-bounded cache for Datalog query strings.
///
/// On each [`QueryCache::check`] call the query is normalized (whitespace
/// collapsed), then looked up in an LRU cache.  A hit promotes the entry to
/// the most-recently-used position and increments the hit counter; a miss
/// inserts the entry and increments the miss counter.
///
/// The cache does not store compiled query plans. It tracks *which queries
/// have been seen* and exposes hit/miss metrics so callers can observe query
/// repetition patterns and make caching decisions from the metrics.
pub struct QueryCache {
    inner: Mutex<LruCache<String, ()>>,
    hits: AtomicU64,
    misses: AtomicU64,
    capacity: NonZeroUsize,
}

impl QueryCache {
    /// Create a new cache that holds at most `capacity` distinct queries.
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(capacity)),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            capacity,
        }
    }

    /// Normalize a query string by collapsing all whitespace runs to a single
    /// space and trimming leading/trailing whitespace.
    ///
    /// Normalization ensures that semantically identical queries with different
    /// formatting are treated as the same cache key.
    #[must_use]
    pub fn normalize(query: &str) -> String {
        query.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Check whether the normalized form of `query` is already in the cache.
    ///
    /// Returns `true` (cache hit) if the normalized query was present, or
    /// `false` (cache miss) if it was not.  On a miss the entry is inserted;
    /// on a hit the entry is promoted to the most-recently-used position.
    ///
    /// The hit or miss counter is incremented to reflect the outcome.
    pub fn check(&self, query: &str) -> bool {
        let normalized = Self::normalize(query);
        // WHY: lock held only for the duration of the LRU lookup — no await points.
        let mut guard = self
            .inner
            .lock()
            .expect("query cache lock must not be poisoned");
        if guard.get(normalized.as_str()).is_some() {
            drop(guard);
            self.hits.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            guard.put(normalized, ());
            drop(guard);
            self.misses.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Return a snapshot of current cache statistics.
    #[must_use]
    pub fn stats(&self) -> QueryCacheStats {
        let guard = self
            .inner
            .lock()
            .expect("query cache lock must not be poisoned");
        QueryCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            capacity: self.capacity.get(),
            len: guard.len(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn cache(cap: usize) -> QueryCache {
        QueryCache::new(NonZeroUsize::new(cap).expect("capacity must be non-zero"))
    }

    #[test]
    fn first_call_is_a_miss() {
        let c = cache(8);
        let hit = c.check("?[x] := *facts{x}");
        assert!(!hit, "first check for a new query should be a cache miss");
    }

    #[test]
    fn second_call_is_a_hit() {
        let c = cache(8);
        c.check("?[x] := *facts{x}");
        let hit = c.check("?[x] := *facts{x}");
        assert!(hit, "repeated identical query should be a cache hit");
    }

    #[test]
    fn normalized_whitespace_matches() {
        let c = cache(8);
        c.check("?[x] := *facts{x}");
        // Extra whitespace and leading/trailing space should normalize to the same key.
        let hit = c.check("  ?[x]   :=  *facts{x}  ");
        assert!(
            hit,
            "query with different whitespace should hit after normalized form is cached"
        );
    }

    #[test]
    fn stats_track_hits_and_misses() {
        let c = cache(8);
        c.check("query_a");
        c.check("query_b");
        c.check("query_a"); // hit

        let stats = c.stats();
        assert_eq!(
            stats.misses, 2,
            "two distinct queries should produce two misses"
        );
        assert_eq!(stats.hits, 1, "one repeated query should produce one hit");
        assert_eq!(stats.len, 2, "cache should hold two distinct entries");
    }

    #[test]
    fn lru_eviction_respects_capacity() {
        let c = cache(2);
        c.check("a");
        c.check("b");
        c.check("c"); // evicts "a" (LRU)

        // "a" was evicted: should be a miss again.
        let hit = c.check("a");
        assert!(!hit, "evicted query should register as a miss");

        let stats = c.stats();
        assert_eq!(
            stats.capacity, 2,
            "capacity should remain at the configured value"
        );
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(
            QueryCache::normalize("  a  b  c  "),
            "a b c",
            "normalize should trim and collapse interior whitespace"
        );
    }

    #[test]
    fn normalize_empty_string() {
        assert_eq!(
            QueryCache::normalize(""),
            "",
            "empty string should normalize to empty string"
        );
    }
}
