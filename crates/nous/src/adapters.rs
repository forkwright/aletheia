//! Trait adapters bridging organon tool traits to mneme SessionStore.
//!
//! # Locking strategy
//!
//! The `NoteStore` and `BlackboardStore` traits have synchronous method
//! signatures, but the shared `SessionStore` is protected by a
//! `tokio::sync::Mutex` to support the async callers elsewhere in the server
//! (pylon routes, diaporeia tools, etc.).
//!
//! `with_store` bridges that gap:
//!
//! 1. `block_in_place` removes the current thread from Tokio's async worker
//!    pool, allowing other tasks (including any task that holds the mutex) to
//!    be scheduled on the remaining worker threads.
//! 2. `Handle::block_on` then drives `store.lock().await`: proper async lock
//!    acquisition: to completion on this now-blocking thread.
//!
//! Together this eliminates the `blocking_lock` shortcut (which internally
//! used Tokio's bare `block_on`) in favour of the documented
//! `block_in_place` + `Handle::block_on` pattern, where the lock is
//! acquired through the runtime's async scheduler rather than a side-channel.
//!
//! # Runtime requirement
//!
//! `block_in_place` requires the **multi-thread** Tokio runtime; the
//! current-thread runtime has only one worker thread and will panic.

use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::Mutex;

use aletheia_mneme::store::SessionStore;
use aletheia_organon::error::{StoreError, StoreSnafu};
use aletheia_organon::types::{BlackboardEntry, BlackboardStore, NoteEntry, NoteStore};

/// Acquire the store lock from a synchronous trait method inside an async context.
///
/// See the module-level doc for the full rationale.  The guard is held only
/// for the duration of `f` and dropped before returning.
fn with_store<F, T>(store: &Arc<Mutex<SessionStore>>, f: F) -> T
where
    F: FnOnce(&SessionStore) -> T,
{
    // WHY: block_in_place moves this thread out of Tokio's worker pool so that
    // Handle::block_on can be called without nesting two async executors on the
    // same thread.  Any task currently holding the mutex can be scheduled on
    // the remaining worker threads, preventing the lock-holder-starvation
    // deadlock that arises when blocking_lock() is called directly from an
    // async worker.  Tokio's documentation explicitly states that
    // Handle::block_on is safe to call from inside block_in_place.
    tokio::task::block_in_place(|| {
        let guard = Handle::current().block_on(store.lock());
        f(&guard)
    })
}

fn store_err(e: impl std::fmt::Display) -> StoreError {
    StoreSnafu {
        message: e.to_string(),
    }
    .build()
}

/// Adapts `SessionStore` note methods to the `NoteStore` trait.
///
/// The inner lock guards `SQLite` write access; acquired via `block_in_place`
/// to avoid holding it across async boundaries.
pub struct SessionNoteAdapter(pub Arc<Mutex<SessionStore>>);

impl NoteStore for SessionNoteAdapter {
    fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64, StoreError> {
        with_store(&self.0, |store| {
            store
                .add_note(session_id, nous_id, category, content)
                .map_err(store_err)
        })
    }

    fn get_notes(&self, session_id: &str) -> Result<Vec<NoteEntry>, StoreError> {
        with_store(&self.0, |store| {
            let notes = store.get_notes(session_id).map_err(store_err)?;
            Ok(notes
                .into_iter()
                .map(|n| NoteEntry {
                    id: n.id,
                    category: n.category,
                    content: n.content,
                    created_at: n.created_at,
                })
                .collect())
        })
    }

    fn delete_note(&self, note_id: i64) -> Result<bool, StoreError> {
        with_store(&self.0, |store| {
            store.delete_note(note_id).map_err(store_err)
        })
    }
}

/// Adapts `SessionStore` blackboard methods to the `BlackboardStore` trait.
///
/// The inner lock guards `SQLite` write access; acquired via `block_in_place`
/// to avoid holding it across async boundaries.
pub struct SessionBlackboardAdapter(pub Arc<Mutex<SessionStore>>);

