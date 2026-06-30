use std::collections::HashMap;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use snafu::ResultExt as _;
use tracing::{info, warn};

use crate::error;

use super::{
    BackupManifest, EntryManifestMetadata, InstanceBackup, InstanceRestoreOptions,
    InstanceRestoreReport, MANIFEST_VERSION, ManifestEvidence, RESTORE_ROLLBACK_DIR_PREFIX,
    RESTORE_STAGING_DIR_PREFIX, RestorePlan, RestorePlanEntry, RollbackEntry, STATUS_EXCLUDED,
    STATUS_OK, StoreEntry, copy_path, ensure_relative_manifest_path, is_required_store_name,
    join_manifest_backup_path, manifest_entry_iter, path_component_count, path_to_selector,
    read_backup_manifest, set_dir_restrictive, verify_integrity_values, verify_manifest_store,
    verify_required_store_path,
};

impl InstanceBackup {
    /// Restore a whole-instance backup set into this manager's instance root.
    ///
    /// The restore reads and verifies `manifest.json`, stages all selected
    /// entries into a temporary directory under the target instance, then swaps
    /// each entry into place. If any publish step fails after live data has been
    /// moved aside, the method attempts to roll all moved entries back.
    ///
    /// # Errors
    ///
    /// Returns an error if the backup set is invalid, selected manifest paths
    /// are unsafe, the service appears to be running and `force_live` is false,
    /// staging fails, publish fails, or rollback fails.
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia backup restore command
    pub fn restore_backup(
        &self,
        options: &InstanceRestoreOptions,
    ) -> error::Result<InstanceRestoreReport> {
        self.ensure_restore_preflight(options.force_live)?;

        let verify = InstanceBackup::verify_backup(&options.backup_path)?;
        if let Some(err) = verify.first_error {
            return error::MaintenanceInvariantSnafu {
                context: format!("backup verification failed before restore: {err}"),
            }
            .fail();
        }
        let Some(manifest) = verify.manifest else {
            return error::MaintenanceInvariantSnafu {
                context: String::from("backup verification did not return a manifest"),
            }
            .fail();
        };
        if manifest.version != MANIFEST_VERSION {
            return error::MaintenanceInvariantSnafu {
                context: format!("unsupported backup manifest version: {}", manifest.version),
            }
            .fail();
        }

        let (_, evidence) = read_backup_manifest(&options.backup_path)?;
        let restore_plan =
            self.build_restore_plan(&options.backup_path, &manifest, &evidence, options)?;
        if restore_plan.entries.is_empty() {
            return error::MaintenanceInvariantSnafu {
                context: String::from("restore selection did not include any manifest entries"),
            }
            .fail();
        }

        let staging_dir = self.prepare_restore_tempdir(RESTORE_STAGING_DIR_PREFIX)?;
        let mut bytes_copied = 0u64;
        for entry in &restore_plan.entries {
            let staged_path = staging_dir.path().join(&entry.target_rel);
            let (bytes, _) = copy_path(&entry.backup_source, &staged_path)?;
            bytes_copied += bytes;
            verify_staged_restore_entry(entry, &staged_path)?;
        }

        let rollback_dir = self.prepare_restore_tempdir(RESTORE_ROLLBACK_DIR_PREFIX)?;
        let mut rollback_entries = Vec::new();
        if let Err(err) = publish_restore_plan(
            &restore_plan.entries,
            staging_dir.path(),
            rollback_dir.path(),
            &mut rollback_entries,
        ) {
            if let Err(rollback_err) = rollback_restore(&rollback_entries) {
                let rollback_path = rollback_dir.keep();
                return error::MaintenanceInvariantSnafu {
                    context: format!(
                        "restore failed after live data was moved aside: {err}; \
                         automatic rollback failed: {rollback_err}; rollback data kept at {}",
                        rollback_path.display()
                    ),
                }
                .fail();
            }
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "restore failed after live data was moved aside: {err}; live data rolled back"
                ),
            }
            .fail();
        }

        let live_entries_replaced = rollback_entries
            .iter()
            .filter(|entry| entry.rollback_path.is_some())
            .count();

        info!(
            backup = %options.backup_path.display(),
            restored = restore_plan.entries.len(),
            replaced = live_entries_replaced,
            bytes = bytes_copied,
            "instance backup restored"
        );

        Ok(InstanceRestoreReport {
            backup_path: options.backup_path.clone(),
            entries_restored: restore_plan.entries.len(),
            entries_skipped: restore_plan.entries_skipped,
            live_entries_replaced,
            bytes_copied,
        })
    }

    fn ensure_restore_preflight(&self, force_live: bool) -> error::Result<()> {
        if force_live {
            warn!(
                instance = %self.config.instance_root.display(),
                "force-live restore requested; skipping live-service preflight"
            );
            return Ok(());
        }

        if let Some(pid) = live_pid_from_pid_file(&self.config.instance_root)? {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "aletheia appears to be running with PID {pid}; stop the service before restore \
                     or pass --force-live to accept unsafe live restore"
                ),
            }
            .fail();
        }

        if configured_gateway_accepts_connections(&self.config.instance_root) {
            return error::MaintenanceInvariantSnafu {
                context: String::from(
                    "aletheia gateway appears to be accepting connections; stop the service before \
                     restore or pass --force-live to accept unsafe live restore",
                ),
            }
            .fail();
        }

        Ok(())
    }

    fn prepare_restore_tempdir(&self, prefix: &str) -> error::Result<tempfile::TempDir> {
        fs::create_dir_all(&self.config.instance_root).context(error::MaintenanceIoSnafu {
            context: format!(
                "creating instance root {}",
                self.config.instance_root.display()
            ),
        })?;
        let tempdir = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(&self.config.instance_root)
            .context(error::MaintenanceIoSnafu {
                context: format!(
                    "creating restore temp dir in {}",
                    self.config.instance_root.display()
                ),
            })?;
        set_dir_restrictive(tempdir.path());
        Ok(tempdir)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "WHY(#4953): restore planning validates manifest evidence, selectors, collisions, and ordering in one pass"
    )]
    fn build_restore_plan(
        &self,
        backup_root: &Path,
        manifest: &BackupManifest,
        evidence: &ManifestEvidence,
        options: &InstanceRestoreOptions,
    ) -> error::Result<RestorePlan> {
        ensure_restore_selectors(manifest, evidence, options)?;

        let mut candidates = Vec::new();
        let mut entries_skipped = 0usize;
        for (section, index, store) in manifest_entry_iter(manifest) {
            if store.status == STATUS_EXCLUDED {
                entries_skipped += 1;
                continue;
            }
            if store.status != STATUS_OK {
                return error::MaintenanceInvariantSnafu {
                    context: format!(
                        "manifest entry {} has non-restorable status {}",
                        store.name, store.status
                    ),
                }
                .fail();
            }

            let entry_evidence = evidence.entry(section, index);
            let target_rel = restore_target_rel(entry_evidence).map_err(|err| {
                error::MaintenanceInvariantSnafu {
                    context: format!("invalid restore target for {}: {err}", store.name),
                }
                .build()
            })?;
            let file_count = entry_evidence
                .and_then(|entry| entry.file_count)
                .ok_or_else(|| {
                    error::MaintenanceInvariantSnafu {
                        context: format!("manifest entry {} is missing file_count", store.name),
                    }
                    .build()
                })?;

            if !options.include.is_empty()
                && !selector_set_matches(store, &options.include, Some(&target_rel))
            {
                entries_skipped += 1;
                continue;
            }
            if selector_set_matches(store, &options.exclude, Some(&target_rel)) {
                entries_skipped += 1;
                continue;
            }

            let backup_source = join_manifest_backup_path(backup_root, &store.backup_path)
                .map_err(|err| {
                    error::MaintenanceInvariantSnafu {
                        context: format!("invalid backup path for {}: {err}", store.name),
                    }
                    .build()
                })?;
            if !backup_source.exists() {
                return error::MaintenanceInvariantSnafu {
                    context: format!(
                        "manifest entry {} is missing from backup set at {}",
                        store.name,
                        backup_source.display()
                    ),
                }
                .fail();
            }

            let target_path = self.config.instance_root.join(&target_rel);
            candidates.push(RestorePlanEntry {
                name: store.name.clone(),
                backup_path: store.backup_path.clone(),
                backup_source,
                target_rel,
                target_path,
                byte_count: store.byte_count,
                file_count,
                sha256: store.sha256.clone(),
            });
        }

        candidates.sort_by_key(|entry| path_component_count(&entry.target_rel));

        let mut entries = Vec::new();
        let mut seen_targets: HashMap<PathBuf, PathBuf> = HashMap::new();
        for candidate in candidates {
            if let Some(existing_backup_path) = seen_targets.get(&candidate.target_rel) {
                if existing_backup_path == &candidate.backup_path {
                    entries_skipped += 1;
                    continue;
                }
                return error::MaintenanceInvariantSnafu {
                    context: format!(
                        "restore target collision at {} between {} and {}",
                        candidate.target_rel.display(),
                        existing_backup_path.display(),
                        candidate.backup_path.display()
                    ),
                }
                .fail();
            }

            if entries
                .iter()
                .any(|entry: &RestorePlanEntry| candidate.target_rel.starts_with(&entry.target_rel))
            {
                entries_skipped += 1;
                continue;
            }

            seen_targets.insert(candidate.target_rel.clone(), candidate.backup_path.clone());
            entries.push(candidate);
        }

        Ok(RestorePlan {
            entries,
            entries_skipped,
        })
    }
}

