//! Whole-instance backup: coherent snapshot of knowledge, sessions, config, and workspace data.
//!
//! WHY(#4856): the legacy `FjallBackup` only copied `knowledge.fjall`. The
//! `aletheia backup` command and the daemon's scheduled backup task now produce
//! a backup *set* that includes `sessions.db`, configuration, and workspace data
//! needed for run replay/review. A JSON manifest records every covered store,
//! its source path, snapshot time, byte count, and verification status.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use tracing::{info, warn};

use crate::error;

use super::fjall_backup::{BackupEntry, FjallBackup};

/// Manifest format version.
const MANIFEST_VERSION: &str = "aletheia-instance-backup-v1";

/// Configuration for whole-instance backups.
#[derive(Debug, Clone)]
pub struct InstanceBackupConfig {
    /// Whether periodic whole-instance backups are enabled.
    pub enabled: bool,
    /// Path to the instance root directory.
    pub instance_root: PathBuf,
    /// Directory where timestamped backup sets are stored.
    pub backup_dir: PathBuf,
    /// Hours between automatic backups.
    pub interval_hours: u64,
    /// Maximum number of backup snapshots to retain.
    pub retention_count: usize,
}

impl Default for InstanceBackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_root: PathBuf::from("instance"),
            backup_dir: PathBuf::from("instance/data/backups/instance"),
            interval_hours: 24,
            retention_count: 7,
        }
    }
}

/// A single store entry recorded in the backup manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreEntry {
    /// Logical store name, e.g. `knowledge.fjall` or `sessions.db`.
    pub name: String,
    /// Absolute source path the store was copied from.
    pub source_path: PathBuf,
    /// Relative backup path inside the backup set.
    pub backup_path: PathBuf,
    /// ISO 8601 snapshot timestamp.
    pub snapshot_time: String,
    /// Total bytes copied for this store.
    pub byte_count: u64,
    /// Verification status: `ok`, `missing`, or `error`.
    pub status: String,
}

/// Whole-instance backup manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupManifest {
    /// Manifest format version.
    pub version: String,
    /// ISO 8601 timestamp when the backup set was created.
    pub created_at: String,
    /// Absolute path to the instance root that was backed up.
    pub source_root: PathBuf,
    /// Required stores (must be present and verifiable for a valid set).
    pub stores: Vec<StoreEntry>,
    /// Optional data directories copied when present.
    pub optional_stores: Vec<StoreEntry>,
    /// Total bytes copied across all stores.
    pub total_bytes: u64,
}

/// Outcome of a whole-instance backup run.
#[derive(Debug, Clone, Default)]
pub struct InstanceBackupReport {
    /// Path to the created backup set directory.
    pub backup_path: Option<PathBuf>,
    /// Total bytes copied.
    pub bytes_copied: u64,
    /// Number of files copied.
    pub files_copied: u32,
    /// Number of old backups pruned.
    pub backups_pruned: u32,
}

/// Result of verifying a whole-instance backup set.
#[derive(Debug, Clone, Default)]
pub struct InstanceVerifyResult {
    /// Loaded manifest.
    pub manifest: Option<BackupManifest>,
    /// Per-store verification outcomes: name, key count (or error message).
    pub store_results: Vec<(String, std::result::Result<usize, String>)>,
    /// Total keys iterated across all fjall stores.
    pub total_keys: usize,
    /// First error encountered, if any.
    pub first_error: Option<String>,
}

/// Manages whole-instance backup sets.
pub struct InstanceBackup {
    config: InstanceBackupConfig,
}

struct BackupBuild {
    stores: Vec<StoreEntry>,
    optional_stores: Vec<StoreEntry>,
    total_bytes: u64,
    total_files: u32,
    snapshot_time: String,
}

impl BackupBuild {
    fn new() -> Self {
        Self {
            stores: Vec::new(),
            optional_stores: Vec::new(),
            total_bytes: 0,
            total_files: 0,
            snapshot_time: jiff::Zoned::now().to_string(),
        }
    }

    fn copy_entry(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        optional: bool,
    ) -> error::Result<()> {
        let (bytes, files) = copy_path(&src, dst)?;
        self.total_bytes += bytes;
        self.total_files += files;
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: self.snapshot_time.clone(),
            byte_count: bytes,
            status: String::from("ok"),
        };
        if optional {
            self.optional_stores.push(entry);
        } else {
            self.stores.push(entry);
        }
        Ok(())
    }
}

