//! Top-level migration orchestration.
//!
//! `run_migration` is the single entry point: open `SQLite` read-only,
//! validate schema, optionally read everything for a dry-run plan, then
//! open the fjall staging destination, verify it, and publish atomically.
//!
//! Source `schema_version` is asserted equal to
//! [`schema::REQUIRED_USER_VERSION`] before any read and the post-write
//! verification pass compares deterministic key/value hashes for every
//! migrated fjall partition; any mismatch aborts before publish.
//!
//! # Durability
//!
//! Migration writes to a staging directory adjacent to the requested
//! destination and atomically renames it into place only after every
//! per-session bundle, the blackboard, and global counters have committed.
//! If the process is interrupted, the leftover staging directory is detected
//! on the next run and refused unless the operator passes `--force`.
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
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graphe::types::{
    AgentNote, BlackboardRow, Message, Session, SessionMetrics, SessionOrigin, SessionStatus,
    SessionType, UsageRecord,
};
use rusqlite::{Connection, OpenFlags};
use snafu::ResultExt as _;
use tracing::{info, warn};

use crate::dest::{Destination, TableCounts, is_empty_or_absent};
use crate::error::{
    AtomicRenameFailedSnafu, DestinationNotEmptySnafu, IoSnafu, MigrationIncompleteSnafu, Result,
    SqliteOpenSnafu, SqliteSnafu, VerificationFailedSnafu,
};
use crate::schema;
use crate::source::{self, DistillationRecord, LegacyExtras, SessionRow};
use crate::verify::{VerificationReport, run_verification};

/// Field-mapping documentation, exported so tests can sanity-check it
/// stays in sync with the actual mapper.
pub const FIELD_MAPPING_DOC: &str = include_str!("../FIELD_MAPPING.md");

/// `SQLite` busy-timeout applied to the source connection so a transient
/// lock contender does not abort the migration. The migrator is the only
/// writer (read-only mode), but the `SQLite` WAL still locks briefly during
/// recovery.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Suffix for the staging directory created next to the final destination.
const STAGING_SUFFIX: &str = "staging";

/// Suffix for the backup directory created when `--force` overwrites an
/// existing destination.
const BACKUP_SUFFIX: &str = "backup";

/// Default deterministic per-session sample count retained for the
/// compatibility portion of verification reports.
const DEFAULT_VERIFY_SAMPLES: usize = 16;

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

/// All data read from the source `SQLite` DB, ready for the destination
/// writer.
pub(crate) struct SourceData {
    pub(crate) sessions: Vec<(Session, LegacyExtras)>,
    pub(crate) messages: Vec<Message>,
    pub(crate) usage: Vec<UsageRecord>,
    pub(crate) distillations: Vec<DistillationRecord>,
    pub(crate) notes: Vec<AgentNote>,
    pub(crate) blackboard: Vec<BlackboardRow>,
    orphans: OrphanReport,
    legacy_extras_preserved: usize,
}

/// A migration that has been written to a staging directory but not yet
/// published to the final destination.
///
/// Dropping the guard without calling [`Self::publish`] removes the staging
/// directory and restores any backup created for `--force` overwrites.
pub struct StagedMigration {
    staging_dir: PathBuf,
    final_dir: PathBuf,
    backup_dir: Option<PathBuf>,
    report: MigrationReport,
    published: bool,
}

impl StagedMigration {
    /// Read-only access to the migration report for the staged data.
    #[must_use]
    pub fn report(&self) -> &MigrationReport {
        &self.report
    }

    /// Run the verification pass against the staged directory.
    ///
    /// # Errors
    ///
    /// Propagates `SQLite` and fjall scan errors.
    pub fn verify(&self, source: &Path, samples: usize) -> Result<VerificationReport> {
        run_verification(source, &self.staging_dir, samples)
    }

