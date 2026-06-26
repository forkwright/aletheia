//! Idempotency-key cache for deduplicating `POST /sessions/{id}/messages`.
//!
//! Stores a bounded, TTL-evicting map of `(principal, idempotency_key, context, state)` tuples so
//! retried requests with the same key, same authenticated principal, same resolved session, and
//! same request body return the cached response instead of creating duplicate turns. Keys are
//! namespaced per principal, then bound to a resolved session and body fingerprint so one session
//! cannot observe or replay another session's cached response.

// WHY: lock not held across await points
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use indexmap::IndexMap;

/// Default TTL for cached idempotency entries.
/// Fallback default; runtime reads `ApiLimitsConfig::idempotency_ttl_secs`.
pub const DEFAULT_TTL: Duration = Duration::from_mins(5);

/// Maximum number of entries in the cache.
/// Fallback default; runtime reads `ApiLimitsConfig::idempotency_capacity`.
pub const DEFAULT_CAPACITY: usize = 10_000;

/// Default maximum allowed length for an idempotency key string (64).
const DEFAULT_MAX_KEY_LENGTH: usize = 64;

/// Thread-safe idempotency cache with bounded insertion-order eviction and TTL expiry.
pub struct IdempotencyCache {
    inner: Mutex<CacheInner>,
    /// Maximum key length for idempotency keys.
    pub(crate) max_key_length: usize,
}

struct CacheInner {
    /// Insertion-ordered map for bounded TTL eviction (front = oldest).
    ///
    // WHY: IndexMap gives O(1) removal by key via swap_remove, avoiding
    // the VecDeque::retain scan that scaled with cache capacity on every
    // error-path removal. The order of remaining entries is perturbed on
    // swap_remove; this is acceptable for an insertion-order cache.
    entries: IndexMap<String, CacheEntry>,
    capacity: usize,
    ttl: Duration,
}

struct CacheEntry {
    context: RequestContext,
    /// Canonical turn id bound to this idempotency key.
    ///
    /// Set when the turn is created so in-flight and completed lookups can
    /// route the caller to the durable turn-event store.
    turn_id: String,
    state: EntryState,
    created_at: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestContext {
    session_id: String,
    body_fingerprint: String,
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
        session_id: String,
        turn_id: String,
    },
    /// A request with this key is still in progress.
    Conflict {
        session_id: String,
        turn_id: String,
    },
    /// This key was already bound to a different session or request body.
    Rejected { reason: RejectionReason },
}

/// Why an idempotency key was rejected.
#[derive(Debug)]
pub(crate) enum RejectionReason {
    /// The same key was already used in a different resolved session.
    CrossSession,
    /// The same key was already used with a different request body.
    BodyMismatch,
}

impl RejectionReason {
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::CrossSession => "idempotency key is already bound to a different session",
            Self::BodyMismatch => "idempotency key is already bound to a different request body",
        }
    }
}

/// Build a composite cache key that namespaces `key` under `principal`.
///
/// Uses a NUL byte separator: NUL cannot appear in valid ASCII HTTP header values,
/// so `"alice\x00k"` can never collide with a principal of `"alice\x00k"` and a
/// separate key component.
fn composite_key(principal: &str, key: &str) -> String {
    format!("{principal}\x00{key}")
}

