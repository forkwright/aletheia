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

use mneme::store::SessionStore;
use mneme::types::BlackboardRow;
use organon::error::{BackendSnafu, StoreError};
use organon::types::{BlackboardEntry, BlackboardStore, BlackboardViewer, NoteEntry, NoteStore};

/// Acquire the store lock from a synchronous trait method inside an async context.
///
/// See the module-level doc for the full rationale.  The guard is held only
/// for the duration of `f` and dropped before returning.
// kanon:ignore RUST/no-arc-mutex-anti-pattern — std::sync::Mutex guarding fjall SessionStore; lock acquired via block_in_place
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
    BackendSnafu {
        message: e.to_string(),
    }
    .build()
}

/// Adapts `SessionStore` note methods to the `NoteStore` trait.
///
/// The inner lock guards fjall session-store access; acquired via `block_in_place`
/// to avoid holding it across async boundaries.
// kanon:ignore RUST/no-arc-mutex-anti-pattern — same: std::sync::Mutex in block_in_place sync bridge
pub struct SessionNoteAdapter(pub Arc<Mutex<SessionStore>>); // kanon:ignore RUST/pub-visibility

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
/// The inner lock guards fjall session-store access; acquired via `block_in_place`
/// to avoid holding it across async boundaries.
// kanon:ignore RUST/no-arc-mutex-anti-pattern — same: std::sync::Mutex in block_in_place sync bridge
pub struct SessionBlackboardAdapter(pub Arc<Mutex<SessionStore>>); // kanon:ignore RUST/pub-visibility

