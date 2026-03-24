//! In-memory API response cache with TTL and request deduplication.
//!
//! # Design
//!
//! - `ApiCache` is `Send + Sync` and intended to be shared via `Arc<Mutex<ApiCache>>`.
//! - Entries expire after their TTL; [`ApiCache::get`] returns `None` for stale entries.
//! - Request deduplication prevents concurrent identical calls within a 500 ms window.
//!   Call [`ApiCache::mark_in_flight`] before issuing a request and
//!   [`ApiCache::mark_complete`] when it finishes.
//! - [`ApiCache::evict_expired`] prunes stale entries; call periodically or before insertion.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default TTL for non-streaming API responses (sessions, entities, metrics).
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(30);

/// Window within which identical in-flight requests are deduplicated.
const DEDUP_WINDOW: Duration = Duration::from_millis(500);

struct Entry {
    value: serde_json::Value,
    inserted_at: Instant,
    ttl: Duration,
}

impl Entry {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() >= self.ttl
    }
}

/// In-memory cache for API responses.
#[derive(Default)]
pub(crate) struct ApiCache {
    entries: HashMap<String, Entry>,
    in_flight: HashMap<String, Instant>,
}

impl ApiCache {
    /// Create a new, empty cache.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Look up a cached value by URL key.
    ///
    /// Returns `None` if the entry is absent or has expired.
    #[must_use]
    pub(crate) fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.entries
            .get(key)
            .filter(|e| !e.is_expired())
            .map(|e| &e.value)
    }

    /// Insert or replace a cache entry with the given TTL.
    pub(crate) fn insert(&mut self, key: impl Into<String>, value: serde_json::Value, ttl: Duration) {
        self.entries.insert(
            key.into(),
            Entry {
                value,
                inserted_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Insert with the default TTL.
    pub(crate) fn insert_default(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.insert(key, value, DEFAULT_TTL);
    }

    /// Invalidate a specific key.
    pub(crate) fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Invalidate all keys that contain the given prefix.
    pub(crate) fn invalidate_prefix(&mut self, prefix: &str) {
        self.entries.retain(|k, _| !k.starts_with(prefix));
    }

    /// Returns `true` if a request for `key` is already in flight.
    ///
    /// In-flight markers older than `DEDUP_WINDOW` are considered stale and
    /// return `false` (the dedup window has expired).
    #[must_use]
    pub(crate) fn is_in_flight(&self, key: &str) -> bool {
        self.in_flight
            .get(key)
            .is_some_and(|t| t.elapsed() < DEDUP_WINDOW)
    }

    /// Mark a request as in-flight to suppress duplicate calls.
    pub(crate) fn mark_in_flight(&mut self, key: impl Into<String>) {
        self.in_flight.insert(key.into(), Instant::now());
    }

    /// Mark a request as complete, removing its in-flight entry.
    pub(crate) fn mark_complete(&mut self, key: &str) {
        self.in_flight.remove(key);
    }

    /// Evict all entries whose TTL has expired and all stale in-flight markers.
    pub(crate) fn evict_expired(&mut self) {
        self.entries.retain(|_, e| !e.is_expired());
        self.in_flight
            .retain(|_, t| t.elapsed() < DEDUP_WINDOW);
    }

    /// Return the number of live (non-expired) cache entries.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn live_count(&self) -> usize {
        self.entries.values().filter(|e| !e.is_expired()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut cache = ApiCache::new();
        cache.insert("url1", serde_json::json!({"a": 1}), Duration::from_secs(60));
        let v = cache.get("url1").expect("should be present");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn expired_entry_returns_none() {
        let mut cache = ApiCache::new();
        cache.insert("url2", serde_json::json!({}), Duration::from_millis(0));
        // TTL=0 → immediately expired
        assert!(cache.get("url2").is_none());
    }

    #[test]
    fn dedup_in_flight() {
        let mut cache = ApiCache::new();
        assert!(!cache.is_in_flight("url3"));
        cache.mark_in_flight("url3");
        assert!(cache.is_in_flight("url3"));
        cache.mark_complete("url3");
        assert!(!cache.is_in_flight("url3"));
    }

    #[test]
    fn evict_clears_expired() {
        let mut cache = ApiCache::new();
        cache.insert("stale", serde_json::json!(1), Duration::from_millis(0));
        cache.insert("fresh", serde_json::json!(2), Duration::from_secs(60));
        cache.evict_expired();
        assert_eq!(cache.live_count(), 1);
        assert!(cache.get("fresh").is_some());
    }

    #[test]
    fn invalidate_prefix() {
        let mut cache = ApiCache::new();
        cache.insert_default("/api/v1/sessions", serde_json::json!([]));
        cache.insert_default("/api/v1/sessions/abc", serde_json::json!({}));
        cache.insert_default("/api/v1/knowledge/facts", serde_json::json!([]));
        cache.invalidate_prefix("/api/v1/sessions");
        assert!(cache.get("/api/v1/sessions").is_none());
        assert!(cache.get("/api/v1/sessions/abc").is_none());
        assert!(cache.get("/api/v1/knowledge/facts").is_some());
    }
}
