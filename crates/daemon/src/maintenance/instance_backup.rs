//! Whole-instance backup: coherent snapshot of knowledge, sessions, runtime state, config, and workspace data.
//!
//! WHY(#4856): the legacy `FjallBackup` only copied `knowledge.fjall`. The
//! `aletheia backup` command and the daemon's scheduled backup task now produce
//! a backup *set* that includes `sessions.db`, auth/task-state stores,
//! configuration, and workspace data needed for run replay/review. A JSON
//! manifest records every covered store, its source path, snapshot time, byte
//! count, and verification status.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use tracing::{info, warn};

use crate::error;

use super::fjall_backup::{BackupEntry, FjallBackup};

/// Manifest format version.
const MANIFEST_VERSION: &str = "aletheia-instance-backup-v1";

/// Snapshot protocol version.
///
/// WHY(#4950): bumped when the stage/verify/atomic-publish protocol changes.
const SNAPSHOT_PROTOCOL_VERSION: &str = "aletheia-instance-backup-v1-snapshot-1";

/// Policy used for all backup source traversal.
const SYMLINK_POLICY: &str = "reject";

/// Prefix for hidden staging directories inside `backup_dir`.
///
/// WHY(#4950): `list_backups` skips these so an in-progress backup is never
/// listed as a valid backup set.
const STAGING_DIR_PREFIX: &str = ".aletheia-backup-staging.";

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
    /// Additional agent workspaces to include: `(logical name, source path)`. (#5139)
    ///
    /// Each path is classified at backup time: paths inside `instance_root` are
    /// copied; absolute paths outside the root are recorded as omissions with a
    /// warning rather than silently dropped.
    pub additional_workspaces: Vec<(String, PathBuf)>,
}

impl Default for InstanceBackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_root: PathBuf::from("instance"),
            backup_dir: PathBuf::from("instance/data/backups/instance"),
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        }
    }
}

/// Records an agent workspace that was not copied into a backup set. (#5139)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceOmission {
    /// Logical workspace name.
    pub name: String,
    /// Source path that was omitted.
    pub source_path: PathBuf,
    /// Why the workspace was omitted (e.g. `absolute-outside-root`).
    pub reason: String,
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
    /// Agent that produced this store, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Workspace source classification, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_source_class: Option<String>,
    /// Reason this store was excluded from the backup, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion_reason: Option<String>,
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
    /// Agent workspaces that were not copied into this set. (#5139)
    #[serde(default)]
    pub workspace_omissions: Vec<WorkspaceOmission>,
    /// Total bytes copied across all stores.
    pub total_bytes: u64,
    /// Snapshot epoch: the ISO 8601 timestamp that bounds this coherent backup set.
    ///
    /// WHY(#4950): all stores in the set are copied under this single epoch so
    /// restore can detect cross-store time skew.
    #[serde(default)]
    pub snapshot_epoch: String,
    /// Snapshot protocol version. Bumped when the staging/verify/publish protocol changes.
    #[serde(default)]
    pub snapshot_protocol_version: String,
    /// Whether writers were quiesced before copying. `false` means the backup is
    /// a live snapshot and may contain minor cross-store write-point skew.
    #[serde(default)]
    pub quiesced: bool,
    /// Per-fjall-store generation (seqno) captured from the staged copy.
    ///
    /// WHY(#4950): generation IDs are evidence of the store state at snapshot
    /// time and help detect restore mismatches.
    #[serde(default)]
    pub store_generations: HashMap<String, u64>,
    /// Symbolic-link traversal policy used when copying source paths.
    #[serde(default = "default_symlink_policy")]
    pub symlink_policy: String,
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
    /// Per-fjall-store generation (seqno) captured during verification.
    pub store_generations: HashMap<String, u64>,
}

/// Manages whole-instance backup sets.
pub struct InstanceBackup {
    config: InstanceBackupConfig,
}

#[derive(Clone)]
struct BackupBuild {
    stores: Vec<StoreEntry>,
    optional_stores: Vec<StoreEntry>,
    workspace_omissions: Vec<WorkspaceOmission>,
    total_bytes: u64,
    total_files: u32,
    snapshot_time: String,
}

/// Arguments for recording an optional store entry without copying.
struct OptionalStoreRecord {
    name: String,
    source_path: PathBuf,
    backup_path: PathBuf,
    status: String,
    agent_id: Option<String>,
    workspace_source_class: Option<String>,
    exclusion_reason: Option<String>,
    byte_count: u64,
}

impl BackupBuild {
    fn new() -> Self {
        Self {
            stores: Vec::new(),
            optional_stores: Vec::new(),
            workspace_omissions: Vec::new(),
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
            agent_id: None,
            workspace_source_class: None,
            exclusion_reason: None,
        };
        if optional {
            self.optional_stores.push(entry);
        } else {
            self.stores.push(entry);
        }
        Ok(())
    }

