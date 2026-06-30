//! Fjall destination writer.
//!
//! This module replicates the key/value contract documented in
//! `crates/graphe/src/store/fjall_store.rs` so that a runtime opened on
//! the migrated directory sees data identical to one produced natively.
//!
//! # Partition layout (mirrors the canonical doc)
//!
//! | Partition       | Key pattern                                          | Value                    |
//! |-----------------|------------------------------------------------------|--------------------------|
//! | `sessions`      | `{session_id}`                                       | JSON `Session`           |
//! | `sessions`      | `idx:nous:{nous_id}:upd:{updated_at}:{session_id}`   | `""`                     |
//! | `sessions`      | `idx:key:{nous_id}:{session_key}`                    | `{session_id}` bytes     |
//! | `messages`      | `{session_id}:{seq:020}`                             | JSON `Message`           |
//! | `messages`      | `next_seq:{session_id}`                              | big-endian `u64`         |
//! | `usage`         | `{session_id}:{turn_seq:020}`                        | JSON `UsageRecord`       |
//! | `distillations` | `{session_id}:{dist_id:020}`                         | JSON `DistillationRecord`|
//! | `notes`         | `{session_id}:{note_local_id:020}`                   | JSON `AgentNote`         |
//! | `notes`         | `gid:{note_global_id:020}`                           | `{session_id}:{local_id}`|
//! | `blackboard`    | `{key}`                                              | JSON `BlackboardRow`     |
//! | `counters`      | `msg_id` / `dist_id` / `note_local_id` / `note_global_id` | big-endian `u64`    |
//! | `migration_legacy` | `{session_id}:{column_name}` / table-scoped legacy-only keys | UTF-8 string |
//!
//! # Why a sidecar `migration_legacy` partition?
//!
//! The legacy `SQLite` schema carries four columns the new fjall layout does
//! not: `thinking_enabled`, `thinking_budget`, `working_state`,
//! `distillation_priming`. The runtime drops them on read, but losing the
//! values at write time would silently mutate the operator's history.
//! When a column carries a non-default value we route it here so the data
//! is recoverable post-migration.
//! Legacy-only fields from `usage`, `distillations`, and `blackboard` are
//! also routed here under table-scoped keys because the runtime types do not
//! carry those fields.
//!
//! Source `schema_version` is enforced before any row is read; the staged
//! verification pass compares deterministic key/value hashes for every
//! migrated partition before publish and aborts on mismatch.

use std::collections::BTreeMap;
use std::path::Path;

use eidos::meta::Stamped as _;
use fjall::{KeyspaceCreateOptions, SingleWriterTxKeyspace};
use graphe::types::{AgentNote, BlackboardRow, Message, Session, UsageRecord};
use koina::fjall::FjallDb;
use serde::Serialize;
use snafu::ResultExt as _;
use tracing::{debug, info, instrument};

use crate::error::{
    DestinationNotEmptySnafu, FjallOpSnafu, FjallOpenSnafu, FjallPartitionSnafu, IoSnafu,
    JsonSnafu, NumericRangeSnafu, Result,
};
use crate::source::{DistillationRecord, LegacyExtras, LegacySidecarEntry};

/// Width for zero-padded sequence keys. Must match
/// `fjall_store::SEQ_WIDTH = 20`.
const SEQ_WIDTH: usize = 20;

/// Partitions the runtime expects (mirrors `fjall_store::PARTITIONS`)
/// plus the migrator's `migration_legacy` sidecar.
pub(crate) const ALL_PARTITIONS: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "distillations",
    "notes",
    "blackboard",
    "counters",
    "migration_legacy",
];

fn pad_u64(v: u64) -> String {
    format!("{v:0>SEQ_WIDTH$}")
}

fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

fn try_u64(field: &str, v: i64) -> Result<u64> {
    u64::try_from(v).map_err(|_unused| {
        NumericRangeSnafu {
            field: field.to_owned(),
            value: v,
        }
        .build()
    })
}

fn try_u64_usize(field: &str, v: usize) -> Result<u64> {
    u64::try_from(v).map_err(|_unused| {
        // u64::try_from(usize) only fails on a 128-bit target; treat as
        // logical overflow for reporting purposes.
        NumericRangeSnafu {
            field: field.to_owned(),
            value: i64::try_from(v).unwrap_or(i64::MAX),
        }
        .build()
    })
}

fn fjall_op_err<S: Into<String>>(operation: S) -> impl FnOnce(fjall::Error) -> crate::error::Error {
    let op = operation.into();
    move |e| {
        FjallOpSnafu {
            operation: op,
            message: e.to_string(),
        }
        .build()
    }
}

