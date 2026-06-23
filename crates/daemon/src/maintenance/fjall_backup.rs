//! Fjall knowledge store backup: periodic file-level copy to timestamped directory.
//!
//! WHY(#3381): the fjall knowledge store has no built-in backup mechanism.
//! If the fjall DB files are corrupted or the machine dies, all session and
//! knowledge data is lost. This module implements periodic file-level backups
//! with configurable retention.
//!
//! WHY(#4645): backups are built in a hidden staging directory, verified, then
//! atomically published. A `backup-complete.json` manifest marks a completed
//! restore point so interrupted backups are never listed or pruned.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fjall::Readable;
use snafu::ResultExt;
use tracing::{info, warn};

use crate::error;

/// Configuration for fjall knowledge store backups.
#[derive(Debug, Clone)]
pub struct FjallBackupConfig {
    /// Whether periodic fjall backups are enabled.
    pub enabled: bool,
    /// Path to the fjall knowledge store data directory.
    pub source_dir: PathBuf,
    /// Directory where timestamped backups are stored.
    pub backup_dir: PathBuf,
    /// Hours between automatic backups.
    pub interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub retention_count: usize,
}

impl Default for FjallBackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source_dir: PathBuf::from("data/knowledge.fjall"),
            backup_dir: PathBuf::from("data/backups/fjall"),
            interval_hours: 24,
            retention_count: 7,
        }
    }
}

/// Outcome of a fjall backup run.
#[derive(Debug, Clone, Default)]
pub struct FjallBackupReport {
    /// Path to the created backup directory.
    pub backup_path: Option<PathBuf>,
    /// Total bytes copied.
    pub bytes_copied: u64,
    /// Number of files copied.
    pub files_copied: u32,
    /// Number of old backups pruned.
    pub backups_pruned: u32,
}

/// A single backup entry found on disk.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    /// Directory name (timestamp-based).
    pub name: String,
    /// Full path to the backup directory.
    pub path: PathBuf,
    /// When the backup was created (from manifest `created_at`).
    pub created: SystemTime,
    /// Total size of the backup in bytes.
    pub size_bytes: u64,
}

/// Result of verifying a single fjall store directory.
#[derive(Debug, Clone, Default)]
pub struct FjallVerifyResult {
    /// Per-partition key counts.
    pub partition_counts: Vec<(String, usize)>,
    /// First validation error encountered, if any.
    pub first_error: Option<String>,
    /// Total keys iterated across all partitions.
    pub total_keys: usize,
    /// Fjall sequence number (generation) captured at open time, if available.
    ///
    /// WHY(#4950): recording the store generation lets restore detect mismatched
    /// write points between stores copied under the same snapshot epoch.
    pub seqno: Option<u64>,
}

/// Manifest format version for fjall backups.
const MANIFEST_VERSION: &str = "aletheia-fjall-backup-v1";

/// Filename of the completion marker inside a published backup directory.
///
/// WHY(#4645): a backup is only considered complete and restorable once this
/// manifest has been written. `list_backups` and pruning rely on its presence.
const COMPLETE_MARKER: &str = "backup-complete.json";

/// Prefix for hidden staging directories inside `backup_dir`.
///
/// WHY(#4645): staging directories are skipped by `list_backups` so an
/// in-progress or crashed backup is never listed as a valid restore point.
const STAGING_DIR_PREFIX: &str = ".aletheia-fjall-backup-staging.";

/// Manifest describing a completed fjall backup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FjallBackupManifest {
    /// Manifest format version.
    version: String,
    /// ISO 8601 timestamp when the backup was created.
    created_at: String,
    /// Absolute source path the backup was copied from.
    source_path: PathBuf,
    /// Total bytes copied.
    bytes_copied: u64,
    /// Number of files copied.
    files_copied: u32,
    /// Fjall sequence number captured during verification, if available.
    seqno: Option<u64>,
    /// Whether the staged copy was successfully verified.
    verified: bool,
}

/// Manages fjall knowledge store backups.
pub struct FjallBackup {
    config: FjallBackupConfig,
}

impl FjallBackup {
    /// Create a new backup manager with the given configuration.
    #[must_use]
    pub fn new(config: FjallBackupConfig) -> Self {
        Self { config }
    }