pub(crate) fn ensure_restore_selectors(
    manifest: &BackupManifest,
    evidence: &ManifestEvidence,
    options: &InstanceRestoreOptions,
) -> error::Result<()> {
    for selector in options.include.iter().chain(options.exclude.iter()) {
        if !manifest_entry_iter(manifest).any(|(section, index, store)| {
            let target_rel = evidence
                .entry(section, index)
                .and_then(|entry| entry.restore_path.as_deref());
            selector_matches_entry(store, selector, target_rel)
        }) {
            return error::MaintenanceInvariantSnafu {
                context: format!("restore selector did not match any manifest entry: {selector}"),
            }
            .fail();
        }
    }
    Ok(())
}

pub(crate) fn selector_set_matches(
    store: &StoreEntry,
    selectors: &[String],
    target_rel: Option<&Path>,
) -> bool {
    !selectors.is_empty()
        && selectors
            .iter()
            .any(|selector| selector_matches_entry(store, selector, target_rel))
}

pub(crate) fn selector_matches_entry(
    store: &StoreEntry,
    selector: &str,
    target_rel: Option<&Path>,
) -> bool {
    if selector == store.name || selector == path_to_selector(&store.backup_path) {
        return true;
    }

    if let Some(target_rel) = target_rel
        && selector == path_to_selector(target_rel)
    {
        return true;
    }

    false
}