impl InstanceBackup {
    /// Create a new whole-instance backup manager.
    #[must_use]
    pub fn new(config: InstanceBackupConfig) -> Self {
        Self { config }
    }

    /// Create a whole-instance backup set under `backup_dir/<timestamp>/`.
    ///
    /// The set contains:
    /// - `manifest.json` describing every covered store.
    /// - `stores/knowledge.fjall/` (required).
    /// - `stores/sessions.db/` (required).
    /// - `config/` copy of `instance/config/`.
    /// - `workspace/nous/`, `workspace/shared/`, `workspace/theke/` if present.
    /// - `data/archive/`, `data/prosoche-audits/`, `data/prompt-audit/` if present.
    ///
    /// After creating the backup, old backups beyond `retention_count` are pruned.
    pub fn create_backup(&self) -> error::Result<InstanceBackupReport> {
        let backup_path = self.prepare_backup_path()?;
        let mut build = BackupBuild::new();

        self.copy_required_stores(&backup_path, &mut build)?;
        self.copy_config(&backup_path, &mut build)?;
        self.copy_workspace_dirs(&backup_path, &mut build)?;
        self.copy_optional_data_dirs(&backup_path, &mut build)?;
        self.copy_prompt_audit_dirs(&backup_path, &mut build)?;

        let total_bytes = build.total_bytes;
        let total_files = build.total_files;
        self.write_manifest(&backup_path, build)?;

        info!(
            backup = %backup_path.display(),
            files = total_files,
            bytes = total_bytes,
            "instance backup created"
        );

        let backups_pruned = self.prune_old_backups()?;

        Ok(InstanceBackupReport {
            backup_path: Some(backup_path),
            bytes_copied: total_bytes,
            files_copied: total_files,
            backups_pruned,
        })
    }

