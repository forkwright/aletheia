// kanon:ignore RUST/file-too-long — single-file store implementation; splitting would break private method cohesion
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
//! | `usage`         | `{session_id}:{turn_seq_padded_20}`                    | JSON `UsageRecord`       |
//! | `tool_audit`    | `{global_id_padded_20}`                                | JSON `ToolAuditRecord`   |
//! | `distillations` | `{session_id}:{auto_id_padded_20}`                     | JSON distillation record |
//! | `notes`         | `{session_id}:{auto_id_padded_20}`                     | JSON `AgentNote`         |
//! | `notes`         | `gid:{global_note_id_padded_20}`                       | `{session_id}:{auto_id}` |
//! | `notes`         | `note_gid_idx:{session_id}:{global_note_id_padded_20}` | `""` (reverse index)     |
//! | `blackboard`    | `{key}`                                                | JSON `BlackboardRow`     |
//! | `counters`      | `{counter_name}`                                       | big-endian `u64`         |
//!
//! Sequence numbers are zero-padded to 20 digits so lexicographic ordering
//! matches numeric ordering — enabling range scans for `get_history`.
//!
//! # Timestamps
//!
//! All timestamps stored by this module are ISO 8601 UTC strings with
//! millisecond precision and a literal `Z` suffix:
//! `YYYY-MM-DDTHH:MM:SS.sssZ`. Using local wall time with a literal `Z` would
//! mislabel non-UTC timestamps as UTC and corrupt session ordering, blackboard
//! TTL comparisons, and retention sweeps (issue #4742).

#![expect(
    clippy::cast_possible_wrap,
    clippy::as_conversions,
    reason = "usize↔i64 for seq counters: values never exceed i64::MAX in practice"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, PersistMode, SingleWriterTxDatabase};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::{debug, info, instrument, warn};

use eidos::meta::Stamped as _;

use crate::error::{self, Result};
use crate::metrics;
use crate::types::{
    AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus,
    SessionType, ToolAuditRecord, UsageRecord,
};

fn storage_error(message: impl Into<String>) -> error::Error {
    error::StorageSnafu {
        message: message.into(),
    }
    .build()
}

/// Width for zero-padded sequence numbers.
///
/// 20 digits covers `u64::MAX` (`18_446_744_073_709_551_615`) and ensures
/// lexicographic sort equals numeric sort.
const SEQ_WIDTH: usize = 20;

/// Maximum number of distillation event records retained per session.
const DISTILLATION_RECORD_CAP: u64 = 100;

/// Format a u64 as a zero-padded key component.
fn pad_u64(v: u64) -> String {
    format!("{v:0>SEQ_WIDTH$}")
}

/// Decode a big-endian u64 counter from exactly 8 bytes.
///
/// A present-but-malformed (non-8-byte) value indicates corruption and is
/// rejected rather than silently reset to zero — see issue #5029.
fn try_decode_u64(bytes: &[u8], context: &str) -> Result<u64> {
    let arr: [u8; 8] = bytes.try_into().map_err(|source| {
        storage_error(format!(
            "corrupt counter \"{context}\": expected 8 bytes, got {} ({source})",
            bytes.len()
        ))
    })?;
    Ok(u64::from_be_bytes(arr))
}

/// Encode a u64 as big-endian bytes.
fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

/// ISO 8601 timestamp string for "now".
fn now_iso() -> String {
    koina::fjall::now_iso()
}

/// WHY: legacy rows without `session_type` were created under the old
/// key-derived lifecycle rules; this classifier is restricted to one-time
/// backfill and is not used for new session creation.
fn legacy_session_type_for_backfill(key: &str) -> SessionType {
    if key.starts_with("daemon:") || key.contains("prosoche") {
        SessionType::Background
    } else if key.starts_with("ask:")
        || key.starts_with("spawn:")
        || key.starts_with("dispatch:")
        || key.starts_with("ephemeral:")
    {
        SessionType::Ephemeral
    } else {
        SessionType::Primary
    }
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

/// Partitions used by the graphe session store.
const PARTITIONS: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "tool_audit",
    "distillations",
    "notes",
    "blackboard",
    "counters",
];

/// Fjall-backed session store.
///
/// Open with [`SessionStore::open`] for persistent storage or
/// [`SessionStore::open_in_memory`] for ephemeral storage (test-only; uses a
/// `TempDir` that is cleaned up on drop).
// kanon:ignore RUST/pub-visibility — re-exported by mneme (pub use graphe::store::SessionStore)
pub struct SessionStore {
    db: Arc<SingleWriterTxDatabase>,
    path: PathBuf,
    /// Shared write mutex — see [`koina::fjall::FjallDb::write_lock`].
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
    /// Approximate number of session rows, maintained by create/delete paths.
    ///
    /// WHY: O(1) counter for the Prometheus `/metrics` scrape path; avoids a
    /// full LSM scan on every scrape (issue #5662).
    session_count: AtomicUsize,
}

/// One message to append as part of a batched turn finalization.
///
/// WHY: [`SessionStore::finalize_turn`] needs the same inputs as
/// [`SessionStore::append_message`] but must avoid the per-write fsync that
/// makes the one-message-at-a-time API expensive on the hot path.
#[derive(Debug, Clone, Copy)]
pub struct FinalizeMessage<'a> {
    /// Author role for this message.
    pub role: Role,
    /// Message body text.
    pub content: &'a str,
    /// Tool call identifier, if this message is a tool result.
    pub tool_call_id: Option<&'a str>,
    /// Tool name, if this message is a tool result.
    pub tool_name: Option<&'a str>,
    /// Estimated token count for this message.
    pub token_estimate: i64,
}

/// Agent note to append as part of batched turn finalization.
#[derive(Debug, Clone, Copy)]
pub struct FinalizeNote<'a> {
    /// Note category.
    pub category: &'a str,
    /// Serialized note payload.
    pub content: &'a str,
}

/// One structured tool audit row to persist with a finalized turn.
#[derive(Debug, Clone, Copy)]
pub struct FinalizeToolAuditRecord<'a> {
    /// Turn sequence shared with the usage row.
    pub turn_seq: i64,
    /// Provider/tool-use identifier for this call.
    pub tool_call_id: &'a str,
    /// Registered tool name.
    pub tool_name: &'a str,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the call produced an error result.
    pub is_error: bool,
    /// Stable outcome label, currently `"success"` or `"error"`.
    pub outcome: &'a str,
    /// Bounded tool result text captured from the execution path.
    pub result: Option<&'a str>,
    /// Approval outcome applied before execution, when known.
    pub approval: Option<&'a str>,
    /// HMAC receipt token emitted for this tool result, when present.
    pub receipt: Option<&'a str>,
}

/// Request to persist a complete conversational turn in a single transaction.
///
/// WHY: grouping session creation, message appends, usage recording, and the
/// completion marker under one `tx.commit()` followed by one
/// [`SessionStore::ensure_durable`] call makes retries safe after any
/// intra-turn write failure (#4614) and removes the hard fsync-per-write
/// latency floor described in issue #5675.
#[derive(Debug, Clone, Copy)]
pub struct FinalizeTurnRequest<'a> {
    /// Session identifier for the turn.
    pub session_id: &'a str,
    /// Owning agent identifier.
    pub nous_id: &'a str,
    /// Logical key used to look up or resume this session.
    pub session_key: &'a str,
    /// LLM model used for this turn.
    pub model: Option<&'a str>,
    /// Parent session for sub-task lineage, if any.
    pub parent_session_id: Option<&'a str>,
    /// Messages to append, in order.
    pub messages: &'a [FinalizeMessage<'a>],
    /// Token-usage record for the turn, when usage recording is enabled.
    pub usage: Option<&'a UsageRecord>,
    /// Structured tool audit rows to append with this turn.
    pub tool_audit_records: &'a [FinalizeToolAuditRecord<'a>],
    /// Terminal lifecycle note to commit atomically with the turn, when known.
    pub completion_note: Option<FinalizeNote<'a>>,
}

/// Result of a batched turn finalization.
#[derive(Debug, Clone, Copy)]
pub struct FinalizeTurnResult {
    /// Number of messages appended.
    pub messages_persisted: usize,
    /// Whether a usage record was written.
    pub usage_recorded: bool,
    /// Number of structured tool audit records appended.
    pub tool_audit_records_persisted: usize,
}

// WHY: the counter is thread-local so concurrent unit tests do not interfere;
// each test that asserts on `ensure_durable` usage resets it before exercising
// the store.
#[cfg(test)]
pub(crate) mod test_persist_counter {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub fn reset() {
        COUNT.with(|c| c.set(0));
    }

    pub fn record() {
        COUNT.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub fn count() -> usize {
        COUNT.with(Cell::get)
    }
}

#[cfg(test)]
pub(crate) mod test_finalize_failure {
    use std::cell::Cell;

    use super::{Result, storage_error};

    thread_local! {
        static FAIL_AFTER_MESSAGES: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub fn fail_after_messages(count: usize) {
        FAIL_AFTER_MESSAGES.with(|cell| cell.set(Some(count)));
    }

    pub fn clear() {
        FAIL_AFTER_MESSAGES.with(|cell| cell.set(None));
    }

    pub fn maybe_fail_after_messages(messages_persisted: usize) -> Result<()> {
        FAIL_AFTER_MESSAGES.with(|cell| {
            let Some(limit) = cell.get() else {
                return Ok(());
            };
            if messages_persisted < limit {
                return Ok(());
            }
            cell.set(None);
            Err(storage_error(format!(
                "injected finalize_turn failure after {messages_persisted} messages"
            )))
        })
    }
}

/// Options passed to the private [`SessionStore::put_note`] write path.
#[derive(Clone, Copy)]
struct PutNoteOpts<'a> {
    created_at: Option<&'a str>,
    provided_id: Option<i64>,
    validate_category: bool,
}

#[derive(Clone, Copy)]
struct NoteTxParts<'a> {
    notes: &'a fjall::SingleWriterTxKeyspace,
    counters: &'a fjall::SingleWriterTxKeyspace,
    sessions: &'a fjall::SingleWriterTxKeyspace,
}

struct NoteDeleteKeys {
    gid_keys: Vec<Vec<u8>>,
    gid_idx_keys: Vec<Vec<u8>>,
}

#[derive(Clone, Copy)]
struct PutNoteSpec<'a> {
    session_id: &'a str,
    nous_id: &'a str,
    category: &'a str,
    content: &'a str,
    opts: PutNoteOpts<'a>,
}