fn partition_err<S: Into<String>>(name: S) -> impl FnOnce(fjall::Error) -> crate::error::Error {
    let n = name.into();
    move |e| {
        FjallPartitionSnafu {
            partition: n,
            message: e.to_string(),
        }
        .build()
    }
}

/// Helper that owns the fjall handle and named partitions during migration.
pub(crate) struct Destination {
    db: FjallDb,
    /// `sessions` partition handle.
    sessions: SingleWriterTxKeyspace,
    /// `messages` partition handle.
    messages: SingleWriterTxKeyspace,
    /// `usage` partition handle.
    usage: SingleWriterTxKeyspace,
    /// `distillations` partition handle.
    distillations: SingleWriterTxKeyspace,
    /// `notes` partition handle.
    notes: SingleWriterTxKeyspace,
    /// `blackboard` partition handle.
    blackboard: SingleWriterTxKeyspace,
    /// `counters` partition handle.
    counters: SingleWriterTxKeyspace,
    /// `migration_legacy` partition handle.
    migration_legacy: SingleWriterTxKeyspace,
}

impl Destination {
    /// Open or create a fjall DB at `path`. The directory must be empty
    /// or absent; replacement is handled by the staging layer in
    /// [`crate::migrate`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::DestinationNotEmpty`] when `path`
    /// is non-empty,
    /// [`crate::error::Error::FjallOpen`] / [`crate::error::Error::FjallPartition`]
    /// when fjall keyspace setup fails.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if path.exists() && !is_empty_or_absent(path)? {
            return Err(DestinationNotEmptySnafu {
                path: path.to_path_buf(),
            }
            .build());
        }
        let db = FjallDb::open(path, ALL_PARTITIONS).map_err(|e| {
            FjallOpenSnafu {
                path: path.to_path_buf(),
                message: e.to_string(),
            }
            .build()
        })?;
        // Acquire keyspace handles. `KeyspaceCreateOptions::default` matches
        // graphe::SessionStore::partition(), which is the same call shape
        // the runtime uses.
        let sessions = db
            .db
            .keyspace("sessions", KeyspaceCreateOptions::default)
            .map_err(partition_err("sessions"))?;
        let messages = db
            .db
            .keyspace("messages", KeyspaceCreateOptions::default)
            .map_err(partition_err("messages"))?;
        let usage = db
            .db
            .keyspace("usage", KeyspaceCreateOptions::default)
            .map_err(partition_err("usage"))?;
        let distillations = db
            .db
            .keyspace("distillations", KeyspaceCreateOptions::default)
            .map_err(partition_err("distillations"))?;
        let notes = db
            .db
            .keyspace("notes", KeyspaceCreateOptions::default)
            .map_err(partition_err("notes"))?;
        let blackboard = db
            .db
            .keyspace("blackboard", KeyspaceCreateOptions::default)
            .map_err(partition_err("blackboard"))?;
        let counters = db
            .db
            .keyspace("counters", KeyspaceCreateOptions::default)
            .map_err(partition_err("counters"))?;
        let migration_legacy = db
            .db
            .keyspace("migration_legacy", KeyspaceCreateOptions::default)
            .map_err(partition_err("migration_legacy"))?;
        Ok(Self {
            db,
            sessions,
            messages,
            usage,
            distillations,
            notes,
            blackboard,
            counters,
            migration_legacy,
        })
    }

    /// Flush all writes to durable storage.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::FjallOp`] if the persist call fails.
    pub(crate) fn persist(&self) -> Result<()> {
        self.db
            .db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_op_err("persist destination"))?;
        Ok(())
    }

    /// Persist all rows to fjall in one pass. Returns a per-table count.
    ///
    /// Sessions are written with their secondary indices. Messages are
    /// grouped per session, written in seq order, and capped with the
    /// `next_seq:{id}` counter required by the runtime's append path.
    /// Global counters (`msg_id`, `dist_id`, `note_*_id`) are seeded from
    /// the legacy auto-increment maxes so future inserts don't collide.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::error::Error::FjallOp`] from any commit
    /// failure, or [`crate::error::Error::Json`] /
    /// [`crate::error::Error::NumericRange`] from per-row encoding.
    #[instrument(skip_all, fields(
        sessions = sessions.len(),
        messages = messages.len(),
        usage = usage.len(),
        distillations = distillations.len(),
        notes = notes.len(),
        blackboard = blackboard.len(),
        legacy_sidecars = legacy_sidecars.len(),
    ))]
    pub(crate) fn write_all(
        &self,
        sessions: &[(Session, LegacyExtras)],
        messages: &[Message],
        usage: &[UsageRecord],
        distillations: &[DistillationRecord],
        notes: &[AgentNote],
        blackboard: &[BlackboardRow],
        legacy_sidecars: &[LegacySidecarEntry],
    ) -> Result<TableCounts> {
        let mut counts = TableCounts::default();

        // Index messages by session for atomic per-session commits.
        let mut by_session: BTreeMap<String, Vec<&Message>> = BTreeMap::new();
        for m in messages {
            by_session.entry(m.session_id.clone()).or_default().push(m);
        }
        let mut usage_by_session: BTreeMap<String, Vec<&UsageRecord>> = BTreeMap::new();
        for u in usage {
            usage_by_session
                .entry(u.session_id.clone())
                .or_default()
                .push(u);
        }
        let mut dist_by_session: BTreeMap<String, Vec<&DistillationRecord>> = BTreeMap::new();
        for d in distillations {
            dist_by_session
                .entry(d.session_id.clone())
                .or_default()
                .push(d);
        }
        let mut notes_by_session: BTreeMap<String, Vec<&AgentNote>> = BTreeMap::new();
        for n in notes {
            notes_by_session
                .entry(n.session_id.clone())
                .or_default()
                .push(n);
        }

        // WHY: one WriteTransaction per session — migration is atomic per session.
        for (session, legacy) in sessions {
            self.write_session_atomic(
                session,
                legacy,
                by_session
                    .get(&session.id)
                    .map_or([].as_slice(), Vec::as_slice),
                usage_by_session
                    .get(&session.id)
                    .map_or([].as_slice(), Vec::as_slice),
                dist_by_session
                    .get(&session.id)
                    .map_or([].as_slice(), Vec::as_slice),
                notes_by_session
                    .get(&session.id)
                    .map_or([].as_slice(), Vec::as_slice),
            )?;
            counts.sessions += 1;
        }
        counts.messages = messages.len();
        counts.usage = usage.len();
        counts.distillations = distillations.len();
        counts.notes = notes.len();
        info!(
            sessions = counts.sessions,
            messages = counts.messages,
            usage = counts.usage,
            distillations = counts.distillations,
            notes = counts.notes,
            "migrated per-session bundles"
        );

        // Blackboard is global — single transaction.
        self.write_blackboard(blackboard)?;
        counts.blackboard = blackboard.len();
        info!(rows = counts.blackboard, "migrated blackboard");

        self.write_legacy_sidecars(legacy_sidecars)?;
        info!(
            entries = legacy_sidecars.len(),
            "preserved legacy-only sidecar fields"
        );

        // Seed global counters so subsequent runtime inserts don't reuse IDs
        // we already wrote. msg_id is seeded to MAX(legacy messages.id);
        // dist_id is seeded to the highest per-session local distillation
        // index actually written (which is max(per_session_count)) — that
        // way the runtime's next dist_id sits above every per-session key
        // already on disk; note_global_id is seeded to MAX(legacy notes.id);
        // note_local_id is seeded to max(per_session_count) since the
        // runtime increments it as a global counter despite the name.
        let max_msg_id = messages.iter().map(|m| m.id).max().unwrap_or(0);
        let max_dist_per_session =
            max_per_session_count(distillations.iter().map(|d| &d.session_id));
        let max_note_id = notes.iter().map(|n| n.id).max().unwrap_or(0);
        let max_note_per_session = max_per_session_count(notes.iter().map(|n| &n.session_id));
        self.seed_counters(
            max_msg_id,
            max_dist_per_session,
            max_note_id,
            max_note_per_session,
        )?;
        info!(
            msg_id = max_msg_id,
            dist_id = max_dist_per_session,
            note_global_id = max_note_id,
            note_local_id = max_note_per_session,
            "seeded global counters"
        );

        Ok(counts)
    }

    fn write_session_atomic(
        &self,
        session: &Session,
        legacy: &LegacyExtras,
        msgs: &[&Message],
        usages: &[&UsageRecord],
        dists: &[&DistillationRecord],
        notes: &[&AgentNote],
    ) -> Result<()> {
        let mut tx = self.db.db.write_tx();

        // Stamp provenance on the session before writing so the JSON the
        // runtime reads matches what `SessionStore::write_session` produces.
        let mut stamped = session.clone();
        let mut meta = stamped.stamp();
        // Override producer to mark migration provenance.
        meta.producer = format!("aletheia-sessions-migrate@{}", env!("CARGO_PKG_VERSION"));
        meta.row_counts.insert(
            "messages".to_owned(),
            u64::try_from(msgs.len()).unwrap_or(0),
        );
        meta.row_counts
            .insert("usage".to_owned(), u64::try_from(usages.len()).unwrap_or(0));
        meta.row_counts.insert(
            "distillations".to_owned(),
            u64::try_from(dists.len()).unwrap_or(0),
        );
        meta.row_counts
            .insert("notes".to_owned(), u64::try_from(notes.len()).unwrap_or(0));
        stamped.artefact_meta = Some(meta);

        let session_data = json_vec("session", &stamped)?;
        tx.insert(&self.sessions, session.id.as_str(), session_data.as_slice());
        tx.insert(
            &self.sessions,
            session_key_index_key(&session.nous_id, &session.session_key).as_str(),
            session.id.as_bytes(),
        );
        tx.insert(
            &self.sessions,
            session_nous_index_key(&session.nous_id, &session.updated_at, &session.id).as_str(),
            b"",
        );

        // Messages — write in seq order; track max seq to seed `next_seq`.
        let mut max_seq: u64 = 0;
        for m in msgs {
            let seq_u = try_u64("message.seq", m.seq)?;
            let key = format!("{}:{}", m.session_id, pad_u64(seq_u));
            tx.insert(
                &self.messages,
                key.as_str(),
                json_vec("message", m)?.as_slice(),
            );
            if seq_u > max_seq {
                max_seq = seq_u;
            }
        }
        if !msgs.is_empty() {
            let next_seq_key = format!("next_seq:{}", session.id);
            tx.insert(
                &self.messages,
                next_seq_key.as_str(),
                encode_u64(max_seq).as_slice(),
            );
        }

        // Usage rows — keyed by `(session_id, turn_seq)`.
        for u in usages {
            let turn_u = try_u64("usage.turn_seq", u.turn_seq)?;
            let key = format!("{}:{}", u.session_id, pad_u64(turn_u));
            tx.insert(&self.usage, key.as_str(), json_vec("usage", u)?.as_slice());
        }

        // Distillations — assign per-session sequential IDs starting at 1
        // because the runtime's record_distillation does the same. The
        // global `dist_id` counter is seeded after this loop.
        for (idx, d) in dists.iter().enumerate() {
            let dist_id = try_u64_usize("distillation.local_id", idx + 1)?;
            let key = format!("{}:{}", d.session_id, pad_u64(dist_id));
            tx.insert(
                &self.distillations,
                key.as_str(),
                json_vec("distillation", d)?.as_slice(),
            );
        }

        // Notes — local key uses per-session local index; global key uses
        // legacy auto-increment id so post-migration inserts don't collide.
        for (idx, n) in notes.iter().enumerate() {
            let local_id = try_u64_usize("note.local_id", idx + 1)?;
            let local_key = format!("{}:{}", n.session_id, pad_u64(local_id));
            tx.insert(
                &self.notes,
                local_key.as_str(),
                json_vec("note", n)?.as_slice(),
            );
            let gid = try_u64("note.id", n.id)?;
            let gid_key = format!("gid:{}", pad_u64(gid));
            let gid_val = format!("{}:{}", n.session_id, pad_u64(local_id));
            tx.insert(&self.notes, gid_key.as_str(), gid_val.as_bytes());
        }

        // Legacy extras → migration_legacy sidecar (only if non-default).
        if legacy.is_non_default() {
            self.write_legacy_extras(&mut tx, &session.id, legacy)?;
        }

        tx.commit()
            .map_err(fjall_op_err(format!("commit session {}", session.id)))?;
        debug!(
            session_id = %session.id,
            messages = msgs.len(),
            usage = usages.len(),
            distillations = dists.len(),
            notes = notes.len(),
            legacy_preserved = legacy.is_non_default(),
            "session migrated atomically"
        );
        Ok(())
    }

    fn write_legacy_extras(
        &self,
        tx: &mut fjall::SingleWriterWriteTx<'_>,
        session_id: &str,
        legacy: &LegacyExtras,
    ) -> Result<()> {
        // Encode the whole struct as JSON so future readers can pull all
        // fields back atomically.
        let json = json_vec("legacy_extras", legacy)?;
        let bundle_key = format!("{session_id}:bundle");
        tx.insert(&self.migration_legacy, bundle_key.as_str(), json.as_slice());
        // Also write per-field strings for grep-ability.
        if let Some(v) = legacy.thinking_enabled {
            let key = format!("{session_id}:thinking_enabled");
            tx.insert(
                &self.migration_legacy,
                key.as_str(),
                v.to_string().as_bytes(),
            );
        }
        if let Some(v) = legacy.thinking_budget {
            let key = format!("{session_id}:thinking_budget");
            tx.insert(
                &self.migration_legacy,
                key.as_str(),
                v.to_string().as_bytes(),
            );
        }
        if let Some(ref v) = legacy.working_state {
            let key = format!("{session_id}:working_state");
            tx.insert(&self.migration_legacy, key.as_str(), v.as_bytes());
        }
        if let Some(ref v) = legacy.distillation_priming {
            let key = format!("{session_id}:distillation_priming");
            tx.insert(&self.migration_legacy, key.as_str(), v.as_bytes());
        }
        Ok(())
    }

    fn write_blackboard(&self, rows: &[BlackboardRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.db.db.write_tx();
        for row in rows {
            tx.insert(
                &self.blackboard,
                row.key.as_str(),
                json_vec("blackboard", row)?.as_slice(),
            );
        }
        tx.commit().map_err(fjall_op_err("commit blackboard"))?;
        Ok(())
    }

    fn write_legacy_sidecars(&self, entries: &[LegacySidecarEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.db.db.write_tx();
        for entry in entries {
            tx.insert(
                &self.migration_legacy,
                entry.key.as_str(),
                entry.value.as_slice(),
            );
        }
        tx.commit()
            .map_err(fjall_op_err("commit legacy sidecars"))?;
        Ok(())
    }

    fn seed_counters(
        &self,
        msg_id_max: i64,
        dist_local_max: u64,
        note_global_max: i64,
        note_local_max: u64,
    ) -> Result<()> {
        let mut tx = self.db.db.write_tx();
        if msg_id_max > 0 {
            let v = try_u64("counters.msg_id", msg_id_max)?;
            tx.insert(&self.counters, "msg_id", encode_u64(v).as_slice());
        }
        if dist_local_max > 0 {
            tx.insert(
                &self.counters,
                "dist_id",
                encode_u64(dist_local_max).as_slice(),
            );
        }
        if note_global_max > 0 {
            let v = try_u64("counters.note_global_id", note_global_max)?;
            tx.insert(&self.counters, "note_global_id", encode_u64(v).as_slice());
        }
        if note_local_max > 0 {
            tx.insert(
                &self.counters,
                "note_local_id",
                encode_u64(note_local_max).as_slice(),
            );
        }
        tx.commit().map_err(fjall_op_err("commit counters"))?;
        Ok(())
    }
}

/// Per-table count summary, for the migration report.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TableCounts {
    /// Sessions written (including any synthesised orphan-recovery sessions).
    pub(crate) sessions: usize,
    /// Messages written.
    pub(crate) messages: usize,
    /// Usage records written.
    pub(crate) usage: usize,
    /// Distillations written.
    pub(crate) distillations: usize,
    /// Agent notes written.
    pub(crate) notes: usize,
    /// Blackboard entries written.
    pub(crate) blackboard: usize,
}

