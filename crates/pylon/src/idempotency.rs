//! Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
//!
//! Stores a bounded, TTL-evicting map of `(idempotency_key, state)` pairs so that
//! retried requests with the same key return the cached response instead of
//! creating duplicate turns.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::StatusCode;

/// Default TTL for cached idempotency entries.
const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);

/// Maximum number of entries in the cache.
const DEFAULT_CAPACITY: usize = 10_000;

/// Maximum allowed length for an idempotency key string.
pub(crate) const MAX_KEY_LENGTH: usize = 64;

/// Thread-safe idempotency cache with LRU eviction and TTL expiry.
pub struct IdempotencyCache {
    inner: Mutex<CacheInner>,
}

struct CacheInner {
    entries: HashMap<String, CacheEntry>,
    /// Insertion order for LRU eviction (front = oldest).
    order: VecDeque<String>,
    capacity: usize,
    ttl: Duration,
}

struct CacheEntry {
    state: EntryState,
    created_at: Instant,
}

/// The state of a cached idempotency entry.
enum EntryState {
    /// Request is currently being processed.
    InFlight,
    /// Request completed — cached response ready to replay.
    Completed { status: StatusCode, body: String },
}

/// Result of looking up an idempotency key.
pub(crate) enum LookupResult {
    /// Key not seen before — caller should proceed with the request.
    Miss,
    /// A previous request with this key completed — return the cached response.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "fields reserved for non-SSE endpoints that replay full cached responses"
        )
    )]
    Hit { status: StatusCode, body: String },
    /// A request with this key is still in progress.
    Conflict,
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IdempotencyCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity: DEFAULT_CAPACITY,
                ttl: DEFAULT_TTL,
            }),
        }
    }

    /// Look up a key. On miss, atomically inserts it as `InFlight`.
    pub(crate) fn check_or_insert(&self, key: &str) -> LookupResult {
        let mut inner = self.inner.lock().expect("idempotency cache lock poisoned");
        inner.evict_expired();

        if let Some(entry) = inner.entries.get(key) {
            return match &entry.state {
                EntryState::InFlight => LookupResult::Conflict,
                EntryState::Completed { status, body } => LookupResult::Hit {
                    status: *status,
                    body: body.clone(),
                },
            };
        }

        // Evict oldest if at capacity
        while inner.entries.len() >= inner.capacity {
            if let Some(oldest_key) = inner.order.pop_front() {
                inner.entries.remove(&oldest_key);
            } else {
                break;
            }
        }

        inner.entries.insert(
            key.to_owned(),
            CacheEntry {
                state: EntryState::InFlight,
                created_at: Instant::now(),
            },
        );
        inner.order.push_back(key.to_owned());

        LookupResult::Miss
    }

    /// Mark a key as completed with the given response.
    pub(crate) fn complete(&self, key: &str, status: StatusCode, body: String) {
        let mut inner = self.inner.lock().expect("idempotency cache lock poisoned");
        if let Some(entry) = inner.entries.get_mut(key) {
            entry.state = EntryState::Completed { status, body };
        }
    }

    /// Remove a key from the cache (e.g. on error, to allow retry).
    pub(crate) fn remove(&self, key: &str) {
        let mut inner = self.inner.lock().expect("idempotency cache lock poisoned");
        inner.entries.remove(key);
        inner.order.retain(|k| k != key);
    }

    /// Number of entries currently in the cache (for testing).
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().expect("lock").entries.len()
    }
}

impl CacheInner {
    fn evict_expired(&mut self) {
        let now = Instant::now();
        while let Some(front_key) = self.order.front() {
            if let Some(entry) = self.entries.get(front_key) {
                if now.duration_since(entry.created_at) > self.ttl {
                    let key = self.order.pop_front().expect("just peeked");
                    self.entries.remove(&key);
                } else {
                    break;
                }
            } else {
                // Stale order entry — remove it
                self.order.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miss_then_hit_after_complete() {
        let cache = IdempotencyCache::new();
        let key = "test-key-001";

        // First lookup: miss, inserted as InFlight
        assert!(matches!(cache.check_or_insert(key), LookupResult::Miss));

        // Second lookup while in-flight: conflict
        assert!(matches!(cache.check_or_insert(key), LookupResult::Conflict));

        // Complete the entry
        cache.complete(key, StatusCode::OK, r#"{"ok":true}"#.to_owned());

        // Third lookup: hit with cached response
        match cache.check_or_insert(key) {
            LookupResult::Hit { status, body } => {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(body, r#"{"ok":true}"#);
            }
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn remove_allows_retry() {
        let cache = IdempotencyCache::new();
        let key = "retry-key";

        assert!(matches!(cache.check_or_insert(key), LookupResult::Miss));
        cache.remove(key);
        assert!(matches!(cache.check_or_insert(key), LookupResult::Miss));
    }

    #[test]
    fn capacity_eviction() {
        let cache = IdempotencyCache {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity: 3,
                ttl: DEFAULT_TTL,
            }),
        };

        for i in 0..4 {
            cache.check_or_insert(&format!("key-{i}"));
        }

        // Oldest (key-0) should be evicted
        assert_eq!(cache.len(), 3);
        let inner = cache.inner.lock().unwrap();
        assert!(!inner.entries.contains_key("key-0"));
        assert!(inner.entries.contains_key("key-3"));
    }

    #[test]
    fn ttl_expiry() {
        let cache = IdempotencyCache {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity: DEFAULT_CAPACITY,
                ttl: Duration::from_millis(0), // immediate expiry
            }),
        };

        cache.check_or_insert("expired-key");
        // Next call triggers eviction of the expired entry, then inserts fresh
        assert!(matches!(
            cache.check_or_insert("expired-key"),
            LookupResult::Miss
        ));
    }

    impl std::fmt::Debug for LookupResult {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Miss => write!(f, "Miss"),
                Self::Hit { status, .. } => write!(f, "Hit({status})"),
                Self::Conflict => write!(f, "Conflict"),
            }
        }
    }
}