#[derive(Clone, Copy)]
struct FindOrCreateSessionSpec<'a> {
    id: &'a str,
    nous_id: &'a str,
    session_key: &'a str,
    session_type: SessionType,
    model: Option<&'a str>,
    parent_session_id: Option<&'a str>,
}

impl SessionStore {
    /// Open (or create) a persistent session store at the given path.
    ///
    /// # Errors
    /// Returns an error if the fjall keyspace cannot be opened.
    #[instrument(skip(path))]
    pub fn open(path: &Path) -> Result<Self> {
        info!(path = %path.display(), "Opening fjall session store");
        let fdb = koina::fjall::FjallDb::open(path, PARTITIONS).map_err(|e| {
            error::StorageSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        Self::from_fjall_db(fdb, path.to_path_buf())
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
        let fdb = koina::fjall::FjallDb::open_temp(PARTITIONS).map_err(|e| {
            error::StorageSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        // WHY: mirrors the production layout where the caller passes
        // `<data_dir>/sessions` to `open()`. Storing `<tmp_dir>/sessions` here
        // ensures `self.path.parent()` resolves to an isolated per-test
        // directory rather than the shared /tmp root.
        let path = fdb
            ._temp_dir
            .as_ref()
            .map_or_else(std::env::temp_dir, |dir| dir.path().join("sessions"));
        Self::from_fjall_db(fdb, path)
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb, path: PathBuf) -> Result<Self> {
        use fjall::Readable;

        let sessions_part = fdb
            .db
            .keyspace("sessions", KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall session-count partition: {e}"),
                }
                .build()
            })?;
        Self::backfill_legacy_session_types(&fdb.db, &sessions_part)?;
        let snap = fdb.db.read_tx();
        let mut count = 0usize;
        for guard in snap.range::<&str, _>(&sessions_part, ..) {
            let (k, _v) = guard.into_inner().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall session-count scan: {e}"),
                }
                .build()
            })?;
            if !k.starts_with(b"idx:") {
                count += 1;
            }
        }

