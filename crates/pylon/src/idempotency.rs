//! Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
//!
//! Stores a bounded, TTL-evicting map of `(principal, idempotency_key, state)` tuples so that
//! retried requests with the same key and same authenticated principal return the cached response
//! instead of creating duplicate turns. Keys are namespaced per principal so one principal cannot
//! observe or replay another principal's cached response.

use std::collections::{HashMap, VecDeque};
// WHY: lock not held across await points
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::StatusCode;

/// Default TTL for cached idempotency entries.
const DEFAULT_TTL: Duration = Duration::from_mins(5);

/// Maximum number of entries in the cache.
const DEFAULT_CAPACITY: usize = 10_000;

/// Default maximum allowed length for an idempotency key string (64).
const DEFAULT_MAX_KEY_LENGTH: usize = 64;

/// Thread-safe idempotency cache with LRU eviction and TTL expiry.
pub struct IdempotencyCache {
    inner: Mutex<CacheInner>,
    /// Maximum key length for idempotency keys.
    pub(crate) max_key_length: usize,
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
    /// Request completed: cached response ready to replay.
    Completed { status: StatusCode, body: String },
}

/// Result of looking up an idempotency key.
pub(crate) enum LookupResult {
    /// Key not seen before: caller should proceed with the request.
    Miss,
    /// A previous request with this key completed: return the cached response.
    Hit {
        // WHY: Reserved for non-SSE endpoints that replay the HTTP status code
        // directly. SSE handlers use only `body` (the serialized event payload).
        #[cfg_attr(
            not(test),
            expect(dead_code, reason = "reserved for non-SSE endpoints")
        )]
        status: StatusCode,
        body: String,
    },
    /// A request with this key is still in progress.
    Conflict,
}

/// Build a composite cache key that namespaces `key` under `principal`.
///
/// Uses a NUL byte separator: NUL cannot appear in valid ASCII HTTP header values,
/// so `"alice\x00k"` can never collide with a principal of `"alice\x00k"` and a
/// separate key component.
fn composite_key(principal: &str, key: &str) -> String {
    format!("{principal}\x00{key}")
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IdempotencyCache {
    /// Create a new idempotency cache with default capacity, TTL, and key length.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(DEFAULT_TTL, DEFAULT_CAPACITY, DEFAULT_MAX_KEY_LENGTH)
    }

    /// Create an idempotency cache from deployment-level config values.
    #[must_use]
    pub fn with_config(ttl: Duration, capacity: usize, max_key_length: usize) -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity,
                ttl,
            }),
            max_key_length,
        }
    }

    /// Acquire the inner lock, recovering from poison.
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, CacheInner> {
        self.inner.lock().unwrap_or_else(|e| {
            tracing::warn!("idempotency cache lock was poisoned, recovering");
            e.into_inner()
        })
    }

    /// Look up a (principal, key) pair. On miss, atomically inserts it as `InFlight`.
    ///
    /// The cache entry is keyed by `"{principal}\x00{key}"` so that two different
    /// principals presenting identical `Idempotency-Key` header values never collide.
    /// The NUL separator cannot appear in valid ASCII header values, ensuring no
    /// ambiguity between a principal that ends with a substring of another key.
    ///
    /// # Complexity
    ///
    /// O(1) for the `HashMap` lookup. O(k) for eviction of expired entries
    /// where k is the number of expired entries at the front of the LRU queue.
    pub(crate) fn check_or_insert(&self, principal: &str, key: &str) -> LookupResult {
        let composite = composite_key(principal, key);
        let mut inner = self.lock_inner();
        inner.evict_expired();

        if let Some(entry) = inner.entries.get(&composite) {
            return match &entry.state {
                EntryState::InFlight => LookupResult::Conflict,
                EntryState::Completed { status, body } => LookupResult::Hit {
                    status: *status,
                    body: body.clone(),
                },
            };
        }

        while inner.entries.len() >= inner.capacity {
            if let Some(oldest_key) = inner.order.pop_front() {
                inner.entries.remove(&oldest_key);
            } else {
                break;
            }
        }

        inner.entries.insert(
            composite.clone(),
            CacheEntry {
                state: EntryState::InFlight,
                created_at: Instant::now(),
            },
        );
        inner.order.push_back(composite);

        LookupResult::Miss
    }

    /// Mark a (principal, key) pair as completed with the given response.
    pub(crate) fn complete(&self, principal: &str, key: &str, status: StatusCode, body: String) {
        let composite = composite_key(principal, key);
        let mut inner = self.lock_inner();
        if let Some(entry) = inner.entries.get_mut(&composite) {
            entry.state = EntryState::Completed { status, body };
        }
    }

    /// Remove a (principal, key) pair from the cache (e.g. on error, to allow retry).
    pub(crate) fn remove(&self, principal: &str, key: &str) {
        let composite = composite_key(principal, key);
        let mut inner = self.lock_inner();
        inner.entries.remove(&composite);
        inner.order.retain(|k| k != &composite);
    }

    /// Number of entries currently in the cache (for testing).
    #[cfg(test)]
    #[expect(clippy::expect_used, reason = "test helper")]
    fn len(&self) -> usize {
        self.inner.lock().expect("lock").entries.len()
    }
}