    /// Copy a configured agent workspace and record its coverage metadata.
    fn copy_configured_workspace_entry(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        agent_id: String,
        workspace_source_class: String,
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
            agent_id: Some(agent_id),
            workspace_source_class: Some(workspace_source_class),
            exclusion_reason: None,
        };
        self.optional_stores.push(entry);
        Ok(())
    }

    fn record_optional_entry(&mut self, record: OptionalStoreRecord) {
        self.total_bytes += record.byte_count;
        let entry = StoreEntry {
            name: record.name,
            source_path: record.source_path,
            backup_path: record.backup_path,
            snapshot_time: self.snapshot_time.clone(),
            byte_count: record.byte_count,
            status: record.status,
            agent_id: record.agent_id,
            workspace_source_class: record.workspace_source_class,
            exclusion_reason: record.exclusion_reason,
        };
        self.optional_stores.push(entry);
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
    /// - `stores/auth.fjall/`, `stores/daemon-task-state/`, and
    ///   `stores/cron-locks.fjall/` if present.
    /// - `config/` copy of `instance/config/`.
    /// - `workspace/nous/`, `workspace/shared/`, `workspace/theke/` if present.
    /// - `workspace/configured/<agent>/` for configured agent workspaces inside the instance root.
    /// - `data/archive/`, `data/prosoche-audits/`, `data/prompt-audit/`, `logs/prompt-audit/` if present.
    ///
    /// WHY(#4950): the backup is built in a hidden staging directory, verified,
    /// then atomically renamed into place. This guarantees that an incomplete
    /// or corrupted backup set is never published as a valid backup.
    ///
    /// After creating the backup, old backups beyond `retention_count` are pruned.
    pub fn create_backup(&self) -> error::Result<InstanceBackupReport> {
        let (staging_path, final_path) = self.prepare_staging_path()?;
        let mut build = BackupBuild::new();

        self.copy_required_stores(&staging_path, &mut build)?;
        self.copy_runtime_state_stores(&staging_path, &mut build)?;
        self.copy_config(&staging_path, &mut build)?;
        self.copy_workspace_dirs(&staging_path, &mut build)?;
        self.copy_configured_agent_workspaces(&staging_path, &mut build)?;
        self.copy_additional_workspaces(&staging_path, &mut build)?;
        self.copy_optional_data_dirs(&staging_path, &mut build)?;
        self.copy_prompt_audit_dirs(&staging_path, &mut build)?;

        // WHY(#4950): verify the staged copy before publishing. If verification
        // fails we leave the staging directory in place (caller can inspect it)
        // and never expose an invalid backup set.
        let total_bytes = build.total_bytes;
        let total_files = build.total_files;
        let snapshot_epoch = build.snapshot_time.clone();

        // Write a preliminary manifest so verify_backup can confirm the set
        // structure; it will be rewritten with captured generation IDs below.
        self.write_manifest(&staging_path, &build, &snapshot_epoch, &HashMap::new())?;

        let verify = InstanceBackup::verify_backup(&staging_path)?;
        if let Some(err) = verify.first_error {
            return error::MaintenanceInvariantSnafu {
                context: format!("staged backup verification failed: {err}"),
            }
            .fail();
        }

        // Final manifest records the generation IDs captured during verification.
        self.write_manifest(
            &staging_path,
            &build,
            &snapshot_epoch,
            &verify.store_generations,
        )?;

        // WHY(#4950): atomic publish on the same filesystem as backup_dir.
        // `std::fs::rename` is atomic on Unix when source and destination are
        // on the same filesystem; we created staging inside backup_dir above.
        fs::rename(&staging_path, &final_path).context(error::MaintenanceIoSnafu {
            context: format!(
                "publishing backup from {} to {}",
                staging_path.display(),
                final_path.display()
            ),
        })?;

        info!(
            backup = %final_path.display(),
            files = total_files,
            bytes = total_bytes,
            "instance backup created"
        );

        let backups_pruned = self.prune_old_backups()?;

        Ok(InstanceBackupReport {
            backup_path: Some(final_path),
            bytes_copied: total_bytes,
            files_copied: total_files,
            backups_pruned,
        })
    }

    fn prepare_staging_path(&self) -> error::Result<(PathBuf, PathBuf)> {
        fs::create_dir_all(&self.config.backup_dir).context(error::MaintenanceIoSnafu {
            context: format!(
                "creating instance backup dir {}",
                self.config.backup_dir.display()
            ),
        })?;

        // WHY: include subsecond precision to avoid collisions when backups
        // are triggered in rapid succession (e.g. tests or manual runs).
        let timestamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f").to_string();
        let final_path = self.config.backup_dir.join(&timestamp);

        if final_path.exists() {
            return error::MaintenanceInvariantSnafu {
                context: format!("backup path already exists: {}", final_path.display()),
            }
            .fail();
        }

        // WHY(#4950): build into a hidden staging directory inside backup_dir so
        // the final rename is on the same filesystem and therefore atomic.
        let staging_temp = tempfile::Builder::new()
            .prefix(STAGING_DIR_PREFIX)
            .tempdir_in(&self.config.backup_dir)
            .context(error::MaintenanceIoSnafu {
                context: format!(
                    "creating staging dir in {}",
                    self.config.backup_dir.display()
                ),
            })?;
        let staging_path = staging_temp.keep();

        // WHY(#5140): a backup set contains credentials and session data; create
        // the set directory eagerly with owner-only (0o700) permissions so the
        // copied contents are never world-readable on a shared host.
        set_dir_restrictive(&staging_path);

        Ok((staging_path, final_path))
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

    fn copy_runtime_state_stores(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        for (name, source_rel, backup_rel) in [
            ("auth.fjall", "data/auth.fjall", "stores/auth.fjall"),
            (
                "daemon-task-state",
                "data/daemon-task-state",
                "stores/daemon-task-state",
            ),
            (
                "cron-locks.fjall",
                "data/cron-locks.fjall",
                "stores/cron-locks.fjall",
            ),
        ] {
            let src = self.config.instance_root.join(source_rel);
            if src.exists() {
                build.copy_entry(
                    name,
                    src,
                    &backup_path.join(backup_rel),
                    PathBuf::from(backup_rel),
                    true,
                )?;
            }
        }
        Ok(())
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

            // WHY(#5140): credentials and TLS keys are copied verbatim into the
            // backup; tighten the copied files to owner-only (0o600) so a backup
            // set never leaks secrets through permissive file modes.
            for sub in ["credentials", "tls"] {
                let dst = backup_path.join("config").join(sub);
                if dst.exists() {
                    set_files_restrictive(&dst);
                }
            }
        }
        Ok(())
    }

    /// Copy operator-configured additional agent workspaces. (#5139)
    ///
    /// Paths inside `instance_root` are copied. Absolute paths outside the root
    /// are recorded as omissions with a warning rather than silently dropped,
    /// because copying arbitrary external paths into a backup set is unsafe.
    fn copy_additional_workspaces(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        for (name, src) in &self.config.additional_workspaces {
            if !src.exists() {
                warn!(
                    workspace = %name,
                    path = %src.display(),
                    "additional workspace not found — skipping"
                );
                build.workspace_omissions.push(WorkspaceOmission {
                    name: name.clone(),
                    source_path: src.clone(),
                    reason: String::from("missing"),
                });
                continue;
            }

            if src.starts_with(&self.config.instance_root) {
                let rel_path = src
                    .strip_prefix(&self.config.instance_root)
                    .unwrap_or(src.as_path());
                let rel = Path::new("workspace").join(rel_path);
                build.copy_entry(name, src.clone(), &backup_path.join(&rel), rel, true)?;
            } else {
                warn!(
                    workspace = %name,
                    path = %src.display(),
                    "additional workspace is outside instance root — omitting from backup"
                );
                build.workspace_omissions.push(WorkspaceOmission {
                    name: name.clone(),
                    source_path: src.clone(),
                    reason: String::from("absolute-outside-root"),
                });
            }
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

    fn copy_configured_agent_workspaces(
        &self,
        backup_path: &Path,
        build: &mut BackupBuild,
    ) -> error::Result<()> {
        // WHY: load the operator config so the backup manifest inventories every
        // configured agent workspace, even paths that are excluded from copying.
        let oikos = taxis::oikos::Oikos::from_root(&self.config.instance_root);
        let instance_root = oikos.root();
        let config = match taxis::loader::load_config(&oikos) {
            Ok(config) => config,
            Err(err) => {
                warn!(
                    error = %err,
                    "skipping configured agent workspace inventory: failed to load aletheia config"
                );
                return Ok(());
            }
        };

        // WHY: two agents may share the same workspace directory. Copy it once
        // and record every agent that points at it with the same backup path.
        let mut copied: HashMap<PathBuf, PathBuf> = HashMap::new();

        for agent in &config.agents.list {
            let source = resolve_workspace_source(instance_root, &agent.workspace);
            let name = format!("workspace:{}", agent.id);
            let configured_backup_path = PathBuf::from("workspace/configured").join(&agent.id);

            let source_class = classify_workspace_source(instance_root, &agent.workspace, &source);

            if source_class == "absolute-outside-root" {
                build.record_optional_entry(OptionalStoreRecord {
                    name,
                    source_path: source,
                    backup_path: configured_backup_path,
                    status: String::from("excluded"),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: Some(String::from(
                        "absolute workspace outside instance root requires explicit backup policy",
                    )),
                    byte_count: 0,
                });
                continue;
            }

            if !source.exists() {
                build.record_optional_entry(OptionalStoreRecord {
                    name,
                    source_path: source,
                    backup_path: configured_backup_path,
                    status: String::from("excluded"),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: Some(String::from("workspace path missing")),
                    byte_count: 0,
                });
                continue;
            }

            // If the workspace is one of the fixed directories already copied,
            // point the manifest at the existing backup location.
            if let Some(existing_backup_path) = existing_fixed_backup_path(instance_root, &source) {
                let byte_count = dir_size(&source);
                build.record_optional_entry(OptionalStoreRecord {
                    name,
                    source_path: source,
                    backup_path: existing_backup_path,
                    status: String::from("ok"),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: None,
                    byte_count,
                });
                continue;
            }

            if let Some(existing_backup_path) = copied.get(&source) {
                build.record_optional_entry(OptionalStoreRecord {
                    name,
                    source_path: source,
                    backup_path: existing_backup_path.clone(),
                    status: String::from("ok"),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: None,
                    byte_count: 0,
                });
                continue;
            }

            let dst = backup_path.join(&configured_backup_path);
            build.copy_configured_workspace_entry(
                &name,
                source.clone(),
                &dst,
                configured_backup_path.clone(),
                agent.id.clone(),
                source_class.clone(),
            )?;
            copied.insert(source, configured_backup_path);
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

    fn write_manifest(
        &self,
        backup_path: &Path,
        build: &BackupBuild,
        snapshot_epoch: &str,
        store_generations: &HashMap<String, u64>,
    ) -> error::Result<()> {
        let manifest = BackupManifest {
            version: String::from(MANIFEST_VERSION),
            created_at: build.snapshot_time.clone(),
            source_root: self.config.instance_root.clone(),
            stores: build.stores.clone(),
            optional_stores: build.optional_stores.clone(),
            workspace_omissions: build.workspace_omissions.clone(),
            total_bytes: build.total_bytes,
            snapshot_epoch: String::from(snapshot_epoch),
            snapshot_protocol_version: String::from(SNAPSHOT_PROTOCOL_VERSION),
            quiesced: false,
            store_generations: store_generations.clone(),
            symlink_policy: String::from(SYMLINK_POLICY),
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
            let name = entry.file_name().to_string_lossy().into_owned();

            // WHY(#4950): ignore in-progress staging directories. A backup set is
            // only valid after it has been atomically renamed into place.
            if name.starts_with(STAGING_DIR_PREFIX)
                || !path.is_dir()
                || !path.join("manifest.json").is_file()
            {
                continue;
            }

            // WHY(#5138): order by the manifest's recorded `created_at` rather
            // than directory mtime, which a restore/rsync/touch can rewrite and
            // would corrupt auto-prune ordering. A manifest that cannot be read
            // or parsed is skipped (and logged) so it is never auto-pruned on a
            // wrong assumption about its age.
            let Some(created) = manifest_created_time(&path) else {
                warn!(
                    path = %path.display(),
                    "skipping backup with unreadable or malformed manifest"
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

    /// Verify a whole-instance backup set.
    ///
    /// Reads `manifest.json`, confirms the required stores (`knowledge.fjall`
    /// and `sessions.db`) are present, and iterates every fjall store to prove
    /// the files are openable. Returns the manifest and per-store results.
    #[expect(
        clippy::too_many_lines,
        reason = "#4950: sequential store verification with per-store error and generation tracking is clearer in one function"
    )]
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
            store_generations: HashMap::new(),
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
                Ok((total, generations)) => {
                    result.total_keys += total;
                    result.store_results.push((String::from(name), Ok(total)));
                    insert_generations(&mut result.store_generations, generations);
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
            // WHY(#4950): excluded entries were intentionally omitted from the
            // backup set by policy. They are recorded in the manifest but do not
            // represent a verification failure.
            if store.status == "excluded" {
                continue;
            }

            if store.status != "ok" {
                let err = if let Some(reason) = &store.exclusion_reason {
                    format!("{}: {}", store.name, reason)
                } else {
                    format!("{}: backup status is not ok", store.name)
                };
                result
                    .store_results
                    .push((store.name.clone(), Err(err.clone())));
                if result.first_error.is_none() {
                    result.first_error = Some(err);
                }
                continue;
            }

            let store_path = path.join(&store.backup_path);
            match verify_manifest_store(&store.name, &store_path) {
                Ok((total, generations)) => {
                    result.store_results.push((store.name.clone(), Ok(total)));
                    insert_generations(&mut result.store_generations, generations);
                }
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

/// Verification outcome for a single store: key count and fjall generations.
///
/// WHY(#4950): seqnos (generations) are captured during verify and recorded in
/// the backup manifest so restore can detect mismatched write points. A logical
/// store may contain several fjall cohorts, such as `knowledge.fjall/shared`.
type VerifyStoreOutcome = (usize, Vec<(String, u64)>);

fn verify_store_path(name: &str, path: &Path) -> std::result::Result<VerifyStoreOutcome, String> {
    if let Some(outcome) = verify_fjall_tree(name, path)? {
        return Ok(outcome);
    }

    if path.is_file() {
        let len = path
            .metadata()
            .map(|m| usize::try_from(m.len()).unwrap_or(usize::MAX))
            .map_err(|e| format!("{name}: failed to read file metadata: {e}"))?;
        return Ok((len, Vec::new()));
    }

    Err(format!(
        "{name}: required store is not a fjall store, fjall cohort root, or file: {}",
        path.display()
    ))
}

fn verify_manifest_store(
    name: &str,
    path: &Path,
) -> std::result::Result<VerifyStoreOutcome, String> {
    if !path.exists() {
        return Err(format!("missing: {name}"));
    }

    if let Some(outcome) = verify_fjall_tree(name, path)? {
        return Ok(outcome);
    }

    if path.is_file() {
        let len = path
            .metadata()
            .map(|m| usize::try_from(m.len()).unwrap_or(usize::MAX))
            .map_err(|e| format!("failed to read file metadata: {e}"))?;
        return Ok((len, Vec::new()));
    }

    Ok((
        usize::try_from(dir_size(path)).unwrap_or(usize::MAX),
        Vec::new(),
    ))
}

fn verify_fjall_tree(
    logical_name: &str,
    path: &Path,
) -> std::result::Result<Option<VerifyStoreOutcome>, String> {
    if path.join("version").is_file() {
        let verify = FjallBackup::verify_store(path).map_err(|e| format!("{logical_name}: {e}"))?;
        if let Some(err) = verify.first_error {
            return Err(format!("{logical_name}: {err}"));
        }
        let generations = verify
            .seqno
            .map_or_else(Vec::new, |seqno| vec![(String::from(logical_name), seqno)]);
        return Ok(Some((verify.total_keys, generations)));
    }

    if !path.is_dir() {
        return Ok(None);
    }

    let mut total_keys = 0usize;
    let mut generations = Vec::new();
    collect_fjall_tree(logical_name, path, path, &mut total_keys, &mut generations)?;

    if generations.is_empty() {
        Ok(None)
    } else {
        Ok(Some((total_keys, generations)))
    }
}

fn collect_fjall_tree(
    logical_name: &str,
    root: &Path,
    current: &Path,
    total_keys: &mut usize,
    generations: &mut Vec<(String, u64)>,
) -> std::result::Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|e| format!("{logical_name}: failed to read {}: {e}", current.display()))?;

    for entry in entries {
        let entry =
            entry.map_err(|e| format!("{logical_name}: failed to read directory entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let child_name = logical_child_name(logical_name, root, &path);
        if path.join("version").is_file() {
            let verify =
                FjallBackup::verify_store(&path).map_err(|e| format!("{child_name}: {e}"))?;
            if let Some(err) = verify.first_error {
                return Err(format!("{child_name}: {err}"));
            }
            *total_keys += verify.total_keys;
            if let Some(seqno) = verify.seqno {
                generations.push((child_name, seqno));
            }
            continue;
        }

        collect_fjall_tree(logical_name, root, &path, total_keys, generations)?;
    }

    Ok(())
}

fn insert_generations(target: &mut HashMap<String, u64>, generations: Vec<(String, u64)>) {
    for (name, seqno) in generations {
        target.insert(name, seqno);
    }
}

fn logical_child_name(logical_name: &str, root: &Path, path: &Path) -> String {
    let Ok(rel) = path.strip_prefix(root) else {
        return String::from(logical_name);
    };
    let suffix = rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if suffix.is_empty() {
        String::from(logical_name)
    } else {
        format!("{logical_name}/{suffix}")
    }
}

/// Resolve a configured workspace string against the instance root.
///
/// WHY: relative workspace paths resolve to the instance root; absolute
/// paths are taken as-is so operators can point outside the oikos.
fn resolve_workspace_source(instance_root: &Path, workspace: &str) -> PathBuf {
    let workspace_path = Path::new(workspace);
    if workspace_path.is_absolute() {
        workspace_path.to_path_buf()
    } else {
        instance_root.join(workspace_path)
    }
}

/// Classify a configured workspace for manifest attribution.
///
/// NOTE: this uses absolute-path prefix checks instead of `canonicalize`
/// so missing or outside-root paths can be classified without requiring
/// the directory to exist on disk.
fn classify_workspace_source(instance_root: &Path, workspace: &str, source: &Path) -> String {
    if !Path::new(workspace).is_absolute() {
        return String::from("in-root");
    }
    if source.starts_with(instance_root) {
        String::from("absolute-inside-root")
    } else {
        String::from("absolute-outside-root")
    }
}

/// Map a workspace that lives under a fixed copied root to its backup path.
fn existing_fixed_backup_path(instance_root: &Path, source: &Path) -> Option<PathBuf> {
    for (dir_name, backup_prefix) in [
        ("nous", "workspace/nous"),
        ("shared", "workspace/shared"),
        ("theke", "workspace/theke"),
    ] {
        let root_dir = instance_root.join(dir_name);
        if let Ok(rel) = source.strip_prefix(&root_dir) {
            return Some(PathBuf::from(backup_prefix).join(rel));
        }
    }
    None
}

#[expect(
    clippy::disallowed_methods,
    reason = "synchronous maintenance utility invoked from spawn_blocking outside the async runtime"
)]
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
    reject_symlinks_in_backup_source(src, src)?;
    copy_path_checked(src, dst, src)
}

fn copy_path_checked(src: &Path, dst: &Path, source_root: &Path) -> error::Result<(u64, u32)> {
    let metadata = fs::symlink_metadata(src).context(error::MaintenanceIoSnafu {
        context: format!("reading source metadata {}", src.display()),
    })?;
    if metadata.file_type().is_symlink() {
        return refuse_backup_source_entry("symbolic link", src, source_root);
    }

    if metadata.is_dir() {
        return copy_dir_recursive(src, dst, source_root);
    }

    if !metadata.is_file() {
        return refuse_backup_source_entry("unsupported file type", src, source_root);
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
fn copy_dir_recursive(src: &Path, dst: &Path, source_root: &Path) -> error::Result<(u64, u32)> {
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
        let metadata = fs::symlink_metadata(&src_path).context(error::MaintenanceIoSnafu {
            context: format!("reading source metadata {}", src_path.display()),
        })?;

        if metadata.file_type().is_symlink() {
            return refuse_backup_source_entry("symbolic link", &src_path, source_root);
        } else if metadata.is_dir() {
            let (bytes, files) = copy_dir_recursive(&src_path, &dst_path, source_root)?;
            total_bytes += bytes;
            total_files += files;
        } else if metadata.is_file() {
            let bytes = fs::copy(&src_path, &dst_path).context(error::MaintenanceIoSnafu {
                context: format!("copying {} to {}", src_path.display(), dst_path.display()),
            })?;
            total_bytes += bytes;
            total_files += 1;
        } else {
            return refuse_backup_source_entry("unsupported file type", &src_path, source_root);
        }
    }

    Ok((total_bytes, total_files))
}

fn reject_symlinks_in_backup_source(path: &Path, source_root: &Path) -> error::Result<()> {
    let metadata = fs::symlink_metadata(path).context(error::MaintenanceIoSnafu {
        context: format!("reading source metadata {}", path.display()),
    })?;
    if metadata.file_type().is_symlink() {
        return refuse_backup_source_entry("symbolic link", path, source_root);
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(path).context(error::MaintenanceIoSnafu {
        context: format!("reading source dir {}", path.display()),
    })?;
    for entry in entries {
        let entry = entry.context(error::MaintenanceIoSnafu {
            context: "reading directory entry",
        })?;
        reject_symlinks_in_backup_source(&entry.path(), source_root)?;
    }
    Ok(())
}

fn refuse_backup_source_entry<T>(
    reason: &str,
    path: &Path,
    source_root: &Path,
) -> error::Result<T> {
    error::BackupTraversalPolicySnafu {
        reason: String::from(reason),
        relative_path: traversal_relative_path(path, source_root),
        source_root: source_root.display().to_string(),
    }
    .fail()
}

fn traversal_relative_path(path: &Path, source_root: &Path) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        String::from(".")
    } else {
        relative.to_string_lossy().replace('\\', "/")
    }
}

/// Calculate total size of a directory tree.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                total += dir_size(&path);
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    total
}

fn default_symlink_policy() -> String {
    String::from(SYMLINK_POLICY)
}

/// Parse the `created_at` field from a backup set's manifest into a
/// [`SystemTime`]. Returns `None` if the manifest is missing, unreadable, or
/// the timestamp cannot be parsed. (#5138)
fn manifest_created_time(backup_path: &Path) -> Option<std::time::SystemTime> {
    let manifest_json = fs::read_to_string(backup_path.join("manifest.json")).ok()?;
    let manifest: BackupManifest = serde_json::from_str(&manifest_json).ok()?;
    let zoned: jiff::Zoned = manifest.created_at.parse().ok()?;
    Some(std::time::SystemTime::from(zoned.timestamp()))
}

/// Set owner-only (0o700) permissions on a directory. No-op on non-Unix. (#5140)
#[cfg(unix)]
fn set_dir_restrictive(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
        warn!(
            path = %path.display(),
            error = %e,
            "failed to set restrictive permissions on backup directory"
        );
    }
}

#[cfg(not(unix))]
fn set_dir_restrictive(_path: &Path) {}

/// Set owner-only (0o600) permissions on every regular file under `dir`,
/// recursing into subdirectories. No-op on non-Unix. (#5140)
#[cfg(unix)]
fn set_files_restrictive(dir: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            set_files_restrictive(&path);
        } else if let Err(e) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
            warn!(
                path = %path.display(),
                error = %e,
                "failed to set restrictive permissions on backup file"
            );
        }
    }
}

#[cfg(not(unix))]
fn set_files_restrictive(_dir: &Path) {}

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

    fn assert_fjall_marker(root: &Path, rel: &[&str]) {
        let mut path = root.to_path_buf();
        for segment in rel {
            path.push(segment);
        }
        assert!(path.join("version").is_file(), "missing {}", path.display());
    }

    fn assert_optional_store(manifest: &BackupManifest, name: &str) {
        assert!(
            manifest
                .optional_stores
                .iter()
                .any(|entry| entry.name == name && entry.status == "ok"),
            "manifest should include runtime store {name}"
        );
    }

    fn assert_generations(verify: &InstanceVerifyResult, names: &[&str]) {
        for name in names {
            assert!(
                verify.store_generations.contains_key(*name),
                "missing generation for {name}: {:?}",
                verify.store_generations
            );
        }
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
            additional_workspaces: Vec::new(),
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
        assert_eq!(manifest.symlink_policy, SYMLINK_POLICY);
        assert_eq!(manifest.stores.len(), 3); // knowledge, sessions, config
        assert!(manifest.stores.iter().any(|s| s.name == "knowledge.fjall"));
        assert!(manifest.stores.iter().any(|s| s.name == "sessions.db"));
        assert!(manifest.stores.iter().any(|s| s.name == "config"));
        assert!(manifest.optional_stores.iter().any(|s| s.name == "nous"));
    }

    #[test]
    fn create_backup_copies_runtime_stores_and_knowledge_cohorts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();
        write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

        make_fjall_store(
            &instance_root
                .join("data")
                .join("knowledge.fjall")
                .join("shared"),
        );
        make_fjall_store(
            &instance_root
                .join("data")
                .join("knowledge.fjall")
                .join("identity"),
        );
        make_fjall_store(&instance_root.join("data").join("sessions.db"));
        make_fjall_store(&instance_root.join("data").join("auth.fjall"));
        make_fjall_store(
            &instance_root
                .join("data")
                .join("daemon-task-state")
                .join("system"),
        );
        make_fjall_store(
            &instance_root
                .join("data")
                .join("daemon-task-state")
                .join("alice"),
        );
        make_fjall_store(&instance_root.join("data").join("cron-locks.fjall"));

        let backup_dir = tmp.path().join("backups");
        let manager = InstanceBackup::new(InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir,
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        });
        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");

        assert_fjall_marker(&backup_path, &["stores", "knowledge.fjall", "shared"]);
        assert_fjall_marker(&backup_path, &["stores", "knowledge.fjall", "identity"]);
        assert_fjall_marker(&backup_path, &["stores", "auth.fjall"]);
        assert_fjall_marker(&backup_path, &["stores", "daemon-task-state", "system"]);
        assert_fjall_marker(&backup_path, &["stores", "cron-locks.fjall"]);

        let manifest: BackupManifest =
            serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
                .unwrap();
        for name in ["auth.fjall", "daemon-task-state", "cron-locks.fjall"] {
            assert_optional_store(&manifest, name);
        }

        let verify = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(
            verify.first_error.is_none(),
            "complete runtime backup should verify: {:?}",
            verify.first_error
        );
        assert_generations(
            &verify,
            &[
                "knowledge.fjall/shared",
                "knowledge.fjall/identity",
                "sessions.db",
                "auth.fjall",
                "daemon-task-state/system",
                "daemon-task-state/alice",
                "cron-locks.fjall",
            ],
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "single fixture covers #5139 workspace classes and duplicate handling"
    )]
    fn create_backup_records_configured_agent_workspaces() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();

        make_fjall_store(&instance_root.join("data").join("knowledge.fjall"));
        make_fjall_store(&instance_root.join("data").join("sessions.db"));

        let relative_source = instance_root.join("workspaces").join("relative");
        let inside_source = instance_root.join("workspaces").join("inside");
        let outside_source = tmp.path().join("outside");
        let duplicate_source = instance_root.join("workspaces").join("duplicate");
        for path in [
            &relative_source,
            &inside_source,
            &outside_source,
            &duplicate_source,
        ] {
            fs::create_dir_all(path).unwrap();
            write_text_file(&path.join("NOTE.md"), "workspace").unwrap();
        }

        let config_toml = format!(
            r#"
[[agents.list]]
id = "alice"
workspace = "workspaces/relative"

[[agents.list]]
id = "bob"
workspace = "{}"

[[agents.list]]
id = "carol"
workspace = "{}"

[[agents.list]]
id = "dana"
workspace = "workspaces/missing"

[[agents.list]]
id = "erin"
workspace = "{}"

[[agents.list]]
id = "frank"
workspace = "{}"
"#,
            inside_source.display(),
            outside_source.display(),
            duplicate_source.display(),
            duplicate_source.display()
        );
        write_text_file(
            &instance_root.join("config").join("aletheia.toml"),
            &config_toml,
        )
        .unwrap();

        let backup_dir = tmp.path().join("backups");
        let manager = InstanceBackup::new(InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir,
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        });
        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");
        let manifest: BackupManifest =
            serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
                .unwrap();

        let entry_for = |agent_id: &str| {
            manifest
                .optional_stores
                .iter()
                .find(|entry| entry.agent_id.as_deref() == Some(agent_id))
                .unwrap_or_else(|| panic!("missing workspace entry for {agent_id}"))
        };

        let alice = entry_for("alice");
        assert_eq!(alice.status, "ok");
        assert_eq!(alice.workspace_source_class.as_deref(), Some("in-root"));
        assert_eq!(
            alice.backup_path,
            PathBuf::from("workspace").join("configured").join("alice")
        );
        assert!(
            backup_path
                .join(&alice.backup_path)
                .join("NOTE.md")
                .is_file()
        );

        let bob = entry_for("bob");
        assert_eq!(bob.status, "ok");
        assert_eq!(
            bob.workspace_source_class.as_deref(),
            Some("absolute-inside-root")
        );
        assert_eq!(
            bob.backup_path,
            PathBuf::from("workspace").join("configured").join("bob")
        );
        assert!(backup_path.join(&bob.backup_path).join("NOTE.md").is_file());

        let carol = entry_for("carol");
        assert_eq!(carol.status, "excluded");
        assert_eq!(
            carol.workspace_source_class.as_deref(),
            Some("absolute-outside-root")
        );
        assert!(
            carol
                .exclusion_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("outside"))
        );

        let dana = entry_for("dana");
        assert_eq!(dana.status, "excluded");
        assert_eq!(dana.workspace_source_class.as_deref(), Some("in-root"));
        assert!(
            dana.exclusion_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("missing"))
        );

        let erin = entry_for("erin");
        let frank = entry_for("frank");
        assert_eq!(erin.status, "ok");
        assert_eq!(frank.status, "ok");
        assert_eq!(&frank.backup_path, &erin.backup_path);
        assert_eq!(
            manifest
                .optional_stores
                .iter()
                .filter(|entry| entry.backup_path == erin.backup_path)
                .count(),
            2
        );
        assert!(
            backup_path
                .join(&erin.backup_path)
                .join("NOTE.md")
                .is_file()
        );

        // WHY(#4950): excluded entries are intentional policy omissions, not
        // verification failures, so the published backup set must verify cleanly.
        let verify = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(
            verify.first_error.is_none(),
            "excluded entries should not fail verification: {:?}",
            verify.first_error
        );
    }

    #[cfg(unix)]
    fn assert_backup_symlink_rejected<T>(
        result: error::Result<T>,
        expected_relative_path: &str,
        expected_source_root: &Path,
    ) {
        let msg = match result {
            Ok(_) => panic!("symlink traversal should be rejected"),
            Err(err) => err.to_string(),
        };
        assert!(
            msg.contains("symbolic link"),
            "error should identify symlink policy: {msg}"
        );
        assert!(
            msg.contains(expected_relative_path),
            "error should include relative path {expected_relative_path:?}: {msg}"
        );
        assert!(
            msg.contains("source root")
                && msg.contains(&expected_source_root.display().to_string()),
            "error should include source root {}: {msg}",
            expected_source_root.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_rejects_symlink_to_outside_instance_4952() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_root = tmp.path().join("instance");
        let workspace = source_root.join("nous").join("alice");
        fs::create_dir_all(&workspace).unwrap();
        write_text_file(&workspace.join("NOTE.md"), "safe").unwrap();

        let outside_dir = tmp.path().join("outside");
        fs::create_dir_all(&outside_dir).unwrap();
        write_text_file(&outside_dir.join("secret.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(outside_dir.join("secret.txt"), workspace.join("leak.txt"))
            .unwrap();

        let dst = tmp.path().join("backup-copy");
        assert_backup_symlink_rejected(
            copy_path(&source_root, &dst),
            "nous/alice/leak.txt",
            &source_root,
        );
        assert!(
            !dst.exists(),
            "pre-walk rejection must not leave a partial destination"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_rejects_symlink_loop_4952() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_root = tmp.path().join("source");
        fs::create_dir_all(&source_root).unwrap();
        write_text_file(&source_root.join("real.txt"), "safe").unwrap();
        std::os::unix::fs::symlink(".", source_root.join("loop")).unwrap();

        let dst = tmp.path().join("backup-copy");
        assert_backup_symlink_rejected(copy_path(&source_root, &dst), "loop", &source_root);
        assert!(
            !dst.exists(),
            "pre-walk rejection must not leave a partial destination"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_rejects_symlink_to_file_4952() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_root = tmp.path().join("source");
        fs::create_dir_all(&source_root).unwrap();
        write_text_file(&source_root.join("real.txt"), "safe").unwrap();
        std::os::unix::fs::symlink("real.txt", source_root.join("link.txt")).unwrap();

        let dst = tmp.path().join("backup-copy");
        assert_backup_symlink_rejected(copy_path(&source_root, &dst), "link.txt", &source_root);
        assert!(
            !dst.exists(),
            "pre-walk rejection must not leave a partial destination"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_rejects_internal_directory_symlink_4952() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_root = tmp.path().join("source");
        let target = source_root.join("target");
        fs::create_dir_all(&target).unwrap();
        write_text_file(&target.join("NOTE.md"), "safe").unwrap();
        std::os::unix::fs::symlink("target", source_root.join("target-link")).unwrap();

        let dst = tmp.path().join("backup-copy");
        assert_backup_symlink_rejected(copy_path(&source_root, &dst), "target-link", &source_root);
        assert!(
            !dst.exists(),
            "pre-walk rejection must not leave a partial destination"
        );
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
            additional_workspaces: Vec::new(),
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
                agent_id: None,
                workspace_source_class: None,
                exclusion_reason: None,
            }],
            optional_stores: Vec::new(),
            workspace_omissions: Vec::new(),
            total_bytes: 0,
            snapshot_epoch: jiff::Zoned::now().to_string(),
            snapshot_protocol_version: String::from(SNAPSHOT_PROTOCOL_VERSION),
            quiesced: false,
            store_generations: HashMap::new(),
            symlink_policy: String::from(SYMLINK_POLICY),
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
    fn verify_backup_rejects_missing_included_runtime_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();

        make_fjall_store(
            &instance_root
                .join("data")
                .join("knowledge.fjall")
                .join("shared"),
        );
        make_fjall_store(&instance_root.join("data").join("sessions.db"));
        make_fjall_store(&instance_root.join("data").join("auth.fjall"));

        let backup_dir = tmp.path().join("backups");
        let manager = InstanceBackup::new(InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir,
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        });
        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");

        fs::remove_dir_all(backup_path.join("stores").join("auth.fjall")).unwrap();

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        let err = result
            .first_error
            .expect("missing included auth store should fail verification");
        assert!(
            err.contains("auth.fjall"),
            "error should mention missing auth store: {err}"
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
            additional_workspaces: Vec::new(),
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
            additional_workspaces: Vec::new(),
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

    /// #4950 regression: backup creation must stage, verify, and atomically
    /// publish the set; the manifest must record snapshot metadata and store
    /// generation IDs.
    #[test]
    fn create_backup_publishes_verified_snapshot_with_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let instance_root = tmp.path().join("instance");
        fs::create_dir_all(instance_root.join("data")).unwrap();
        fs::create_dir_all(instance_root.join("config")).unwrap();
        write_text_file(&instance_root.join("config").join("aletheia.toml"), "test").unwrap();

        make_fjall_store_with_data(&instance_root.join("data").join("knowledge.fjall"), "k1");
        make_fjall_store_with_data(&instance_root.join("data").join("sessions.db"), "s1");

        let backup_dir = tmp.path().join("backups");
        let config = InstanceBackupConfig {
            enabled: true,
            instance_root,
            backup_dir: backup_dir.clone(),
            interval_hours: 24,
            retention_count: 7,
            additional_workspaces: Vec::new(),
        };

        let manager = InstanceBackup::new(config);
        let report = manager.create_backup().expect("backup succeeds");
        let backup_path = report.backup_path.expect("backup path set");

        // The published path must live directly under backup_dir, not in a
        // hidden staging directory.
        assert_eq!(
            backup_path.parent(),
            Some(backup_dir.as_path()),
            "backup was not atomically published into backup_dir"
        );
        assert!(
            !backup_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(STAGING_DIR_PREFIX),
            "backup path is still a staging directory"
        );

        // No staging directories should remain visible after publish.
        let leftover_staging: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(STAGING_DIR_PREFIX)
            })
            .collect();
        assert!(
            leftover_staging.is_empty(),
            "staging directories leaked after publish: {leftover_staging:?}"
        );

        let manifest: BackupManifest =
            serde_json::from_str(&fs::read_to_string(backup_path.join("manifest.json")).unwrap())
                .unwrap();
        assert!(
            !manifest.snapshot_epoch.is_empty(),
            "snapshot_epoch must be recorded"
        );
        assert_eq!(
            manifest.snapshot_protocol_version,
            SNAPSHOT_PROTOCOL_VERSION
        );
        assert!(
            !manifest.quiesced,
            "live snapshot must be recorded as not quiesced"
        );
        assert!(
            manifest.store_generations.contains_key("knowledge.fjall"),
            "knowledge generation must be captured"
        );
        assert!(
            manifest.store_generations.contains_key("sessions.db"),
            "sessions generation must be captured"
        );

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(result.first_error.is_none());
        assert_eq!(result.total_keys, 2);
    }

    /// #4950 regression: in-progress staging directories must never be listed
    /// as valid backups.
    #[test]
    fn list_backups_skips_staging_directories() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let backup_dir = tmp.path().join("backups");
        fs::create_dir_all(&backup_dir).unwrap();

        let manifest = BackupManifest {
            version: String::from(MANIFEST_VERSION),
            created_at: jiff::Zoned::now().to_string(),
            source_root: backup_dir.clone(),
            stores: Vec::new(),
            optional_stores: Vec::new(),
            workspace_omissions: Vec::new(),
            total_bytes: 0,
            snapshot_epoch: jiff::Zoned::now().to_string(),
            snapshot_protocol_version: String::from(SNAPSHOT_PROTOCOL_VERSION),
            quiesced: false,
            store_generations: HashMap::new(),
            symlink_policy: String::from(SYMLINK_POLICY),
        };
        let manifest_json = serde_json::to_string(&manifest).unwrap();

        // Create a valid backup set.
        let valid = backup_dir.join("20260101-000000.000");
        fs::create_dir_all(&valid).unwrap();
        write_text_file(&valid.join("manifest.json"), &manifest_json).unwrap();

        // Create a fake staging directory with a manifest (simulating an
        // interrupted backup).
        let staging = backup_dir.join(format!("{STAGING_DIR_PREFIX}fake"));
        fs::create_dir_all(&staging).unwrap();
        write_text_file(&staging.join("manifest.json"), &manifest_json).unwrap();

        let manager = InstanceBackup::new(InstanceBackupConfig {
            backup_dir,
            ..InstanceBackupConfig::default()
        });
        let backups = manager.list_backups().expect("list succeeds");
        assert_eq!(backups.len(), 1);
        assert_eq!(backups.first().unwrap().path, valid);
    }

    fn make_fjall_store_with_data(path: &Path, key: &str) {
        fs::create_dir_all(path).unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(path)
            .worker_threads_unchecked(0)
            .open()
            .unwrap();
        let partition = db
            .keyspace("test_data", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        partition.insert(key, b"value").unwrap();
        db.persist(fjall::PersistMode::SyncAll).unwrap();
        drop(db);
    }
}