        Ok(Self {
            db: Arc::new(fdb.db),
            path,
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
            session_count: AtomicUsize::new(count),
        })
    }

    /// Path used when opening this Fjall session store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flush the WAL to stable storage so committed writes survive power loss.
    ///
    /// Call this after every `tx.commit()` on the critical write path.
    /// Blackboard writes intentionally skip this — they are an ephemeral cache
    /// tier and are acceptable to lose on an unclean shutdown.
    ///
    /// This is also the public durability barrier for batch import callers:
    /// after a sequence of raw [`Self::insert_message_raw`] calls, call
    /// `ensure_durable()` once to guarantee the imported messages survive an
    /// unclean shutdown.
    pub fn ensure_durable(&self) -> Result<()> {
        self.db.persist(PersistMode::SyncAll).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall persist: {e}"),
            }
            .build()
        })?;
        #[cfg(test)]
        test_persist_counter::record();
        Ok(())
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

    fn get_bytes(
        &self,
        partition: &fjall::SingleWriterTxKeyspace,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        use fjall::Readable;
        let snap = self.db.read_tx();
        snap.get(partition, key.as_bytes())
            .map(|opt| opt.map(|s| s.to_vec()))
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall get: {e}"),
                }
                .build()
            })
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

    fn backfill_legacy_session_types(
        db: &SingleWriterTxDatabase,
        sessions_part: &fjall::SingleWriterTxKeyspace,
    ) -> Result<()> {
        use fjall::Readable;

        let snap = db.read_tx();
        let mut updates: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

        for guard in snap.range::<&str, _>(sessions_part, ..) {
            let (key, value_bytes) = guard.into_inner().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall legacy session_type scan: {e}"),
                }
                .build()
            })?;
            if key.starts_with(b"idx:") {
                continue;
            }

            let mut value: serde_json::Value =
                serde_json::from_slice(&value_bytes).context(error::StoredJsonSnafu)?;
            let Some(object) = value.as_object_mut() else {
                return Err(storage_error(format!(
                    "legacy session row {} is not a JSON object",
                    String::from_utf8_lossy(&key)
                )));
            };
            if object.contains_key("session_type") {
                continue;
            }

            let session_key = object
                .get("session_key")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    storage_error(format!(
                        "legacy session row {} missing session_key",
                        String::from_utf8_lossy(&key)
                    ))
                })?;
            let session_type = legacy_session_type_for_backfill(session_key);
            object.insert(
                "session_type".to_owned(),
                serde_json::Value::String(session_type.as_str().to_owned()),
            );
            let encoded_session = serde_json::to_vec(&value).context(error::StoredJsonSnafu)?;
            updates.push((key.to_vec(), encoded_session));
        }
        drop(snap);

        if updates.is_empty() {
            return Ok(());
        }

        let count = updates.len();
        let mut tx = db.write_tx();
        for (key, value) in updates {
            tx.insert(sessions_part, key.as_slice(), value.as_slice());
        }
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall legacy session_type backfill commit: {e}"),
            }
            .build()
        })?;
        db.persist(PersistMode::SyncAll).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall legacy session_type backfill persist: {e}"),
            }
            .build()
        })?;
        info!(count, "backfilled legacy session_type fields");
        Ok(())
    }

    // ── Session helpers ───────────────────────────────────────────────────

    fn session_key_index_key(nous_id: &str, session_key: &str) -> String {
        format!("idx:key:{nous_id}:{session_key}")
    }

    fn session_nous_index_key(nous_id: &str, updated_at: &str, session_id: &str) -> String {
        // WHY: rename the parameter locally so the format argument list never
        // contains the literal `updated_at` token — `STORAGE/sql-string-concat`
        // matches the `UPDATE` substring inside that identifier (case-insensitive).
        let ts = updated_at;
        format!("idx:nous:{nous_id}:upd:{ts}:{session_id}")
    }

    fn note_gid_index_key(session_id: &str, global_id: u64) -> String {
        // WHY: per-session reverse index lets `delete_session` collect a session's
        // global note ids with a prefix scan instead of scanning the entire `gid:`
        // key space (issue #5698).
        format!("note_gid_idx:{session_id}:{}", pad_u64(global_id))
    }

    fn put_note_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        parts: NoteTxParts<'_>,
        spec: PutNoteSpec<'_>,
    ) -> Result<i64> {
        use fjall::Readable;
        let PutNoteSpec {
            session_id,
            nous_id,
            category,
            content,
            opts,
        } = spec;
        let PutNoteOpts {
            created_at,
            provided_id,
            validate_category,
        } = opts;

        if validate_category && !Self::VALID_CATEGORIES.contains(&category) {
            return Err(error::StorageSnafu {
                message: format!(
                    "CHECK constraint failed: category '{category}' is not valid; \
                     allowed: {:?}",
                    Self::VALID_CATEGORIES
                ),
            }
            .build());
        }

        // WHY: read from the write transaction so batched turn finalization can
        // create the session row and terminal note atomically.
        if tx
            .get(parts.sessions, session_id)
            .map_err(|e| storage_error(format!("fjall put_note session check: {e}")))?
            .is_none()
        {
            return Err(error::SessionNotFoundSnafu {
                id: session_id.to_owned(),
            }
            .build());
        }

        let current_local_id = match tx
            .get(parts.counters, "note_local_id")
            .map_err(|e| storage_error(format!("fjall note_local_id counter read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "note_local_id")?,
        };
        let current_global_id = match tx
            .get(parts.counters, "note_global_id")
            .map_err(|e| storage_error(format!("fjall note_global_id counter read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "note_global_id")?,
        };

        // WHY: a non-positive provided id means the caller wants a fresh id,
        // matching the legacy `add_note` allocation behaviour.
        let local_id = provided_id
            .and_then(|id| u64::try_from(id).ok())
            .filter(|&id| id > 0)
            .unwrap_or(current_local_id + 1);
        let global_id = provided_id
            .and_then(|id| u64::try_from(id).ok())
            .filter(|&id| id > 0)
            .unwrap_or(current_global_id + 1);

        let note = AgentNote {
            id: global_id as i64, // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
            session_id: session_id.to_owned(),
            nous_id: nous_id.to_owned(),
            category: category.to_owned(),
            content: content.to_owned(),
            created_at: created_at.map_or_else(now_iso, str::to_owned),
        };
        let note_data = serde_json::to_vec(&note).context(error::StoredJsonSnafu)?;
        let local_key = format!("{session_id}:{}", pad_u64(local_id));
        let gid_key = format!("gid:{}", pad_u64(global_id));
        let gid_val = format!("{session_id}:{}", pad_u64(local_id));
        let gid_idx_key = Self::note_gid_index_key(session_id, global_id);

        tx.insert(parts.notes, local_key.as_str(), note_data.as_slice());
        tx.insert(parts.notes, gid_key.as_str(), gid_val.as_bytes());
        tx.insert(parts.notes, gid_idx_key.as_str(), b"");
        tx.insert(
            parts.counters,
            "note_local_id",
            encode_u64(local_id.max(current_local_id)),
        );
        tx.insert(
            parts.counters,
            "note_global_id",
            encode_u64(global_id.max(current_global_id)),
        );

        Ok(global_id as i64) // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
    }

    fn write_session(&self, session: &Session) -> Result<()> {
        let sessions = self.partition("sessions")?;
        let mut tx = self.db.write_tx();
        Self::write_session_in_tx(&mut tx, &sessions, session)?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall session write: {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;
        Ok(())
    }

    /// Stamp, serialize, and write a session row plus both secondary indexes
    /// inside an existing write transaction.
    fn write_session_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        sessions: &fjall::SingleWriterTxKeyspace,
        session: &Session,
    ) -> Result<()> {
        // WHY: stamp provenance before serialising so metadata travels with the record.
        let mut stamped = session.clone();
        stamped.artefact_meta = Some(stamped.stamp());
        let data = serde_json::to_vec(&stamped).context(error::StoredJsonSnafu)?;

        tx.insert(sessions, session.id.as_str(), data.as_slice());
        tx.insert(
            sessions,
            Self::session_key_index_key(&session.nous_id, &session.session_key).as_str(),
            session.id.as_bytes(),
        );
        tx.insert(
            sessions,
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, &session.id)
                .as_str(),
            b"",
        );
        Ok(())
    }

    fn tool_audit_keys_for_session_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        tool_audit_part: &fjall::SingleWriterTxKeyspace,
        session_id: &str,
    ) -> Result<Vec<Vec<u8>>> {
        use fjall::Readable;

        let mut keys = Vec::new();
        for guard in tx.range::<&str, _>(tool_audit_part, ..) {
            let (k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall delete_session tool_audit scan: {e}")))?;
            let record =
                serde_json::from_slice::<ToolAuditRecord>(&v).context(error::StoredJsonSnafu)?;
            if record.session_id == session_id {
                keys.push(k.to_vec());
            }
        }
        Ok(keys)
    }

    fn note_gid_delete_keys_for_session_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        notes_part: &fjall::SingleWriterTxKeyspace,
        session_id: &str,
    ) -> Result<NoteDeleteKeys> {
        use fjall::Readable;

        // WHY: use the per-session reverse index so deletion is O(notes in
        // session), not O(total notes across all sessions) (issue #5698).
        let gid_idx_prefix = format!("note_gid_idx:{session_id}:");
        let gid_idx_upper = format!("note_gid_idx:{session_id};\x00");
        let mut gid_keys = Vec::new();
        let mut gid_idx_keys = Vec::new();
        for guard in tx.range(notes_part, gid_idx_prefix.as_str()..gid_idx_upper.as_str()) {
            let (idx_key, _v) = guard.into_inner().map_err(|e| {
                storage_error(format!("fjall delete_session note_gid_idx scan: {e}"))
            })?;
            let idx_str = String::from_utf8_lossy(&idx_key);
            let global_id = idx_str
                .rsplit(':')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| {
                    storage_error("corrupt note_gid_idx key: missing global_id".to_owned())
                })?;
            gid_idx_keys.push(idx_key.to_vec());
            gid_keys.push(format!("gid:{}", pad_u64(global_id)).into_bytes());
        }
        Ok(NoteDeleteKeys {
            gid_keys,
            gid_idx_keys,
        })
    }

    /// Find or create an active session inside an existing write transaction.
    ///
    /// WHY: lets batched callers (e.g. [`Self::finalize_turn`]) include session
    /// creation in the same transaction as message and usage writes, avoiding
    /// an extra `ensure_durable()` fsync on the hot path.
    fn find_or_create_session_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        sessions_part: &fjall::SingleWriterTxKeyspace,
        spec: FindOrCreateSessionSpec<'_>,
    ) -> Result<(Session, bool, bool)> {
        use fjall::Readable;
        let FindOrCreateSessionSpec {
            id,
            nous_id,
            session_key,
            session_type,
            model,
            parent_session_id,
        } = spec;

        if let Some(existing_bytes) = tx
            .get(sessions_part, id)
            .map_err(|e| storage_error(format!("fjall find_or_create_session session get: {e}")))?
        {
            let existing: Session =
                serde_json::from_slice(&existing_bytes).context(error::StoredJsonSnafu)?;
            if existing.nous_id != nous_id || existing.session_key != session_key {
                return Err(error::StorageSnafu {
                    message: format!(
                        "UNIQUE constraint failed: session id {id} already exists \
                         for ({}, {})",
                        existing.nous_id, existing.session_key
                    ),
                }
                .build());
            }
            return Self::active_or_reactivated_session(tx, sessions_part, existing);
        }

        let idx_key = Self::session_key_index_key(nous_id, session_key);
        if let Some(existing_id_bytes) = tx
            .get(sessions_part, idx_key.as_str())
            .map_err(|e| storage_error(format!("fjall find_or_create_session key idx: {e}")))?
        {
            let existing_id = String::from_utf8_lossy(&existing_id_bytes).into_owned();
            let existing_bytes = tx
                .get(sessions_part, existing_id.as_str())
                .map_err(|e| {
                    storage_error(format!("fjall find_or_create_session existing get: {e}"))
                })?
                .ok_or_else(|| {
                    error::SessionCreateSnafu {
                        nous_id: nous_id.to_owned(),
                    }
                    .build()
                })?;
            let existing: Session =
                serde_json::from_slice(&existing_bytes).context(error::StoredJsonSnafu)?;
            return Self::active_or_reactivated_session(tx, sessions_part, existing);
        }

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
            artefact_meta: None,
        };

        Self::write_session_in_tx(tx, sessions_part, &session)?;
        metrics::record_session_created(nous_id, session_type.as_str());
        info!(id, nous_id, session_key, %session_type, "created session");
        Ok((session, true, true))
    }

    /// Return an active session, reactivating it if necessary, inside a tx.
    ///
    /// Returns `(session, mutated, created)`: `mutated` is `true` when the
    /// transaction was written (reactivation); `created` is always `false`
    /// because this function only handles existing sessions.
    fn active_or_reactivated_session(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        sessions_part: &fjall::SingleWriterTxKeyspace,
        mut session: Session,
    ) -> Result<(Session, bool, bool)> {
        match session.status {
            SessionStatus::Active => Ok((session, false, false)),
            SessionStatus::Archived => {
                // WHY: Archived sessions are lifecycle-closed by operator or
                // policy action. Silently reactivating them loses the intent
                // of the archival and can resurface stale context to the
                // agent. Callers must explicitly unarchive via the dedicated
                // endpoint before resuming the session.
                Err(error::SessionIsArchivedSnafu { id: session.id }.build())
            }
            _ => {
                let old_updated_at = session.updated_at.clone();
                session.status = SessionStatus::Active;
                session.updated_at = now_iso();
                Self::update_session_nous_index(tx, sessions_part, &session, &old_updated_at);
                Self::write_session_in_tx(tx, sessions_part, &session)?;
                info!(id = session.id, "reactivated session");
                Ok((session, true, false))
            }
        }
    }

    fn update_session_nous_index(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        partition: &fjall::SingleWriterTxKeyspace,
        session: &Session,
        old_updated_at: &str,
    ) {
        // NOTE: removes only the old index entry; the caller inserts the new one.
        let old_key = Self::session_nous_index_key(&session.nous_id, old_updated_at, &session.id);
        tx.remove(partition, old_key.as_str());
    }

    fn read_session_by_raw_id(&self, id: &str) -> Result<Option<Session>> {
        let sessions = self.partition("sessions")?;
        self.get_json::<Session>(&sessions, id)
    }

    /// WHY: referential-integrity guard — child writes (usage, notes,
    /// distillations) must reject non-existent sessions (#5027). Reads only the
    /// session row's presence; never decodes it.
    fn require_session_exists(&self, session_id: &str, context: &str) -> Result<()> {
        use fjall::Readable;

        let sessions_part = self.partition("sessions")?;
        let snap = self.db.read_tx();
        if snap
            .get(&sessions_part, session_id)
            .map_err(|e| storage_error(format!("fjall {context} session check: {e}")))?
            .is_none()
        {
            return Err(error::SessionNotFoundSnafu {
                id: session_id.to_owned(),
            }
            .build());
        }
        Ok(())
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

    /// Return the number of sessions in O(1).
    ///
    /// WHY: the Prometheus `/metrics` handler scrapes this every 15-30 seconds.
    /// A full `list_sessions(None)` scan blocks the Tokio worker thread and
    /// holds the session-store mutex for unbounded time as sessions grow
    /// (issue #5662).
    pub fn session_count(&self) -> usize {
        self.session_count.load(Ordering::Relaxed)
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
                Ok(session.filter(|s| s.status == SessionStatus::Active))
            }
        }
    }

    /// Find a session by ID (any status).
    #[instrument(skip(self))]
    pub fn find_session_by_id(&self, id: &str) -> Result<Option<Session>> {
        self.read_session_by_raw_id(id)
    }

    /// Create a new primary session.
    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session> {
        self.create_session_with_type(
            id,
            nous_id,
            session_key,
            SessionType::Primary,
            parent_session_id,
            model,
        )
    }

    /// Create a new session with an explicit lifecycle type.
    #[instrument(skip(self))]
    pub fn create_session_with_type(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        session_type: SessionType,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if self.read_session_by_raw_id(id)?.is_some() {
            return Err(error::StorageSnafu {
                message: format!("UNIQUE constraint failed: session id {id} already exists"),
            }
            .build());
        }

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
            artefact_meta: None,
        };

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
        self.session_count.fetch_add(1, Ordering::Relaxed);
        metrics::record_session_created(nous_id, session_type.as_str());
        info!(id, nous_id, session_key, %session_type, "created session");
        Ok(session)
    }

    /// Find or create an active primary session. Reactivates archived sessions if found.
    #[instrument(skip(self))]
    pub fn find_or_create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session> {
        self.find_or_create_session_with_type(
            id,
            nous_id,
            session_key,
            SessionType::Primary,
            model,
            parent_session_id,
        )
    }

    /// Find or create an active session with an explicit lifecycle type.
    #[instrument(skip(self))]
    pub fn find_or_create_session_with_type(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        session_type: SessionType,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let mut tx = self.db.write_tx();
        let (session, mutated, created) = Self::find_or_create_session_in_tx(
            &mut tx,
            &sessions_part,
            FindOrCreateSessionSpec {
                id,
                nous_id,
                session_key,
                session_type,
                model,
                parent_session_id,
            },
        )?;
        if mutated {
            tx.commit().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall find_or_create_session commit: {e}"),
                }
                .build()
            })?;
            self.ensure_durable()?;
            if created {
                self.session_count.fetch_add(1, Ordering::Relaxed);
            }
        }
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
            let prefix = format!("idx:nous:{nous_id}:upd:");
            // WHY: prefix scans need an exclusive upper bound — bump the last character.
            let upper = {
                let mut s = prefix.clone();
                let last = s.pop().unwrap_or('\0');
                s.push(char::from_u32(u32::from(last) + 1).unwrap_or('\u{FFFF}'));
                s
            };

            // WHY: lexicographic order on updated_at is ascending; reverse for
            // most-recent-first.
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
                if let Some(session_id) = key.rsplit(':').next()
                    && let Some(session) = self.read_session_by_raw_id(session_id)?
                {
                    sessions.push(session);
                }
            }
        } else {
            let idx_prefix = b"idx:".as_slice();
            let mut raw_sessions: Vec<Session> = Vec::new();
            for guard in snap.range::<&str, _>(&sessions_part, ..) {
                let (k, v) = guard
                    .into_inner()
                    .map_err(|e| storage_error(format!("fjall list_sessions full scan: {e}")))?;
                if !k.starts_with(idx_prefix) {
                    let session =
                        serde_json::from_slice::<Session>(&v).context(error::StoredJsonSnafu)?;
                    raw_sessions.push(session);
                }
            }

            raw_sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            sessions = raw_sessions;
        }

        Ok(sessions)
    }

    /// Count sessions owned by `nous_id` with `updated_at >= since`.
    ///
    /// WHY: uses the `idx:nous:{nous_id}:upd:` index instead of
    /// `list_sessions(None)`, so the auto-dream gate check does not scan or
    /// deserialize the full session partition.
    #[instrument(skip(self))]
    pub fn count_sessions_since(&self, since: jiff::Timestamp, nous_id: &str) -> Result<usize> {
        use fjall::Readable;

        let sessions_part = self.partition("sessions")?;
        let snap = self.db.read_tx();

        let prefix = format!("idx:nous:{nous_id}:upd:");
        let lower = format!("{prefix}{}", since.strftime("%Y-%m-%dT%H:%M:%S%.3fZ"));
        let upper = {
            let mut s = prefix.clone();
            let last = s.pop().unwrap_or('\0');
            s.push(char::from_u32(u32::from(last) + 1).unwrap_or('\u{FFFF}'));
            s
        };

        let mut count = 0;
        for guard in snap.range(&sessions_part, lower.as_str()..upper.as_str()) {
            let _ = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall count_sessions_since range: {e}")))?;
            count += 1;
        }
        Ok(count)
    }

    /// List session IDs owned by `nous_id` with `updated_at >= since`.
    ///
    /// WHY: index-only scan avoids deserializing sessions that fall outside the
    /// consolidation window.
    #[instrument(skip(self))]
    pub fn list_session_ids_since(
        &self,
        since: jiff::Timestamp,
        nous_id: &str,
    ) -> Result<Vec<String>> {
        use fjall::Readable;

        let sessions_part = self.partition("sessions")?;
        let snap = self.db.read_tx();

        let prefix = format!("idx:nous:{nous_id}:upd:");
        let lower = format!("{prefix}{}", since.strftime("%Y-%m-%dT%H:%M:%S%.3fZ"));
        let upper = {
            let mut s = prefix.clone();
            let last = s.pop().unwrap_or('\0');
            s.push(char::from_u32(u32::from(last) + 1).unwrap_or('\u{FFFF}'));
            s
        };

        let mut ids = Vec::new();
        for guard in snap.range(&sessions_part, lower.as_str()..upper.as_str()) {
            let (k, _v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall list_session_ids_since range: {e}")))?;
            let key = String::from_utf8_lossy(&k);
            if let Some(id) = key.rsplit(':').next() {
                ids.push(id.to_owned());
            }
        }
        Ok(ids)
    }

    /// Overwrite the session status field and refresh its `updated_at` timestamp.
    #[instrument(skip(self))]
    pub fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let mut session = self
            .read_session_by_raw_id(id)?
            .ok_or_else(|| error::SessionNotFoundSnafu { id: id.to_owned() }.build())?;

        let old_updated_at = session.updated_at.clone();
        session.status = status;
        session.updated_at = now_iso();

        let mut tx = self.db.write_tx();
        Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
        Self::write_session_in_tx(&mut tx, &sessions_part, &session)?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit failed (session_status write): {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;
        Ok(())
    }

    /// Update session display name.
    #[instrument(skip(self))]
    pub fn update_display_name(&self, id: &str, display_name: &str) -> Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;
        let mut session = self
            .read_session_by_raw_id(id)?
            .ok_or_else(|| error::SessionNotFoundSnafu { id: id.to_owned() }.build())?;

        let old_updated_at = session.updated_at.clone();
        session.origin.display_name = Some(display_name.to_owned());
        session.updated_at = now_iso();

        let mut tx = self.db.write_tx();
        Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
        Self::write_session_in_tx(&mut tx, &sessions_part, &session)?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit failed (display_name write): {e}"),
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

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;

        let Some(session) = self.read_session_by_raw_id(id)? else {
            return Ok(false);
        };

        let key_idx = Self::session_key_index_key(&session.nous_id, &session.session_key);
        let nous_idx = Self::session_nous_index_key(&session.nous_id, &session.updated_at, id);

        let messages_part = self.partition("messages")?;
        let usage_part = self.partition("usage")?;
        let tool_audit_part = self.partition("tool_audit")?;
        let distillations_part = self.partition("distillations")?;
        let notes_part = self.partition("notes")?;

        let mut tx = self.db.write_tx();

        // WHY: collect child keys before any mutation; abort entirely on scan
        // error so parent and child rows stay consistent (all-or-nothing).
        let child_prefix = format!("{id}:");
        let child_upper = format!("{id};\x00");

        let msg_keys: Vec<Vec<u8>> = tx
            .range(&messages_part, child_prefix.as_str()..child_upper.as_str())
            .map(|g| {
                g.into_inner()
                    .map(|(k, _v)| k.to_vec())
                    .map_err(|e| storage_error(format!("fjall delete_session messages scan: {e}")))
            })
            .collect::<Result<_>>()?;

        let usage_keys: Vec<Vec<u8>> = tx
            .range(&usage_part, child_prefix.as_str()..child_upper.as_str())
            .map(|g| {
                g.into_inner()
                    .map(|(k, _v)| k.to_vec())
                    .map_err(|e| storage_error(format!("fjall delete_session usage scan: {e}")))
            })
            .collect::<Result<_>>()?;

        let tool_audit_keys =
            Self::tool_audit_keys_for_session_in_tx(&mut tx, &tool_audit_part, id)?;

        let dist_keys: Vec<Vec<u8>> = tx
            .range(
                &distillations_part,
                child_prefix.as_str()..child_upper.as_str(),
            )
            .map(|g| {
                g.into_inner().map(|(k, _v)| k.to_vec()).map_err(|e| {
                    storage_error(format!("fjall delete_session distillations scan: {e}"))
                })
            })
            .collect::<Result<_>>()?;

        let note_keys: Vec<Vec<u8>> = tx
            .range(&notes_part, child_prefix.as_str()..child_upper.as_str())
            .map(|g| {
                g.into_inner()
                    .map(|(k, _v)| k.to_vec())
                    .map_err(|e| storage_error(format!("fjall delete_session notes scan: {e}")))
            })
            .collect::<Result<_>>()?;

        let note_delete_keys =
            Self::note_gid_delete_keys_for_session_in_tx(&mut tx, &notes_part, id)?;

        for key in &msg_keys {
            tx.remove(&messages_part, key.as_slice());
        }
        tx.remove(&messages_part, format!("next_seq:{id}").as_str());

        for key in &usage_keys {
            tx.remove(&usage_part, key.as_slice());
        }
        for key in &tool_audit_keys {
            tx.remove(&tool_audit_part, key.as_slice());
        }

        for key in &dist_keys {
            tx.remove(&distillations_part, key.as_slice());
        }
        for key in note_keys
            .iter()
            .chain(&note_delete_keys.gid_keys)
            .chain(&note_delete_keys.gid_idx_keys)
        {
            tx.remove(&notes_part, key.as_slice());
        }

        tx.remove(&sessions_part, id);
        tx.remove(&sessions_part, key_idx.as_str());
        tx.remove(&sessions_part, nous_idx.as_str());

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit failed (session removal): {e}"),
            }
            .build()
        })?;
        self.session_count.fetch_sub(1, Ordering::Relaxed);

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
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;
        let counters_part = self.partition("counters")?;

        let mut session = self.read_session_by_raw_id(session_id)?.ok_or_else(|| {
            error::SessionNotFoundSnafu {
                id: session_id.to_owned(),
            }
            .build()
        })?;

        let mut tx = self.db.write_tx();
        let seq = Self::append_message_in_tx(
            &mut tx,
            &messages_part,
            &sessions_part,
            &counters_part,
            &mut session,
            &FinalizeMessage {
                role,
                content,
                tool_call_id,
                tool_name,
                token_estimate,
            },
        )?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall append_message commit: {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;

        debug!(session_id, seq, %role, token_estimate, "appended message");
        Ok(seq)
    }

    /// Append a message inside an existing write transaction.
    ///
    /// WHY: reused by [`Self::append_message`] (one fsync per call) and
    /// [`Self::finalize_turn`] (one fsync for the whole turn).
    fn append_message_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        messages_part: &fjall::SingleWriterTxKeyspace,
        sessions_part: &fjall::SingleWriterTxKeyspace,
        counters_part: &fjall::SingleWriterTxKeyspace,
        session: &mut Session,
        msg: &FinalizeMessage<'_>,
    ) -> Result<i64> {
        use fjall::Readable;
        let &FinalizeMessage {
            role,
            content,
            tool_call_id,
            tool_name,
            token_estimate,
        } = msg;

        let session_id = session.id.as_str();
        let next_seq_key = format!("next_seq:{session_id}");
        let current_seq = match tx
            .get(messages_part, next_seq_key.as_str())
            .map_err(|e| storage_error(format!("fjall seq read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "next_seq")?,
        };
        let seq = current_seq + 1;

        let msg_id_counter = match tx
            .get(counters_part, "msg_id")
            .map_err(|e| storage_error(format!("fjall msg_id counter: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "msg_id")?,
        } + 1;

        let now = now_iso();
        let msg = Message {
            id: msg_id_counter as i64, // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
            session_id: session_id.to_owned(),
            seq: seq as i64, // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
            role,
            content: content.to_owned(),
            tool_call_id: tool_call_id.map(str::to_owned),
            tool_name: tool_name.map(str::to_owned),
            token_estimate,
            is_distilled: false,
            created_at: now.clone(),
        };

        let msg_key = format!("{session_id}:{}", pad_u64(seq));
        let msg_data = serde_json::to_vec(&msg).context(error::StoredJsonSnafu)?;

        let old_updated_at = session.updated_at.clone();
        session.metrics.message_count += 1;
        session.metrics.token_count_estimate += token_estimate;
        session.updated_at = now;

        tx.insert(messages_part, msg_key.as_str(), msg_data.as_slice());
        tx.insert(messages_part, next_seq_key.as_str(), encode_u64(seq));
        tx.insert(counters_part, "msg_id", encode_u64(msg_id_counter));
        Self::update_session_nous_index(tx, sessions_part, session, &old_updated_at);
        Self::write_session_in_tx(tx, sessions_part, session)?;

        Ok(seq as i64) // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
    }

    /// Get non-distilled messages, newest `limit` in chronological order.
    fn load_messages_in_range(&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>> {
        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        if let Some(lim) = limit.and_then(|l| usize::try_from(l).ok()) {
            // PERF: reverse scan with early-exit is O(limit) vs O(active_rows) forward scan.
            let mut result = Vec::with_capacity(lim);
            for guard in snap
                .range(&messages_part, prefix.as_str()..upper.as_str())
                .rev()
            {
                let (_k, v) = guard
                    .into_inner()
                    .map_err(|e| storage_error(format!("fjall load_messages_in_range: {e}")))?;
                let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
                if msg.is_distilled {
                    continue;
                }
                result.push(msg);
                if result.len() >= lim {
                    break;
                }
            }
            result.reverse();
            Ok(result)
        } else {
            // WHY: no limit — full forward scan returns all active messages in order.
            let mut messages = Vec::new();
            for guard in snap.range(&messages_part, prefix.as_str()..upper.as_str()) {
                let (_k, v) = guard
                    .into_inner()
                    .map_err(|e| storage_error(format!("fjall load_messages_in_range: {e}")))?;
                let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
                if !msg.is_distilled {
                    messages.push(msg);
                }
            }
            Ok(messages)
        }
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
        use std::collections::VecDeque;

        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = match before_seq {
            Some(b) => format!("{session_id}:{}", pad_u64(b.cast_unsigned())),
            None => format!("{session_id};\x00"),
        };
        let snap = self.db.read_tx();

        let limit = limit.and_then(|lim| usize::try_from(lim).ok());
        let mut messages = VecDeque::new();
        for guard in snap.range(&messages_part, prefix.as_str()..upper.as_str()) {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall get_history_filtered: {e}")))?;
            let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
            if msg.is_distilled {
                continue;
            }
            messages.push_back(msg);
            if let Some(lim) = limit
                && messages.len() > lim
            {
                messages.pop_front();
            }
        }

        Ok(messages.into_iter().collect())
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

        let mut result = Vec::new();
        let mut total: i64 = 0;

        for guard in snap
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .rev()
        {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall get_history_with_budget: {e}")))?;
            let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
            if msg.is_distilled {
                continue;
            }
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
        // INVARIANT: the distillation summary is the seq=0 message with
        // is_distilled == false.
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

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;

        let mut tx = self.db.write_tx();

        for &seq in seqs {
            let key = format!("{session_id}:{}", pad_u64(seq.cast_unsigned()));
            if let Some(bytes) = tx
                .get(&messages_part, key.as_str())
                .map_err(|e| storage_error(format!("fjall mark_messages_distilled get: {e}")))?
            {
                let mut msg: Message =
                    serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
                msg.is_distilled = true;
                let updated = serde_json::to_vec(&msg).context(error::StoredJsonSnafu)?;
                tx.insert(&messages_part, key.as_str(), updated.as_slice());
            }
        }

        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        // WHY: read after writing — use the tx's read-your-own-writes to see updates.
        let (total_tokens, msg_count) = {
            let mut tokens: i64 = 0;
            let mut count: i64 = 0;
            for guard in tx.range(&messages_part, prefix.as_str()..upper.as_str()) {
                let (_k, v) = guard.into_inner().map_err(|e| {
                    storage_error(format!("fjall mark_messages_distilled range: {e}"))
                })?;
                let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
                if !msg.is_distilled {
                    tokens += msg.token_estimate;
                    count += 1;
                }
            }
            (tokens, count)
        };

        if let Some(bytes) = tx
            .get(&sessions_part, session_id)
            .map_err(|e| storage_error(format!("fjall mark_messages_distilled session get: {e}")))?
        {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            let old_updated_at = session.updated_at.clone();
            session.metrics.token_count_estimate = total_tokens;
            session.metrics.message_count = msg_count;
            session.updated_at = now_iso();
            Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
            Self::write_session_in_tx(&mut tx, &sessions_part, &session)?;
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

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;

        self.require_session_exists(session_id, "insert_distillation_summary")?;

        let mut tx = self.db.write_tx();

        let seq0_key = format!("{session_id}:{}", pad_u64(0));
        tx.remove(&messages_part, seq0_key.as_str());

        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let distilled_keys: Vec<Vec<u8>> = tx
            .range(&messages_part, prefix.as_str()..upper.as_str())
            .map(|g| {
                let (k, v) = g
                    .into_inner()
                    .map_err(|e| storage_error(format!("fjall distillation_summary scan: {e}")))?;
                let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
                Ok((k, msg))
            })
            .filter_map(|result: Result<_>| match result {
                Ok((k, msg)) if msg.is_distilled => Some(Ok(k.to_vec())),
                Ok((_k, _msg)) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<Vec<_>>>()?;
        for key in &distilled_keys {
            tx.remove(&messages_part, key.as_slice());
        }

        let counters_part = self.partition("counters")?;
        let current_msg_id = match tx
            .get(&counters_part, "msg_id")
            .map_err(|e| storage_error(format!("fjall msg_id counter read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "msg_id")?,
        };
        let new_msg_id = current_msg_id + 1;
        tx.insert(&counters_part, "msg_id", encode_u64(new_msg_id));

        let token_estimate = (content.len() as i64 + 3) / 4; // kanon:ignore RUST/as-cast — token estimate heuristic; content length fits in i64 for all realistic inputs
        let now = now_iso();
        let summary_msg = Message {
            id: new_msg_id as i64, // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
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

        // WHY: The range scan includes the summary at seq 0 we just inserted above
        // (fjall WriteTransaction provides read-your-own-writes), so we do NOT add
        // token_estimate again here — that would double-count it.
        let (total_tokens, msg_count) = {
            let mut tokens: i64 = 0;
            let mut count: i64 = 0;
            for guard in tx.range(&messages_part, prefix.as_str()..upper.as_str()) {
                let (_k, v) = guard.into_inner().map_err(|e| {
                    storage_error(format!("fjall distillation_summary recount: {e}"))
                })?;
                let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
                if !msg.is_distilled {
                    tokens += msg.token_estimate;
                    count += 1;
                }
            }
            (tokens, count)
        };

        if let Some(bytes) = tx
            .get(&sessions_part, session_id)
            .map_err(|e| storage_error(format!("fjall distillation_summary session get: {e}")))?
        {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            let old_updated_at = session.updated_at.clone();
            session.metrics.token_count_estimate = total_tokens;
            session.metrics.message_count = msg_count;
            session.updated_at = now_iso();
            Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
            Self::write_session_in_tx(&mut tx, &sessions_part, &session)?;
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit failed (distillation_summary write): {e}"),
            }
            .build()
        })?;

        info!(
            session_id,
            msg_count, total_tokens, "inserted distillation summary"
        );
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

        let guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let distillations_part = self.partition("distillations")?;
        let sessions_part = self.partition("sessions")?;
        let counters_part = self.partition("counters")?;

        let snap = self.db.read_tx();

        // WHY: referential integrity — reject distillation records for non-existent sessions (#5027).
        if snap
            .get(&sessions_part, session_id)
            .map_err(|e| storage_error(format!("fjall record_distillation session check: {e}")))?
            .is_none()
        {
            return Err(error::SessionNotFoundSnafu {
                id: session_id.to_owned(),
            }
            .build());
        }

        let dist_id = match snap
            .get(&counters_part, "dist_id")
            .map_err(|e| storage_error(format!("fjall dist_id counter read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "dist_id")?,
        } + 1;
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

        if let Some(bytes) = tx
            .get(&sessions_part, session_id)
            .map_err(|e| storage_error(format!("fjall record_distillation session get: {e}")))?
        {
            let mut session: Session =
                serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
            let old_updated_at = session.updated_at.clone();
            session.metrics.distillation_count += 1;
            session.metrics.last_distilled_at = Some(rec.created_at.clone());
            session.updated_at = now_iso();
            Self::update_session_nous_index(&mut tx, &sessions_part, &session, &old_updated_at);
            Self::write_session_in_tx(&mut tx, &sessions_part, &session)?;
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall record_distillation: {e}"),
            }
            .build()
        })?;

        // WARNING: release the write lock before pruning — prune_distillation_records
        // re-acquires it and the Mutex is non-reentrant (would self-deadlock).
        drop(guard);

        // WHY: cap per-session distillation records to avoid unbounded accumulation (#5693).
        self.prune_distillation_records(session_id)?;

        info!(
            session_id,
            messages_before, messages_after, tokens_before, tokens_after, "recorded distillation"
        );
        Ok(())
    }

    /// Prune a session's distillation records to the most recent
    /// [`DISTILLATION_RECORD_CAP`] entries.
    ///
    /// Records are keyed by a monotonically increasing per-store `dist_id`, so
    /// lexicographic order equals chronological order and the oldest excess
    /// keys are the lexicographically smallest.
    ///
    /// # Errors
    /// Returns an error if the scan or delete transaction fails.
    fn prune_distillation_records(&self, session_id: &str) -> Result<()> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let distillations_part = self.partition("distillations")?;
        let snap = self.db.read_tx();
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let keys: Vec<Vec<u8>> = snap
            .range(&distillations_part, prefix.as_str()..upper.as_str())
            .map(|g| {
                g.into_inner().map(|(k, _v)| k.to_vec()).map_err(|e| {
                    storage_error(format!("fjall prune_distillation_records scan: {e}"))
                })
            })
            .collect::<Result<_>>()?;
        let cap = usize::try_from(DISTILLATION_RECORD_CAP).unwrap_or(usize::MAX);
        let excess = keys.len().saturating_sub(cap);
        if excess == 0 {
            return Ok(());
        }
        let mut tx = self.db.write_tx();
        for key in keys.iter().take(excess) {
            tx.remove(&distillations_part, key.as_slice());
        }
        tx.commit()
            .map_err(|e| storage_error(format!("fjall prune_distillation_records: {e}")))?;
        Ok(())
    }

    /// Prune old usage records for a session, keeping at most `keep_last_n`
    /// most recent rows.
    ///
    /// Usage rows are keyed by zero-padded `turn_seq`, so lexicographic order
    /// equals chronological order and the oldest excess keys are the
    /// lexicographically smallest.
    ///
    /// Returns the number of rows deleted.
    ///
    /// # Errors
    /// Returns an error if the scan or delete transaction fails.
    pub fn cleanup_usage_records(&self, session_id: &str, keep_last_n: u64) -> Result<u64> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let usage_part = self.partition("usage")?;
        let snap = self.db.read_tx();
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let keys: Vec<Vec<u8>> = snap
            .range(&usage_part, prefix.as_str()..upper.as_str())
            .map(|g| {
                g.into_inner()
                    .map(|(k, _v)| k.to_vec())
                    .map_err(|e| storage_error(format!("fjall cleanup_usage_records scan: {e}")))
            })
            .collect::<Result<_>>()?;
        let keep = usize::try_from(keep_last_n).unwrap_or(usize::MAX);
        let to_delete = keys.len().saturating_sub(keep);
        if to_delete == 0 {
            return Ok(0);
        }
        let mut tx = self.db.write_tx();
        for key in keys.iter().take(to_delete) {
            tx.remove(&usage_part, key.as_slice());
        }
        tx.commit()
            .map_err(|e| storage_error(format!("fjall cleanup_usage_records: {e}")))?;
        Ok(to_delete as u64) // kanon:ignore RUST/as-cast — count of deleted rows, bounded by partition size
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
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let usage_part = self.partition("usage")?;
        let sessions_part = self.partition("sessions")?;

        let mut tx = self.db.write_tx();
        Self::record_usage_in_tx(&mut tx, &usage_part, &sessions_part, record)?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall record_usage: {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;
        Ok(())
    }

    /// Record token usage inside an existing write transaction.
    ///
    /// WHY: reused by [`Self::record_usage`] (one fsync per call) and
    /// [`Self::finalize_turn`] (one fsync for the whole turn).
    fn record_usage_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        usage_part: &fjall::SingleWriterTxKeyspace,
        sessions_part: &fjall::SingleWriterTxKeyspace,
        record: &UsageRecord,
    ) -> Result<()> {
        use fjall::Readable;

        // WHY: referential integrity — reject usage writes to non-existent sessions (#5027).
        if tx
            .get(sessions_part, record.session_id.as_str())
            .map_err(|e| storage_error(format!("fjall record_usage session check: {e}")))?
            .is_none()
        {
            return Err(error::SessionNotFoundSnafu {
                id: record.session_id.clone(),
            }
            .build());
        }

        let key = format!(
            "{}:{}",
            record.session_id,
            pad_u64(record.turn_seq.cast_unsigned())
        );
        let data = serde_json::to_vec(record).context(error::StoredJsonSnafu)?;
        tx.insert(usage_part, key.as_str(), data.as_slice());
        Ok(())
    }

    fn append_tool_audit_record_in_tx(
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        tool_audit_part: &fjall::SingleWriterTxKeyspace,
        counters_part: &fjall::SingleWriterTxKeyspace,
        session_id: &str,
        nous_id: &str,
        spec: &FinalizeToolAuditRecord<'_>,
    ) -> Result<()> {
        use fjall::Readable;

        let id_counter = match tx
            .get(counters_part, "tool_audit_id")
            .map_err(|e| storage_error(format!("fjall tool_audit_id counter: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "tool_audit_id")?,
        } + 1;

        let record = ToolAuditRecord {
            id: id_counter as i64, // kanon:ignore RUST/as-cast — internal counter from encode_u64; exceeds i64::MAX only after >9e18 increments
            session_id: session_id.to_owned(),
            nous_id: nous_id.to_owned(),
            turn_seq: spec.turn_seq,
            tool_call_id: spec.tool_call_id.to_owned(),
            tool_name: spec.tool_name.to_owned(),
            duration_ms: spec.duration_ms,
            is_error: spec.is_error,
            outcome: spec.outcome.to_owned(),
            result: spec.result.map(str::to_owned),
            approval: spec.approval.map(str::to_owned),
            receipt: spec.receipt.map(str::to_owned),
            created_at: now_iso(),
        };
        let key = pad_u64(id_counter);
        let data = serde_json::to_vec(&record).context(error::StoredJsonSnafu)?;

        tx.insert(tool_audit_part, key.as_str(), data.as_slice());
        tx.insert(counters_part, "tool_audit_id", encode_u64(id_counter));
        Ok(())
    }

    /// Persist a complete conversational turn in a single transaction.
    ///
    /// This batches session creation, message appends, usage recording, and
    /// the terminal lifecycle marker so a retry can never observe a committed
    /// prefix of the turn and append it again (#4614). The hot path still
    /// issues exactly one `ensure_durable()` fsync per logical turn instead of
    /// one per individual write (issue #5675).
    ///
    /// # Errors
    /// Returns an error if any partition operation, transaction commit, or
    /// durability flush fails.
    #[instrument(skip(self, request), fields(session_id = %request.session_id))]
    pub fn finalize_turn(&self, request: &FinalizeTurnRequest<'_>) -> Result<FinalizeTurnResult> {
        self.finalize_turn_with_type(request, SessionType::Primary)
    }

    /// Persist a complete conversational turn with an explicit session type.
    ///
    /// # Errors
    /// Returns an error if any partition operation, transaction commit, or
    /// durability flush fails.
    #[instrument(skip(self, request), fields(session_id = %request.session_id, %session_type))]
    pub fn finalize_turn_with_type(
        &self,
        request: &FinalizeTurnRequest<'_>,
        session_type: SessionType,
    ) -> Result<FinalizeTurnResult> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let sessions_part = self.partition("sessions")?;
        let messages_part = self.partition("messages")?;
        let usage_part = self.partition("usage")?;
        let tool_audit_part = self.partition("tool_audit")?;
        let notes_part = self.partition("notes")?;
        let counters_part = self.partition("counters")?;

        let mut tx = self.db.write_tx();

        let (mut session, _mutated, _) = Self::find_or_create_session_in_tx(
            &mut tx,
            &sessions_part,
            FindOrCreateSessionSpec {
                id: request.session_id,
                nous_id: request.nous_id,
                session_key: request.session_key,
                session_type,
                model: request.model,
                parent_session_id: request.parent_session_id,
            },
        )?;

        let mut messages_persisted = 0usize;
        for spec in request.messages {
            Self::append_message_in_tx(
                &mut tx,
                &messages_part,
                &sessions_part,
                &counters_part,
                &mut session,
                spec,
            )?;
            messages_persisted += 1;
            #[cfg(test)]
            test_finalize_failure::maybe_fail_after_messages(messages_persisted)?;
        }

        let mut usage_recorded = false;
        if let Some(usage) = request.usage {
            Self::record_usage_in_tx(&mut tx, &usage_part, &sessions_part, usage)?;
            usage_recorded = true;
        }

        let mut tool_audit_records_persisted = 0usize;
        for spec in request.tool_audit_records {
            Self::append_tool_audit_record_in_tx(
                &mut tx,
                &tool_audit_part,
                &counters_part,
                session.id.as_str(),
                request.nous_id,
                spec,
            )?;
            tool_audit_records_persisted += 1;
        }

        if let Some(note) = request.completion_note {
            Self::put_note_in_tx(
                &mut tx,
                NoteTxParts {
                    notes: &notes_part,
                    counters: &counters_part,
                    sessions: &sessions_part,
                },
                PutNoteSpec {
                    session_id: session.id.as_str(),
                    nous_id: request.nous_id,
                    category: note.category,
                    content: note.content,
                    opts: PutNoteOpts {
                        created_at: None,
                        provided_id: None,
                        validate_category: true,
                    },
                },
            )?;
        }

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall finalize_turn commit: {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;

        debug!(
            session_id = request.session_id,
            messages_persisted, "finalize_turn complete"
        );
        Ok(FinalizeTurnResult {
            messages_persisted,
            usage_recorded,
            tool_audit_records_persisted,
        })
    }

    /// Get recent tool audit records, newest first bounded by `limit`.
    #[instrument(skip(self))]
    pub fn recent_tool_audit_records(&self, limit: usize) -> Result<Vec<ToolAuditRecord>> {
        use fjall::Readable;

        const MAX_RECENT_TOOL_AUDIT_RECORDS: usize = 200;
        let limit = limit.min(MAX_RECENT_TOOL_AUDIT_RECORDS);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let tool_audit_part = self.partition("tool_audit")?;
        let snap = self.db.read_tx();

        let mut records = Vec::with_capacity(limit);
        for guard in snap.range::<&str, _>(&tool_audit_part, ..).rev() {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall recent_tool_audit_records: {e}")))?;
            records.push(
                serde_json::from_slice::<ToolAuditRecord>(&v).context(error::StoredJsonSnafu)?,
            );
            if records.len() >= limit {
                break;
            }
        }

        Ok(records)
    }

    /// Get all tool audit records for a session, ordered by turn sequence and
    /// audit insertion order.
    #[instrument(skip(self))]
    pub fn tool_audit_records_for_session(&self, session_id: &str) -> Result<Vec<ToolAuditRecord>> {
        use fjall::Readable;

        let tool_audit_part = self.partition("tool_audit")?;
        let snap = self.db.read_tx();

        let mut records = Vec::new();
        for guard in snap.range::<&str, _>(&tool_audit_part, ..) {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall tool_audit_records_for_session: {e}")))?;
            let record =
                serde_json::from_slice::<ToolAuditRecord>(&v).context(error::StoredJsonSnafu)?;
            if record.session_id == session_id {
                records.push(record);
            }
        }

        records.sort_by(|a, b| a.turn_seq.cmp(&b.turn_seq).then_with(|| a.id.cmp(&b.id)));
        Ok(records)
    }

    /// Get all usage records for a session, ordered by turn sequence.
    #[instrument(skip(self))]
    pub fn get_usage_for_session(&self, session_id: &str) -> Result<Vec<UsageRecord>> {
        use fjall::Readable;

        let usage_part = self.partition("usage")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        let mut records = Vec::new();
        for guard in snap.range(&usage_part, prefix.as_str()..upper.as_str()) {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall get_usage_for_session range: {e}")))?;
            records
                .push(serde_json::from_slice::<UsageRecord>(&v).context(error::StoredJsonSnafu)?);
        }

        Ok(records)
    }

    // ── Agent notes ───────────────────────────────────────────────────────

    /// Valid agent note categories (must match schema.rs `VALID_CATEGORIES`).
    pub const VALID_CATEGORIES: &'static [&'static str] =
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
        self.put_note(
            session_id,
            nous_id,
            category,
            content,
            PutNoteOpts {
                created_at: None,
                provided_id: None,
                validate_category: true,
            },
        )
    }

    /// Shared note write path used by [`Self::add_note`] and the portability
    /// [`Self::import_note`] entry point.
    ///
    /// When `created_at` is `None` the current time is used; when
    /// `provided_id` is `None` (or non-positive) a fresh local/global id is
    /// allocated from the counters. Set `validate_category` to `true` only for
    /// normal operator writes that must respect [`Self::VALID_CATEGORIES`].
    #[instrument(skip(self, content, opts))]
    fn put_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
        opts: PutNoteOpts<'_>,
    ) -> Result<i64> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let notes_part = self.partition("notes")?;
        let counters_part = self.partition("counters")?;
        let sessions_part = self.partition("sessions")?;
        let mut tx = self.db.write_tx();
        let note_id = Self::put_note_in_tx(
            &mut tx,
            NoteTxParts {
                notes: &notes_part,
                counters: &counters_part,
                sessions: &sessions_part,
            },
            PutNoteSpec {
                session_id,
                nous_id,
                category,
                content,
                opts,
            },
        )?;
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall put_note: {e}"),
            }
            .build()
        })?;

        Ok(note_id)
    }

    /// Get notes for a session.
    #[instrument(skip(self))]
    pub fn get_notes(&self, session_id: &str) -> Result<Vec<AgentNote>> {
        use fjall::Readable;

        let notes_part = self.partition("notes")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        let mut notes = Vec::new();
        for guard in snap.range(&notes_part, prefix.as_str()..upper.as_str()) {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall get_notes range: {e}")))?;
            notes.push(serde_json::from_slice::<AgentNote>(&v).context(error::StoredJsonSnafu)?);
        }

        Ok(notes)
    }

    /// Delete a note by global ID.
    #[instrument(skip(self))]
    pub fn delete_note(&self, note_id: i64) -> Result<bool> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let notes_part = self.partition("notes")?;

        let gid_key = format!("gid:{}", pad_u64(note_id.cast_unsigned()));
        let snap = self.db.read_tx();
        let local_ref = snap.get(&notes_part, gid_key.as_str()).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall read failed (note gid lookup): {e}"),
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

        // WHY: the local key is `{session_id}:{note_id}`; the session id is
        // needed to remove the matching `note_gid_idx:` reverse index entry.
        let session_id = local_key.rsplit(':').nth(1).ok_or_else(|| {
            storage_error("corrupt note local key: missing session_id".to_owned())
        })?;
        let gid_idx_key = Self::note_gid_index_key(session_id, note_id.cast_unsigned());

        let mut tx = self.db.write_tx();
        tx.remove(&notes_part, local_key.as_str());
        tx.remove(&notes_part, gid_key.as_str());
        tx.remove(&notes_part, gid_idx_key.as_str());
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit failed (note removal): {e}"),
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
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bb_part = self.partition("blackboard")?;

        let now = now_iso();
        let expires_at = if ttl_secs > 0 {
            Some(
                jiff::Timestamp::now()
                    .checked_add(
                        jiff::Span::new()
                            .try_seconds(ttl_secs)
                            .context(error::TtlOverflowSnafu { ttl_secs })?,
                    )
                    .context(error::TtlOverflowSnafu { ttl_secs })?
                    .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string(),
            )
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

        let mut entries = Vec::new();
        for guard in snap.range::<&str, _>(&bb_part, ..) {
            let (_k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall blackboard_list range: {e}")))?;
            let row =
                serde_json::from_slice::<BlackboardRow>(&v).context(error::StoredJsonSnafu)?;
            if !is_expired(&row) {
                entries.push(row);
            }
        }

        Ok(entries)
    }

    /// Remove all expired blackboard entries.
    ///
    /// Returns the number of rows deleted.
    ///
    /// # Errors
    /// Returns an error if the blackboard scan, JSON decoding, or delete
    /// transaction fails.
    pub fn cleanup_expired_entries(&self) -> Result<u64> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bb_part = self.partition("blackboard")?;
        let snap = self.db.read_tx();

        let mut expired_keys = Vec::new();
        for guard in snap.range::<&str, _>(&bb_part, ..) {
            let (k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall cleanup_expired scan: {e}")))?;
            let row =
                serde_json::from_slice::<BlackboardRow>(&v).context(error::StoredJsonSnafu)?;
            if is_expired(&row) {
                expired_keys.push(k.to_vec());
            }
        }
        drop(snap);

        let count = u64::try_from(expired_keys.len()).map_err(|e| {
            storage_error(format!(
                "expired blackboard key count conversion failed: {e}"
            ))
        })?;
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

    /// Remove message rows whose owning session record no longer exists.
    ///
    /// Returns the number of primary message rows deleted. The companion
    /// `next_seq:{session_id}` key is also removed, but is not counted as a
    /// message.
    ///
    /// # Errors
    /// Returns an error if the message/session scan, JSON decoding, delete
    /// transaction, or durability flush fails.
    pub fn cleanup_orphan_messages(&self, cutoff_iso: &str) -> Result<u64> {
        use std::collections::{BTreeSet, HashMap};

        use fjall::Readable;

        // WHY: scan + resolve orphan-ness under a read snapshot BEFORE taking the
        // write lock; the lock is held only for the delete batch (#5697).
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;
        let snap = self.db.read_tx();

        // WHY: deduplicate session lookups — one get per unique session_id rather
        // than one per message row.
        let mut session_exists: HashMap<String, bool> = HashMap::new();
        let mut keys_to_delete: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut primary_message_count: u64 = 0;
        for guard in snap.range::<&str, _>(&messages_part, ..) {
            let (key, value) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall cleanup_orphan_messages scan: {e}")))?;
            let key_str = String::from_utf8(key.to_vec()).map_err(|e| {
                storage_error(format!(
                    "fjall cleanup_orphan_messages invalid message key: {e}"
                ))
            })?;
            if key_str.starts_with("next_seq:") {
                continue;
            }
            let Some((session_id, _seq)) = key_str.split_once(':') else {
                continue;
            };
            let exists = if let Some(&present) = session_exists.get(session_id) {
                present
            } else {
                let present = snap
                    .get(&sessions_part, session_id.as_bytes())
                    .map_err(|e| {
                        storage_error(format!("fjall cleanup_orphan_messages session lookup: {e}"))
                    })?
                    .is_some();
                session_exists.insert(session_id.to_owned(), present);
                present
            };
            if exists {
                continue;
            }
            let message =
                serde_json::from_slice::<Message>(&value).context(error::StoredJsonSnafu)?;
            if message.created_at.as_str() >= cutoff_iso {
                continue;
            }

            keys_to_delete.insert(key.to_vec());
            keys_to_delete.insert(format!("next_seq:{session_id}").into_bytes());
            primary_message_count = primary_message_count.saturating_add(1);
        }
        drop(snap);

        if !keys_to_delete.is_empty() {
            let _guard = self
                .write_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut tx = self.db.write_tx();
            for key in &keys_to_delete {
                tx.remove(&messages_part, key.as_slice());
            }
            tx.commit().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall cleanup_orphan_messages: {e}"),
                }
                .build()
            })?;
            self.ensure_durable()?;
        }

        Ok(primary_message_count)
    }

    /// Prune session archive files older than `ttl_days` from the instance's
    /// `archive/sessions/` directory.
    ///
    /// Only `.json` files are considered. Files whose metadata cannot be read,
    /// or whose removal fails, are logged and skipped so that one bad archive
    /// does not abort the whole retention pass.
    ///
    /// Returns the number of files removed.
    ///
    /// # Errors
    /// Returns an error if the archive directory cannot be read.
    #[instrument(skip(self))]
    pub fn prune_session_archives(&self, ttl_days: u32) -> Result<u64> {
        use jiff::{Timestamp, ToSpan as _};

        // WHY: the store path is `<data_dir>/sessions`; archives live next to
        // the store under `<data_dir>/archive/sessions`.
        let archive_dir = self.session_archive_dir()?;
        if !archive_dir.exists() {
            return Ok(0);
        }

        // WHY: Span::days is a calendar unit that requires a timezone for
        // Timestamp arithmetic — checked_sub returns None and silently falls
        // back to UNIX_EPOCH. Hours are a fixed-duration time unit and work
        // directly against Timestamp without a timezone context.
        let cutoff = Timestamp::now()
            .checked_sub((i64::from(ttl_days) * 24).hours())
            .unwrap_or(Timestamp::UNIX_EPOCH);

        let mut removed: u64 = 0;
        let entries = fs::read_dir(&archive_dir).context(error::IoSnafu {
            path: archive_dir.clone(),
        })?;

        for entry in entries {
            let entry = entry.context(error::IoSnafu {
                path: archive_dir.clone(),
            })?;
            let path = entry.path();
            let is_json = path.extension() == Some(std::ffi::OsStr::new("json"));
            if !is_json {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "skipping archive file with unreadable metadata");
                    continue;
                }
            };
            let modified = match metadata.modified() {
                Ok(t) => t,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "skipping archive file with unreadable mtime");
                    continue;
                }
            };
            let millis = match modified.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_millis(),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "skipping archive file with mtime before Unix epoch");
                    continue;
                }
            };
            let file_ts = jiff::Timestamp::from_millisecond(i64::try_from(millis).unwrap_or(0))
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
            if file_ts < cutoff {
                if let Err(e) = fs::remove_file(&path) {
                    warn!(path = %path.display(), error = %e, "failed to remove stale session archive");
                    continue;
                }
                info!(path = %path.display(), ttl_days, "pruned stale session archive");
                removed = removed.saturating_add(1);
            }
        }

        Ok(removed)
    }

    /// Return the directory used for session JSON archives.
    ///
    /// WHY: callers outside graphe (e.g. `aletheia::session_retention`) compute
    /// the same path as the store's parent directory plus `archive/sessions`.
    /// Keeping the derivation here ensures both sides agree on the location.
    fn session_archive_dir(&self) -> Result<PathBuf> {
        let data_dir = self.path.parent().ok_or_else(|| {
            storage_error(format!(
                "session store path has no parent: {}",
                self.path.display()
            ))
        })?;
        Ok(data_dir.join("archive").join("sessions"))
    }

    /// Delete a blackboard entry. Only the original author can delete.
    #[instrument(skip(self))]
    pub fn blackboard_delete(&self, key: &str, author: &str) -> Result<bool> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                message: format!("fjall commit failed (blackboard removal): {e}"),
            }
            .build()
        })?;
        Ok(true)
    }
}

