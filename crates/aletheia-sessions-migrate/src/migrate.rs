//! Top-level migration orchestration.
//!
//! `run_migration` is the single entry point: open `SQLite` read-only,
//! validate schema, optionally read everything for a dry-run plan, then
//! open the fjall destination and write atomically per session.
//!
//! Source `schema_version` is asserted equal to
//! [`schema::REQUIRED_USER_VERSION`] before any read and the post-write
//! `--verify` pass computes a SHA-256 checksum over message bodies on
//! both stores; any mismatch aborts with a non-zero exit status.
//!
//! # Orphan recovery
//!
//! The legacy schema declared `messages.session_id REFERENCES sessions(id)`
//! without `ON DELETE CASCADE` (#2959). Live operator data therefore can
//! contain messages whose parent session row has been deleted. Per the
//! no-compromise contract, the migrator never silently drops these rows —
//! instead it synthesises a phantom `Session` for each orphan group,
//! marked `status = 'archived'` and `nous_id = "orphan-recovery"`, so
//! the messages stay queryable through the runtime API. Orphan counts
//! are surfaced on `MigrationReport`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graphe::types::{
    AgentNote, Message, Session, SessionMetrics, SessionOrigin, SessionStatus, SessionType,
    UsageRecord,
};
use rusqlite::{Connection, OpenFlags};
use snafu::ResultExt as _;
use tracing::{info, warn};

use crate::dest::{Destination, TableCounts};
use crate::error::{Result, SqliteOpenSnafu, SqliteSnafu};
use crate::schema;
use crate::source::{self, DistillationRecord, LegacyExtras, SessionRow};

/// Field-mapping documentation, exported so tests can sanity-check it
/// stays in sync with the actual mapper.
pub const FIELD_MAPPING_DOC: &str = include_str!("../FIELD_MAPPING.md");

/// `SQLite` busy-timeout applied to the source connection so a transient
/// lock contender does not abort the migration. The migrator is the only
/// writer (read-only mode), but the `SQLite` WAL still locks briefly during
/// recovery.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// What the migrator plans to do (used by `--dry-run`).
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    /// Path to the `SQLite` source DB.
    pub source: PathBuf,
    /// Path the migrator would write fjall data to.
    pub dest: PathBuf,
    /// Per-table counts read from the source (excludes synthesised orphans).
    pub counts: TableCounts,
    /// First `session_id` the migrator would touch, for log line cross-reference.
    pub sample_session_id: Option<String>,
    /// Sessions that carry non-default `thinking_*` / `working_state` /
    /// `distillation_priming` columns; preserved in `migration_legacy`.
    pub legacy_extras_present: usize,
    /// Orphan messages (session row missing) that the migrator would
    /// preserve under synthesised orphan-recovery sessions.
    pub orphan_messages_detected: usize,
    /// Number of distinct orphan `session_ids` the migrator would synthesise.
    pub orphan_sessions_to_synthesise: usize,
}

/// What the migrator actually did.
#[derive(Debug, Clone)]
pub struct MigrationReport {
    /// Path to the `SQLite` source DB.
    pub source: PathBuf,
    /// Path the migrator wrote fjall data to.
    pub dest: PathBuf,
    /// Per-table counts written (sessions includes synthesised orphans).
    pub counts: TableCounts,
    /// Sessions whose legacy extras were preserved in `migration_legacy`.
    pub legacy_extras_preserved: usize,
    /// Count of orphan messages (whose parent session row was missing in
    /// the legacy DB) that were preserved under synthesised
    /// `orphan-recovery` sessions.
    pub orphan_messages_recovered: usize,
    /// Number of synthesised orphan-recovery sessions.
    pub orphan_sessions_synthesised: usize,
    /// Wall time of the migration.
    pub elapsed_secs: f64,
}

/// Open the source `SQLite` DB read-only with a sane busy-timeout.
///
/// # Errors
///
/// Returns [`crate::error::Error::SqliteOpen`] when the source path
/// cannot be opened or the busy-timeout PRAGMA fails to apply.
pub fn open_source(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context(SqliteOpenSnafu {
        path: path.to_path_buf(),
    })?;
    conn.busy_timeout(SQLITE_BUSY_TIMEOUT)
        .context(SqliteSnafu {
            context: "setting busy_timeout".to_owned(),
        })?;
    Ok(conn)
}

/// Run a dry-run plan: read source, validate, summarise. No fjall writes.
///
/// # Errors
///
/// Propagates schema validation failures and any source read errors.
pub fn run_dry_run(source: &Path) -> Result<MigrationPlan> {
    let conn = open_source(source)?;
    schema::validate(&conn)?;
    let session_rows = source::read_sessions(&conn)?;
    let messages = source::read_messages(&conn)?;
    let usage = source::read_usage(&conn)?;
    let dists = source::read_distillations(&conn)?;
    let notes = source::read_notes(&conn)?;
    let blackboard = source::read_blackboard(&conn)?;
    let legacy_extras_present = session_rows
        .iter()
        .filter(|s| s.legacy.is_non_default())
        .count();
    let orphans = detect_orphans(&session_rows, &messages, &usage, &dists, &notes);
    let plan = MigrationPlan {
        source: source.to_path_buf(),
        dest: PathBuf::new(),
        counts: TableCounts {
            sessions: session_rows.len(),
            messages: messages.len(),
            usage: usage.len(),
            distillations: dists.len(),
            notes: notes.len(),
            blackboard: blackboard.len(),
        },
        sample_session_id: session_rows.first().map(|s| s.session.id.clone()),
        legacy_extras_present,
        orphan_messages_detected: orphans.orphan_message_count,
        orphan_sessions_to_synthesise: orphans.synthetic_sessions.len(),
    };
    Ok(plan)
}