impl CacheInner {
    /// Evict expired entries from the cache.
    ///
    /// # Complexity
    ///
    /// O(k) where k is the number of consecutive expired entries at the
    /// front of the LRU order queue.
    fn evict_expired(&mut self) {
        let now = Instant::now();
        while let Some(front_key) = self.order.front() {
            if let Some(entry) = self.entries.get(front_key) {
                if now.duration_since(entry.created_at) > self.ttl {
                    #[expect(clippy::expect_used, reason = "just peeked front()")]
                    let key = self.order.pop_front().expect("just peeked");
                    self.entries.remove(&key);
                } else {
                    break;
                }
            } else {
                self.order.pop_front();
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn miss_then_hit_after_complete() {
        let cache = IdempotencyCache::new();
        let principal = "alice";
        let key = "test-key-001";

        assert!(matches!(
            cache.check_or_insert(principal, key),
            LookupResult::Miss
        ));

        assert!(matches!(
            cache.check_or_insert(principal, key),
            LookupResult::Conflict
        ));

        cache.complete(principal, key, StatusCode::OK, r#"{"ok":true}"#.to_owned());

        match cache.check_or_insert(principal, key) {
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
        let principal = "alice";
        let key = "retry-key";

        assert!(matches!(
            cache.check_or_insert(principal, key),
            LookupResult::Miss
        ));
        cache.remove(principal, key);
        assert!(matches!(
            cache.check_or_insert(principal, key),
            LookupResult::Miss
        ));
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
            max_key_length: DEFAULT_MAX_KEY_LENGTH,
        };

        let principal = "alice";
        for i in 0..4 {
            cache.check_or_insert(principal, &format!("key-{i}"));
        }

        assert_eq!(cache.len(), 3);
        let inner = cache.inner.lock().unwrap();
        assert!(!inner.entries.contains_key(&composite_key(principal, "key-0")));
        assert!(inner.entries.contains_key(&composite_key(principal, "key-3")));
    }

    #[test]
    fn ttl_expiry() {
        let cache = IdempotencyCache {
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity: DEFAULT_CAPACITY,
                ttl: Duration::from_millis(0),
            }),
            max_key_length: DEFAULT_MAX_KEY_LENGTH,
        };

        cache.check_or_insert("alice", "expired-key");
        assert!(matches!(
            cache.check_or_insert("alice", "expired-key"),
            LookupResult::Miss
        ));
    }

    /// Two principals using the same `Idempotency-Key` value must not share a
    /// cache entry. Alice's completed response must not be served to Bob, and
    /// Bob's first use of that key must be a Miss (not a Hit or Conflict).
    #[test]
    fn different_principals_same_key_are_isolated() {
        let cache = IdempotencyCache::new();
        let key = "shared-key-value";

        // Alice sends a request and it completes.
        assert!(matches!(
            cache.check_or_insert("alice", key),
            LookupResult::Miss
        ));
        cache.complete("alice", key, StatusCode::OK, r#"{"user":"alice"}"#.to_owned());

        // Alice's replay returns her own cached response.
        match cache.check_or_insert("alice", key) {
            LookupResult::Hit { body, .. } => {
                assert_eq!(body, r#"{"user":"alice"}"#);
            }
            other => panic!("expected Hit for alice, got {other:?}"),
        }

        // Bob uses the same Idempotency-Key but must see a Miss — not Alice's response.
        assert!(
            matches!(cache.check_or_insert("bob", key), LookupResult::Miss),
            "bob must not see alice's cache entry"
        );

        // Bob's entry is in-flight until he completes; a second call from Bob is a Conflict.
        assert!(matches!(
            cache.check_or_insert("bob", key),
            LookupResult::Conflict
        ));

        // Bob completing his entry does not disturb Alice's cached entry.
        cache.complete("bob", key, StatusCode::OK, r#"{"user":"bob"}"#.to_owned());
        match cache.check_or_insert("alice", key) {
            LookupResult::Hit { body, .. } => {
                assert_eq!(body, r#"{"user":"alice"}"#, "alice's entry must be unchanged");
            }
            other => panic!("expected Hit for alice after bob's complete, got {other:?}"),
        }
        match cache.check_or_insert("bob", key) {
            LookupResult::Hit { body, .. } => {
                assert_eq!(body, r#"{"user":"bob"}"#);
            }
            other => panic!("expected Hit for bob, got {other:?}"),
        }
    }

    /// Removing a key for one principal must not affect the same key for another.
    #[test]
    fn remove_is_principal_scoped() {
        let cache = IdempotencyCache::new();
        let key = "shared-removable-key";

        cache.check_or_insert("alice", key);
        cache.check_or_insert("bob", key);

        // Remove only alice's entry.
        cache.remove("alice", key);

        assert!(
            matches!(cache.check_or_insert("alice", key), LookupResult::Miss),
            "alice's entry should be gone after remove"
        );
        assert!(
            matches!(cache.check_or_insert("bob", key), LookupResult::Conflict),
            "bob's in-flight entry must be unaffected"
        );
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
