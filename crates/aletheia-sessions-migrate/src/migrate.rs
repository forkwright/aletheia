//! Top-level migration orchestration.
//!
//! `stage_migration` is the orchestration entry point: open `SQLite` read-only,
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
//! on the next run and refused unless the operator explicitly requests
//! replacement.
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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    SqliteOpenSnafu, SqliteSnafu,
};
use crate::schema;
use crate::source::{self, DistillationRecord, LegacyExtras, SessionRow};
use crate::verify::{VerificationReport, run_verification};

/// Field-mapping documentation, exported so tests can sanity-check it
/// stays in sync with the actual mapper.
pub(crate) const FIELD_MAPPING_DOC: &str = include_str!("../FIELD_MAPPING.md");

/// `SQLite` busy-timeout applied to the source connection so a transient
/// lock contender does not abort the migration. The migrator is the only
/// writer (read-only mode), but the `SQLite` WAL still locks briefly during
/// recovery.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Suffix for the staging directory created next to the final destination.
const STAGING_SUFFIX: &str = "staging";

/// Suffix segment for backup directories created when replacement publishes
/// over an existing destination.
const BACKUP_SUFFIX: &str = "backup";

const BACKUP_MARKER_MAGIC: &str = "aletheia-sessions-migrate backup\n";

/// What the migrator plans to do (used by `--dry-run`).
#[derive(Debug, Clone)]
pub(crate) struct MigrationPlan {
    /// Path to the `SQLite` source DB.
    pub(crate) source: PathBuf,
    /// Per-table counts read from the source (excludes synthesised orphans).
    pub(crate) counts: TableCounts,
    /// First `session_id` the migrator would touch, for log line cross-reference.
    pub(crate) sample_session_id: Option<String>,
    /// Sessions that carry non-default `thinking_*` / `working_state` /
    /// `distillation_priming` columns; preserved in `migration_legacy`.
    pub(crate) legacy_extras_present: usize,
    /// Legacy-only field entries that would be written to `migration_legacy`.
    pub(crate) legacy_sidecar_entries_present: usize,
    /// Orphan messages (session row missing) that the migrator would
    /// preserve under synthesised orphan-recovery sessions.
    pub(crate) orphan_messages_detected: usize,
    /// Number of distinct orphan `session_ids` the migrator would synthesise.
    pub(crate) orphan_sessions_to_synthesise: usize,
}

/// What the migrator actually did.
#[derive(Debug, Clone)]
pub(crate) struct MigrationReport {
    /// Path to the `SQLite` source DB.
    pub(crate) source: PathBuf,
    /// Path the migrator wrote fjall data to.
    pub(crate) dest: PathBuf,
    /// Per-table counts written (sessions includes synthesised orphans).
    pub(crate) counts: TableCounts,
    /// Sessions whose legacy extras were preserved in `migration_legacy`.
    pub(crate) legacy_extras_preserved: usize,
    /// Legacy-only field entries preserved in `migration_legacy`.
    pub(crate) legacy_sidecar_entries_preserved: usize,
    /// Count of orphan messages (whose parent session row was missing in
    /// the legacy DB) that were preserved under synthesised
    /// `orphan-recovery` sessions.
    pub(crate) orphan_messages_recovered: usize,
    /// Number of synthesised orphan-recovery sessions.
    pub(crate) orphan_sessions_synthesised: usize,
    /// Wall time of the migration.
    pub(crate) elapsed_secs: f64,
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
    pub(crate) legacy_sidecars: Vec<source::LegacySidecarEntry>,
    orphans: OrphanReport,
    legacy_extras_preserved: usize,
}

/// A migration that has been written to a staging directory but not yet
/// published to the final destination.
///
/// Dropping the guard without calling [`Self::publish`] removes the staging
/// directory and restores any backup created for replacement publishes.
pub(crate) struct StagedMigration {
    staging_dir: PathBuf,
    final_dir: PathBuf,
    backup: Option<BackupPaths>,
    report: MigrationReport,
    published: bool,
}

#[derive(Debug, Clone)]
struct BackupPaths {
    dir: PathBuf,
    marker: PathBuf,
}

impl StagedMigration {
    /// Read-only access to the migration report for the staged data.
    #[must_use]
    pub(crate) fn report(&self) -> &MigrationReport {
        &self.report
    }

    /// Run the verification pass against the staged directory.
    ///
    /// # Errors
    ///
    /// Propagates `SQLite` and fjall scan errors.
    pub(crate) fn verify(&self, source: &Path, samples: usize) -> Result<VerificationReport> {
        run_verification(source, &self.staging_dir, samples)
    }

