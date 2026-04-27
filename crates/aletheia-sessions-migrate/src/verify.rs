//! Post-migration verification.
//!
//! Opens both source `SQLite` and dest fjall, then compares:
//!
//! 1. Total session count.
//! 2. Total message count (including distilled rows).
//! 3. SHA-256 of every message body in `(session_id, seq)` order.
//! 4. Per-session message count for `samples` deterministically-chosen
//!    sessions.
//!
//! The dest scan reads the fjall keyspace directly because
//! `SessionStore::get_history*` filters out `is_distilled = true` rows.
//! For verification we want a true byte-for-byte equivalence check.

use std::path::Path;

use fjall::{KeyspaceCreateOptions, Readable};
use koina::fjall::FjallDb;
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use snafu::ResultExt as _;
use tracing::info;

use crate::error::{FjallOpSnafu, FjallOpenSnafu, FjallPartitionSnafu, Result, SqliteSnafu};
use crate::migrate::open_source;
use crate::schema;

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
}

impl VerificationReport {
    /// Did every check pass?
    #[must_use]
    pub fn ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Run a verification pass against a freshly-migrated fjall directory.
///
/// # Errors
///
/// Propagates `SQLite` and fjall scan errors.
pub fn run_verification(source: &Path, dest: &Path, samples: usize) -> Result<VerificationReport> {
    let conn = open_source(source)?;
    schema::validate(&conn)?;
    let dest_db = FjallDb::open_existing(dest).map_err(|e| {
        FjallOpenSnafu {
            path: dest.to_path_buf(),
            message: e.to_string(),
        }
        .build()
    })?;

    let (src_sessions, src_messages) = source_counts(&conn)?;
    let dest_sessions = dest_session_count(&dest_db)?;
    let dest_messages = dest_message_count(&dest_db)?;

    let src_hash = source_message_body_hash(&conn)?;
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

    // Per-session sampling.
    let mut src_per_session = source_per_session_counts(&conn)?;
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
    })
}

/// Source-side counts include every session row plus any extra session
/// IDs that appear in dependent tables but have no parent session row
/// (orphans). The migrator synthesises orphan-recovery sessions for
/// each such ID so the dest reflects the complete population.
fn source_counts(conn: &Connection) -> Result<(usize, usize)> {
    // sessions = real session rows + distinct orphan session_ids in any
    // dependent table.
    let s: i64 = conn
        .query_row(
            "WITH all_session_ids AS (
                SELECT id FROM sessions
                UNION SELECT session_id FROM messages
                UNION SELECT session_id FROM usage
                UNION SELECT session_id FROM distillations
                UNION SELECT session_id FROM agent_notes
            )
            SELECT COUNT(*) FROM all_session_ids",
            [],
            |r| r.get(0),
        )
        .context(SqliteSnafu {
            context: "source sessions count (incl orphan IDs)".to_owned(),
        })?;
    let m: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
        .context(SqliteSnafu {
            context: "source messages count".to_owned(),
        })?;
    Ok((
        usize::try_from(s).unwrap_or(0),
        usize::try_from(m).unwrap_or(0),
    ))
}

fn source_per_session_counts(conn: &Connection) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn
        .prepare("SELECT session_id, COUNT(*) FROM messages GROUP BY session_id")
        .context(SqliteSnafu {
            context: "source per-session counts prepare".to_owned(),
        })?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .context(SqliteSnafu {
            context: "source per-session counts query".to_owned(),
        })?;
    let mut out = Vec::new();
    for r in rows {
        let (sid, n) = r.context(SqliteSnafu {
            context: "per-session count row".to_owned(),
        })?;
        out.push((sid, usize::try_from(n).unwrap_or(0)));
    }
    Ok(out)
}

fn source_message_body_hash(conn: &Connection) -> Result<[u8; 32]> {
    let mut stmt = conn
        .prepare("SELECT session_id, seq, content FROM messages ORDER BY session_id, seq")
        .context(SqliteSnafu {
            context: "source body hash prepare".to_owned(),
        })?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .context(SqliteSnafu {
            context: "source body hash query".to_owned(),
        })?;
    let mut h = Sha256::new();
    for r in rows {
        let (sid, seq, content) = r.context(SqliteSnafu {
            context: "body hash row".to_owned(),
        })?;
        h.update(sid.as_bytes());
        h.update(b"\x1f");
        h.update(seq.to_be_bytes());
        h.update(b"\x1f");
        h.update(content.as_bytes());
        h.update(b"\x1e");
    }
    Ok(h.finalize().into())
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
        if serde_json::from_slice::<graphe::types::Session>(&v).is_ok() {
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
        if serde_json::from_slice::<graphe::types::Message>(&v).is_ok() {
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
    // Collect into a Vec so we can sort by (session_id, seq), matching
    // the source-side ORDER BY.
    let mut buffered: Vec<(String, i64, String)> = Vec::new();
    for guard in snap.range::<&str, _>(&messages, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan messages"))?;
        if k.starts_with(b"next_seq:") || k.starts_with(b"distilled:") {
            continue;
        }
        let Ok(msg) = serde_json::from_slice::<graphe::types::Message>(&v) else {
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

fn dest_per_session_counts(db: &FjallDb) -> Result<std::collections::BTreeMap<String, usize>> {
    let messages = db
        .db
        .keyspace("messages", KeyspaceCreateOptions::default)
        .map_err(fjall_partition_err("messages"))?;
    let snap = db.db.read_tx();
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for guard in snap.range::<&str, _>(&messages, ..) {
        let (k, v) = guard.into_inner().map_err(fjall_op_err("scan messages"))?;
        if k.starts_with(b"next_seq:") || k.starts_with(b"distilled:") {
            continue;
        }
        if let Ok(msg) = serde_json::from_slice::<graphe::types::Message>(&v) {
            *counts.entry(msg.session_id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