    /// Create a backup by copying the fjall data directory to a timestamped subdirectory.
    ///
    /// The backup directory name uses ISO 8601 format: `YYYYMMDD-HHMMSS`.
    /// After creating the backup, old backups beyond `retention_count` are pruned.
    pub fn create_backup(&self) -> error::Result<FjallBackupReport> {
        self.create_backup_with_quiesce(|| Ok(()))
    }

    /// Create a backup with an optional quiesce hook.
    ///
    /// WHY(#4645): the caller can flush the live fjall store (e.g., call
    /// `persist(SyncAll)`) and pause writers before the filesystem copy begins.
    /// Fjall itself does not yet expose an online checkpoint API, so a
    /// caller-supplied quiesce hook is the strongest consistency guarantee
    /// available.
    ///
    /// The backup is built in a hidden staging directory, verified, then
    /// atomically renamed into place. A `backup-complete.json` manifest is
    /// written so interrupted backups are never listed or pruned.
    pub fn create_backup_with_quiesce<F>(&self, quiesce: F) -> error::Result<FjallBackupReport>
    where
        F: FnOnce() -> error::Result<()>,
    {
        if !self.config.source_dir.exists() {
            info!(
                source = %self.config.source_dir.display(),
                "fjall backup skipped: source directory does not exist"
            );
            return Ok(FjallBackupReport::default());
        }

        fs::create_dir_all(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "creating fjall backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        // WHY(#4645): remove staging directories left behind by crashed or
        // interrupted backup runs before starting a new one.
        self.cleanup_stale_staging_dirs()?;

        let (staging_path, final_path) = self.prepare_staging_path()?;

        // WHY(#4645): if the caller can quiesce the live store (flush WAL, pause
        // writers), invoke that hook before reading files from disk.
        quiesce()?;

        let source_has_version_marker = self.config.source_dir.join("version").is_file();

        let (bytes_copied, files_copied) =
            copy_dir_recursive(&self.config.source_dir, &staging_path)?;

        // WHY(#4645): verify the staged copy before publishing. If the source is
        // a real fjall store, open it read-only and iterate every partition. If
        // the source is not a fjall store (e.g., tests or migration fixtures),
        // verification is skipped but we still require a manifest marker.
        let mut verify_seqno = None;
        let mut verified = false;
        if source_has_version_marker {
            let verify = Self::verify_store(&staging_path)?;
            if let Some(err) = verify.first_error {
                return error::MaintenanceInvariantSnafu {
                    context: format!("staged backup verification failed: {err}"),
                }
                .fail();
            }
            verify_seqno = verify.seqno;
            verified = true;
        }

        self.write_manifest(
            &staging_path,
            &FjallBackupManifest {
                version: String::from(MANIFEST_VERSION),
                created_at: jiff::Zoned::now().to_string(),
                source_path: self.config.source_dir.clone(),
                bytes_copied,
                files_copied,
                seqno: verify_seqno,
                verified,
            },
        )?;

        // WHY(#4645): atomic publish on the same filesystem as backup_dir.
        // `std::fs::rename` is atomic on Unix when source and destination are
        // on the same filesystem; we created staging inside backup_dir above.
        fs::rename(&staging_path, &final_path).context(error::MaintenanceIoSnafu {
            context: format!(
                "publishing fjall backup from {} to {}",
                staging_path.display(),
                final_path.display()
            ),
        })?;

        info!(
            backup = %final_path.display(),
            files = files_copied,
            bytes = bytes_copied,
            verified,
            "fjall backup created"
        );

        let backups_pruned = self.prune_old_backups()?;

        Ok(FjallBackupReport {
            backup_path: Some(final_path),
            bytes_copied,
            files_copied,
            backups_pruned,
        })
    }

    /// Prepare a staging directory and the final publish path for a new backup.
    fn prepare_staging_path(&self) -> error::Result<(PathBuf, PathBuf)> {
        // WHY: include subsecond precision to avoid collisions when backups
        // are triggered in rapid succession (e.g., tests or manual runs).
        let timestamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f").to_string();
        let final_path = self.config.backup_dir.join(&timestamp);

        if final_path.exists() {
            return error::MaintenanceInvariantSnafu {
                context: format!("backup path already exists: {}", final_path.display()),
            }
            .fail();
        }

        // WHY(#4645): build into a hidden staging directory inside backup_dir so
        // the final rename is on the same filesystem and therefore atomic.
        let staging_temp = tempfile::Builder::new()
            .prefix(STAGING_DIR_PREFIX)
            .tempdir_in(&self.config.backup_dir)
            .context(error::MaintenanceIoSnafu {
                context: format!(
                    "creating fjall staging dir in {}",
                    self.config.backup_dir.display()
                ),
            })?;
        let staging_path = staging_temp.keep();

        Ok((staging_path, final_path))
    }

    /// Write the completion manifest into a staged backup directory.
    fn write_manifest(&self, path: &Path, manifest: &FjallBackupManifest) -> error::Result<()> {
        let manifest_path = path.join(COMPLETE_MARKER);
        let manifest_json = serde_json::to_string_pretty(manifest)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: format!("serializing fjall backup manifest {}", manifest_path.display()),
            })?;

