//! Fjall-backed session store.
//!
//! Pure-Rust LSM-tree storage via `fjall`. Zero C dependencies.
//!
//! # Key schema
//!
//! All keys are UTF-8 strings. Values are JSON-encoded domain structs.
//!
//! | Partition       | Key pattern                                            | Value                    |
//! |-----------------|--------------------------------------------------------|--------------------------|
//! | `sessions`      | `{session_id}`                                         | JSON `Session`           |
//! | `sessions`      | `idx:nous:{nous_id}:upd:{updated_at}:{session_id}`     | `""` (index for list)    |
//! | `sessions`      | `idx:key:{nous_id}:{session_key}`                      | `{session_id}`           |
//! | `messages`      | `{session_id}:{seq_padded_20}`                         | JSON `Message`           |
//! | `messages`      | `next_seq:{session_id}`                                | big-endian `u64`         |
//! | `messages`      | `distilled:{session_id}:{seq_padded_20}`               | `"1"` flag               |
//! | `usage`         | `{session_id}:{turn_seq_padded_20}`                    | JSON `UsageRecord`       |
//! | `distillations` | `{session_id}:{auto_id_padded_20}`                     | JSON distillation record |
//! | `notes`         | `{session_id}:{auto_id_padded_20}`                     | JSON `AgentNote`         |
//! | `notes`         | `gid:{global_note_id_padded_20}`                       | `{session_id}:{auto_id}` |
//! | `blackboard`    | `{key}`                                                | JSON `BlackboardRow`     |
//! | `counters`      | `{counter_name}`                                       | big-endian `u64`         |
//!
//! Sequence numbers are zero-padded to 20 digits so lexicographic ordering
//! matches numeric ordering — enabling range scans for `get_history`.

#![expect(
    clippy::cast_possible_wrap,
    clippy::as_conversions,
    reason = "usize↔i64 for seq counters: values never exceed i64::MAX in practice"
)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use jiff::Zoned;
use serde::{Deserialize, Serialize};
use snafu::{IntoError as _, ResultExt};
use tracing::{debug, info, instrument, warn};

use crate::error::{self, Result};
use crate::metrics;
use crate::types::{
    AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus,
    SessionType, UsageRecord,
};

/// Width for zero-padded sequence numbers.
///
/// 20 digits covers `u64::MAX` (`18_446_744_073_709_551_615`) and ensures
/// lexicographic sort equals numeric sort.
const SEQ_WIDTH: usize = 20;

/// Format a u64 as a zero-padded key component.
fn pad_u64(v: u64) -> String {
    format!("{v:0>SEQ_WIDTH$}")
}

/// Decode a big-endian u64 from 8 bytes.
fn decode_u64(bytes: &[u8]) -> u64 {
    let arr: [u8; 8] = bytes.get(..8).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8]);
    u64::from_be_bytes(arr)
}

/// Encode a u64 as big-endian bytes.
fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