/// Run a full migration. Reads source, validates, writes fjall.
///
/// # Errors
///
/// Propagates schema validation failures, source read errors, and any
/// fjall write failure as the structured error type defined in
/// [`crate::error`].
pub fn run_migration(source: &Path, dest: &Path, force: bool) -> Result<MigrationReport> {
    let started = Instant::now();
    let conn = open_source(source)?;
    schema::validate(&conn)?;

    info!(source = %source.display(), "reading SQLite source");
    let session_rows = source::read_sessions(&conn)?;
    let messages = source::read_messages(&conn)?;
    let usage = source::read_usage(&conn)?;
    let dists = source::read_distillations(&conn)?;
    let notes = source::read_notes(&conn)?;
    let blackboard = source::read_blackboard(&conn)?;
    info!(
        sessions = session_rows.len(),
        messages = messages.len(),
        usage = usage.len(),
        distillations = dists.len(),
        notes = notes.len(),
        blackboard = blackboard.len(),
        "source loaded"
    );

    let legacy_extras_preserved = session_rows
        .iter()
        .filter(|s| s.legacy.is_non_default())
        .count();

    // Detect orphan messages and synthesise phantom sessions for them.
    let orphans = detect_orphans(&session_rows, &messages, &usage, &dists, &notes);
    if !orphans.synthetic_sessions.is_empty() {
        warn!(
            orphan_sessions = orphans.synthetic_sessions.len(),
            orphan_messages = orphans.orphan_message_count,
            "orphan messages detected; synthesising orphan-recovery sessions to preserve data"
        );
    }

    let mut all_sessions = session_rows;
    all_sessions.extend(orphans.synthetic_sessions.iter().cloned());

    let dest_handle = Destination::open(dest, force)?;
    let sessions_pairs: Vec<_> = all_sessions
        .into_iter()
        .map(|s| (s.session, s.legacy))
        .collect();
    let counts = dest_handle.write_all(
        &sessions_pairs,
        &messages,
        &usage,
        &dists,
        &notes,
        &blackboard,
    )?;

    Ok(MigrationReport {
        source: source.to_path_buf(),
        dest: dest.to_path_buf(),
        counts,
        legacy_extras_preserved,
        orphan_messages_recovered: orphans.orphan_message_count,
        orphan_sessions_synthesised: orphans.synthetic_sessions.len(),
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

/// Inventory of orphan rows (messages / usage / dists / notes whose
/// parent session row is missing) plus the synthesised phantom sessions
/// that the migrator will write so the data is preserved.
struct OrphanReport {
    synthetic_sessions: Vec<SessionRow>,
    orphan_message_count: usize,
}

fn detect_orphans(
    sessions: &[SessionRow],
    messages: &[Message],
    usage: &[UsageRecord],
    dists: &[DistillationRecord],
    notes: &[AgentNote],
) -> OrphanReport {
    let known: BTreeSet<&str> = sessions.iter().map(|s| s.session.id.as_str()).collect();

    // Group orphan rows by session_id so we synthesise one phantom per group.
    let mut orphan_message_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut orphan_message_total = 0;
    for m in messages {
        if !known.contains(m.session_id.as_str()) {
            *orphan_message_counts
                .entry(m.session_id.clone())
                .or_insert(0) += 1;
            orphan_message_total += 1;
        }
    }
    let mut other_orphan_ids: BTreeSet<String> = BTreeSet::new();
    for u in usage {
        if !known.contains(u.session_id.as_str()) {
            other_orphan_ids.insert(u.session_id.clone());
        }
    }
    for d in dists {
        if !known.contains(d.session_id.as_str()) {
            other_orphan_ids.insert(d.session_id.clone());
        }
    }
    for n in notes {
        if !known.contains(n.session_id.as_str()) {
            other_orphan_ids.insert(n.session_id.clone());
        }
    }
    // Union the keys.
    let mut all_orphan_ids: BTreeSet<String> = orphan_message_counts.keys().cloned().collect();
    all_orphan_ids.extend(other_orphan_ids);

    let synthetic_sessions: Vec<SessionRow> = all_orphan_ids
        .iter()
        .map(|id| synthesise_orphan_session(id, *orphan_message_counts.get(id).unwrap_or(&0)))
        .collect();

    OrphanReport {
        synthetic_sessions,
        orphan_message_count: orphan_message_total,
    }
}

fn synthesise_orphan_session(session_id: &str, msg_count: usize) -> SessionRow {
    let now = "2026-04-27T00:00:00.000Z".to_owned();
    SessionRow {
        session: Session {
            id: session_id.to_owned(),
            nous_id: "orphan-recovery".to_owned(),
            session_key: format!("orphan:{session_id}"),
            status: SessionStatus::Archived,
            model: None,
            session_type: SessionType::Primary,
            created_at: now.clone(),
            updated_at: now,
            metrics: SessionMetrics {
                token_count_estimate: 0,
                message_count: i64::try_from(msg_count).unwrap_or(0),
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
                display_name: Some(format!(
                    "Recovered orphan session {session_id} ({msg_count} message(s))"
                )),
            },
            artefact_meta: None,
        },
        legacy: LegacyExtras::default(),
    }
}