pub(crate) fn restore_target_rel(
    evidence: Option<&EntryManifestMetadata>,
) -> std::result::Result<PathBuf, String> {
    let Some(rel) = evidence.and_then(|entry| entry.restore_path.as_deref()) else {
        return Err(String::from("manifest entry is missing restore_path"));
    };
    ensure_relative_manifest_path(rel, "source-relative target path")?;
    Ok(rel.to_path_buf())
}

pub(crate) fn verify_staged_restore_entry(
    entry: &RestorePlanEntry,
    staged_path: &Path,
) -> error::Result<()> {
    let verify_result = if is_required_store_name(&entry.name) {
        verify_required_store_path(&entry.name, staged_path)
    } else {
        verify_manifest_store(&entry.name, staged_path)
    };
    let (_, _) = verify_result.map_err(|err| {
        error::MaintenanceInvariantSnafu {
            context: format!(
                "staged restore verification failed for {}: {err}",
                entry.name
            ),
        }
        .build()
    })?;
    let sha256 = entry.sha256.as_deref().ok_or_else(|| {
        error::MaintenanceInvariantSnafu {
            context: format!("staged restore entry {} is missing sha256", entry.name),
        }
        .build()
    })?;
    verify_integrity_values(
        &entry.name,
        staged_path,
        entry.byte_count,
        entry.file_count,
        sha256,
    )
    .map_err(|err| {
        error::MaintenanceInvariantSnafu {
            context: format!("staged restore integrity verification failed: {err}"),
        }
        .build()
    })
}