/// ISO 8601 timestamp string for "now".
fn now_iso() -> String {
    Zoned::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

// ── Distillation record (fjall-internal, not a public type) ────────────────

#[derive(Serialize, Deserialize)]
struct DistillationRecord {
    session_id: String,
    messages_before: i64,
    messages_after: i64,
    tokens_before: i64,
    tokens_after: i64,
    model: Option<String>,
    created_at: String,
}

// ── SessionStore ───────────────────────────────────────────────────────────

/// Fjall-backed session store.
///
/// Open with [`SessionStore::open`] for persistent storage or
/// [`SessionStore::open_in_memory`] for ephemeral storage (test-only; uses a
/// `TempDir` that is cleaned up on drop).
pub struct SessionStore {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex.
    ///
    /// WHY: fjall's `SingleWriterTxDatabase` serialises writers internally,
    /// but the graphe API takes `&self` (shared ref) for all write methods —
    /// matching the `SQLite` backend where `Connection` uses interior mutability.
    /// We use a `Mutex<()>` to ensure only one logical "graphe write" runs at
    /// a time, mirroring the serial-write contract of `SingleWriterTxDatabase`.
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    ///
    /// WHY: The leading `_` makes Rust suppress `dead_code` warnings for a field
    /// that is intentionally unused for its value but needed for its `Drop` side
    /// effect (deleting the temp directory when `SessionStore` is dropped).
    _temp_dir: Option<tempfile::TempDir>,
}

impl SessionStore {
    /// Open (or create) a persistent session store at the given path.
    ///
    /// # Errors
    /// Returns an error if the fjall keyspace cannot be opened.
    #[instrument(skip(path))]
    pub fn open(path: &Path) -> Result<Self> {
        info!(path = %path.display(), "Opening fjall session store");
        std::fs::create_dir_all(path).map_err(|e| {
            error::IoSnafu {
                path: path.to_path_buf(),
            }
            .into_error(e)
        })?;

        let db = SingleWriterTxDatabase::builder(path)
            .open()
            .map_err(|e| error::StorageSnafu { message: format!("fjall open: {e}") }.build())?;

        Self::init(db, None)
    }

    /// Open an ephemeral session store backed by a `TempDir` (for testing).
    ///
    /// The directory and all data are deleted when the returned store is
    /// dropped.
    ///
    /// # Errors
    /// Returns an error if the temporary directory or fjall keyspace cannot be
    /// created.
    #[instrument]
    pub fn open_in_memory() -> Result<Self> {
        let dir = tempfile::TempDir::new().map_err(|e| {
            error::IoSnafu {
                path: std::path::PathBuf::from("<tempdir>"),
            }
            .into_error(e)
        })?;

        let db = SingleWriterTxDatabase::builder(dir.path())
            .open()
            .map_err(|e| error::StorageSnafu { message: format!("fjall open temp: {e}") }.build())?;

        Self::init(db, Some(dir))
    }

    fn init(db: SingleWriterTxDatabase, temp_dir: Option<tempfile::TempDir>) -> Result<Self> {
        // Open all partitions eagerly so they exist before any read/write.
        for name in &["sessions", "messages", "usage", "distillations", "notes", "blackboard", "counters"] {
            db.keyspace(name, KeyspaceCreateOptions::default)
                .map_err(|e| {
                    error::StorageSnafu {
                        message: format!("fjall open partition {name}: {e}"),
                    }
                    .build()
                })?;
        }
        Ok(Self {
            db: Arc::new(db),
            write_lock: Mutex::new(()),
            _temp_dir: temp_dir,
        })
    }

    // ── Partition helpers ─────────────────────────────────────────────────

    fn partition(&self, name: &str) -> Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall partition {name}: {e}"),
                }
                .build()
            })
    }

    fn get_bytes(&self, partition: &fjall::SingleWriterTxKeyspace, key: &str) -> Result<Option<Vec<u8>>> {
        use fjall::Readable;
        let snap = self.db.read_tx();
        snap.get(partition, key.as_bytes())
            .map(|opt| opt.map(|s| s.to_vec()))
            .map_err(|e| error::StorageSnafu { message: format!("fjall get: {e}") }.build())
    }

    fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        partition: &fjall::SingleWriterTxKeyspace,
        key: &str,
    ) -> Result<Option<T>> {
        match self.get_bytes(partition, key)? {
            None => Ok(None),
            Some(bytes) => {
                let v: T = serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
                Ok(Some(v))
            }
        }
    }

    // ── Session helpers ───────────────────────────────────────────────────

    fn session_key_index_key(nous_id: &str, session_key: &str) -> String {
        format!("idx:key:{nous_id}:{session_key}")
    }

    fn session_nous_index_key(nous_id: &str, updated_at: &str, session_id: &str) -> String {
        format!("idx:nous:{nous_id}:upd:{updated_at}:{session_id}")
    }

    fn write_session(&self, session: &Session) -> Result<()> {
        let sessions = self.partition("sessions")?;
        let data = serde_json::to_vec(session).context(error::StoredJsonSnafu)?;

        let mut tx = self.db.write_tx();
        tx.insert(&sessions, session.id.as_str(), data.as_slice());
        tx.insert(
            &sessions,
            Self::session_key_index_key(&session.nous_id, &session.session_key).as_str(),
            session.id.as_bytes(),
        );
        tx.insert(
            &sessions,
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, &session.id).as_str(),
            b"",
        );
        tx.commit()
            .map_err(|e| error::StorageSnafu { message: format!("fjall session write: {e}") }.build())?;
        Ok(())
    }

    fn update_session_nous_index(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        partition: &fjall::SingleWriterTxKeyspace,
        session: &Session,
        old_updated_at: &str,
    ) {
        // Remove the old index entry before writing the new one.
        let old_key = Self::session_nous_index_key(&session.nous_id, old_updated_at, &session.id);
        tx.remove(partition, old_key.as_str());
    }

    fn read_session_by_raw_id(&self, id: &str) -> Result<Option<Session>> {
        let sessions = self.partition("sessions")?;
        self.get_json::<Session>(&sessions, id)
    }

    // ── Public API ────────────────────────────────────────────────────────

    /// Lightweight liveness check.
    ///
    /// Reads the `counters` partition to verify the keyspace is accessible.
    ///
    /// # Errors
    /// Returns an error if the fjall keyspace is unreachable.
    pub fn ping(&self) -> Result<()> {
        let _ = self.partition("counters")?;
        Ok(())
    }

    /// Find an active session by nous ID and session key.
    #[instrument(skip(self))]
    pub fn find_session(&self, nous_id: &str, session_key: &str) -> Result<Option<Session>> {
        let sessions = self.partition("sessions")?;
        let idx_key = Self::session_key_index_key(nous_id, session_key);
        match self.get_bytes(&sessions, &idx_key)? {
            None => Ok(None),
            Some(id_bytes) => {
                let id = String::from_utf8(id_bytes).map_err(|e| {
                    error::StorageSnafu {
                        message: format!("invalid session id bytes: {e}"),
                    }
                    .build()
                })?;
                let session = self.read_session_by_raw_id(&id)?;
                // Only return active sessions.
                Ok(session.filter(|s| s.status == SessionStatus::Active))
            }
        }
    }

    /// Find a session by ID (any status).
    #[instrument(skip(self))]
    pub fn find_session_by_id(&self, id: &str) -> Result<Option<Session>> {
        self.read_session_by_raw_id(id)
    }

    /// Create a new session.
    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let session_type = SessionType::from_key(session_key);
        let now = now_iso();

        let session = Session {
            id: id.to_owned(),
            nous_id: nous_id.to_owned(),
            session_key: session_key.to_owned(),
            status: SessionStatus::Active,
            model: model.map(str::to_owned),
            session_type,
            created_at: now.clone(),
            updated_at: now,
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
                parent_session_id: parent_session_id.map(str::to_owned),
                thread_id: None,
                transport: None,
                display_name: None,
            },
        };

        // Check for uniqueness on (nous_id, session_key).
        let sessions = self.partition("sessions")?;
        let idx_key = Self::session_key_index_key(nous_id, session_key);
        if self.get_bytes(&sessions, &idx_key)?.is_some() {
            return Err(error::StorageSnafu {
                message: format!(
                    "UNIQUE constraint failed: session ({nous_id}, {session_key}) already exists"
                ),
            }
            .build());
        }

        self.write_session(&session)?;
        metrics::record_session_created(nous_id, session_type.as_str());
        info!(id, nous_id, session_key, %session_type, "created session");
        Ok(session)
    }

    /// Find or create an active session. Reactivates archived sessions if found.
    #[instrument(skip(self))]
    pub fn find_or_create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let idx_key = Self::session_key_index_key(nous_id, session_key);

        if let Some(existing_id_bytes) = self.get_bytes(&sessions_part, &idx_key)? {
            let existing_id = String::from_utf8(existing_id_bytes).map_err(|e| {
                error::StorageSnafu {
                    message: format!("invalid session id bytes: {e}"),
                }
                .build()
            })?;

            let mut session = self
                .read_session_by_raw_id(&existing_id)?
                .ok_or_else(|| {
                    error::SessionCreateSnafu {
                        nous_id: nous_id.to_owned(),
                    }
                    .build()
                })?;

            if session.status != SessionStatus::Active {
                let old_updated_at = session.updated_at.clone();
                session.status = SessionStatus::Active;
                session.updated_at = now_iso();

                let data = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
                let mut tx = self.db.write_tx();
                tx.insert(&sessions_part, session.id.as_str(), data.as_slice());
                Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
                let new_nous_key = Self::session_nous_index_key(
                    &session.nous_id,
                    &session.updated_at,
                    &session.id,
                );
                tx.insert(&sessions_part, new_nous_key.as_str(), b"");
                tx.commit().map_err(|e| {
                    error::StorageSnafu {
                        message: format!("fjall reactivate session: {e}"),
                    }
                    .build()
                })?;

                info!(id = session.id, nous_id, session_key, "reactivated session");
            }

            return Ok(session);
        }

        // No existing session: create new.
        let session_type = SessionType::from_key(session_key);
        let now = now_iso();
        let session = Session {
            id: id.to_owned(),
            nous_id: nous_id.to_owned(),
            session_key: session_key.to_owned(),
            status: SessionStatus::Active,
            model: model.map(str::to_owned),
            session_type,
            created_at: now.clone(),
            updated_at: now,
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
                parent_session_id: parent_session_id.map(str::to_owned),
                thread_id: None,
                transport: None,
                display_name: None,
            },
        };

        self.write_session(&session)?;
        metrics::record_session_created(nous_id, session_type.as_str());
        info!(id, nous_id, session_key, %session_type, "created session (find_or_create)");
        Ok(session)
    }

    /// List sessions, optionally filtered by nous ID.
    ///
    /// Returns sessions ordered by `updated_at` descending.
    #[instrument(skip(self))]
    pub fn list_sessions(&self, nous_id: Option<&str>) -> Result<Vec<Session>> {
        use fjall::Readable;

        let sessions_part = self.partition("sessions")?;
        let snap = self.db.read_tx();
        let mut sessions = Vec::new();

        if let Some(nous_id) = nous_id {
            // Scan the `idx:nous:{nous_id}:upd:` prefix.
            let prefix = format!("idx:nous:{nous_id}:upd:");
            // Compute the exclusive upper bound: replace last character with next.
            let upper = {
                let mut s = prefix.clone();
                let last = s.pop().unwrap_or('\0');
                s.push(char::from_u32(last as u32 + 1).unwrap_or('\u{FFFF}'));
                s
            };

            // Keys are `idx:nous:{nous_id}:upd:{updated_at}:{session_id}`.
            // Collect session IDs from the suffix, then load sessions.
            // The lexicographic sort on updated_at gives us ascending order;
            // we reverse to get descending (most recently updated first).
            let mut index_keys: Vec<Vec<u8>> = Vec::new();
            for guard in snap.range(&sessions_part, prefix.as_str()..upper.as_str()) {
                let (k, _v) = guard.into_inner().map_err(|e| {
                    error::StorageSnafu {
                        message: format!("fjall list_sessions range: {e}"),
                    }
                    .build()
                })?;
                index_keys.push(k.to_vec());
            }

            for raw_key in index_keys.into_iter().rev() {
                let key = String::from_utf8_lossy(&raw_key).into_owned();
                // Extract session_id: everything after the last ':'.
                if let Some(session_id) = key.rsplit(':').next()
                    && let Some(session) = self.read_session_by_raw_id(session_id)?
                {
                    sessions.push(session);
                }
            }
        } else {
            // Full scan: find all keys that don't start with "idx:" (raw session rows).
            let idx_prefix = b"idx:".as_slice();
            let mut raw_sessions: Vec<Session> = Vec::new();
            for guard in snap.range::<&str, _>(&sessions_part, ..) {
                if let Ok((k, v)) = guard.into_inner()
                    && !k.starts_with(idx_prefix)
                    && let Ok(session) = serde_json::from_slice::<Session>(&v)
                {
                    raw_sessions.push(session);
                }
            }

            // Sort descending by updated_at.
            raw_sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            sessions = raw_sessions;
        }

        Ok(sessions)
    }

    /// Update session status.
    #[instrument(skip(self))]
    pub fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let mut session = self
            .read_session_by_raw_id(id)?
            .ok_or_else(|| error::SessionNotFoundSnafu { id: id.to_owned() }.build())?;

        let old_updated_at = session.updated_at.clone();
        session.status = status;
        session.updated_at = now_iso();

        let data = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
        let mut tx = self.db.write_tx();
        tx.insert(&sessions_part, id, data.as_slice());
        Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
        let new_nous_key =
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, id);
        tx.insert(&sessions_part, new_nous_key.as_str(), b"");
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall update_session_status: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Update session display name.
    #[instrument(skip(self))]
    pub fn update_display_name(&self, id: &str, display_name: &str) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let mut session = self
            .read_session_by_raw_id(id)?
            .ok_or_else(|| error::SessionNotFoundSnafu { id: id.to_owned() }.build())?;

        let old_updated_at = session.updated_at.clone();
        session.origin.display_name = Some(display_name.to_owned());
        session.updated_at = now_iso();

        let data = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
        let mut tx = self.db.write_tx();
        tx.insert(&sessions_part, id, data.as_slice());
        Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
        let new_nous_key =
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, id);
        tx.insert(&sessions_part, new_nous_key.as_str(), b"");
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall update_display_name: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Hard-delete a session and all its messages by ID.
    ///
    /// # Errors
    /// Returns an error if any partition operation fails.
    #[instrument(skip(self))]
    pub fn delete_session(&self, id: &str) -> Result<bool> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;

        let Some(session) = self.read_session_by_raw_id(id)? else {
            return Ok(false);
        };

        // Gather index keys to delete.
        let key_idx = Self::session_key_index_key(&session.nous_id, &session.session_key);
        let nous_idx =
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, id);

        let messages_part = self.partition("messages")?;
        let usage_part = self.partition("usage")?;
        let distillations_part = self.partition("distillations")?;
        let notes_part = self.partition("notes")?;

        let mut tx = self.db.write_tx();

        // Delete messages.
        let msg_prefix = format!("{id}:");
        let msg_upper = format!("{id};\x00");
        let msg_keys: Vec<Vec<u8>> = tx
            .range(&messages_part, msg_prefix.as_str()..msg_upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .map(|(k, _v)| k.to_vec())
            .collect();
        for key in &msg_keys {
            tx.remove(&messages_part, key.as_slice());
        }
        // Delete next_seq counter for this session.
        tx.remove(&messages_part, format!("next_seq:{id}").as_str());

        // Delete usage.
        let usage_prefix = format!("{id}:");
        let usage_upper = format!("{id};\x00");
        let usage_keys: Vec<Vec<u8>> = tx
            .range(&usage_part, usage_prefix.as_str()..usage_upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .map(|(k, _v)| k.to_vec())
            .collect();
        for key in &usage_keys {
            tx.remove(&usage_part, key.as_slice());
        }

        // Delete distillations.
        let dist_prefix = format!("{id}:");
        let dist_upper = format!("{id};\x00");
        let dist_keys: Vec<Vec<u8>> = tx
            .range(
                &distillations_part,
                dist_prefix.as_str()..dist_upper.as_str(),
            )
            .filter_map(|g| g.into_inner().ok())
            .map(|(k, _v)| k.to_vec())
            .collect();
        for key in &dist_keys {
            tx.remove(&distillations_part, key.as_slice());
        }

        // Delete notes.
        let notes_prefix = format!("{id}:");
        let notes_upper = format!("{id};\x00");
        let note_keys: Vec<Vec<u8>> = tx
            .range(&notes_part, notes_prefix.as_str()..notes_upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .map(|(k, _v)| k.to_vec())
            .collect();
        // Also delete gid index entries for this session.
        let id_prefix = format!("{id}:");
        let gid_keys: Vec<Vec<u8>> = tx
            .range(&notes_part, "gid:".."gid;\x00")
            .filter_map(|g| g.into_inner().ok())
            .filter(|(_k, v)| v.starts_with(id_prefix.as_bytes()))
            .map(|(k, _v)| k.to_vec())
            .collect();
        for key in &note_keys {
            tx.remove(&notes_part, key.as_slice());
        }
        for key in &gid_keys {
            tx.remove(&notes_part, key.as_slice());
        }

        // Delete session and its index entries.
        tx.remove(&sessions_part, id);
        tx.remove(&sessions_part, key_idx.as_str());
        tx.remove(&sessions_part, nous_idx.as_str());

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall delete_session: {e}"),
            }
            .build()
        })?;

        Ok(true)
    }

    // ── Message operations ────────────────────────────────────────────────

    /// Append a message to a session. Returns the sequence number.
    #[instrument(skip(self, content))]
    pub fn append_message(
        &self,
        session_id: &str,
        role: Role,
        content: &str,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        token_estimate: i64,
    ) -> Result<i64> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;

        // Read current next_seq for this session.
        let next_seq_key = format!("next_seq:{session_id}");
        let snap = self.db.read_tx();
        let current_seq = snap
            .get(&messages_part, next_seq_key.as_str())
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall seq read: {e}"),
                }
                .build()
            })?
            .map_or(0, |b| decode_u64(&b));
        let seq = current_seq + 1;
        drop(snap);

        let msg_id_counter = {
            // Use a global message counter for the `id` field (unique across sessions).
            let counters = self.partition("counters")?;
            let snap2 = self.db.read_tx();
            let c = snap2
                .get(&counters, "msg_id")
                .map_err(|e| {
                    error::StorageSnafu {
                        message: format!("fjall msg_id counter: {e}"),
                    }
                    .build()
                })?
                .map_or(0, |b| decode_u64(&b))
                + 1;
            drop(snap2);
            c
        };

        let now = now_iso();
        let msg = Message {
            id: msg_id_counter as i64,
            session_id: session_id.to_owned(),
            seq: seq as i64,
            role,
            content: content.to_owned(),
            tool_call_id: tool_call_id.map(str::to_owned),
            tool_name: tool_name.map(str::to_owned),
            token_estimate,
            is_distilled: false,
            created_at: now,
        };

        let msg_key = format!("{session_id}:{}", pad_u64(seq));
        let msg_data = serde_json::to_vec(&msg).context(error::StoredJsonSnafu)?;

        // Update session counters.
        let mut session = self
            .read_session_by_raw_id(session_id)?
            .ok_or_else(|| {
                error::SessionNotFoundSnafu {
                    id: session_id.to_owned(),
                }
                .build()
            })?;
        let old_updated_at = session.updated_at.clone();
        session.metrics.message_count += 1;
        session.metrics.token_count_estimate += token_estimate;
        session.updated_at = now_iso();
        let session_data = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;

        let counters_part = self.partition("counters")?;

        let mut tx = self.db.write_tx();
        tx.insert(&messages_part, msg_key.as_str(), msg_data.as_slice());
        tx.insert(&messages_part, next_seq_key.as_str(), encode_u64(seq));
        tx.insert(&counters_part, "msg_id", encode_u64(msg_id_counter));
        tx.insert(&sessions_part, session_id, session_data.as_slice());
        Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
        let new_nous_key =
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, session_id);
        tx.insert(&sessions_part, new_nous_key.as_str(), b"");
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall append_message commit: {e}"),
            }
            .build()
        })?;

        debug!(session_id, seq, %role, token_estimate, "appended message");
        Ok(seq as i64)
    }

    /// Get non-distilled messages, newest `limit` in chronological order.
    fn load_messages_in_range(
        &self,
        session_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Message>> {
        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        // All messages in ascending seq order (lexicographic on padded key).
        let mut messages: Vec<Message> = snap
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .filter_map(|(_k, v)| serde_json::from_slice::<Message>(&v).ok())
            .filter(|m| !m.is_distilled)
            .collect();

        // Apply limit: take the N most recent (end of list), then restore chronological order.
        if let Some(lim) = limit {
            let lim = usize::try_from(lim).unwrap_or(usize::MAX);
            if messages.len() > lim {
                let start = messages.len() - lim;
                messages = messages.split_off(start);
            }
        }

        Ok(messages)
    }

    /// Get message history for a session.
    #[instrument(skip(self))]
    pub fn get_history(&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>> {
        self.load_messages_in_range(session_id, limit)
    }

    /// Get message history for a session with optional `seq < before_seq` filter.
    #[instrument(skip(self))]
    pub fn get_history_filtered(
        &self,
        session_id: &str,
        limit: Option<i64>,
        before_seq: Option<i64>,
    ) -> Result<Vec<Message>> {
        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = match before_seq {
            Some(b) => format!("{session_id}:{}", pad_u64(b.cast_unsigned())),
            None => format!("{session_id};\x00"),
        };
        let snap = self.db.read_tx();

        let mut messages: Vec<Message> = snap
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .filter_map(|(_k, v)| serde_json::from_slice::<Message>(&v).ok())
            .filter(|m| !m.is_distilled)
            .collect();

        if let Some(lim) = limit {
            let lim = usize::try_from(lim).unwrap_or(usize::MAX);
            if messages.len() > lim {
                let start = messages.len() - lim;
                messages = messages.split_off(start);
            }
        }

        Ok(messages)
    }

    /// Get message history within a token budget (most recent first, working backward).
    ///
    /// At least one message is always returned.
    ///
    /// # Errors
    /// Returns an error if the partition scan fails.
    #[instrument(skip(self), level = "debug")]
    pub fn get_history_with_budget(
        &self,
        session_id: &str,
        max_tokens: i64,
    ) -> Result<Vec<Message>> {
        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        // Collect in ascending order, then iterate in reverse for budget window.
        let all: Vec<Message> = snap
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .filter_map(|(_k, v)| serde_json::from_slice::<Message>(&v).ok())
            .filter(|m| !m.is_distilled)
            .collect();

        let mut result = Vec::new();
        let mut total: i64 = 0;

        for msg in all.into_iter().rev() {
            if total + msg.token_estimate > max_tokens && !result.is_empty() {
                break;
            }
            total += msg.token_estimate;
            result.push(msg);
        }

        result.reverse();
        Ok(result)
    }

    /// Return the most recent distillation summary for a session, if any.
    pub fn get_distillation_summary(&self, session_id: &str) -> Result<Option<String>> {
        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        // The distillation summary is the message at seq=0 that is NOT distilled.
        let key = format!("{session_id}:{}", pad_u64(0));
        let snap = self.db.read_tx();
        let bytes = snap.get(&messages_part, key.as_str()).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall get_distillation_summary: {e}"),
            }
            .build()
        })?;
        let Some(bytes) = bytes else { return Ok(None) };
        let msg: Message = serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
        if msg.is_distilled {
            return Ok(None);
        }
        Ok(Some(msg.content))
    }

    /// Mark messages as distilled and recalculate session token count.
    #[instrument(skip(self, seqs), fields(count = seqs.len()))]
    pub fn mark_messages_distilled(&self, session_id: &str, seqs: &[i64]) -> Result<()> {
        use fjall::Readable;

        if seqs.is_empty() {
            return Ok(());
        }

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;

        let mut tx = self.db.write_tx();

        for &seq in seqs {
            let key = format!("{session_id}:{}", pad_u64(seq.cast_unsigned()));
            if let Ok(Some(bytes)) = tx.get(&messages_part, key.as_str()) {
                let mut msg: Message =
                    serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
                msg.is_distilled = true;
                let updated = serde_json::to_vec(&msg).context(error::StoredJsonSnafu)?;
                tx.insert(&messages_part, key.as_str(), updated.as_slice());
            }
        }

        // Recalculate session token/message count from remaining undistilled messages.
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        // WHY: read after writing — use the tx's read-your-own-writes to see updates.
        let (total_tokens, msg_count) = {
            let mut tokens: i64 = 0;
            let mut count: i64 = 0;
            for guard in tx.range(&messages_part, prefix.as_str()..upper.as_str()) {
                if let Ok((_k, v)) = guard.into_inner()
                    && let Ok(msg) = serde_json::from_slice::<Message>(&v)
                    && !msg.is_distilled
                {
                    tokens += msg.token_estimate;
                    count += 1;
                }
            }
            (tokens, count)
        };

        if let Ok(Some(bytes)) = tx.get(&sessions_part, session_id) {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            session.metrics.token_count_estimate = total_tokens;
            session.metrics.message_count = msg_count;
            session.updated_at = now_iso();
            let updated = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
            tx.insert(&sessions_part, session_id, updated.as_slice());
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall mark_messages_distilled: {e}"),
            }
            .build()
        })?;

        info!(session_id, distilled = seqs.len(), "distilled messages");
        Ok(())
    }

    /// Insert a distillation summary as a system message and remove distilled messages.
    #[instrument(skip(self, content))]
    pub fn insert_distillation_summary(&self, session_id: &str, content: &str) -> Result<()> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;

        let mut tx = self.db.write_tx();

        // Delete any existing summary at seq 0.
        let seq0_key = format!("{session_id}:{}", pad_u64(0));
        tx.remove(&messages_part, seq0_key.as_str());

        // Delete all messages marked is_distilled = true.
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let distilled_keys: Vec<Vec<u8>> = tx
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .filter(|(_k, v)| {
                serde_json::from_slice::<Message>(v)
                    .map(|m| m.is_distilled)
                    .unwrap_or(false)
            })
            .map(|(k, _v)| k.to_vec())
            .collect();
        for key in &distilled_keys {
            tx.remove(&messages_part, key.as_slice());
        }

        // Increment global msg_id counter.
        let counters_part = self.partition("counters")?;
        let current_msg_id = tx
            .get(&counters_part, "msg_id")
            .unwrap_or(None)
            .map_or(0, |b| decode_u64(&b));
        let new_msg_id = current_msg_id + 1;
        tx.insert(&counters_part, "msg_id", encode_u64(new_msg_id));

        let token_estimate = (content.len() as i64 + 3) / 4;
        let now = now_iso();
        let summary_msg = Message {
            id: new_msg_id as i64,
            session_id: session_id.to_owned(),
            seq: 0,
            role: Role::System,
            content: content.to_owned(),
            tool_call_id: None,
            tool_name: None,
            token_estimate,
            is_distilled: false,
            created_at: now,
        };
        let summary_data = serde_json::to_vec(&summary_msg).context(error::StoredJsonSnafu)?;
        tx.insert(&messages_part, seq0_key.as_str(), summary_data.as_slice());

        // Recalculate session token/message counts via read-your-own-writes.
        // WHY: The range scan includes the summary at seq 0 we just inserted above
        // (fjall WriteTransaction provides read-your-own-writes), so we do NOT add
        // token_estimate again here — that would double-count it.
        let (total_tokens, msg_count) = {
            let mut tokens: i64 = 0;
            let mut count: i64 = 0;
            for guard in tx.range(&messages_part, prefix.as_str()..upper.as_str()) {
                if let Ok((_k, v)) = guard.into_inner()
                    && let Ok(msg) = serde_json::from_slice::<Message>(&v)
                    && !msg.is_distilled
                {
                    tokens += msg.token_estimate;
                    count += 1;
                }
            }
            (tokens, count)
        };

        if let Ok(Some(bytes)) = tx.get(&sessions_part, session_id) {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            session.metrics.token_count_estimate = total_tokens;
            session.metrics.message_count = msg_count;
            session.updated_at = now_iso();
            let updated = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
            tx.insert(&sessions_part, session_id, updated.as_slice());
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall insert_distillation_summary: {e}"),
            }
            .build()
        })?;

        info!(session_id, msg_count, total_tokens, "inserted distillation summary");
        Ok(())
    }

    /// Record a distillation event.
    #[instrument(skip(self))]
    pub fn record_distillation(
        &self,
        session_id: &str,
        messages_before: i64,
        messages_after: i64,
        tokens_before: i64,
        tokens_after: i64,
        model: Option<&str>,
    ) -> Result<()> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let distillations_part = self.partition("distillations")?;
        let sessions_part = self.partition("sessions")?;
        let counters_part = self.partition("counters")?;

        let snap = self.db.read_tx();
        let dist_id = snap
            .get(&counters_part, "dist_id")
            .unwrap_or(None)
            .map_or(0, |b| decode_u64(&b))
            + 1;
        drop(snap);

        let rec = DistillationRecord {
            session_id: session_id.to_owned(),
            messages_before,
            messages_after,
            tokens_before,
            tokens_after,
            model: model.map(str::to_owned),
            created_at: now_iso(),
        };
        let rec_data = serde_json::to_vec(&rec).context(error::StoredJsonSnafu)?;
        let dist_key = format!("{session_id}:{}", pad_u64(dist_id));

        let mut tx = self.db.write_tx();
        tx.insert(&distillations_part, dist_key.as_str(), rec_data.as_slice());
        tx.insert(&counters_part, "dist_id", encode_u64(dist_id));

        // Update session distillation counter.
        if let Ok(Some(bytes)) = tx.get(&sessions_part, session_id) {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            session.metrics.distillation_count += 1;
            session.metrics.last_distilled_at = Some(rec.created_at.clone());
            session.updated_at = now_iso();
            let updated = serde_json::to_vec(&session).context(error::StoredJsonSnafu)?;
            tx.insert(&sessions_part, session_id, updated.as_slice());
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall record_distillation: {e}"),
            }
            .build()
        })?;

        info!(
            session_id,
            messages_before, messages_after, tokens_before, tokens_after, "recorded distillation"
        );
        Ok(())
    }

    // ── Usage ─────────────────────────────────────────────────────────────

    /// Check if usage has already been recorded for a given session + turn.
    #[instrument(skip(self), level = "debug")]
    pub fn usage_exists_for_turn(&self, session_id: &str, turn_seq: i64) -> Result<bool> {
        let usage_part = self.partition("usage")?;
        let key = format!("{session_id}:{}", pad_u64(turn_seq.cast_unsigned()));
        Ok(self.get_bytes(&usage_part, &key)?.is_some())
    }

    /// Record token usage for a turn.
    #[instrument(skip(self, record), level = "debug")]
    pub fn record_usage(&self, record: &UsageRecord) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let usage_part = self.partition("usage")?;
        let key = format!("{}:{}", record.session_id, pad_u64(record.turn_seq.cast_unsigned()));
        let data = serde_json::to_vec(record).context(error::StoredJsonSnafu)?;
        let mut tx = self.db.write_tx();
        tx.insert(&usage_part, key.as_str(), data.as_slice());
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall record_usage: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    // ── Agent notes ───────────────────────────────────────────────────────

    /// Valid agent note categories (must match schema.rs `VALID_CATEGORIES`).
    const VALID_CATEGORIES: &'static [&'static str] =
        &["task", "decision", "preference", "correction", "context"];

    /// Add an agent note.
    #[instrument(skip(self, content))]
    pub fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64> {
        use fjall::Readable;

        if !Self::VALID_CATEGORIES.contains(&category) {
            return Err(error::StorageSnafu {
                message: format!(
                    "CHECK constraint failed: category '{category}' is not valid; \
                     allowed: {:?}",
                    Self::VALID_CATEGORIES
                ),
            }
            .build());
        }

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let notes_part = self.partition("notes")?;
        let counters_part = self.partition("counters")?;

        let snap = self.db.read_tx();
        let note_id = snap
            .get(&counters_part, "note_local_id")
            .unwrap_or(None)
            .map_or(0, |b| decode_u64(&b))
            + 1;
        let global_id = snap
            .get(&counters_part, "note_global_id")
            .unwrap_or(None)
            .map_or(0, |b| decode_u64(&b))
            + 1;
        drop(snap);

        let note = AgentNote {
            id: global_id as i64,
            session_id: session_id.to_owned(),
            nous_id: nous_id.to_owned(),
            category: category.to_owned(),
            content: content.to_owned(),
            created_at: now_iso(),
        };
        let note_data = serde_json::to_vec(&note).context(error::StoredJsonSnafu)?;
        let local_key = format!("{session_id}:{}", pad_u64(note_id));
        let gid_key = format!("gid:{}", pad_u64(global_id));
        let gid_val = format!("{session_id}:{}", pad_u64(note_id));

        let mut tx = self.db.write_tx();
        tx.insert(&notes_part, local_key.as_str(), note_data.as_slice());
        tx.insert(&notes_part, gid_key.as_str(), gid_val.as_bytes());
        tx.insert(&counters_part, "note_local_id", encode_u64(note_id));
        tx.insert(&counters_part, "note_global_id", encode_u64(global_id));
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall add_note: {e}"),
            }
            .build()
        })?;

        Ok(global_id as i64)
    }

    /// Get notes for a session.
    #[instrument(skip(self))]
    pub fn get_notes(&self, session_id: &str) -> Result<Vec<AgentNote>> {
        use fjall::Readable;

        let notes_part = self.partition("notes")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        let notes: Vec<AgentNote> = snap
            .range(&notes_part, prefix.as_str()..upper.as_str())
            .filter_map(|g| g.into_inner().ok())
            .filter_map(|(_k, v)| serde_json::from_slice::<AgentNote>(&v).ok())
            .collect();

        Ok(notes)
    }

    /// Delete a note by global ID.
    #[instrument(skip(self))]
    pub fn delete_note(&self, note_id: i64) -> Result<bool> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let notes_part = self.partition("notes")?;

        let gid_key = format!("gid:{}", pad_u64(note_id.cast_unsigned()));
        let snap = self.db.read_tx();
        let local_ref = snap.get(&notes_part, gid_key.as_str()).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall delete_note gid: {e}"),
            }
            .build()
        })?;
        drop(snap);

        let Some(local_bytes) = local_ref else {
            return Ok(false);
        };
        let local_key = String::from_utf8(local_bytes.to_vec()).map_err(|e| {
            error::StorageSnafu {
                message: format!("invalid note local key: {e}"),
            }
            .build()
        })?;

        let mut tx = self.db.write_tx();
        tx.remove(&notes_part, local_key.as_str());
        tx.remove(&notes_part, gid_key.as_str());
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall delete_note: {e}"),
            }
            .build()
        })?;

        Ok(true)
    }

    // ── Blackboard ────────────────────────────────────────────────────────

    /// Write or update a blackboard entry. Upserts on key.
    #[instrument(skip(self, value), level = "debug")]
    pub fn blackboard_write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bb_part = self.partition("blackboard")?;

        let now = now_iso();
        let expires_at = if ttl_secs > 0 {
            // Approximate: add ttl_secs to now.
            jiff::Zoned::now()
                .checked_add(jiff::Span::new().seconds(ttl_secs))
                .ok()
                .map(|z| z.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        } else {
            None
        };

        let row = BlackboardRow {
            key: key.to_owned(),
            value: value.to_owned(),
            author_nous_id: author.to_owned(),
            ttl_seconds: ttl_secs,
            created_at: now,
            expires_at,
        };
        let data = serde_json::to_vec(&row).context(error::StoredJsonSnafu)?;
        let mut tx = self.db.write_tx();
        tx.insert(&bb_part, key, data.as_slice());
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall blackboard_write: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Read a blackboard entry by key, filtering expired entries.
    #[instrument(skip(self))]
    pub fn blackboard_read(&self, key: &str) -> Result<Option<BlackboardRow>> {
        let bb_part = self.partition("blackboard")?;
        let row: Option<BlackboardRow> = self.get_json(&bb_part, key)?;
        Ok(row.filter(|r| !is_expired(r)))
    }

    /// List all non-expired blackboard entries.
    #[instrument(skip(self))]
    pub fn blackboard_list(&self) -> Result<Vec<BlackboardRow>> {
        use fjall::Readable;

        let bb_part = self.partition("blackboard")?;
        let snap = self.db.read_tx();

        let entries: Vec<BlackboardRow> = snap
            .range::<&str, _>(&bb_part, ..)
            .filter_map(|g| g.into_inner().ok())
            .filter_map(|(_k, v)| serde_json::from_slice::<BlackboardRow>(&v).ok())
            .filter(|r| !is_expired(r))
            .collect();

        Ok(entries)
    }

    /// Remove all expired blackboard entries.
    ///
    /// Returns the number of rows deleted.
    #[expect(dead_code, reason = "blackboard cleanup called by retention runner")]
    pub(crate) fn cleanup_expired_entries(&self) -> Result<u64> {
        use fjall::Readable;

        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bb_part = self.partition("blackboard")?;
        let snap = self.db.read_tx();

        let expired_keys: Vec<Vec<u8>> = snap
            .range::<&str, _>(&bb_part, ..)
            .filter_map(|g| g.into_inner().ok())
            .filter(|(_k, v)| {
                serde_json::from_slice::<BlackboardRow>(v)
                    .map(|r| is_expired(&r))
                    .unwrap_or(false)
            })
            .map(|(k, _v)| k.to_vec())
            .collect();
        drop(snap);

        let count = expired_keys.len() as u64;
        if count > 0 {
            let mut tx = self.db.write_tx();
            for key in &expired_keys {
                tx.remove(&bb_part, key.as_slice());
            }
            tx.commit().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall cleanup_expired: {e}"),
                }
                .build()
            })?;
        }

        Ok(count)
    }

    /// Delete a blackboard entry. Only the original author can delete.
    #[instrument(skip(self))]
    pub fn blackboard_delete(&self, key: &str, author: &str) -> Result<bool> {
        let _guard = self.write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bb_part = self.partition("blackboard")?;

        let row: Option<BlackboardRow> = self.get_json(&bb_part, key)?;
        let Some(row) = row else { return Ok(false) };
        if row.author_nous_id != author {
            return Ok(false);
        }

        let mut tx = self.db.write_tx();
        tx.remove(&bb_part, key);
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall blackboard_delete: {e}"),
            }
            .build()
        })?;
        Ok(true)
    }
}

