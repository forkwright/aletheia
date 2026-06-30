use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use snafu::ResultExt as _;
use tracing::{info, warn};

use crate::error;

use super::super::fjall_backup::BackupEntry;
use super::{
    BackupBuild, BackupManifest, InstanceBackup, InstanceBackupConfig, InstanceBackupReport,
    MANIFEST_VERSION, OptionalStoreRecord, SNAPSHOT_PROTOCOL_VERSION, STAGING_DIR_PREFIX,
    STATUS_EXCLUDED, SYMLINK_POLICY, WorkspaceOmission, classify_workspace_source, dir_size,
    inject_manifest_evidence, manifest_created_time, resolve_workspace_source, set_dir_restrictive,
    set_files_restrictive, write_text_file,
};

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(0);

impl InstanceBackup {
    /// Create a new whole-instance backup manager.
    #[must_use]
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia backup and maintenance commands
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
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia backup and maintenance commands
    pub fn create_backup(&self) -> error::Result<InstanceBackupReport> {
        let (staging_path, final_path) = self.prepare_staging_path()?;
        let mut build = BackupBuild::new(self.config.instance_root.clone());

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
            files_copied: u32::try_from(total_files).unwrap_or(u32::MAX),
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

        // WHY(#4645): suffix the timestamp with a local sequence so rapid
        // manual/test invocations do not collide inside the same millisecond.
        let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
        let timestamp = format!(
            "{}-{seq:04}",
            jiff::Zoned::now().strftime("%Y%m%d-%H%M%S%.3f")
        );
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
        if !knowledge_src.is_dir() {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "knowledge store must be a current fjall directory or cohort root: {}",
                    knowledge_src.display()
                ),
            }
            .fail();
        }
        if !sessions_src.is_dir() {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "session store must be a current fjall directory or cohort root: {}",
                    sessions_src.display()
                ),
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
                    restore_path: None,
                    status: String::from(STATUS_EXCLUDED),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: Some(String::from(
                        "absolute workspace outside instance root requires explicit backup policy",
                    )),
                    byte_count: 0,
                    file_count: 0,
                    sha256: None,
                });
                continue;
            }

            if !source.exists() {
                let restore_path = build.restore_path_for_source(&source)?;
                build.record_optional_entry(OptionalStoreRecord {
                    name,
                    source_path: source,
                    backup_path: configured_backup_path,
                    restore_path: Some(restore_path),
                    status: String::from(STATUS_EXCLUDED),
                    agent_id: Some(agent.id.clone()),
                    workspace_source_class: Some(source_class),
                    exclusion_reason: Some(String::from("workspace path missing")),
                    byte_count: 0,
                    file_count: 0,
                    sha256: None,
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
                let (name, rel) = if src.components().any(|c| c.as_os_str() == "logs") {
                    ("prompt-audit:logs", "logs/prompt-audit")
                } else {
                    ("prompt-audit:data", "data/prompt-audit")
                };
                build.copy_entry(name, src, &backup_path.join(rel), PathBuf::from(rel), true)?;
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
        let mut manifest_value = serde_json::to_value(&manifest)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: String::from("serializing backup manifest"),
            })?;
        inject_manifest_evidence(&mut manifest_value, build, store_generations)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: String::from("serializing backup manifest integrity evidence"),
            })?;
        let manifest_path = backup_path.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&manifest_value)
            .map_err(std::io::Error::other)
            .context(error::MaintenanceIoSnafu {
                context: String::from("serializing backup manifest"),
            })?;
        write_text_file(&manifest_path, &manifest_json)?;
        Ok(())
    }

    /// List existing whole-instance backups, newest first.
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia backup list/latest commands
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