/// Map a stored row to the trait-level entry type, carrying visibility and
/// session scope through so [`BlackboardViewer::can_see`] can filter it.
fn map_entry(row: BlackboardRow) -> BlackboardEntry {
    BlackboardEntry {
        key: row.key,
        value: row.value,
        author_nous_id: row.author_nous_id,
        ttl_seconds: row.ttl_seconds,
        created_at: row.created_at,
        expires_at: row.expires_at,
        session_id: row.session_id,
        visibility: row.visibility,
    }
}

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

    fn read(
        &self,
        key: &str,
        viewer: &BlackboardViewer,
    ) -> Result<Option<BlackboardEntry>, StoreError> {
        with_store(&self.0, |store| {
            let row = store.blackboard_read(key).map_err(store_err)?;
            Ok(row.map(map_entry).filter(|entry| viewer.can_see(entry)))
        })
    }

    fn list(&self, viewer: &BlackboardViewer) -> Result<Vec<BlackboardEntry>, StoreError> {
        with_store(&self.0, |store| {
            let rows = store.blackboard_list().map_err(store_err)?;
            Ok(rows
                .into_iter()
                .map(map_entry)
                .filter(|entry| viewer.can_see(entry))
                .collect())
        })
    }

    fn delete(&self, key: &str, author: &str) -> Result<bool, StoreError> {
        with_store(&self.0, |store| {
            store.blackboard_delete(key, author).map_err(store_err)
        })
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use mneme::store::{ImportSessionBundle, ImportSessionWorkingState};
    use mneme::types::{
        BlackboardVisibility, Session, SessionMetrics, SessionOrigin, SessionStatus, SessionType,
    };
    use organon::types::NoteStore;

    use super::*;

    fn make_store() -> Arc<Mutex<SessionStore>> {
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
        let store = make_store();

        {
            let s = store.lock().await;
            s.create_session("sess-1", "alice", "test-key", None, None)
                .expect("create session");
        }

        let adapter = SessionNoteAdapter(Arc::clone(&store));

        let id = adapter
            .add_note("sess-1", "alice", "task", "buy oat milk")
            .expect("add_note");
        assert!(id > 0, "note id should be positive");

        let notes = adapter.get_notes("sess-1").expect("get_notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "buy oat milk");

        let deleted = adapter.delete_note(id).expect("delete_note");
        assert!(deleted);
        let notes_after = adapter.get_notes("sess-1").expect("get_notes after delete");
        assert!(notes_after.is_empty());
    }

    /// Verify that two concurrent tasks can each acquire the adapter lock
    /// without deadlocking: lock is released between calls.
    #[tokio::test(flavor = "multi_thread")]
    async fn note_adapter_lock_released_between_calls() {
        let store = make_store();
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

    /// Seed one row at each visibility level, including a `ws:`-style
    /// internal working-state key, via the raw store's scoped writer — the
    /// same entry point `aletheia::commands::agent_io` uses to persist
    /// working state (aletheia#5032).
    async fn seed_visibility_matrix(store: &Arc<Mutex<SessionStore>>) {
        let s = store.lock().await;
        s.blackboard_write("shared-goal", "ship M0b", "alice", 3600)
            .expect("write shared");
        s.blackboard_write_scoped(
            "alice-private-note",
            "quiet thought",
            "alice",
            3600,
            BlackboardVisibility::NousPrivate,
            None,
        )
        .expect("write nous-private");
        s.blackboard_write_scoped(
            "ws:alice:ses-1",
            "task-stack",
            "alice",
            3600,
            BlackboardVisibility::SessionPrivate,
            Some("ses-1"),
        )
        .expect("write session-private");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blackboard_list_scopes_by_viewer() {
        let store = make_store();
        seed_visibility_matrix(&store).await;
        let adapter = SessionBlackboardAdapter(Arc::clone(&store));

        let keys = |viewer: &BlackboardViewer| -> Vec<String> {
            adapter
                .list(viewer)
                .expect("list")
                .into_iter()
                .map(|e| e.key)
                .collect()
        };

        // The author, in their own session, sees everything they wrote.
        let mut alice_in_session = keys(&BlackboardViewer::Session {
            nous_id: "alice".to_owned(),
            session_id: "ses-1".to_owned(),
        });
        alice_in_session.sort();
        assert_eq!(
            alice_in_session,
            vec!["alice-private-note", "shared-goal", "ws:alice:ses-1"],
            "author viewer scoped to the right session must see Shared + own NousPrivate + own SessionPrivate"
        );

        // The author with no session context (e.g. a session-less MCP
        // caller) sees Shared + their own NousPrivate, but never the
        // SessionPrivate row — this is the exact ws: leak path (aletheia#5032).
        let mut alice_no_session = keys(&BlackboardViewer::Nous {
            nous_id: "alice".to_owned(),
        });
        alice_no_session.sort();
        assert_eq!(
            alice_no_session,
            vec!["alice-private-note", "shared-goal"],
            "nous-only viewer must never see a SessionPrivate row, even when authored by itself"
        );

        // A different agent, even inside alice's exact session id, sees
        // only the Shared row — ownership beats a session-id coincidence.
        let mut bob_in_alices_session = keys(&BlackboardViewer::Session {
            nous_id: "bob".to_owned(),
            session_id: "ses-1".to_owned(),
        });
        bob_in_alices_session.sort();
        assert_eq!(
            bob_in_alices_session,
            vec!["shared-goal"],
            "another agent must never see alice's private rows, even from inside session ses-1"
        );

        // A different agent with no session context sees only Shared too.
        let bob_no_session = keys(&BlackboardViewer::Nous {
            nous_id: "bob".to_owned(),
        });
        assert_eq!(
            bob_no_session,
            vec!["shared-goal"],
            "another agent with no session context must see only Shared rows"
        );
    }

    /// Pin aletheia#5032/#5033 against the real import path (not a
    /// hand-seeded row): `SessionStore::import_session_bundle`'s
    /// working-state write must classify `ws:` rows `SessionPrivate`,
    /// scoped to the imported session. A regression that leaves the bundle
    /// writing the default `Shared` visibility reopens the exact leak
    /// #6731 closed — the general Nous-viewer list path used by
    /// session-less MCP callers would surface another session's scratch.
    #[tokio::test(flavor = "multi_thread")]
    async fn import_session_bundle_working_state_stays_session_private() {
        let store = make_store();
        let session = Session {
            id: "ses-bundle-carol".to_owned(),
            nous_id: "carol".to_owned(),
            session_key: "key-ses-bundle-carol".to_owned(),
            status: SessionStatus::Active,
            model: Some("mock-model".to_owned()),
            session_type: SessionType::Primary,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-01T00:00:00Z".to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 0,
                message_count: 0,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
            artefact_meta: None,
        };
        let ws_key = format!("ws:carol:{}", session.id);
        let working_state = Some(ImportSessionWorkingState {
            key: &ws_key,
            value: "{\"step\":1}",
            author: "carol",
            ttl_secs: 3600,
        });

        {
            let s = store.lock().await;
            s.import_session_bundle(
                &ImportSessionBundle {
                    session: &session,
                    messages: &[],
                    usage_records: &[],
                    notes: &[],
                    working_state,
                },
                false,
            )
            .expect("bundle import succeeds");
        }

        let adapter = SessionBlackboardAdapter(Arc::clone(&store));

        // The owner, inside the imported session, sees its own working state.
        let owner_view = BlackboardViewer::Session {
            nous_id: "carol".to_owned(),
            session_id: session.id.clone(),
        };
        let owner_keys: Vec<String> = adapter
            .list(&owner_view)
            .expect("list")
            .into_iter()
            .map(|e| e.key)
            .collect();
        assert!(
            owner_keys.contains(&ws_key),
            "owner viewer scoped to the imported session must see its own working state"
        );

        // The general, unscoped Nous viewer — the leak path #6731 closed —
        // must never see it, even though carol authored it.
        let nous_view = BlackboardViewer::Nous {
            nous_id: "carol".to_owned(),
        };
        let nous_keys: Vec<String> = adapter
            .list(&nous_view)
            .expect("list")
            .into_iter()
            .map(|e| e.key)
            .collect();
        assert!(
            !nous_keys.contains(&ws_key),
            "imported working state must not leak through the general Nous-viewer list path"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn blackboard_read_of_private_row_is_indistinguishable_from_absent() {
        let store = make_store();
        seed_visibility_matrix(&store).await;
        let adapter = SessionBlackboardAdapter(Arc::clone(&store));

        // The owning viewer, in the right session, can read the ws: row.
        let owner_view = BlackboardViewer::Session {
            nous_id: "alice".to_owned(),
            session_id: "ses-1".to_owned(),
        };
        let entry = adapter
            .read("ws:alice:ses-1", &owner_view)
            .expect("read")
            .expect("owner in the right session must see the row");
        assert_eq!(entry.value, "task-stack");

        // A viewer that cannot see the row gets `Ok(None)` — the same
        // result as a genuinely missing key, so `read` cannot be used to
        // probe whether a private key exists.
        let outsider_view = BlackboardViewer::Nous {
            nous_id: "bob".to_owned(),
        };
        assert!(
            adapter
                .read("ws:alice:ses-1", &outsider_view)
                .expect("read")
                .is_none(),
            "an unauthorized viewer must not be able to read a SessionPrivate row"
        );
        assert!(
            adapter
                .read("no-such-key", &outsider_view)
                .expect("read")
                .is_none(),
            "a missing key must read identically to a row the viewer cannot see"
        );
    }
}