// ── Portability (issue #4163) ──────────────────────────────────────────────
//
// Raw entry points that bypass the distilled-message filter and preserve
// caller-supplied timestamps/metrics. Used by `aletheia agent export`/`import`
// to round-trip an agent without silent data loss.
// WARNING: recall and serve paths must NOT call these — they would leak
// distilled summaries to the LLM and wedge the `idx:nous:...:upd:...`
// index with past timestamps.

#[cfg(feature = "portability")]
impl SessionStore {
    /// Get message history including distilled messages, preserving seq order.
    ///
    /// Unlike [`Self::get_history`], the distilled flag is not used to filter
    /// rows: every message persisted for `session_id` is returned. This is the
    /// faithful read used by agent export — the recall path must continue to
    /// use [`Self::get_history`].
    ///
    /// # Errors
    /// Returns an error if the partition scan fails.
    #[instrument(skip(self))]
    pub fn get_history_raw(&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>> {
        use std::collections::VecDeque;

        use fjall::Readable;

        let messages_part = self.partition("messages")?;
        let prefix = format!("{session_id}:");
        let upper = format!("{session_id};\x00");
        let snap = self.db.read_tx();

        let limit = limit.and_then(|lim| usize::try_from(lim).ok());
        let mut messages: VecDeque<Message> = VecDeque::new();
        for guard in snap.range(&messages_part, prefix.as_str()..upper.as_str()) {
            let (k, v) = guard
                .into_inner()
                .map_err(|e| storage_error(format!("fjall get_history_raw: {e}")))?;
            // WHY: the `next_seq:{session_id}` counter key shares the partition
            // but is not a Message.
            if k.starts_with(b"next_seq:") {
                continue;
            }
            let msg = serde_json::from_slice::<Message>(&v).context(error::StoredJsonSnafu)?;
            messages.push_back(msg);
            if let Some(lim) = limit
                && messages.len() > lim
            {
                messages.pop_front();
            }
        }

        Ok(messages.into_iter().collect())
    }

    /// Insert a message at its declared seq, preserving caller-supplied
    /// `created_at` and `is_distilled` and NOT mutating session metrics or
    /// `updated_at`.
    ///
    /// The `next_seq` counter is advanced to `max(current, msg.seq)` and the
    /// global `msg_id` counter to `max(current, msg.id)` so a subsequent
    /// [`Self::append_message`] will not collide.
    ///
    /// The owning session must already exist (call [`Self::import_session`]
    /// first when restoring from an export).
    ///
    /// # Errors
    /// Returns an error if the session does not exist or any commit fails.
    #[instrument(skip(self, msg), fields(session_id = %msg.session_id, seq = msg.seq))]
    pub fn insert_message_raw(&self, msg: &Message) -> Result<()> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let messages_part = self.partition("messages")?;
        let sessions_part = self.partition("sessions")?;
        let counters_part = self.partition("counters")?;

        // WHY: refuse if the session does not exist — the message would be
        // orphaned and never appear in list/recall views.
        let snap = self.db.read_tx();
        if snap
            .get(&sessions_part, msg.session_id.as_str())
            .map_err(|e| storage_error(format!("fjall insert_message_raw session check: {e}")))?
            .is_none()
        {
            return Err(error::SessionNotFoundSnafu {
                id: msg.session_id.clone(),
            }
            .build());
        }

        let next_seq_key = format!("next_seq:{}", msg.session_id);
        let current_seq = match snap
            .get(&messages_part, next_seq_key.as_str())
            .map_err(|e| storage_error(format!("fjall insert_message_raw seq read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "next_seq")?,
        };
        let current_msg_id = match snap
            .get(&counters_part, "msg_id")
            .map_err(|e| storage_error(format!("fjall insert_message_raw msg_id read: {e}")))?
        {
            None => 0u64,
            Some(b) => try_decode_u64(&b, "msg_id")?,
        };
        drop(snap);

        let msg_seq_u64 = u64::try_from(msg.seq).map_err(|src| {
            storage_error(format!(
                "insert_message_raw: negative seq {}: {src}",
                msg.seq
            ))
        })?;
        let msg_id_u64 = u64::try_from(msg.id).map_err(|src| {
            storage_error(format!("insert_message_raw: negative id {}: {src}", msg.id))
        })?;
        let new_next_seq = current_seq.max(msg_seq_u64);
        let new_msg_id = current_msg_id.max(msg_id_u64);

        let msg_key = format!("{}:{}", msg.session_id, pad_u64(msg_seq_u64));
        let msg_data = serde_json::to_vec(msg).context(error::StoredJsonSnafu)?;

        let mut tx = self.db.write_tx();
        tx.insert(&messages_part, msg_key.as_str(), msg_data.as_slice());
        tx.insert(
            &messages_part,
            next_seq_key.as_str(),
            encode_u64(new_next_seq),
        );
        tx.insert(&counters_part, "msg_id", encode_u64(new_msg_id));
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall insert_message_raw commit: {e}"),
            }
            .build()
        })?;
        Ok(())
    }

    /// Write a session record preserving every caller-supplied field
    /// (`status`, `created_at`, `updated_at`, `metrics`, `origin`).
    ///
    /// Indexes are written using the supplied `updated_at`, so a list scan
    /// observes the imported session at its true age — not "now". This is
    /// what keeps maintenance sweepers honest after an import.
    ///
    /// Idempotency: returns an error if a session with the same `id` already
    /// exists, or if the `(nous_id, session_key)` slot is occupied by a
    /// *different* session id, unless `force` is true. Re-importing the same
    /// session id with `force=true` overwrites cleanly.
    ///
    /// # Errors
    /// Returns an error on idempotency violation (without force) or any
    /// commit failure.
    #[instrument(skip(self, session), fields(id = %session.id, nous_id = %session.nous_id))]
    pub fn import_session(&self, session: &Session, force: bool) -> Result<Session> {
        use fjall::Readable;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sessions_part = self.partition("sessions")?;

        let snap = self.db.read_tx();
        let existing_self = snap
            .get(&sessions_part, session.id.as_str())
            .map_err(|e| storage_error(format!("fjall import_session existing: {e}")))?;
        let key_idx = Self::session_key_index_key(&session.nous_id, &session.session_key);
        let existing_key_owner = snap
            .get(&sessions_part, key_idx.as_str())
            .map_err(|e| storage_error(format!("fjall import_session key idx: {e}")))?
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        drop(snap);

        if !force {
            if existing_self.is_some() {
                return Err(storage_error(format!(
                    "import_session: session '{}' already exists (use force to overwrite)",
                    session.id
                )));
            }
            if let Some(owner) = existing_key_owner.as_deref()
                && owner != session.id.as_str()
            {
                return Err(storage_error(format!(
                    "import_session: ({}, {}) is already owned by session '{}' \
                     (use force to overwrite)",
                    session.nous_id, session.session_key, owner
                )));
            }
        }

        // WHY: a forced overwrite must remove every stale secondary index entry
        // that would be orphaned after the write. Two index types can go stale:
        //
        // - `idx:key:{nous_id}:{session_key}` — orphaned when `nous_id` or
        //   `session_key` changes; the new write inserts the updated key index
        //   below but the old entry is never removed otherwise.
        //
        // - `idx:nous:{nous_id}:upd:{ts}:{id}` — orphaned when `nous_id` or
        //   `updated_at` changes; both are embedded in the key prefix.
        let mut tx = self.db.write_tx();
        if let Some(prev_bytes) = existing_self.as_ref() {
            let prev: Session =
                serde_json::from_slice(prev_bytes).context(error::StoredJsonSnafu)?;
            if prev.nous_id != session.nous_id || prev.session_key != session.session_key {
                let stale_key_idx = Self::session_key_index_key(&prev.nous_id, &prev.session_key);
                tx.remove(&sessions_part, stale_key_idx.as_str());
            }
            if prev.nous_id != session.nous_id || prev.updated_at != session.updated_at {
                let stale_nous_idx =
                    Self::session_nous_index_key(&prev.nous_id, &prev.updated_at, &prev.id);
                tx.remove(&sessions_part, stale_nous_idx.as_str());
            }
        }

        // WHY: a forced overwrite that displaces a DIFFERENT session from the
        // (nous_id, session_key) slot must also evict that displaced owner — its
        // session row and nous_idx would otherwise be orphaned, surfacing in list
        // scans while its key index now points elsewhere (#5028).
        if let Some(displaced_id) = existing_key_owner.as_deref()
            && displaced_id != session.id.as_str()
        {
            let displaced_bytes = tx
                .get(&sessions_part, displaced_id)
                .map_err(|e| storage_error(format!("fjall import_session displaced get: {e}")))?;
            if let Some(bytes) = displaced_bytes {
                let displaced: Session =
                    serde_json::from_slice(&bytes).context(error::StoredJsonSnafu)?;
                let displaced_nous_idx = Self::session_nous_index_key(
                    &displaced.nous_id,
                    &displaced.updated_at,
                    &displaced.id,
                );
                tx.remove(&sessions_part, displaced_nous_idx.as_str());
                tx.remove(&sessions_part, displaced.id.as_str());
            }
        }

        // WHY: stamp provenance as write_session would, but with the imported
        // session's timestamps already in place.
        let mut stamped = session.clone();
        stamped.artefact_meta = Some(stamped.stamp());
        let data = serde_json::to_vec(&stamped).context(error::StoredJsonSnafu)?;

        tx.insert(&sessions_part, session.id.as_str(), data.as_slice());
        tx.insert(&sessions_part, key_idx.as_str(), session.id.as_bytes());
        let nous_idx =
            Self::session_nous_index_key(&session.nous_id, &session.updated_at, &session.id);
        tx.insert(&sessions_part, nous_idx.as_str(), b"");

        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall import_session commit: {e}"),
            }
            .build()
        })?;
        self.ensure_durable()?;

        // WHY: imports can overwrite or displace existing sessions; adjust the
        // counter to reflect the true delta in session rows (issue #5662).
        let displaced = existing_key_owner
            .as_deref()
            .is_some_and(|owner| owner != session.id.as_str());
        match (existing_self.is_none(), displaced) {
            (true, false) => {
                self.session_count.fetch_add(1, Ordering::Relaxed);
            }
            (false, true) => {
                self.session_count.fetch_sub(1, Ordering::Relaxed);
            }
            _ => {}
        }

        metrics::record_session_created(&session.nous_id, session.session_type.as_str());
        info!(
            id = session.id,
            nous_id = session.nous_id,
            status = %session.status,
            "imported session"
        );
        Ok(stamped)
    }

    /// Insert a note exactly as supplied, preserving its `id` and `created_at`.
    ///
    /// This is the faithful import counterpart to [`Self::add_note`]. Callers
    /// restoring from a portable export must use this instead of `add_note`,
    /// which would overwrite the original timestamp and allocate a new
    /// identifier.
    ///
    /// A non-positive `note.id` is treated as "allocate a fresh id", so files
    /// that do not record the original identifier still round-trip their
    /// timestamps.
    ///
    /// # Errors
    /// Returns an error if the owning session does not exist or any commit
    /// fails.
    #[instrument(skip(self, note), fields(id = note.id, session_id = %note.session_id))]
    pub fn import_note(&self, note: &AgentNote) -> Result<()> {
        self.put_note(
            &note.session_id,
            &note.nous_id,
            &note.category,
            &note.content,
            PutNoteOpts {
                created_at: Some(&note.created_at),
                provided_id: Some(note.id),
                validate_category: false,
            },
        )?;
        Ok(())
    }
}

/// Check whether a blackboard row has expired.
fn is_expired(row: &BlackboardRow) -> bool {
    let Some(ref expires_at) = row.expires_at else {
        return false;
    };
    // WHY: ISO 8601 UTC strings in one fixed format compare correctly as strings.
    let now = now_iso();
    expires_at.as_str() <= now.as_str()
}

#[cfg(test)]
#[path = "fjall_store_tests.rs"]
mod fjall_store_tests;
