//! Trait adapters bridging organon tool traits to mneme SessionStore.

use std::sync::Arc;

use tokio::sync::Mutex;

use aletheia_mneme::store::SessionStore;
use aletheia_organon::types::{BlackboardEntry, BlackboardStore, NoteEntry, NoteStore};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Acquire the store lock from a synchronous trait method inside an async context.
///
/// Uses `block_in_place` so the Tokio runtime can continue driving other tasks
/// while the calling thread blocks waiting for the lock. Callers must be running
/// on the **multi-thread** Tokio runtime; single-thread runtimes will panic.
///
/// The guard is held only for the duration of `f` and dropped before returning.
fn with_store<F, T>(store: &Arc<Mutex<SessionStore>>, f: F) -> T
where
    F: FnOnce(&SessionStore) -> T,
{
    tokio::task::block_in_place(|| {
        let guard = tokio::runtime::Handle::current().block_on(store.lock());
        f(&guard)
    })
}

/// Adapts `SessionStore` note methods to the `NoteStore` trait.
pub struct SessionNoteAdapter(pub Arc<Mutex<SessionStore>>);

impl NoteStore for SessionNoteAdapter {
    fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64, BoxError> {
        with_store(&self.0, |store| {
            store
                .add_note(session_id, nous_id, category, content)
                .map_err(|e| Box::new(e) as BoxError)
        })
    }

    fn get_notes(&self, session_id: &str) -> Result<Vec<NoteEntry>, BoxError> {
        with_store(&self.0, |store| {
            let notes = store
                .get_notes(session_id)
                .map_err(|e| Box::new(e) as BoxError)?;
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

    fn delete_note(&self, note_id: i64) -> Result<bool, BoxError> {
        with_store(&self.0, |store| {
            store
                .delete_note(note_id)
                .map_err(|e| Box::new(e) as BoxError)
        })
    }
}

/// Adapts `SessionStore` blackboard methods to the `BlackboardStore` trait.
pub struct SessionBlackboardAdapter(pub Arc<Mutex<SessionStore>>);

impl BlackboardStore for SessionBlackboardAdapter {
    fn write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> Result<(), BoxError> {
        with_store(&self.0, |store| {
            store
                .blackboard_write(key, value, author, ttl_seconds)
                .map_err(|e| Box::new(e) as BoxError)
        })
    }

    fn read(&self, key: &str) -> Result<Option<BlackboardEntry>, BoxError> {
        with_store(&self.0, |store| {
            let row = store
                .blackboard_read(key)
                .map_err(|e| Box::new(e) as BoxError)?;
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

    fn list(&self) -> Result<Vec<BlackboardEntry>, BoxError> {
        with_store(&self.0, |store| {
            let rows = store
                .blackboard_list()
                .map_err(|e| Box::new(e) as BoxError)?;
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

    fn delete(&self, key: &str, author: &str) -> Result<bool, BoxError> {
        with_store(&self.0, |store| {
            store
                .blackboard_delete(key, author)
                .map_err(|e| Box::new(e) as BoxError)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aletheia_organon::types::NoteStore;

    fn test_store() -> Arc<Mutex<SessionStore>> {
        Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("in-memory store"),
        ))
    }

    /// Verify that `SessionNoteAdapter` can be locked and used from an async
    /// context running on a multi-thread Tokio runtime.
    ///
    /// The adapter uses `block_in_place` internally, which requires the
    /// multi-thread runtime — this test confirms that path works end-to-end.
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

        // add_note calls block_in_place internally
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
    /// without deadlocking — lock is released between calls.
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