        let mut file = fs::File::create(&manifest_path).context(error::MaintenanceIoSnafu {
            context: format!("creating fjall backup manifest {}", manifest_path.display()),
        })?;
        file.write_all(manifest_json.as_bytes())
            .context(error::MaintenanceIoSnafu {
                context: format!("writing fjall backup manifest {}", manifest_path.display()),
            })?;
        file.sync_all().context(error::MaintenanceIoSnafu {
            context: format!("syncing fjall backup manifest {}", manifest_path.display()),
        })?;

        Ok(())
    }

    /// Remove leftover staging directories from prior interrupted runs.
    fn cleanup_stale_staging_dirs(&self) -> error::Result<()> {
        if !self.config.backup_dir.exists() {
            return Ok(());
        }

        let dir = fs::read_dir(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "reading fjall backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading backup entry",
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();

            if name.starts_with(STAGING_DIR_PREFIX) && path.is_dir() {
                if let Err(e) = fs::remove_dir_all(&path) {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to remove stale fjall backup staging directory"
                    );
                } else {
                    info!(path = %path.display(), "removed stale fjall backup staging directory");
                }
            }
        }

        Ok(())
    }

    /// List existing backups, newest first.
    ///
    /// WHY(#4645): only backups containing a `backup-complete.json` manifest are
    /// returned. In-progress staging directories are ignored.
    pub fn list_backups(&self) -> error::Result<Vec<BackupEntry>> {
        if !self.config.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let dir = fs::read_dir(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "reading fjall backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading backup entry",
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();

            // WHY(#4645): ignore in-progress staging directories and backups
            // that were not atomically published.
            if name.starts_with(STAGING_DIR_PREFIX)
                || !path.is_dir()
                || !path.join(COMPLETE_MARKER).is_file()
            {
                continue;
            }

            // WHY(#4645): order by the manifest's recorded `created_at` rather
            // than directory mtime, which a restore/rsync/touch can rewrite and
            // would corrupt auto-prune ordering.
            let Some(created) = manifest_created_time(&path) else {
                warn!(
                    path = %path.display(),
                    "skipping fjall backup with unreadable or malformed manifest"
                );
                continue;
            };

            let size_bytes = dir_size(&path);
            entries.push(BackupEntry {
                name,
                path,
                created,
                size_bytes,
            });
        }

        entries.sort_by_key(|e| std::cmp::Reverse(e.created));
        Ok(entries)
    }

    /// Verify a fjall store directory by opening it read-only and iterating
    /// every partition. Returns per-partition key counts and the first
    /// validation error encountered, if any.
    ///
    /// WHY: this is used both by the legacy `aletheia backup verify` path and
    /// by whole-instance backup verification for the `knowledge.fjall` and
    /// `sessions.db` stores.
    ///
    /// SAFETY(#5754): `fjall::open()` applies destructive auto-recovery that
    /// deletes segment files absent from the levels manifest. Verify is a
    /// forensic/read-only operation with respect to the canonical backup path:
    /// we copy the directory to a throwaway temp location and open only the
    /// copy, with background workers disabled so compaction/flush cannot mutate
    /// it during iteration.
    pub fn verify_store(path: &Path) -> error::Result<FjallVerifyResult> {
        // WHY: FjallDb::open_existing eagerly creates `version`, `keyspaces/`,
        // and a fresh journal in the target directory if it doesn't already
        // look like a fjall store. Guard against that by requiring the fjall
        // marker file before opening.
        if !path.join("version").is_file() {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "not a fjall store (missing `version` marker): {}",
                    path.display()
                ),
            }
            .fail();
        }

        // SAFETY(#5754): copy the backup to a temp dir before opening so that
        // any destructive auto-recovery only touches the disposable copy.
        let temp_dir = tempfile::TempDir::new().context(error::MaintenanceIoSnafu {
            context: String::from("creating temp dir for fjall verify"),
        })?;
        let temp_copy = temp_dir.path().join("store");
        copy_dir_recursive(path, &temp_copy)?;

        // WHY(#5754): open the temp copy with zero background workers to
        // prevent flush/compaction from mutating the copy during iteration.
        // `worker_threads_unchecked` is used because the public
        // `worker_threads` panics on zero in non-test builds, and verify has
        // no need for background work.
        let db = fjall::SingleWriterTxDatabase::builder(&temp_copy)
            .worker_threads_unchecked(0)
            .open()
            .map_err(|e| std::io::Error::other(e.to_string()))
            .context(error::MaintenanceIoSnafu {
                context: format!("opening fjall store copy for verify: {}", path.display()),
            })?;

        let mut result = FjallVerifyResult {
            partition_counts: Vec::new(),
            first_error: None,
            total_keys: 0,
            seqno: Some(db.inner().seqno()),
        };

        let names = db.list_keyspace_names();
        for name in names {
            let name_str = name.as_ref();
            let ks = db
                .keyspace(name_str, fjall::KeyspaceCreateOptions::default)
                .map_err(|e| std::io::Error::other(e.to_string()))
                .context(error::MaintenanceIoSnafu {
                    context: format!("opening partition {name_str}"),
                })?;

            let snap = db.read_tx();
            let mut count = 0usize;

            for guard in snap.range::<&str, _>(&ks, ..) {
                let (key, value): (fjall::Slice, fjall::Slice) = guard
                    .into_inner()
                    .map_err(|e: fjall::Error| std::io::Error::other(e.to_string()))
                    .context(error::MaintenanceIoSnafu {
                        context: format!("reading partition {name_str}"),
                    })?;

                count += 1;
                result.total_keys += 1;

                if result.first_error.is_none()
                    && let Err(e) = validate_kv(name_str, key.as_ref(), value.as_ref())
                {
                    let key_display = String::from_utf8_lossy(key.as_ref());
                    result.first_error = Some(format!("{name_str}/{key_display}: {e}"));
                }
            }

            result.partition_counts.push((name_str.to_owned(), count));
        }

        Ok(result)
    }

    /// Remove old backups beyond the configured retention count.
    ///
    /// WHY(#4645): only completed backups are considered for pruning because
    /// `list_backups` filters out in-progress staging directories.
    fn prune_old_backups(&self) -> error::Result<u32> {
        let entries = self.list_backups()?;
        let mut pruned = 0u32;

        for entry in entries.into_iter().skip(self.config.retention_count) {
            if let Err(e) = fs::remove_dir_all(&entry.path) {
                warn!(
                    path = %entry.path.display(),
                    error = %e,
                    "failed to remove old fjall backup"
                );
            } else {
                pruned += 1;
            }
        }

        if pruned > 0 {
            info!(pruned, "pruned old fjall backups");
        }

        Ok(pruned)
    }
}