/// Check whether a blackboard row has expired.
fn is_expired(row: &BlackboardRow) -> bool {
    let Some(ref expires_at) = row.expires_at else {
        return false;
    };
    // Compare ISO 8601 strings lexicographically (both in UTC, same format).
    let now = now_iso();
    expires_at.as_str() <= now.as_str()
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    #![expect(clippy::unwrap_used, reason = "test assertions")]

    use super::SessionStore;
    use crate::types::{Role, SessionStatus};

    fn test_store() -> SessionStore {
        SessionStore::open_in_memory().expect("open fjall in-memory store")
    }

    #[test]
    fn create_and_find_session() {
        let store = test_store();
        let session = store
            .create_session("ses-1", "syn", "main", None, None)
            .expect("create session");
        assert_eq!(session.id, "ses-1");
        assert_eq!(session.nous_id, "syn");
        assert_eq!(session.session_key, "main");
        assert_eq!(session.status, SessionStatus::Active);

        let found = store.find_session("syn", "main").expect("find session");
        assert!(found.is_some(), "session should exist after creation");
        assert_eq!(found.unwrap().id, "ses-1");
    }

    #[test]
    fn find_session_returns_none_for_missing() {
        let store = test_store();
        let found = store.find_session("syn", "nonexistent").expect("find session");
        assert!(found.is_none());
    }

    #[test]
    fn create_session_unique_constraint() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("first create");
        let result = store.create_session("ses-2", "syn", "main", None, None);
        assert!(result.is_err(), "duplicate (nous_id, session_key) must fail");
    }

    #[test]
    fn append_and_retrieve_messages() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");

        let seq1 = store.append_message("ses-1", Role::User, "hello", None, None, 10).expect("append");
        let seq2 = store.append_message("ses-1", Role::Assistant, "hi there", None, None, 15).expect("append");

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        let history = store.get_history("ses-1", None).expect("get history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].content, "hi there");
    }

    #[test]
    fn message_updates_session_counts() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.append_message("ses-1", Role::User, "hello", None, None, 100).expect("append");
        store.append_message("ses-1", Role::Assistant, "world", None, None, 200).expect("append");

        let session = store.find_session_by_id("ses-1").expect("query").unwrap();
        assert_eq!(session.metrics.message_count, 2);
        assert_eq!(session.metrics.token_count_estimate, 300);
    }

    #[test]
    fn list_sessions_by_nous_id() {
        let store = test_store();
        store.create_session("ses-a", "agent-x", "main", None, None).expect("create a");
        store.create_session("ses-b", "agent-x", "secondary", None, None).expect("create b");
        store.create_session("ses-c", "agent-y", "main", None, None).expect("create c");

        let agent_x = store.list_sessions(Some("agent-x")).expect("list");
        assert_eq!(agent_x.len(), 2);
        let all = store.list_sessions(None).expect("list all");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn update_session_status() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.update_session_status("ses-1", SessionStatus::Archived).expect("update status");
        let session = store.find_session_by_id("ses-1").expect("query").unwrap();
        assert_eq!(session.status, SessionStatus::Archived);
    }

    #[test]
    fn find_or_create_reactivates_archived() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.update_session_status("ses-1", SessionStatus::Archived).expect("archive");

        let session = store
            .find_or_create_session("ses-new", "syn", "main", None, None)
            .expect("find_or_create");
        assert_eq!(session.id, "ses-1", "should return existing, not create new");
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[test]
    fn distillation_marks_and_recalculates() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.append_message("ses-1", Role::User, "old 1", None, None, 100).expect("append");
        store.append_message("ses-1", Role::User, "old 2", None, None, 150).expect("append");
        store.append_message("ses-1", Role::User, "keep this", None, None, 50).expect("append");

        store.mark_messages_distilled("ses-1", &[1, 2]).expect("distill");

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "keep this");

        let session = store.find_session_by_id("ses-1").expect("query").unwrap();
        assert_eq!(session.metrics.message_count, 1);
        assert_eq!(session.metrics.token_count_estimate, 50);
    }

    #[test]
    fn insert_distillation_summary() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.append_message("ses-1", Role::User, "msg1", None, None, 100).expect("append");
        store.append_message("ses-1", Role::Assistant, "msg2", None, None, 200).expect("append");
        store.append_message("ses-1", Role::User, "msg3", None, None, 50).expect("append");

        store.mark_messages_distilled("ses-1", &[1, 2]).expect("distill");
        store.insert_distillation_summary("ses-1", "[Distillation #1]\n\nSummary").expect("summary");

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 2, "summary + undistilled msg3");
        assert_eq!(history[0].role, Role::System);
        assert!(history[0].content.contains("Distillation #1"));
        assert_eq!(history[1].content, "msg3");
    }

    #[test]
    fn blackboard_crud() {
        let store = test_store();
        store.blackboard_write("goal", "finish M0b", "syn", 3600).expect("write");
        let entry = store.blackboard_read("goal").expect("read").unwrap();
        assert_eq!(entry.value, "finish M0b");
        assert_eq!(entry.author_nous_id, "syn");

        store.blackboard_write("goal", "updated goal", "syn", 3600).expect("overwrite");
        let updated = store.blackboard_read("goal").expect("read").unwrap();
        assert_eq!(updated.value, "updated goal");

        let deleted = store.blackboard_delete("goal", "syn").expect("delete");
        assert!(deleted);
        assert!(store.blackboard_read("goal").expect("read").is_none());
    }

    #[test]
    fn notes_crud() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.add_note("ses-1", "syn", "task", "do something").expect("add note");
        store.add_note("ses-1", "syn", "context", "background").expect("add note");

        let notes = store.get_notes("ses-1").expect("get notes");
        assert_eq!(notes.len(), 2);

        let note_id = notes[0].id;
        let deleted = store.delete_note(note_id).expect("delete note");
        assert!(deleted);
        let notes_after = store.get_notes("ses-1").expect("get notes after delete");
        assert_eq!(notes_after.len(), 1);
    }

    #[test]
    fn delete_session_removes_all_data() {
        let store = test_store();
        store.create_session("ses-1", "syn", "main", None, None).expect("create");
        store.append_message("ses-1", Role::User, "hi", None, None, 10).expect("append");

        let deleted = store.delete_session("ses-1").expect("delete");
        assert!(deleted);
        assert!(store.find_session_by_id("ses-1").expect("query").is_none());
        assert!(store.get_history("ses-1", None).expect("history").is_empty());
    }

    #[test]
    fn ping_succeeds() {
        let store = test_store();
        store.ping().expect("ping should succeed");
    }
}