pub(crate) fn publish_restore_plan(
    entries: &[RestorePlanEntry],
    staging_root: &Path,
    rollback_root: &Path,
    rollback_entries: &mut Vec<RollbackEntry>,
) -> error::Result<()> {
    for entry in entries {
        let staged_path = staging_root.join(&entry.target_rel);
        if !staged_path.exists() {
            return error::MaintenanceInvariantSnafu {
                context: format!(
                    "staged restore entry missing before publish: {}",
                    staged_path.display()
                ),
            }
            .fail();
        }

        if let Some(parent) = entry.target_path.parent() {
            fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
                context: format!("creating restore target parent {}", parent.display()),
            })?;
        }

        let rollback_path = if entry.target_path.exists() {
            let rollback_path = rollback_root.join(&entry.target_rel);
            if let Some(parent) = rollback_path.parent() {
                fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
                    context: format!("creating rollback parent {}", parent.display()),
                })?;
            }
            fs::rename(&entry.target_path, &rollback_path).context(error::MaintenanceIoSnafu {
                context: format!(
                    "moving live entry {} to rollback {}",
                    entry.target_path.display(),
                    rollback_path.display()
                ),
            })?;
            Some(rollback_path)
        } else {
            None
        };

        rollback_entries.push(RollbackEntry {
            target_path: entry.target_path.clone(),
            rollback_path,
            published: false,
        });

        fs::rename(&staged_path, &entry.target_path).context(error::MaintenanceIoSnafu {
            context: format!(
                "publishing staged restore {} to {}",
                staged_path.display(),
                entry.target_path.display()
            ),
        })?;

        if let Some(last) = rollback_entries.last_mut() {
            last.published = true;
        }
    }
    Ok(())
}

pub(crate) fn rollback_restore(entries: &[RollbackEntry]) -> error::Result<()> {
    for entry in entries.iter().rev() {
        if entry.published && entry.target_path.exists() {
            remove_path(&entry.target_path)?;
        }
        if let Some(rollback_path) = &entry.rollback_path {
            if let Some(parent) = entry.target_path.parent() {
                fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
                    context: format!("creating rollback restore parent {}", parent.display()),
                })?;
            }
            fs::rename(rollback_path, &entry.target_path).context(error::MaintenanceIoSnafu {
                context: format!(
                    "restoring rollback entry {} to {}",
                    rollback_path.display(),
                    entry.target_path.display()
                ),
            })?;
        }
    }
    Ok(())
}

pub(crate) fn remove_path(path: &Path) -> error::Result<()> {
    let metadata = fs::symlink_metadata(path).context(error::MaintenanceIoSnafu {
        context: format!("reading path metadata for removal {}", path.display()),
    })?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).context(error::MaintenanceIoSnafu {
            context: format!("removing directory {}", path.display()),
        })
    } else {
        fs::remove_file(path).context(error::MaintenanceIoSnafu {
            context: format!("removing file {}", path.display()),
        })
    }
}

pub(crate) fn live_pid_from_pid_file(instance_root: &Path) -> error::Result<Option<u32>> {
    let pid_path = instance_root.join("aletheia.pid");
    if !pid_path.is_file() {
        return Ok(None);
    }
    let pid_text = fs::read_to_string(&pid_path).context(error::MaintenanceIoSnafu {
        context: format!("reading PID file {}", pid_path.display()),
    })?;
    let Ok(pid) = pid_text.trim().parse::<u32>() else {
        return Ok(None);
    };
    if process_id_is_live(pid) {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

#[cfg(unix)]
pub(crate) fn process_id_is_live(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(unix))]
pub(crate) fn process_id_is_live(_pid: u32) -> bool {
    false
}

pub(crate) fn configured_gateway_accepts_connections(instance_root: &Path) -> bool {
    let oikos = taxis::oikos::Oikos::from_root(instance_root);
    let config = match taxis::loader::load_config(&oikos) {
        Ok(config) => config,
        Err(err) => {
            warn!(
                error = %err,
                "skipping gateway live-service probe: failed to load aletheia config"
            );
            return false;
        }
    };
    let host = gateway_probe_host(&config.gateway.bind);
    let Ok(addrs) = (host.as_str(), config.gateway.port).to_socket_addrs() else {
        return false;
    };

    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok() {
            return true;
        }
    }
    false
}

pub(crate) fn gateway_probe_host(bind: &str) -> String {
    match bind {
        "lan" | "0.0.0.0" | "localhost" => String::from("127.0.0.1"),
        "::" => String::from("::1"),
        other => String::from(other),
    }
}