/// Parse the `created_at` field from a backup's manifest into a [`SystemTime`].
///
/// WHY(#4645): returns `None` if the manifest is missing, unreadable, or the
/// timestamp cannot be parsed so malformed backups are never auto-pruned on a
/// wrong assumption about their age.
fn manifest_created_time(backup_path: &Path) -> Option<SystemTime> {
    let manifest_json = fs::read_to_string(backup_path.join(COMPLETE_MARKER)).ok()?;
    let manifest: FjallBackupManifest = serde_json::from_str(&manifest_json).ok()?;
    let zoned: jiff::Zoned = manifest.created_at.parse().ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

/// Recursively copy a directory. Returns `(bytes_copied, files_copied)`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> error::Result<(u64, u32)> {
    fs::create_dir_all(dst).context(error::MaintenanceIoSnafu {
        context: format!("creating backup dir {}", dst.display()),
    })?;

    let mut total_bytes = 0u64;
    let mut total_files = 0u32;

    let entries = fs::read_dir(src).context(error::MaintenanceIoSnafu {
        context: format!("reading source dir {}", src.display()),
    })?;

    for entry in entries {
        let entry = entry.context(error::MaintenanceIoSnafu {
            context: "reading directory entry",
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            let (bytes, files) = copy_dir_recursive(&src_path, &dst_path)?;
            total_bytes += bytes;
            total_files += files;
        } else {
            let bytes = fs::copy(&src_path, &dst_path).context(error::MaintenanceIoSnafu {
                context: format!("copying {} to {}", src_path.display(), dst_path.display()),
            })?;
            total_bytes += bytes;
            total_files += 1;
        }
    }

    Ok((total_bytes, total_files))
}

// ── Per-partition validation ───────────────────────────────────────────────

fn validate_kv(partition: &str, key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    match partition {
        "sessions" => validate_sessions(key, value),
        "messages" => validate_messages(key, value),
        "usage" => serde_json::from_slice::<mneme::types::UsageRecord>(value)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "distillations" | "ops:tasks" => validate_json(value),
        "notes" => validate_notes(key, value),
        "blackboard" => serde_json::from_slice::<mneme::types::BlackboardRow>(value)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "counters" => validate_u64(value),
        "users" => validate_users(key, value),
        "api_keys" => validate_api_keys(key, value),
        "revoked_tokens" => validate_utf8(value),
        // Known partitions with opaque/internal encoding, plus unknown partitions:
        // all verified by successful read (iteration implicitly verifies checksums).
        other => validate_opaque_or_unknown_partition(other),
    }
}

fn validate_opaque_or_unknown_partition(partition: &str) -> std::result::Result<(), String> {
    if partition.is_empty() {
        return Err("partition name must not be empty".into());
    }
    Ok(())
}

fn validate_sessions(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("idx:nous:") {
        if !value.is_empty() {
            return Err("session nous index value should be empty".into());
        }
        Ok(())
    } else if key_str.starts_with("idx:key:") {
        std::str::from_utf8(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        serde_json::from_slice::<mneme::types::Session>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_messages(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("next_seq:") {
        if value.len() != 8 {
            return Err(format!(
                "next_seq value should be 8 bytes, got {}",
                value.len()
            ));
        }
        Ok(())
    } else if key_str.starts_with("distilled:") {
        if value != b"1" {
            return Err("distilled flag should be \"1\"".into());
        }
        Ok(())
    } else {
        serde_json::from_slice::<mneme::types::Message>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_notes(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("gid:") {
        std::str::from_utf8(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        serde_json::from_slice::<mneme::types::AgentNote>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_users(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if !key_str.starts_with("user:") {
        return Err(format!(
            "users key should start with 'user:', got {key_str}"
        ));
    }
    validate_json(value)
}

fn validate_api_keys(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("hash:") {
        std::str::from_utf8(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    } else {
        validate_json(value)
    }
}

fn validate_u64(value: &[u8]) -> std::result::Result<(), String> {
    if value.len() != 8 {
        return Err(format!("u64 value should be 8 bytes, got {}", value.len()));
    }
    Ok(())
}

fn validate_json(value: &[u8]) -> std::result::Result<(), String> {
    serde_json::from_slice::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn validate_utf8(value: &[u8]) -> std::result::Result<(), String> {
    std::str::from_utf8(value)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Calculate total size of a directory tree.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn write_fixture(path: impl AsRef<Path>, content: &str) {
        #[expect(
            clippy::disallowed_methods,
            reason = "test fixture: synchronous write in non-async test context"
        )]
        fs::write(path.as_ref(), content).expect("write fixture");
        let mut perms = fs::metadata(path.as_ref())
            .expect("read fixture metadata")
            .permissions();
        perms.set_mode(0o644);
        fs::set_permissions(path.as_ref(), perms).expect("set fixture permissions");
    }

    /// Create a real fjall store with a small amount of data for backup tests.
    fn make_fjall_store(path: &Path) {
        fs::create_dir_all(path).expect("create store dir");
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .worker_threads_unchecked(0)
            .open()
            .expect("open test fjall store");
        let partition = db
            .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
            .expect("create test partition");
        for i in 0..5u8 {
            partition
                .insert(format!("key-{i}"), vec![i])
                .expect("insert test key");
        }
        db.persist(fjall::PersistMode::SyncAll)
            .expect("persist test store");
        drop(db);
    }

    #[test]
    fn create_backup_stages_verifies_and_publishes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        make_fjall_store(&source);

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
        };

        let manager = FjallBackup::new(config);
        let report = manager.create_backup().expect("backup succeeds");

        let backup_path = report.backup_path.expect("backup path set");
        assert!(backup_path.exists());
        assert!(backup_path.join(COMPLETE_MARKER).is_file());
        assert_eq!(backup_path.parent(), Some(backup_dir.as_path()));
        assert!(report.files_copied > 0);
        assert!(report.bytes_copied > 0);

        // No staging directories should remain after a successful publish.
        let staging_left = fs::read_dir(&backup_dir)
            .expect("read backup dir")
            .flatten()
            .any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(STAGING_DIR_PREFIX)
            });
        assert!(!staging_left, "staging directory leaked after publish");

        // The published backup must be a valid fjall store.
        let verify = FjallBackup::verify_store(&backup_path).expect("verify published backup");
        assert!(verify.first_error.is_none());
        assert_eq!(verify.total_keys, 5);

        let backups = manager.list_backups().expect("list succeeds");
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn prune_respects_retention_count() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        make_fjall_store(&source);

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 2,
        };

        let manager = FjallBackup::new(config);

        // Create 4 backups.
        for _ in 0..4 {
            manager.create_backup().expect("backup succeeds");
            // Small sleep to ensure distinct timestamps.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let backups = manager.list_backups().expect("list succeeds");
        assert_eq!(backups.len(), 2, "should keep only 2 backups");
    }

    #[test]
    fn nonexistent_source_returns_empty_report() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = FjallBackupConfig {
            source_dir: tmp.path().join("nonexistent"),
            backup_dir: tmp.path().join("backups"),
            ..FjallBackupConfig::default()
        };

        let manager = FjallBackup::new(config);
        let report = manager.create_backup().expect("should not error");
        assert!(report.backup_path.is_none());
        assert_eq!(report.files_copied, 0);
    }

    #[test]
    fn list_empty_backup_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = FjallBackupConfig {
            backup_dir: tmp.path().join("nonexistent-backups"),
            ..FjallBackupConfig::default()
        };

        let manager = FjallBackup::new(config);
        let backups = manager.list_backups().expect("list succeeds");
        assert!(backups.is_empty());
    }

    #[test]
    fn list_backups_skips_incomplete_staging() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let backup_dir = tmp.path().join("backups");
        fs::create_dir_all(&backup_dir).unwrap();

        // Create a fake staging directory that mimics a crashed backup.
        let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}crashed"));
        fs::create_dir_all(&staging).unwrap();
        write_fixture(staging.join("partial.sst"), "partial data");

        let config = FjallBackupConfig {
            backup_dir,
            ..FjallBackupConfig::default()
        };
        let manager = FjallBackup::new(config);
        let backups = manager.list_backups().expect("list succeeds");
        assert!(backups.is_empty(), "staging directory must not be listed");
    }

    #[test]
    fn prune_only_completed_backups() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        make_fjall_store(&source);

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 1,
        };
        let manager = FjallBackup::new(config);

        // Create two completed backups.
        manager.create_backup().expect("backup 1 succeeds");
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.create_backup().expect("backup 2 succeeds");

        // Inject an incomplete staging directory.
        let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}incomplete"));
        fs::create_dir_all(&staging).unwrap();
        write_fixture(staging.join("partial.sst"), "partial data");

        // Prune should reduce completed backups to retention_count.
        let pruned = manager.prune_old_backups().expect("prune succeeds");
        assert_eq!(pruned, 1, "should prune one completed backup");

        let listed = manager.list_backups().expect("list succeeds");
        assert_eq!(listed.len(), 1, "only one completed backup should remain");

        // The incomplete staging directory must survive pruning.
        assert!(staging.exists(), "incomplete staging must not be pruned");
    }

    #[test]
    fn create_backup_cleans_stale_staging_and_restores() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        make_fjall_store(&source);

        // Simulate an interrupted previous backup run.
        let stale = backup_dir.join(format!("{STAGING_DIR_PREFIX}interrupted"));
        fs::create_dir_all(&stale).unwrap();
        write_fixture(stale.join("partial.sst"), "partial data");

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
        };
        let manager = FjallBackup::new(config);

        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");

        // The stale staging directory from the interrupted run must be gone.
        assert!(!stale.exists(), "stale staging directory should be cleaned");

        // The new backup must be restorable.
        let verify = FjallBackup::verify_store(&backup_path).expect("verify restored backup");
        assert!(verify.first_error.is_none());
        assert_eq!(verify.total_keys, 5);
    }

    #[test]
    fn create_backup_with_quiesce_calls_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        make_fjall_store(&source);

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir,
            interval_hours: 24,
            retention_count: 7,
        };
        let manager = FjallBackup::new(config);

        let mut quiesce_called = false;
        let report = manager
            .create_backup_with_quiesce(|| {
                quiesce_called = true;
                Ok(())
            })
            .expect("backup succeeds");

        assert!(quiesce_called, "quiesce hook must be invoked");
        assert!(report.backup_path.is_some());
    }

    #[test]
    fn default_config_values() {
        let config = FjallBackupConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval_hours, 24);
        assert_eq!(config.retention_count, 7);
    }

    /// #5754 regression: verify must not destructively recover the canonical
    /// backup directory. A hot-copy may contain orphan segment files; opening
    /// the backup in-place would delete them. Verify should copy first and only
    /// open the disposable copy.
    #[test]
    fn verify_store_does_not_mutate_backup_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup = tmp.path().join("backup").join("knowledge.fjall");

        // Create a live store with one partition and a few keys.
        {
            fs::create_dir_all(&source).unwrap();
            let db = fjall::SingleWriterTxDatabase::builder(&source)
                .worker_threads_unchecked(0)
                .open()
                .unwrap();
            let partition = db
                .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
                .unwrap();
            for i in 0..5u8 {
                partition.insert(format!("key-{i}"), vec![i]).unwrap();
            }
            db.persist(fjall::PersistMode::SyncAll).unwrap();
            drop(db);
        }

        // Simulate a hot-copy backup.
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        copy_dir_recursive(&source, &backup).unwrap();

        // Inject a fake orphan segment file into the backup. A real hot-copy
        // could contain compaction-in-progress segments not yet referenced by
        // the manifest; the name here is chosen to look like an SST.
        let orphan_rel = PathBuf::from("orphan-0001.sst");
        let orphan = backup.join(&orphan_rel);
        write_fixture(&orphan, "orphan segment data");

        let files_before: std::collections::HashSet<PathBuf> =
            walk_paths(&backup).into_iter().collect();
        assert!(files_before.contains(&orphan_rel));

        // Verify the backup. This must open only a temp copy, leaving the
        // canonical backup untouched.
        let result = FjallBackup::verify_store(&backup).expect("verify succeeds");
        assert!(result.first_error.is_none());
        assert_eq!(result.total_keys, 5);

        let files_after: std::collections::HashSet<PathBuf> =
            walk_paths(&backup).into_iter().collect();
        assert!(
            files_after.contains(&orphan_rel),
            "verify deleted the orphan segment from the canonical backup"
        );
        assert_eq!(
            files_before, files_after,
            "verify mutated the canonical backup directory"
        );

        // Simulate restore by opening the backup directly. The orphan is
        // expected to be discarded by recovery, but all real records must
        // survive.
        let restored = fjall::SingleWriterTxDatabase::builder(&backup)
            .worker_threads_unchecked(0)
            .open()
            .unwrap();
        let partition = restored
            .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let snap = restored.read_tx();
        let count = snap.range::<&str, _>(&partition, ..).count();
        assert_eq!(count, 5, "restore lost real records");
    }

    fn walk_paths(dir: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    paths.extend(walk_paths(&path));
                } else {
                    paths.push(path.strip_prefix(dir).unwrap().to_path_buf());
                }
            }
        }
        paths
    }
}