    /// Atomically publish the staged directory to the final destination.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::AtomicRenameFailed`] if the final
    /// atomic rename cannot complete.
    pub fn publish(mut self) -> Result<MigrationReport> {
        atomic_rename_dir(&self.staging_dir, &self.final_dir)?;
        self.published = true;
        if let Some(ref backup) = self.backup_dir {
            let _ = fs::remove_dir_all(backup);
        }
        Ok(self.report.clone())
    }
}

impl Drop for StagedMigration {
    fn drop(&mut self) {
        if self.published {
            return;
        }

        // Migration failed or was abandoned: remove the partial staging
        // directory so later tooling does not treat it as authoritative.
        let _ = fs::remove_dir_all(&self.staging_dir);

        // Restore the backup so the operator does not lose the prior store
        // when `--force` overwrite was in progress.
        if let Some(ref backup) = self.backup_dir {
            let _ = fs::rename(backup, &self.final_dir);
        }
    }
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

/// Run a full migration. Reads source, validates, writes fjall to a staging
/// directory, verifies every migrated partition, then atomically renames it
/// to `dest`.
///
/// # Errors
///
/// Propagates schema validation failures, source read errors, any fjall
/// write failure, or an atomic-rename error as the structured error type
/// defined in [`crate::error`].
pub fn run_migration(source: &Path, dest: &Path, force: bool) -> Result<MigrationReport> {
    let staged = stage_migration(source, dest, force)?;
    let verification = staged.verify(source, DEFAULT_VERIFY_SAMPLES)?;
    if !verification.ok() {
        return Err(VerificationFailedSnafu {
            mismatches: verification.mismatches.len(),
            summary: verification.mismatches.join("; "),
        }
        .build());
    }
    staged.publish()
}

/// Stage a full migration without publishing it.
///
/// Returns a [`StagedMigration`] guard that owns the staging directory.
/// Call [`StagedMigration::publish`] to atomically rename the staging
/// directory to `dest`. Dropping the guard without publishing cleans up
/// the staging directory and restores any backup created for a `--force`
/// overwrite.
///
/// # Errors
///
/// Returns [`crate::error::Error::MigrationIncomplete`] when a previous
/// run left a staging directory behind and `force` is false,
/// [`crate::error::Error::DestinationNotEmpty`] when `dest` is non-empty
/// and `force` is false, or any source/fjall error.
pub fn stage_migration(source: &Path, dest: &Path, force: bool) -> Result<StagedMigration> {
    let started = Instant::now();
    let staging_dir = dest.with_extension(STAGING_SUFFIX);
    let backup_dir = dest.with_extension(BACKUP_SUFFIX);

    let dest_existed = prepare_destination_dirs(dest, force, &staging_dir, &backup_dir)?;

    // Read and validate the source before touching the destination.
    let conn = open_source(source)?;
    schema::validate(&conn)?;

    let source_data = load_source_data(&conn)?;

    // Move an existing destination out of the way when `--force` is set.
    // This is done after source validation so we do not wipe live data for
    // an invalid migration input.
    if dest_existed {
        rotate_destination_to_backup(dest, &backup_dir)?;
    }

    let counts = write_staging(&staging_dir, &source_data)?;

    fsync_dir(dest.parent().unwrap_or_else(|| Path::new(".")))?;

    let report = MigrationReport {
        source: source.to_path_buf(),
        dest: dest.to_path_buf(),
        counts,
        legacy_extras_preserved: source_data.legacy_extras_preserved,
        orphan_messages_recovered: source_data.orphans.orphan_message_count,
        orphan_sessions_synthesised: source_data.orphans.synthetic_sessions.len(),
        elapsed_secs: started.elapsed().as_secs_f64(),
    };

    Ok(StagedMigration {
        staging_dir,
        final_dir: dest.to_path_buf(),
        backup_dir: if dest_existed { Some(backup_dir) } else { None },
        report,
        published: false,
    })
}

/// Check destination preconditions and clean up leftover staging when
/// `force` is set. Returns whether a non-empty destination already exists.
fn prepare_destination_dirs(
    dest: &Path,
    force: bool,
    staging_dir: &Path,
    backup_dir: &Path,
) -> Result<bool> {
    if staging_dir.exists() {
        if !force {
            return Err(MigrationIncompleteSnafu {
                path: dest.to_path_buf(),
                marker: staging_dir.display().to_string(),
            }
            .build());
        }
        fs::remove_dir_all(staging_dir).context(IoSnafu {
            context: format!(
                "removing leftover staging directory {}",
                staging_dir.display()
            ),
        })?;
    }

    let dest_existed = dest.exists() && !is_empty_or_absent(dest)?;
    if dest_existed && !force {
        return Err(DestinationNotEmptySnafu {
            path: dest.to_path_buf(),
        }
        .build());
    }

    if backup_dir.exists() {
        fs::remove_dir_all(backup_dir).context(IoSnafu {
            context: format!("removing old backup directory {}", backup_dir.display()),
        })?;
    }

    Ok(dest_existed)
}

/// Rotate an existing destination directory to the backup path.
fn rotate_destination_to_backup(dest: &Path, backup_dir: &Path) -> Result<()> {
    fs::rename(dest, backup_dir).context(IoSnafu {
        context: format!(
            "moving existing destination {} to backup {}",
            dest.display(),
            backup_dir.display()
        ),
    })
}

/// Read every table from the validated source connection and return the
/// data in the shape the destination writer expects.
pub(crate) fn load_source_data(conn: &Connection) -> Result<SourceData> {
    info!("reading SQLite source");
    let session_rows = source::read_sessions(conn)?;
    let messages = source::read_messages(conn)?;
    let usage = source::read_usage(conn)?;
    let dists = source::read_distillations(conn)?;
    let notes = source::read_notes(conn)?;
    let blackboard = source::read_blackboard(conn)?;
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

    let sessions_pairs: Vec<_> = all_sessions
        .into_iter()
        .map(|s| (s.session, s.legacy))
        .collect();

    Ok(SourceData {
        sessions: sessions_pairs,
        messages,
        usage,
        distillations: dists,
        notes,
        blackboard,
        orphans,
        legacy_extras_preserved,
    })
}

/// Write all source data to the staging directory and flush it to disk.
fn write_staging(staging_dir: &Path, source_data: &SourceData) -> Result<TableCounts> {
    let dest_handle = Destination::open(staging_dir)?;
    let counts = dest_handle.write_all(
        &source_data.sessions,
        &source_data.messages,
        &source_data.usage,
        &source_data.distillations,
        &source_data.notes,
        &source_data.blackboard,
    )?;

    // Make the staging store durable before the atomic publish.
    dest_handle.persist()?;
    drop(dest_handle);

    Ok(counts)
}

/// Atomically rename `source` to `dest`.
///
/// On Unix this is atomic when both paths live on the same filesystem.
/// The parent directory is fsynced after the rename so the operation
/// survives a crash.
fn atomic_rename_dir(source: &Path, dest: &Path) -> Result<()> {
    fs::rename(source, dest).context(AtomicRenameFailedSnafu {
        source_path: source.to_path_buf(),
        dest_path: dest.to_path_buf(),
    })?;
    fsync_dir(dest.parent().unwrap_or_else(|| Path::new(".")))?;
    Ok(())
}

/// Fsync a directory so a preceding rename is durable.
#[cfg(unix)]
fn fsync_dir(path: &Path) -> Result<()> {
    let file = fs::File::open(path).context(IoSnafu {
        context: format!("opening directory {} for fsync", path.display()),
    })?;
    if file.metadata().is_ok_and(|m| m.file_type().is_dir()) {
        file.sync_all().context(IoSnafu {
            context: format!("fsync directory {}", path.display()),
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn fsync_dir(path: &Path) -> Result<()> {
    // Directory fsync is not portable; on non-Unix targets the atomic
    // rename itself is the best-effort durability boundary.
    let _ = path;
    Ok(())
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