    /// Atomically publish the staged directory to the final destination.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::AtomicRenameFailed`] if the final
    /// atomic rename cannot complete.
    pub(crate) fn publish(mut self) -> Result<MigrationReport> {
        if let Some(ref backup) = self.backup {
            rotate_destination_to_backup(&self.final_dir, backup)?;
        }
        atomic_rename_dir(&self.staging_dir, &self.final_dir)?;
        self.published = true;
        if let Some(ref backup) = self.backup {
            cleanup_marker_owned_backup(backup);
        }
        Ok(self.report.clone())
    }
}

impl Drop for StagedMigration {
    fn drop(&mut self) {
        if self.published {
            return;
        }

        if let Err(source) = fs::remove_dir_all(&self.staging_dir) {
            warn!(
                error = %source,
                staging = %self.staging_dir.display(),
                "failed to remove abandoned migration staging directory"
            );
        }

        if let Some(ref backup) = self.backup {
            restore_backup(&self.final_dir, backup);
        }
    }
}

/// Open the source `SQLite` DB read-only with a sane busy-timeout.
///
/// # Errors
///
/// Returns [`crate::error::Error::SqliteOpen`] when the source path
/// cannot be opened or the busy-timeout PRAGMA fails to apply.
pub(crate) fn open_source(path: &Path) -> Result<Connection> {
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
pub(crate) fn run_dry_run(source: &Path) -> Result<MigrationPlan> {
    let conn = open_source(source)?;
    schema::validate(&conn)?;
    let session_rows = source::read_sessions(&conn)?;
    let messages = source::read_messages(&conn)?;
    let usage = source::read_usage(&conn)?;
    let dists = source::read_distillations(&conn)?;
    let notes = source::read_notes(&conn)?;
    let blackboard = source::read_blackboard(&conn)?;
    let legacy_sidecars = source::read_legacy_sidecars(&conn)?;
    let legacy_extras_present = session_rows
        .iter()
        .filter(|s| s.legacy.is_non_default())
        .count();
    let orphans = detect_orphans(&session_rows, &messages, &usage, &dists, &notes);
    let plan = MigrationPlan {
        source: source.to_path_buf(),
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
        legacy_sidecar_entries_present: legacy_sidecars.len(),
        orphan_messages_detected: orphans.orphan_message_count,
        orphan_sessions_to_synthesise: orphans.synthetic_sessions.len(),
    };
    Ok(plan)
}

/// Stage a full migration without publishing it.
///
/// Returns a [`StagedMigration`] guard that owns the staging directory.
/// Call [`StagedMigration::publish`] to atomically rename the staging
/// directory to `dest`. Dropping the guard without publishing cleans up
/// the staging directory and restores any backup created for a replacement
/// overwrite.
///
/// # Errors
///
/// Returns [`crate::error::Error::MigrationIncomplete`] when a previous
/// run left a staging directory behind and `replace_existing` is false,
/// [`crate::error::Error::DestinationNotEmpty`] when `dest` is non-empty
/// and `replace_existing` is false, or any source/fjall error.
pub(crate) fn stage_migration(
    source: &Path,
    dest: &Path,
    replace_existing: bool,
) -> Result<StagedMigration> {
    let started = Instant::now();
    let staging_dir = dest.with_extension(STAGING_SUFFIX);

    let dest_existed = prepare_destination_dirs(dest, replace_existing, &staging_dir)?;

    // Read and validate the source before touching the destination.
    let conn = open_source(source)?;
    schema::validate(&conn)?;

    let source_data = load_source_data(&conn)?;

    let counts = match write_staging(&staging_dir, &source_data) {
        Ok(counts) => counts,
        Err(err) => {
            if let Err(source) = fs::remove_dir_all(&staging_dir) {
                warn!(
                    error = %source,
                    staging = %staging_dir.display(),
                    "failed to remove migration staging directory after write error"
                );
            }
            return Err(err);
        }
    };

    fsync_dir(dest.parent().unwrap_or_else(|| Path::new(".")))?;

    let report = MigrationReport {
        source: source.to_path_buf(),
        dest: dest.to_path_buf(),
        counts,
        legacy_extras_preserved: source_data.legacy_extras_preserved,
        legacy_sidecar_entries_preserved: source_data.legacy_sidecars.len(),
        orphan_messages_recovered: source_data.orphans.orphan_message_count,
        orphan_sessions_synthesised: source_data.orphans.synthetic_sessions.len(),
        elapsed_secs: started.elapsed().as_secs_f64(),
    };

    Ok(StagedMigration {
        staging_dir,
        final_dir: dest.to_path_buf(),
        backup: dest_existed.then(|| unique_backup_paths(dest)),
        report,
        published: false,
    })
}

/// Check destination preconditions and clean up leftover staging when
/// replacement is set. Returns whether a non-empty destination already exists.
fn prepare_destination_dirs(
    dest: &Path,
    replace_existing: bool,
    staging_dir: &Path,
) -> Result<bool> {
    if staging_dir.exists() {
        if !replace_existing {
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
    if dest_existed && !replace_existing {
        return Err(DestinationNotEmptySnafu {
            path: dest.to_path_buf(),
        }
        .build());
    }

    Ok(dest_existed)
}

/// Rotate an existing destination directory to the backup path.
fn rotate_destination_to_backup(dest: &Path, backup: &BackupPaths) -> Result<()> {
    fs::rename(dest, &backup.dir).context(IoSnafu {
        context: format!(
            "moving existing destination {} to backup {}",
            dest.display(),
            backup.dir.display()
        ),
    })?;
    let marker = format!(
        "{BACKUP_MARKER_MAGIC}dest={}\nbackup={}\n",
        dest.display(),
        backup.dir.display()
    );
    fs::write(&backup.marker, marker).context(IoSnafu {
        context: format!("writing backup marker {}", backup.marker.display()),
    })?;
    fsync_dir(dest.parent().unwrap_or_else(|| Path::new(".")))?;
    Ok(())
}

fn unique_backup_paths(dest: &Path) -> BackupPaths {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let base = dest
        .file_name()
        .map_or_else(|| "destination".into(), |name| name.to_string_lossy());
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let pid = std::process::id();

    for attempt in 0..1024u16 {
        let dir = parent.join(format!("{base}.{BACKUP_SUFFIX}-{stamp}-{pid}-{attempt}"));
        let marker = backup_marker_path(&dir);
        if !dir.exists() && !marker.exists() {
            return BackupPaths { dir, marker };
        }
    }

    let dir = parent.join(format!("{base}.{BACKUP_SUFFIX}-{stamp}-{pid}-fallback"));
    let marker = backup_marker_path(&dir);
    BackupPaths { dir, marker }
}

fn backup_marker_path(backup_dir: &Path) -> PathBuf {
    let mut marker = backup_dir.as_os_str().to_os_string();
    marker.push(".marker");
    PathBuf::from(marker)
}

fn cleanup_marker_owned_backup(backup: &BackupPaths) {
    if !backup_marker_is_owned(backup) {
        warn!(
            backup = %backup.dir.display(),
            marker = %backup.marker.display(),
            "leaving replacement backup in place because marker ownership was not confirmed"
        );
        return;
    }

    if let Err(source) = fs::remove_dir_all(&backup.dir) {
        warn!(
            error = %source,
            backup = %backup.dir.display(),
            "failed to remove replacement backup after publish"
        );
        return;
    }

    remove_backup_marker(backup);
}

fn backup_marker_is_owned(backup: &BackupPaths) -> bool {
    match fs::read_to_string(&backup.marker) {
        Ok(contents) => contents.starts_with(BACKUP_MARKER_MAGIC),
        Err(source) => {
            warn!(
                error = %source,
                marker = %backup.marker.display(),
                "failed to read replacement backup marker"
            );
            false
        }
    }
}

fn restore_backup(final_dir: &Path, backup: &BackupPaths) {
    if !backup.dir.exists() {
        return;
    }
    if final_dir.exists() {
        warn!(
            final_dir = %final_dir.display(),
            backup = %backup.dir.display(),
            "leaving replacement backup in place because final destination exists"
        );
        return;
    }

    if let Err(source) = fs::rename(&backup.dir, final_dir) {
        warn!(
            error = %source,
            final_dir = %final_dir.display(),
            backup = %backup.dir.display(),
            "failed to restore replacement backup"
        );
        return;
    }

    remove_backup_marker(backup);
    if let Err(err) = fsync_dir(final_dir.parent().unwrap_or_else(|| Path::new("."))) {
        warn!(
            error = ?err,
            final_dir = %final_dir.display(),
            "failed to fsync restored destination parent directory"
        );
    }
}

fn remove_backup_marker(backup: &BackupPaths) {
    match fs::remove_file(&backup.marker) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            warn!(
                error = %source,
                marker = %backup.marker.display(),
                "failed to remove replacement backup marker"
            );
        }
    }
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
    let legacy_sidecars = source::read_legacy_sidecars(conn)?;
    info!(
        sessions = session_rows.len(),
        messages = messages.len(),
        usage = usage.len(),
        distillations = dists.len(),
        notes = notes.len(),
        blackboard = blackboard.len(),
        legacy_sidecars = legacy_sidecars.len(),
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
        legacy_sidecars,
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
        &source_data.legacy_sidecars,
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
