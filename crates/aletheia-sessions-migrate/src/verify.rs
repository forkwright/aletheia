//! Post-migration verification.
//!
//! Opens the source `SQLite` DB read-only, rebuilds the deterministic fjall
//! key/value entries the migrator is expected to produce, then compares those
//! entries with the destination for every migrated partition:
//! `sessions`, `messages`, `usage`, `distillations`, `notes`, `blackboard`,
//! `counters`, and `migration_legacy`.
//!
//! The report keeps the original logical session/message counters and
//! message-body hash for operator familiarity, but pass/fail status is driven
//! by complete per-partition entry counts and deterministic key/value hashes.

use std::collections::BTreeMap;
use std::path::Path;

use eidos::meta::Stamped as _;
use fjall::{KeyspaceCreateOptions, Readable};
use graphe::types::{AgentNote, BlackboardRow, Message, Session, UsageRecord};
use koina::fjall::FjallDb;
use serde::Serialize;
use sha2::{Digest, Sha256};
use snafu::ResultExt as _;
use tracing::info;

use crate::dest::ALL_PARTITIONS;
use crate::error::{
    FjallOpSnafu, FjallOpenSnafu, FjallPartitionSnafu, JsonSnafu, NumericRangeSnafu, Result,
};
use crate::migrate::{SourceData, load_source_data, open_source};
use crate::schema;
use crate::source::{DistillationRecord, LegacyExtras};

/// Verification result for one migrated fjall partition.
#[derive(Debug, Clone, Serialize)]
pub struct PartitionVerification {
    /// Partition name.
    pub partition: String,
    /// Expected key/value entry count derived from the source.
    pub source_entry_count: usize,
    /// Actual key/value entry count found in fjall.
    pub dest_entry_count: usize,
    /// Expected deterministic key/value SHA-256, hex encoded.
    pub source_sha256: String,
    /// Actual deterministic key/value SHA-256, hex encoded.
    pub dest_sha256: String,
    /// Whether count and hash matched.
    pub ok: bool,
}

/// Result of a `--verify` run.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    /// Number of per-session samples spot-checked.
    pub samples_checked: usize,
    /// Each detected mismatch as a human-readable line.
    pub mismatches: Vec<String>,
    /// Sessions known to the source (real rows + distinct orphan IDs).
    pub source_session_count: usize,
    /// Sessions present in dest fjall.
    pub dest_session_count: usize,
    /// Messages in source.
    pub source_message_count: usize,
    /// Messages in dest fjall.
    pub dest_message_count: usize,
    /// Whether the SHA-256 of every message body matches between stores.
    pub message_body_hash_match: bool,
    /// Source-side hash, hex-encoded.
    pub source_message_body_sha256: String,
    /// Destination-side hash, hex-encoded.
    pub dest_message_body_sha256: String,
    /// Complete per-partition verification results.
    pub partition_checks: Vec<PartitionVerification>,
}

