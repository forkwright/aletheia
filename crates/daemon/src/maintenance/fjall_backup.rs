//! Fjall knowledge store backup: periodic file-level copy to timestamped directory.
//!
//! WHY(#3381): the fjall knowledge store has no built-in backup mechanism.
//! If the fjall DB files are corrupted or the machine dies, all session and
//! knowledge data is lost. This module implements periodic file-level backups
//! with configurable retention.

use std::fs;
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
    /// When the backup was created (from directory mtime).
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

        // WHY: include subsecond precision to avoid collisions when backups
        // are triggered in rapid succession (e.g., tests or manual runs).
        let timestamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f").to_string();
        let backup_path = self.config.backup_dir.join(&timestamp);

        let (bytes_copied, files_copied) =
            copy_dir_recursive(&self.config.source_dir, &backup_path)?;

        info!(
            backup = %backup_path.display(),
            files = files_copied,
            bytes = bytes_copied,
            "fjall backup created"
        );

        let backups_pruned = self.prune_old_backups()?;

        Ok(FjallBackupReport {
            backup_path: Some(backup_path),
            bytes_copied,
            files_copied,
            backups_pruned,
        })
    }

    /// List existing backups, newest first.
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
            if !path.is_dir() {
                continue;
            }
            let metadata = entry.metadata().context(error::MaintenanceIoSnafu {
                context: format!("reading backup metadata: {}", path.display()),
            })?;
            let created = metadata.modified().context(error::MaintenanceIoSnafu {
                context: format!("reading backup mtime: {}", path.display()),
            })?;
            let size_bytes = dir_size(&path);
            let name = entry.file_name().to_string_lossy().into_owned();
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

        let fdb = koina::fjall::FjallDb::open_existing(path)
            .map_err(|e| std::io::Error::other(e.to_string()))
            .context(error::MaintenanceIoSnafu {
                context: format!("opening fjall store {}", path.display()),
            })?;

        let mut result = FjallVerifyResult {
            partition_counts: Vec::new(),
            first_error: None,
            total_keys: 0,
        };

        let names = fdb.db.list_keyspace_names();
        for name in names {
            let name_str = name.as_ref();
            let ks = fdb
                .db
                .keyspace(name_str, fjall::KeyspaceCreateOptions::default)
                .map_err(|e| std::io::Error::other(e.to_string()))
                .context(error::MaintenanceIoSnafu {
                    context: format!("opening partition {name_str}"),
                })?;

            let snap = fdb.db.read_tx();
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

    #[test]
    fn create_backup_copies_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        fs::create_dir_all(&source).unwrap();
        write_fixture(source.join("data.sst"), "sst data");
        write_fixture(source.join("manifest"), "manifest data");

        let config = FjallBackupConfig {
            enabled: true,
            source_dir: source,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
        };

        let manager = FjallBackup::new(config);
        let report = manager.create_backup().expect("backup succeeds");

        assert!(report.backup_path.is_some());
        assert_eq!(report.files_copied, 2);
        assert!(report.bytes_copied > 0);

        let backups = manager.list_backups().expect("list succeeds");
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn prune_respects_retention_count() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("knowledge.fjall");
        let backup_dir = tmp.path().join("backups");
        fs::create_dir_all(&source).unwrap();
        write_fixture(source.join("data"), "data");

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
    fn default_config_values() {
        let config = FjallBackupConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval_hours, 24);
        assert_eq!(config.retention_count, 7);
    }
}
