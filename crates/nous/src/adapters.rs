//! Trait adapters bridging organon tool traits to mneme SessionStore.

use std::sync::{Arc, Mutex};

use aletheia_mneme::store::SessionStore;
use aletheia_organon::types::{BlackboardEntry, BlackboardStore, NoteEntry, NoteStore};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn lock_store(
    store: &Mutex<SessionStore>,
) -> Result<std::sync::MutexGuard<'_, SessionStore>, BoxError> {
    store
        .lock()
        .map_err(|e| -> BoxError { e.to_string().into() })
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
        let store = lock_store(&self.0)?;
        store
            .add_note(session_id, nous_id, category, content)
            .map_err(|e| Box::new(e) as BoxError)
    }

    fn get_notes(&self, session_id: &str) -> Result<Vec<NoteEntry>, BoxError> {
        let store = lock_store(&self.0)?;
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
    }

    fn delete_note(&self, note_id: i64) -> Result<bool, BoxError> {
        let store = lock_store(&self.0)?;
        store
            .delete_note(note_id)
            .map_err(|e| Box::new(e) as BoxError)
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
        let store = lock_store(&self.0)?;
        store
            .blackboard_write(key, value, author, ttl_seconds)
            .map_err(|e| Box::new(e) as BoxError)
    }

    fn read(&self, key: &str) -> Result<Option<BlackboardEntry>, BoxError> {
        let store = lock_store(&self.0)?;
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
    }

    fn list(&self) -> Result<Vec<BlackboardEntry>, BoxError> {
        let store = lock_store(&self.0)?;
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
    }

    fn delete(&self, key: &str, author: &str) -> Result<bool, BoxError> {
        let store = lock_store(&self.0)?;
        store
            .blackboard_delete(key, author)
            .map_err(|e| Box::new(e) as BoxError)
    }
}