impl VerificationReport {
    /// Did every check pass?
    #[must_use]
    pub fn ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

type EntryMap = BTreeMap<Vec<u8>, Vec<u8>>;
type PartitionMap = BTreeMap<&'static str, EntryMap>;

/// Run a verification pass against a freshly-migrated fjall directory.
///
/// # Errors
///
/// Propagates `SQLite`, source-mapping, JSON encoding, numeric range, and
/// fjall scan errors.
pub fn run_verification(source: &Path, dest: &Path, samples: usize) -> Result<VerificationReport> {
    let conn = open_source(source)?;
    schema::validate(&conn)?;
    let source_data = load_source_data(&conn)?;
    let source_partitions = source_expected_partitions(&source_data)?;

    let dest_db = FjallDb::open_existing(dest).map_err(|e| {
        FjallOpenSnafu {
            path: dest.to_path_buf(),
            message: e.to_string(),
        }
        .build()
    })?;
    let dest_partitions = dest_partitions(&dest_db)?;

    let src_sessions = source_data.sessions.len();
    let src_messages = source_data.messages.len();
    let dest_sessions = dest_session_count(&dest_db)?;
    let dest_messages = dest_message_count(&dest_db)?;

    let src_hash = source_message_body_hash(&source_data.messages);
    let dest_hash = dest_message_body_hash(&dest_db)?;

    info!(
        src_sessions,
        dest_sessions, src_messages, dest_messages, "verification counts loaded"
    );

    let mut mismatches = Vec::new();
    if src_sessions != dest_sessions {
        mismatches.push(format!(
            "session count mismatch: source={src_sessions}, dest={dest_sessions}"
        ));
    }
    if src_messages != dest_messages {
        mismatches.push(format!(
            "message count mismatch: source={src_messages}, dest={dest_messages}"
        ));
    }
    if src_hash != dest_hash {
        mismatches.push(format!(
            "message body hash mismatch: source=sha256:{} dest=sha256:{}",
            hex(&src_hash),
            hex(&dest_hash)
        ));
    }

    let mut partition_checks = Vec::new();
    for &name in ALL_PARTITIONS {
        let source_entries = source_partitions.get(name).cloned().unwrap_or_default();
        let dest_entries = dest_partitions.get(name).cloned().unwrap_or_default();
        let source_sha = hash_entries(&source_entries);
        let dest_sha = hash_entries(&dest_entries);
        let source_count = source_entries.len();
        let dest_count = dest_entries.len();

        if source_count != dest_count {
            mismatches.push(format!(
                "partition {name} entry count mismatch: source={source_count}, dest={dest_count}"
            ));
        }
        if source_sha != dest_sha {
            mismatches.push(format!(
                "partition {name} hash mismatch: source=sha256:{} dest=sha256:{}",
                hex(&source_sha),
                hex(&dest_sha)
            ));
        }

        partition_checks.push(PartitionVerification {
            partition: name.to_owned(),
            source_entry_count: source_count,
            dest_entry_count: dest_count,
            source_sha256: hex(&source_sha),
            dest_sha256: hex(&dest_sha),
            ok: source_count == dest_count && source_sha == dest_sha,
        });
    }

    let mut src_per_session = source_per_session_counts(&source_data.messages);
    src_per_session.sort_by(|a, b| a.0.cmp(&b.0));
    let take = samples.min(src_per_session.len());
    let dest_per_session = dest_per_session_counts(&dest_db)?;
    for (sid, src_count) in src_per_session.iter().take(take) {
        let dest_count = dest_per_session.get(sid.as_str()).copied().unwrap_or(0);
        if *src_count != dest_count {
            mismatches.push(format!(
                "session {sid}: source has {src_count} messages, dest has {dest_count}"
            ));
        }
    }

    Ok(VerificationReport {
        samples_checked: take,
        mismatches,
        source_session_count: src_sessions,
        dest_session_count: dest_sessions,
        source_message_count: src_messages,
        dest_message_count: dest_messages,
        message_body_hash_match: src_hash == dest_hash,
        source_message_body_sha256: hex(&src_hash),
        dest_message_body_sha256: hex(&dest_hash),
        partition_checks,
    })
}

fn source_expected_partitions(source_data: &SourceData) -> Result<PartitionMap> {
    let mut partitions = empty_partitions();

    let mut by_session: BTreeMap<String, Vec<&Message>> = BTreeMap::new();
    for message in &source_data.messages {
        by_session
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }
    let mut usage_by_session: BTreeMap<String, Vec<&UsageRecord>> = BTreeMap::new();
    for usage in &source_data.usage {
        usage_by_session
            .entry(usage.session_id.clone())
            .or_default()
            .push(usage);
    }
    let mut dist_by_session: BTreeMap<String, Vec<&DistillationRecord>> = BTreeMap::new();
    for distillation in &source_data.distillations {
        dist_by_session
            .entry(distillation.session_id.clone())
            .or_default()
            .push(distillation);
    }
    let mut notes_by_session: BTreeMap<String, Vec<&AgentNote>> = BTreeMap::new();
    for note in &source_data.notes {
        notes_by_session
            .entry(note.session_id.clone())
            .or_default()
            .push(note);
    }

    for (session, legacy) in &source_data.sessions {
        let messages = by_session
            .get(&session.id)
            .map_or([].as_slice(), Vec::as_slice);
        let usage = usage_by_session
            .get(&session.id)
            .map_or([].as_slice(), Vec::as_slice);
        let distillations = dist_by_session
            .get(&session.id)
            .map_or([].as_slice(), Vec::as_slice);
        let notes = notes_by_session
            .get(&session.id)
            .map_or([].as_slice(), Vec::as_slice);

        add_session_entries(
            &mut partitions,
            session,
            legacy,
            messages,
            usage,
            distillations,
            notes,
        )?;
    }

    add_blackboard_entries(&mut partitions, &source_data.blackboard)?;
    add_legacy_sidecar_entries(
        partition_mut(&mut partitions, "migration_legacy"),
        &source_data.legacy_sidecars,
    );
    add_counter_entries(&mut partitions, source_data)?;

    Ok(partitions)
}

fn empty_partitions() -> PartitionMap {
    ALL_PARTITIONS
        .iter()
        .map(|&name| (name, EntryMap::new()))
        .collect()
}

fn add_session_entries(
    partitions: &mut PartitionMap,
    session: &Session,
    legacy: &LegacyExtras,
    messages: &[&Message],
    usage: &[&UsageRecord],
    distillations: &[&DistillationRecord],
    notes: &[&AgentNote],
) -> Result<()> {
    let mut stamped = session.clone();
    let mut meta = stamped.stamp();
    meta.producer = format!("aletheia-sessions-migrate@{}", env!("CARGO_PKG_VERSION"));
    meta.row_counts.insert(
        "messages".to_owned(),
        u64::try_from(messages.len()).unwrap_or(0),
    );
    meta.row_counts
        .insert("usage".to_owned(), u64::try_from(usage.len()).unwrap_or(0));
    meta.row_counts.insert(
        "distillations".to_owned(),
        u64::try_from(distillations.len()).unwrap_or(0),
    );
    meta.row_counts
        .insert("notes".to_owned(), u64::try_from(notes.len()).unwrap_or(0));
    stamped.artefact_meta = Some(meta);

    {
        let sessions = partition_mut(partitions, "sessions");
        insert_bytes(
            sessions,
            session.id.as_bytes(),
            json_vec("session", &stamped)?,
        );
        insert_bytes(
            sessions,
            session_key_index_key(&session.nous_id, &session.session_key).as_bytes(),
            session.id.as_bytes().to_vec(),
        );
        insert_bytes(
            sessions,
            session_nous_index_key(&session.nous_id, &session.updated_at, &session.id).as_bytes(),
            Vec::new(),
        );
    }

    add_message_entries(partition_mut(partitions, "messages"), session, messages)?;
    add_usage_entries(partition_mut(partitions, "usage"), usage)?;
    add_distillation_entries(partition_mut(partitions, "distillations"), distillations)?;
    add_note_entries(partition_mut(partitions, "notes"), notes)?;
    add_legacy_entries(
        partition_mut(partitions, "migration_legacy"),
        &session.id,
        legacy,
    )?;

    Ok(())
}

fn add_message_entries(
    entries: &mut EntryMap,
    session: &Session,
    messages: &[&Message],
) -> Result<()> {
    let mut max_seq = 0;
    for message in messages {
        let seq = try_u64("message.seq", message.seq)?;
        let key = format!("{}:{}", message.session_id, pad_u64(seq));
        insert_bytes(entries, key.as_bytes(), json_vec("message", message)?);
        max_seq = max_seq.max(seq);
    }
    if !messages.is_empty() {
        let key = format!("next_seq:{}", session.id);
        insert_bytes(entries, key.as_bytes(), encode_u64(max_seq).to_vec());
    }
    Ok(())
}

fn add_usage_entries(entries: &mut EntryMap, usage: &[&UsageRecord]) -> Result<()> {
    for row in usage {
        let turn_seq = try_u64("usage.turn_seq", row.turn_seq)?;
        let key = format!("{}:{}", row.session_id, pad_u64(turn_seq));
        insert_bytes(entries, key.as_bytes(), json_vec("usage", row)?);
    }
    Ok(())
}

fn add_distillation_entries(
    entries: &mut EntryMap,
    distillations: &[&DistillationRecord],
) -> Result<()> {
    for (idx, row) in distillations.iter().enumerate() {
        let dist_id = try_u64_usize("distillation.local_id", idx + 1)?;
        let key = format!("{}:{}", row.session_id, pad_u64(dist_id));
        insert_bytes(entries, key.as_bytes(), json_vec("distillation", row)?);
    }
    Ok(())
}

fn add_note_entries(entries: &mut EntryMap, notes: &[&AgentNote]) -> Result<()> {
    for (idx, note) in notes.iter().enumerate() {
        let local_id = try_u64_usize("note.local_id", idx + 1)?;
        let local_key = format!("{}:{}", note.session_id, pad_u64(local_id));
        insert_bytes(entries, local_key.as_bytes(), json_vec("note", note)?);

        let gid = try_u64("note.id", note.id)?;
        let gid_key = format!("gid:{}", pad_u64(gid));
        let gid_val = format!("{}:{}", note.session_id, pad_u64(local_id));
        insert_bytes(entries, gid_key.as_bytes(), gid_val.into_bytes());
    }
    Ok(())
}

fn add_legacy_entries(
    entries: &mut EntryMap,
    session_id: &str,
    legacy: &LegacyExtras,
) -> Result<()> {
    if !legacy.is_non_default() {
        return Ok(());
    }

    let bundle_key = format!("{session_id}:bundle");
    insert_bytes(
        entries,
        bundle_key.as_bytes(),
        json_vec("legacy_extras", legacy)?,
    );

    if let Some(value) = legacy.thinking_enabled {
        let key = format!("{session_id}:thinking_enabled");
        insert_bytes(entries, key.as_bytes(), value.to_string().into_bytes());
    }
    if let Some(value) = legacy.thinking_budget {
        let key = format!("{session_id}:thinking_budget");
        insert_bytes(entries, key.as_bytes(), value.to_string().into_bytes());
    }
    if let Some(ref value) = legacy.working_state {
        let key = format!("{session_id}:working_state");
        insert_bytes(entries, key.as_bytes(), value.as_bytes().to_vec());
    }
    if let Some(ref value) = legacy.distillation_priming {
        let key = format!("{session_id}:distillation_priming");
        insert_bytes(entries, key.as_bytes(), value.as_bytes().to_vec());
    }

    Ok(())
}

fn add_blackboard_entries(partitions: &mut PartitionMap, rows: &[BlackboardRow]) -> Result<()> {
    let blackboard = partition_mut(partitions, "blackboard");
    for row in rows {
        insert_bytes(blackboard, row.key.as_bytes(), json_vec("blackboard", row)?);
    }
    Ok(())
}

fn add_legacy_sidecar_entries(entries: &mut EntryMap, rows: &[crate::source::LegacySidecarEntry]) {
    for row in rows {
        insert_bytes(entries, row.key.as_bytes(), row.value.clone());
    }
}

fn add_counter_entries(partitions: &mut PartitionMap, source_data: &SourceData) -> Result<()> {
    let counters = partition_mut(partitions, "counters");
    let max_msg_id = source_data
        .messages
        .iter()
        .map(|message| message.id)
        .max()
        .unwrap_or(0);
    if max_msg_id > 0 {
        insert_bytes(
            counters,
            b"msg_id",
            encode_u64(try_u64("counters.msg_id", max_msg_id)?).to_vec(),
        );
    }

    let max_dist_per_session =
        max_per_session_count(source_data.distillations.iter().map(|row| &row.session_id));
    if max_dist_per_session > 0 {
        insert_bytes(
            counters,
            b"dist_id",
            encode_u64(max_dist_per_session).to_vec(),
        );
    }

    let max_note_id = source_data
        .notes
        .iter()
        .map(|note| note.id)
        .max()
        .unwrap_or(0);
    if max_note_id > 0 {
        insert_bytes(
            counters,
            b"note_global_id",
            encode_u64(try_u64("counters.note_global_id", max_note_id)?).to_vec(),
        );
    }

    let max_note_per_session =
        max_per_session_count(source_data.notes.iter().map(|note| &note.session_id));
    if max_note_per_session > 0 {
        insert_bytes(
            counters,
            b"note_local_id",
            encode_u64(max_note_per_session).to_vec(),
        );
    }

    Ok(())
}

fn dest_partitions(db: &FjallDb) -> Result<PartitionMap> {
    let mut partitions = empty_partitions();
    for &name in ALL_PARTITIONS {
        partitions.insert(name, dest_partition_entries(db, name)?);
    }
    Ok(partitions)
}

fn dest_partition_entries(db: &FjallDb, name: &'static str) -> Result<EntryMap> {
    let partition = db
        .db
        .keyspace(name, KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err(name))?;
    let snap = db.db.read_tx();
    let mut entries = EntryMap::new();
    for guard in snap.range::<&str, _>(&partition, ..) {
        let (key, value) = guard
            .into_inner()
            .map_err(fjall_op_err(format!("scan {name}")))?;
        entries.insert(key.to_vec(), value.to_vec());
    }
    Ok(entries)
}

fn partition_mut<'a>(partitions: &'a mut PartitionMap, name: &'static str) -> &'a mut EntryMap {
    partitions.entry(name).or_default()
}

fn insert_bytes(entries: &mut EntryMap, key: &[u8], value: Vec<u8>) {
    entries.insert(key.to_vec(), value);
}

fn json_vec<T: Serialize>(operation: &str, value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).context(JsonSnafu {
        operation: format!("serialise {operation}"),
    })
}