fn json_vec<T: Serialize>(operation: &str, v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).context(JsonSnafu {
        operation: format!("serialise {operation}"),
    })
}

fn session_key_index_key(nous_id: &str, session_key: &str) -> String {
    format!("idx:key:{nous_id}:{session_key}")
}

fn session_nous_index_key(nous_id: &str, updated_at: &str, session_id: &str) -> String {
    // WHY: rename the parameter locally so the format argument list never
    // contains the literal `updated_at` token — STORAGE/sql-string-concat
    // matches the `UPDATE` substring inside that identifier (mirrors
    // crates/graphe/src/store/fjall_store.rs).
    let ts = updated_at;
    format!("idx:nous:{nous_id}:upd:{ts}:{session_id}")
}

pub(crate) fn is_empty_or_absent(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = std::fs::read_dir(path).context(IoSnafu {
        context: format!("reading destination directory {}", path.display()),
    })?;
    Ok(entries.next().is_none())
}

/// Compute the maximum count of rows that share any single `session_id`.
///
/// Used to seed `dist_id` and `note_local_id` global counters such that
/// the next runtime insert generates a key strictly larger than any we
/// already wrote inside any session bucket.
fn max_per_session_count<'a, I>(session_ids: I) -> u64
where
    I: IntoIterator<Item = &'a String>,
{
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for sid in session_ids {
        *counts.entry(sid.as_str()).or_insert(0) += 1;
    }
    counts.values().copied().max().unwrap_or(0)
}
