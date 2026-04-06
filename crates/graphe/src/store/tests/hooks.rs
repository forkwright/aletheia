//! Tests for connection lifecycle hooks on `SessionStore`.
#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::store::{ConnectionHook, SessionStore};

/// A hook that counts invocations of `before_acquire` and `after_release`.
struct CountingHook {
    before_count: Arc<AtomicUsize>,
    after_count: Arc<AtomicUsize>,
}

impl ConnectionHook for CountingHook {
    fn before_acquire(&self) {
        self.before_count.fetch_add(1, Ordering::SeqCst);
    }

    fn after_release(&self) {
        self.after_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn before_acquire_called_on_open() {
    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));

    let hook = Box::new(CountingHook {
        before_count: Arc::clone(&before),
        after_count: Arc::clone(&after),
    });

    let _store = SessionStore::open_in_memory_with_hook(hook)
        .expect("open_in_memory_with_hook should succeed");

    assert_eq!(
        before.load(Ordering::SeqCst),
        1,
        "before_acquire should be called exactly once on open"
    );
    assert_eq!(
        after.load(Ordering::SeqCst),
        0,
        "after_release should not be called while the store is alive"
    );
}

#[test]
fn after_release_called_on_drop() {
    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));

    let hook = Box::new(CountingHook {
        before_count: Arc::clone(&before),
        after_count: Arc::clone(&after),
    });

    {
        let _store = SessionStore::open_in_memory_with_hook(hook)
            .expect("open_in_memory_with_hook should succeed");
        // Drop happens at end of this block.
    }

    assert_eq!(
        after.load(Ordering::SeqCst),
        1,
        "after_release should be called exactly once on drop"
    );
}

#[test]
#[expect(
    clippy::items_after_statements,
    reason = "test-local helper struct scoped near its usage"
)]
fn hooks_called_in_correct_order() {
    use std::sync::Mutex;

    let events: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    struct OrderHook {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ConnectionHook for OrderHook {
        fn before_acquire(&self) {
            self.events
                .lock()
                .expect("events lock should not be poisoned")
                .push("before_acquire");
        }

        fn after_release(&self) {
            self.events
                .lock()
                .expect("events lock should not be poisoned")
                .push("after_release");
        }
    }

    let hook = Box::new(OrderHook {
        events: Arc::clone(&events),
    });

    {
        let _store = SessionStore::open_in_memory_with_hook(hook)
            .expect("open_in_memory_with_hook should succeed");
    }

    let recorded = events
        .lock()
        .expect("events lock should not be poisoned")
        .clone();
    assert_eq!(
        recorded,
        vec!["before_acquire", "after_release"],
        "before_acquire must fire before after_release"
    );
}

#[test]
fn store_without_hook_works_normally() {
    let store = SessionStore::open_in_memory().expect("open_in_memory without hook should succeed");
    store
        .ping()
        .expect("ping should succeed on hook-free store");
}