fn source_per_session_counts(messages: &[Message]) -> Vec<(String, usize)> {
    let mut counts = BTreeMap::new();
    for message in messages {
        *counts.entry(message.session_id.clone()).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

fn source_message_body_hash(messages: &[Message]) -> [u8; 32] {
    let mut buffered: Vec<_> = messages
        .iter()
        .map(|message| {
            (
                message.session_id.as_str(),
                message.seq,
                message.content.as_str(),
            )
        })
        .collect();
    buffered.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(&b.1)));

    let mut h = Sha256::new();
    for (sid, seq, content) in buffered {
        h.update(sid.as_bytes());
        h.update(b"\x1f");
        h.update(seq.to_be_bytes());
        h.update(b"\x1f");
        h.update(content.as_bytes());
        h.update(b"\x1e");
    }
    h.finalize().into()
}

fn fjall_partition_err<S: Into<String>>(
    name: S,
) -> impl FnOnce(fjall::Error) -> crate::error::Error {
    let n = name.into();
    move |e| {
        FjallPartitionSnafu {
            partition: n,
            message: e.to_string(),
        }
        .build()
    }
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

fn dest_session_count(db: &FjallDb) -> Result<usize> {
    let sessions = db
        .db
        .keyspace("sessions", KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err("sessions"))?;
    let snap = db.db.read_tx();
    let idx_prefix = b"idx:".as_slice();
    let mut count = 0usize;
    for guard in snap.range::<&str, _>(&sessions, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan sessions"))?;
        if k.starts_with(idx_prefix) {
            continue;
        }
        if serde_json::from_slice::<Session>(&v).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

fn dest_message_count(db: &FjallDb) -> Result<usize> {
    let messages = db
        .db
        .keyspace("messages", KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err("messages"))?;
    let snap = db.db.read_tx();
    let mut count = 0usize;
    for guard in snap.range::<&str, _>(&messages, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan messages"))?;
        if k.starts_with(b"next_seq:") || k.starts_with(b"distilled:") {
            continue;
        }
        if serde_json::from_slice::<Message>(&v).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

fn dest_message_body_hash(db: &FjallDb) -> Result<[u8; 32]> {
    let messages = db
        .db
        .keyspace("messages", KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err("messages"))?;
    let snap = db.db.read_tx();
    let mut buffered: Vec<(String, i64, String)> = Vec::new();
    for guard in snap.range::<&str, _>(&messages, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan messages"))?;
        if k.starts_with(b"next_seq:") || k.starts_with(b"distilled:") {
            continue;
        }
        let Ok(msg) = serde_json::from_slice::<Message>(&v) else {
            continue;
        };
        buffered.push((msg.session_id, msg.seq, msg.content));
    }
    buffered.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut h = Sha256::new();
    for (sid, seq, content) in buffered {
        h.update(sid.as_bytes());
        h.update(b"\x1f");
        h.update(seq.to_be_bytes());
        h.update(b"\x1f");
        h.update(content.as_bytes());
        h.update(b"\x1e");
    }
    Ok(h.finalize().into())
}

fn dest_per_session_counts(db: &FjallDb) -> Result<BTreeMap<String, usize>> {
    let messages = db
        .db
        .keyspace("messages", KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err("messages"))?;
    let snap = db.db.read_tx();
    let mut counts = BTreeMap::new();
    for guard in snap.range::<&str, _>(&messages, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan messages"))?;
        if k.starts_with(b"next_seq:") || k.starts_with(b"distilled:") {
            continue;
        }
        if let Ok(msg) = serde_json::from_slice::<Message>(&v) {
            *counts.entry(msg.session_id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

fn hash_entries(entries: &EntryMap) -> [u8; 32] {
    let mut h = Sha256::new();
    for (key, value) in entries {
        h.update(u64::try_from(key.len()).unwrap_or(u64::MAX).to_be_bytes());
        h.update(key);
        h.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
        h.update(value);
    }
    h.finalize().into()
}

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

fn pad_u64(value: u64) -> String {
    format!("{value:0>20}")
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn try_u64(field: &str, value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_unused| {
        NumericRangeSnafu {
            field: field.to_owned(),
            value,
        }
        .build()
    })
}

fn try_u64_usize(field: &str, value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_unused| {
        NumericRangeSnafu {
            field: field.to_owned(),
            value: i64::try_from(value).unwrap_or(i64::MAX),
        }
        .build()
    })
}

fn session_key_index_key(nous_id: &str, session_key: &str) -> String {
    format!("idx:key:{nous_id}:{session_key}")
}

fn session_nous_index_key(nous_id: &str, updated_at: &str, session_id: &str) -> String {
    let ts = updated_at;
    format!("idx:nous:{nous_id}:upd:{ts}:{session_id}")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        // WHY: a hex nibble is always 0..=15, so `from_digit` always returns
        // `Some`; the `unwrap_or` fallback is unreachable but keeps this panic-free.
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 0x0f), 16).unwrap_or('0'));
    }
    s
}