impl BlackboardStore for SessionBlackboardAdapter {
    fn write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> Result<(), StoreError> {
        with_store(&self.0, |store| {
            store
                .blackboard_write(key, value, author, ttl_seconds)
                .map_err(store_err)
        })
    }

    fn read(&self, key: &str) -> Result<Option<BlackboardEntry>, StoreError> {
        with_store(&self.0, |store| {
            let row = store.blackboard_read(key).map_err(store_err)?;
            Ok(row.map(|r| BlackboardEntry {
                key: r.key,
                value: r.value,
                author_nous_id: r.author_nous_id,
                ttl_seconds: r.ttl_seconds,
                created_at: r.created_at,
                expires_at: r.expires_at,
            }))
        })
    }

    fn list(&self) -> Result<Vec<BlackboardEntry>, StoreError> {
        with_store(&self.0, |store| {
            let rows = store.blackboard_list().map_err(store_err)?;
            Ok(rows
                .into_iter()
                .map(|r| BlackboardEntry {
                    key: r.key,
                    value: r.value,
                    author_nous_id: r.author_nous_id,
                    ttl_seconds: r.ttl_seconds,
                    created_at: r.created_at,
                    expires_at: r.expires_at,
                })
                .collect())
        })
    }

    fn delete(&self, key: &str, author: &str) -> Result<bool, StoreError> {
        with_store(&self.0, |store| {
            store.blackboard_delete(key, author).map_err(store_err)
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use aletheia_organon::types::NoteStore;

    use super::*;

    fn test_store() -> Arc<Mutex<SessionStore>> {
        Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory store"),
        ))
    }

    /// Verify that `SessionNoteAdapter` can be locked and used from an async
    /// context running on a multi-thread Tokio runtime.
    ///
    /// The adapter uses `block_in_place` + `Handle::block_on(store.lock().await)`
    /// internally, which requires the multi-thread runtime: this test confirms
    /// that path works end-to-end without deadlocking.
    #[tokio::test(flavor = "multi_thread")]
    async fn note_adapter_lock_works_in_async_context() {
        let store = test_store();

        // Seed a session
        {
            let s = store.lock().await;
            s.create_session("sess-1", "alice", "test-key", None, None)
                .expect("create session");
        }

        let adapter = SessionNoteAdapter(Arc::clone(&store));

        // add_note uses block_in_place + Handle::block_on(store.lock().await)
        let id = adapter
            .add_note("sess-1", "alice", "task", "buy oat milk")
            .expect("add_note");
        assert!(id > 0, "note id should be positive");

        // get_notes should return the note
        let notes = adapter.get_notes("sess-1").expect("get_notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "buy oat milk");

        // delete_note should remove it
        let deleted = adapter.delete_note(id).expect("delete_note");
        assert!(deleted);
        let notes_after = adapter.get_notes("sess-1").expect("get_notes after delete");
        assert!(notes_after.is_empty());
    }

    /// Verify that two concurrent tasks can each acquire the adapter lock
    /// without deadlocking: lock is released between calls.
    #[tokio::test(flavor = "multi_thread")]
    async fn note_adapter_lock_released_between_calls() {
        let store = test_store();
        {
            let s = store.lock().await;
            s.create_session("sess-a", "bob", "key-a", None, None)
                .expect("create session");
        }

        let adapter = Arc::new(SessionNoteAdapter(Arc::clone(&store)));
        let adapter2 = Arc::clone(&adapter);

        let h1 = tokio::task::spawn_blocking(move || {
            adapter
                .add_note("sess-a", "bob", "task", "first")
                .expect("h1")
        });
        let h2 = tokio::task::spawn_blocking(move || {
            adapter2
                .add_note("sess-a", "bob", "task", "second")
                .expect("h2")
        });

        let (id1, id2) = tokio::try_join!(h1, h2).expect("both tasks succeed");
        assert_ne!(id1, id2, "two notes should have distinct ids");
    }
}