    fn prepare_backup_path(&self) -> error::Result<PathBuf> {
        fs::create_dir_all(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "creating instance backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        // WHY: include subsecond precision to avoid collisions when backups
        // are triggered in rapid succession (e.g. tests or manual runs).
        let timestamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f").to_string();
        Ok(self.config.backup_dir.join(timestamp))
    }

    fn required_store_paths(&self) -> error::Result<(PathBuf, PathBuf)> {
        let knowledge_src = self
            .config
            .instance_root
            .join("data")
            .join("knowledge.fjall");
        let sessions_src = self.config.instance_root.join("data").join("sessions.db");

        if !knowledge_src.exists() {
            return error::MaintenanceInvariantSnafu {
                context: format!("knowledge store not found at {}", knowledge_src.display()),
            }
            .fail();
        }
        if !sessions_src.exists() {
            return error::MaintenanceInvariantSnafu {
                context: format!("session store not found at {}", sessions_src.display()),
            }
            .fail();
        }

        Ok((knowledge_src, sessions_src))
    }

    fn copy_required_stores(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        let (knowledge_src, sessions_src) = self.required_store_paths()?;
        build.copy_entry(
            "knowledge.fjall",
            knowledge_src,
            &backup_path.join("stores").join("knowledge.fjall"),
            PathBuf::from("stores/knowledge.fjall"),
            false,
        )?;
        build.copy_entry(
            "sessions.db",
            sessions_src,
            &backup_path.join("stores").join("sessions.db"),
            PathBuf::from("stores/sessions.db"),
            false,
        )
    }

    fn copy_config(&self, backup_path: &Path, build: &mut BackupBuild) -> error::Result<()> {
        let config_src = self.config.instance_root.join("config");
        if config_src.exists() {
            build.copy_entry(
                "config",
                config_src,
                &backup_path.join("config"),
                PathBuf::from("config"),
                false,
            )?;
        }
        Ok(())
    }

    fn copy_workspace_dirs(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        for (name, rel) in [
            ("nous", "workspace/nous"),
            ("shared", "workspace/shared"),
            ("theke", "workspace/theke"),
        ] {
            let src = self.config.instance_root.join(name);
            if src.exists() {
                build.copy_entry(name, src, &backup_path.join(rel), PathBuf::from(rel), true)?;
            }
        }
        Ok(())
    }

    fn copy_optional_data_dirs(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        let optional_data = [
            ("archive", "data/archive"),
            ("prosoche-audits", "data/prosoche-audits"),
        ];
        for (name, rel) in optional_data {
            let src = self.config.instance_root.join("data").join(name);
            if src.exists() {
                build.copy_entry(name, src, &backup_path.join(rel), PathBuf::from(rel), true)?;
            }
        }
        Ok(())
    }

    fn copy_prompt_audit_dirs(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        for src in [
            self.config.instance_root.join("data").join("prompt-audit"),
            self.config.instance_root.join("logs").join("prompt-audit"),
        ] {
            if src.exists() {
                let rel = if src.components().any(|c| c.as_os_str() == "logs") {
                    "logs/prompt-audit"
                } else {
                    "data/prompt-audit"
                };
                build.copy_entry(
                    "prompt-audit",
                    src,
                    &backup_path.join(rel),
                    PathBuf::from(rel),
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn write_manifest(&self, backup_path: &Path, build: BackupBuild) -> error::Result<()> {
        let manifest = BackupManifest {
            version: String::from(MANIFEST_VERSION),
            created_at: build.snapshot_time,
            source_root: self.config.instance_root.clone(),
            stores: build.stores,
            optional_stores: build.optional_stores,
            total_bytes: build.total_bytes,
        };
        let manifest_path = backup_path.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: String::from("serializing backup manifest"),
            })?;
        write_text_file(&manifest_path, &manifest_json)?;
        Ok(())
    }

    /// List existing whole-instance backups, newest first.
    pub fn list_backups(&self) -> error::Result<Vec<BackupEntry>> {
        if !self.config.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let dir = fs::read_dir(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "reading instance backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        for entry in dir {
            let entry = entry.context(error::MaintenanceIoSnafu {
                context: "reading backup entry",
            })?;
            let path = entry.path();
            if !path.is_dir() || !path.join("manifest.json").is_file() {
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

    /// Verify a whole-instance backup set.
    ///
    /// Reads `manifest.json`, confirms the required stores (`knowledge.fjall`
    /// and `sessions.db`) are present, and iterates every fjall store to prove
    /// the files are openable. Returns the manifest and per-store results.
    pub fn verify_backup(path: &Path) -> error::Result<InstanceVerifyResult> {
        let manifest_path = path.join("manifest.json");
        if !manifest_path.is_file() {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "not an instance backup set (missing manifest.json): {}",
                    path.display()
                ),
            }
            .fail();
        }

        let manifest_json =
            fs::read_to_string(&manifest_path).context(error::MaintenanceIoSnafu {
                context: format!("reading manifest {}", manifest_path.display()),
            })?;
        let manifest: BackupManifest = serde_json::from_str(&manifest_json)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: format!("parsing manifest {}", manifest_path.display()),
            })?;

        let mut result = InstanceVerifyResult {
            manifest: Some(manifest.clone()),
            store_results: Vec::new(),
            total_keys: 0,
            first_error: None,
        };

        // Required stores.
        for name in ["knowledge.fjall", "sessions.db"] {
            let found = manifest.stores.iter().any(|s| s.name == name);
            if !found {
                let err = format!("required store missing from manifest: {name}");
                result
                    .store_results
                    .push((String::from(name), Err(err.clone())));
                if result.first_error.is_none() {
                    result.first_error = Some(err);
                }
                continue;
            }

            let store_path = path.join("stores").join(name);
            if !store_path.exists() {
                let err = format!("required store directory missing: {}", store_path.display());
                result
                    .store_results
                    .push((String::from(name), Err(err.clone())));
                if result.first_error.is_none() {
                    result.first_error = Some(err);
                }
                continue;
            }

            match verify_store_path(name, &store_path) {
                Ok(total) => {
                    result.total_keys += total;
                    result.store_results.push((String::from(name), Ok(total)));
                }
                Err(err) => {
                    result
                        .store_results
                        .push((String::from(name), Err(err.clone())));
                    if result.first_error.is_none() {
                        result.first_error = Some(err);
                    }
                }
            }
        }

        // Verify every remaining manifest entry. This covers config and
        // workspace/data directories, proving the restore set matches the
        // manifest instead of only checking the two required fjall stores.
        for store in manifest
            .stores
            .iter()
            .filter(|store| store.name != "knowledge.fjall" && store.name != "sessions.db")
            .chain(manifest.optional_stores.iter())
        {
            let store_path = path.join(&store.backup_path);
            match verify_manifest_store(&store.name, &store_path) {
                Ok(total) => result.store_results.push((store.name.clone(), Ok(total))),
                Err(err) => {
                    result
                        .store_results
                        .push((store.name.clone(), Err(err.clone())));
                    if result.first_error.is_none() {
                        result.first_error = Some(err);
                    }
                }
            }
        }

        Ok(result)
    }

    /// Remove old whole-instance backups beyond the configured retention count.
    fn prune_old_backups(&self) -> error::Result<u32> {
        let entries = self.list_backups()?;
        let mut pruned = 0u32;

        for entry in entries.into_iter().skip(self.config.retention_count) {
            if let Err(e) = fs::remove_dir_all(&entry.path) {
                warn!(
                    path = %entry.path.display(),
                    error = %e,
                    "failed to remove old instance backup"
                );
            } else {
                pruned += 1;
            }
        }

        if pruned > 0 {
            info!(pruned, "pruned old instance backups");
        }

        Ok(pruned)
    }
}

fn verify_store_path(name: &str, path: &Path) -> std::result::Result<usize, String> {
    if path.join("version").is_file() {
        let verify = FjallBackup::verify_store(path).map_err(|e| format!("{name}: {e}"))?;
        if let Some(err) = verify.first_error {
            return Err(format!("{name}: {err}"));
        }
        return Ok(verify.total_keys);
    }

    if path.is_file() {
        return path
            .metadata()
            .map(|m| usize::try_from(m.len()).unwrap_or(usize::MAX))
            .map_err(|e| format!("{name}: failed to read file metadata: {e}"));
    }

    Err(format!(
        "{name}: required store is not a fjall store or file: {}",
        path.display()
    ))
}

fn verify_manifest_store(name: &str, path: &Path) -> std::result::Result<usize, String> {
    if !path.exists() {
        return Err(format!("missing: {name}"));
    }

    if path.join("version").is_file() {
        let verify = FjallBackup::verify_store(path).map_err(|e| e.to_string())?;
        if let Some(err) = verify.first_error {
            return Err(err);
        }
        return Ok(verify.total_keys);
    }

    if path.is_file() {
        return path
            .metadata()
            .map(|m| usize::try_from(m.len()).unwrap_or(usize::MAX))
            .map_err(|e| format!("failed to read file metadata: {e}"));
    }

    Ok(usize::try_from(dir_size(path)).unwrap_or(usize::MAX))
}

fn write_text_file(path: &Path, contents: &str) -> error::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
            context: format!("creating parent dir {}", parent.display()),
        })?;
    }
    let mut file = fs::File::create(path).context(error::MaintenanceIoSnafu {
        context: format!("creating file {}", path.display()),
    })?;
    file.write_all(contents.as_bytes())
        .context(error::MaintenanceIoSnafu {
            context: format!("writing file {}", path.display()),
        })
}