fn request_context(session_id: &str, body_fingerprint: &str) -> RequestContext {
    RequestContext {
        session_id: session_id.to_owned(),
        body_fingerprint: body_fingerprint.to_owned(),
    }
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
                entries: IndexMap::new(),
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

    /// Look up a (principal, key, session, body) tuple.
    ///
    /// On miss, atomically inserts it as `InFlight`. The map entry is keyed by
    /// `"{principal}\x00{key}"` so two different principals presenting identical
    /// `Idempotency-Key` header values never collide. The stored context binds that
    /// key to the resolved session and request body fingerprint; reusing it for any
    /// other session or body is rejected instead of being treated as a replay.
    ///
    /// # Complexity
    ///
    /// O(1) for the `IndexMap` lookup. O(k) for eviction of expired entries
    /// where k is the number of expired entries at the front of the order.
    pub(crate) fn check_or_insert(
        &self,
        principal: &str,
        key: &str,
        session_id: &str,
        body_fingerprint: &str,
    ) -> LookupResult {
        let composite = composite_key(principal, key);
        let context = request_context(session_id, body_fingerprint);
        let mut inner = self.lock_inner();
        inner.evict_expired();

        if let Some(entry) = inner.entries.get(&composite) {
            if entry.context.session_id != context.session_id {
                return LookupResult::Rejected {
                    reason: RejectionReason::CrossSession,
                };
            }
            if entry.context.body_fingerprint != context.body_fingerprint {
                return LookupResult::Rejected {
                    reason: RejectionReason::BodyMismatch,
                };
            }
            let session_id = entry.context.session_id.clone();
            let turn_id = entry.turn_id.clone();
            return match &entry.state {
                EntryState::InFlight => LookupResult::Conflict { session_id, turn_id },
                EntryState::Completed { status, body } => LookupResult::Hit {
                    status: *status,
                    body: body.clone(),
                    session_id,
                    turn_id,
                },
            };
        }

        while inner.entries.len() >= inner.capacity {
            // WHY: swap_remove_index keeps capacity eviction O(1); order of
            // the remaining entries is slightly perturbed, which is acceptable
            // for this bounded insertion-order cache.
            inner.entries.swap_remove_index(0);
        }

        inner.entries.insert(
            composite,
            CacheEntry {
                context,
                // WHY: empty placeholder until the turn is created and
                // `bind_turn_id` is called. A conflict/hit before binding
                // returns an empty turn id, which the caller treats as
                // "not yet associated".
                turn_id: String::new(),
                state: EntryState::InFlight,
                created_at: Instant::now(),
            },
        );

        LookupResult::Miss
    }

    /// Bind a canonical turn id to an in-flight idempotency entry.
    ///
    /// WHY: the turn id is created after `check_or_insert` returns a miss;
    /// this call stores it so subsequent conflicts/hits can route the caller
    /// to the turn-event store.
    pub(crate) fn bind_turn_id(
        &self,
        principal: &str,
        key: &str,
        session_id: &str,
        body_fingerprint: &str,
        turn_id: &str,
    ) {
        let composite = composite_key(principal, key);
        let context = request_context(session_id, body_fingerprint);
        let mut inner = self.lock_inner();
        if let Some(entry) = inner.entries.get_mut(&composite)
            && entry.context == context
            && matches!(entry.state, EntryState::InFlight)
        {
            entry.turn_id = turn_id.to_owned();
        }
    }

    /// Mark a (principal, key, session, body) tuple as completed with the given response.
    pub(crate) fn complete(
        &self,
        principal: &str,
        key: &str,
        session_id: &str,
        body_fingerprint: &str,
        turn_id: &str,
        status: StatusCode,
        body: String,
    ) {
        let composite = composite_key(principal, key);
        let context = request_context(session_id, body_fingerprint);
        let mut inner = self.lock_inner();
        if let Some(entry) = inner.entries.get_mut(&composite)
            && entry.context == context
        {
            entry.turn_id = turn_id.to_owned();
            entry.state = EntryState::Completed { status, body };
        }
    }

    /// Remove a (principal, key, session, body) tuple from the cache.
    ///
    /// Used on errors to allow an exact retry without clearing another session's binding.
    pub(crate) fn remove(
        &self,
        principal: &str,
        key: &str,
        session_id: &str,
        body_fingerprint: &str,
    ) {
        let composite = composite_key(principal, key);
        let context = request_context(session_id, body_fingerprint);
        let mut inner = self.lock_inner();
        if inner
            .entries
            .get(&composite)
            .is_some_and(|entry| entry.context == context)
        {
            // WHY: O(1) key-based removal; order of remaining entries is
            // slightly perturbed on swap_remove, which is acceptable for
            // this bounded insertion-order cache.
            inner.entries.swap_remove(&composite);
        }
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
    /// front of the insertion order.
    fn evict_expired(&mut self) {
        let now = Instant::now();
        while let Some((_, entry)) = self.entries.get_index(0) {
            if now.duration_since(entry.created_at) > self.ttl {
                self.entries.swap_remove_index(0);
            } else {
                break;
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
        let session_id = "session-a";
        let body_fingerprint = "sha256:body-a";

        assert!(matches!(
            cache.check_or_insert(principal, key, session_id, body_fingerprint),
            LookupResult::Miss
        ));

        assert!(matches!(
            cache.check_or_insert(principal, key, session_id, body_fingerprint),
            LookupResult::Conflict { .. }
        ));

        cache.complete(
            principal,
            key,
            session_id,
            body_fingerprint,
            "turn-001",
            StatusCode::OK,
            r#"{"ok":true}"#.to_owned(),
        );

        match cache.check_or_insert(principal, key, session_id, body_fingerprint) {
            LookupResult::Hit { status, body, turn_id, .. } => {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(body, r#"{"ok":true}"#);
                assert_eq!(turn_id, "turn-001");
            }
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn remove_allows_retry() {
        let cache = IdempotencyCache::new();
        let principal = "alice";
        let key = "retry-key";
        let session_id = "session-a";
        let body_fingerprint = "sha256:body-a";

        assert!(matches!(
            cache.check_or_insert(principal, key, session_id, body_fingerprint),
            LookupResult::Miss
        ));
        cache.remove(principal, key, session_id, body_fingerprint);
        assert!(matches!(
            cache.check_or_insert(principal, key, session_id, body_fingerprint),
            LookupResult::Miss
        ));
    }

    #[test]
    fn capacity_eviction() {
        let cache = IdempotencyCache {
            inner: Mutex::new(CacheInner {
                entries: IndexMap::new(),
                capacity: 3,
                ttl: DEFAULT_TTL,
            }),
            max_key_length: DEFAULT_MAX_KEY_LENGTH,
        };

        let principal = "alice";
        for i in 0..4 {
            cache.check_or_insert(principal, &format!("key-{i}"), "session-a", "sha256:body-a");
        }

        assert_eq!(cache.len(), 3);
        let inner = cache.inner.lock().unwrap();
        assert!(
            !inner
                .entries
                .contains_key(&composite_key(principal, "key-0"))
        );
        assert!(
            inner
                .entries
                .contains_key(&composite_key(principal, "key-3"))
        );
    }

    #[test]
    fn ttl_expiry() {
        let cache = IdempotencyCache {
            inner: Mutex::new(CacheInner {
                entries: IndexMap::new(),
                capacity: DEFAULT_CAPACITY,
                ttl: Duration::from_millis(0),
            }),
            max_key_length: DEFAULT_MAX_KEY_LENGTH,
        };

        cache.check_or_insert("alice", "expired-key", "session-a", "sha256:body-a");
        assert!(matches!(
            cache.check_or_insert("alice", "expired-key", "session-a", "sha256:body-a"),
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
        let session_id = "session-a";
        let body_fingerprint = "sha256:body-a";

        // Alice sends a request and it completes.
        assert!(matches!(
            cache.check_or_insert("alice", key, session_id, body_fingerprint),
            LookupResult::Miss
        ));
        cache.complete(
            "alice",
            key,
            session_id,
            body_fingerprint,
            "turn-alice",
            StatusCode::OK,
            r#"{"user":"alice"}"#.to_owned(),
        );

        // Alice's replay returns her own cached response.
        match cache.check_or_insert("alice", key, session_id, body_fingerprint) {
            LookupResult::Hit { body, turn_id, .. } => {
                assert_eq!(body, r#"{"user":"alice"}"#);
                assert_eq!(turn_id, "turn-alice");
            }
            other => panic!("expected Hit for alice, got {other:?}"),
        }

        // Bob uses the same Idempotency-Key but must see a Miss — not Alice's response.
        assert!(
            matches!(
                cache.check_or_insert("bob", key, session_id, body_fingerprint),
                LookupResult::Miss
            ),
            "bob must not see alice's cache entry"
        );

        // Bob's entry is in-flight until he completes; a second call from Bob is a Conflict.
        assert!(matches!(
            cache.check_or_insert("bob", key, session_id, body_fingerprint),
            LookupResult::Conflict { .. }
        ));

        // Bob completing his entry does not disturb Alice's cached entry.
        cache.complete(
            "bob",
            key,
            session_id,
            body_fingerprint,
            "turn-bob",
            StatusCode::OK,
            r#"{"user":"bob"}"#.to_owned(),
        );
        match cache.check_or_insert("alice", key, session_id, body_fingerprint) {
            LookupResult::Hit { body, .. } => {
                assert_eq!(
                    body, r#"{"user":"alice"}"#,
                    "alice's entry must be unchanged"
                );
            }
            other => panic!("expected Hit for alice after bob's complete, got {other:?}"),
        }
        match cache.check_or_insert("bob", key, session_id, body_fingerprint) {
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
        let session_id = "session-a";
        let body_fingerprint = "sha256:body-a";

        cache.check_or_insert("alice", key, session_id, body_fingerprint);
        cache.check_or_insert("bob", key, session_id, body_fingerprint);

        // Remove only alice's entry.
        cache.remove("alice", key, session_id, body_fingerprint);

        assert!(
            matches!(
                cache.check_or_insert("alice", key, session_id, body_fingerprint),
                LookupResult::Miss
            ),
            "alice's entry should be gone after remove"
        );
        assert!(
            matches!(
                cache.check_or_insert("bob", key, session_id, body_fingerprint),
                LookupResult::Conflict { .. }
            ),
            "bob's in-flight entry must be unaffected"
        );
    }

    #[test]
    fn same_key_different_session_rejected() {
        let cache = IdempotencyCache::new();
        let key = "shared-session-key";

        assert!(matches!(
            cache.check_or_insert("alice", key, "session-a", "sha256:body-a"),
            LookupResult::Miss
        ));

        match cache.check_or_insert("alice", key, "session-b", "sha256:body-a") {
            LookupResult::Rejected {
                reason: RejectionReason::CrossSession,
            } => {}
            other => panic!("expected cross-session rejection, got {other:?}"),
        }
    }

    #[test]
    fn same_key_different_body_rejected() {
        let cache = IdempotencyCache::new();
        let key = "shared-body-key";

        assert!(matches!(
            cache.check_or_insert("alice", key, "session-a", "sha256:body-a"),
            LookupResult::Miss
        ));

        match cache.check_or_insert("alice", key, "session-a", "sha256:body-b") {
            LookupResult::Rejected {
                reason: RejectionReason::BodyMismatch,
            } => {}
            other => panic!("expected body mismatch rejection, got {other:?}"),
        }
    }

    impl std::fmt::Debug for LookupResult {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Miss => write!(f, "Miss"),
                Self::Hit { status, .. } => write!(f, "Hit({status})"),
                Self::Conflict { .. } => write!(f, "Conflict"),
                Self::Rejected { reason } => write!(f, "Rejected({reason:?})"),
            }
        }
    }
}