/// Copy a file or directory tree. Returns `(bytes_copied, files_copied)`.
fn copy_path(src: &Path, dst: &Path) -> error::Result<(u64, u32)> {
    if src.is_dir() {
        return copy_dir_recursive(src, dst);
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
            context: format!("creating backup dir {}", parent.display()),
        })?;
    }
    let bytes = fs::copy(src, dst).context(error::MaintenanceIoSnafu {
        context: format!("copying {} to {}", src.display(), dst.display()),
    })?;
    Ok((bytes, 1))
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
    use super::*;

    fn make_fjall_store(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(path).open().unwrap();
        let _ = db
            .keyspace("test", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        drop(db);
    }

    #[test]
    fn create_backup_copies_required_stores_and_manifest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();
        fs::create_dir_all(instance_root.join("nous").join("syn")).unwrap();
        write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();
        write_text_file(
            &instance_root.join("nous").join("syn").join("SOUL.md"),
            "soul",
        )
        .unwrap();

        make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
        make_fjall_store(&instance_root.join("data").join("sessions.db"));

        let backup_dir = tmp.path().join("backups");
        let config = InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
        };

        let manager = InstanceBackup::new(config);
        let report = manager.create_backup().expect("backup succeeds");

        let backup_path = report.backup_path.expect("backup path set");
        assert!(backup_path.join("manifest.json").is_file());
        assert!(
            backup_path
                .join("stores")
                .join("knowledge.fjall")
                .join("version")
                .is_file()
        );
        assert!(
            backup_path
                .join("stores")
                .join("sessions.db")
                .join("version")
                .is_file()
        );
        assert!(backup_path.join("config").join("aletheia.toml").is_file());
        assert!(
            backup_path
                .join("workspace")
                .join("nous")
                .join("syn")
                .join("SOUL.md")
                .is_file()
        );

        let manifest: BackupManifest =
            serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert_eq!(manifest.stores.len(), 3); // knowledge, sessions, config
        assert!(manifest.stores.iter().any(|s| s.name == "knowledge.fjall"));
        assert!(manifest.stores.iter().any(|s| s.name == "sessions.db"));
        assert!(manifest.stores.iter().any(|s| s.name == "config"));
        assert!(manifest.optional_stores.iter().any(|s| s.name == "nous"));
    }

    #[test]
    fn verify_backup_passes_for_complete_set() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();
        write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

        make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
        make_fjall_store(&instance_root.join("data").join("sessions.db"));

        let backup_dir = tmp.path().join("backups");
        let config = InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
        };

        let manager = InstanceBackup::new(config);
        let report = manager.create_backup().unwrap();
        let backup_path = report.backup_path.unwrap();

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(result.first_error.is_none());
        assert_eq!(result.store_results.len(), 3);
        assert!(result.store_results.iter().all(|(_, r)| r.is_ok()));
        assert!(
            result
                .store_results
                .iter()
                .any(|(name, result)| name == "config" && result.is_ok())
        );
    }

    #[test]
    fn verify_backup_rejects_missing_sessions_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let backup_path = tmp.path().join("bad-backup");
        fs::create_dir_all(&backup_path).unwrap();

        // Create a manifest that claims only knowledge.fjall was backed up.
        let manifest = BackupManifest {
            version: String::from(MANIFEST_VERSION),
            created_at: jiff::Zoned::now().to_string(),
            source_root: tmp.path().join("instance"),
            stores: vec![StoreEntry {
                name: String::from("knowledge.fjall"),
                source_path: tmp
                    .path()
                    .join("instance")
                    .join("data")
                    .join("knowledge.fjall"),
                backup_path: PathBuf::from("stores/knowledge.fjall"),
                snapshot_time: jiff::Zoned::now().to_string(),
                byte_count: 0,
                status: String::from("ok"),
            }],
            optional_stores: Vec::new(),
            total_bytes: 0,
        };
        write_text_file(
            &backup_path.join("manifest.json"),
            &serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        // Create the knowledge store so that the failure is the missing sessions
        // entry, not a missing directory.
        make_fjall_store(&backup_path.join("stores").join("knowledge.fjall"));

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(result.first_error.is_some());
        let err = result.first_error.unwrap();
        assert!(
            err.contains("sessions.db"),
            "error should mention sessions.db: {err}"
        );
    }

    #[test]
    fn prune_respects_retention_count() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();
        write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

        make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
        make_fjall_store(&instance_root.join("data").join("sessions.db"));

        let backup_dir = tmp.path().join("backups");
        let config = InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 2,
        };

        let manager = InstanceBackup::new(config);

        // Create 4 backups.
        for _ in 0..4 {
            manager.create_backup().expect("backup succeeds");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let backups = manager.list_backups().expect("list succeeds");
        assert_eq!(backups.len(), 2, "should keep only 2 backups");
    }

    #[test]
    fn create_backup_accepts_file_shaped_sessions_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
        write_text_file(
            &instance_root.join("data").join("sessions.db"),
            "legacy-session-store",
        )
        .unwrap();

        let backup_dir = tmp.path().join("backups");
        let config = InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir,
            interval_hours: 24,
            retention_count: 7,
        };

        let manager = InstanceBackup::new(config);
        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");
        assert!(backup_path.join("stores").join("sessions.db").is_file());

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(result.first_error.is_none());
        assert!(
            result
                .store_results
                .iter()
                .any(|(name, result)| name == "sessions.db" && result.is_ok())
        );
    }
}
