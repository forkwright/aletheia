use std::path::{Path, PathBuf};

use crate::error;

use super::{
    BackupBuild, BackupSourceEntry, EntryManifestMetadata, ExclusionCounts, OptionalStoreRecord,
    STATUS_EXCLUDED, STATUS_OK, StoreEntry, copy_path, copy_path_excluding,
    ensure_relative_manifest_path, hash_path, resolve_backup_source_entry,
};

impl BackupBuild {
    pub(crate) fn new(source_root: PathBuf) -> Self {
        Self {
            source_root,
            stores: Vec::new(),
            optional_stores: Vec::new(),
            store_metadata: Vec::new(),
            optional_store_metadata: Vec::new(),
            workspace_omissions: Vec::new(),
            total_bytes: 0,
            total_files: 0,
            exclusions: ExclusionCounts::default(),
            snapshot_time: jiff::Zoned::now().to_string(),
            first_entry_copied_at: None,
            last_entry_copied_at: None,
        }
    }

    /// Record that an entry's copy just completed, for skew measurement.
    fn record_copy_instant(&mut self) {
        let now = std::time::Instant::now();
        self.first_entry_copied_at.get_or_insert(now);
        self.last_entry_copied_at = Some(now);
    }

    /// Observed wall-clock spread between the first and last entry actually
    /// copied into this backup set, in seconds.
    ///
    /// WHY(#6442): `snapshot_epoch` is stamped once before any copying starts
    /// and cannot show cross-store skew. This is sampled from a monotonic
    /// clock around each real copy, so it is immune to wall-clock
    /// adjustments and genuinely answers "how non-atomic was this backup".
    /// `None` when fewer than two entries were copied (nothing to measure).
    pub(crate) fn observed_snapshot_skew_seconds(&self) -> Option<f64> {
        let first = self.first_entry_copied_at?;
        let last = self.last_entry_copied_at?;
        Some(last.saturating_duration_since(first).as_secs_f64())
    }

    pub(crate) fn copy_entry(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        optional: bool,
    ) -> error::Result<()> {
        let (bytes, files, planning_excluded) = copy_path(&src, dst)?;
        let sha256 = Some(hash_path(dst)?);
        let file_count = u64::from(files);
        let restore_path = self.restore_path_for_source(&src)?;
        self.total_bytes += bytes;
        self.total_files += file_count;
        self.exclusions.planning_symlinks += planning_excluded;
        self.record_copy_instant();
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: jiff::Zoned::now().to_string(),
            byte_count: bytes,
            status: String::from(STATUS_OK),
            agent_id: None,
            workspace_source_class: None,
            exclusion_reason: None,
            sha256,
        };
        let metadata = EntryManifestMetadata {
            file_count: Some(file_count),
            restore_path: Some(restore_path),
        };
        if optional {
            self.optional_stores.push(entry);
            self.optional_store_metadata.push(metadata);
        } else {
            self.stores.push(entry);
            self.store_metadata.push(metadata);
        }
        Ok(())
    }

    /// Like [`Self::copy_entry`], but skips any source file for which
    /// `exclude` returns `true` and tallies how many were skipped into
    /// [`ExclusionCounts::credential_keys`]. (#5353)
    pub(crate) fn copy_entry_excluding<F: Fn(&Path) -> bool>(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        optional: bool,
        exclude: &F,
    ) -> error::Result<()> {
        let (bytes, files, excluded, planning_excluded) = copy_path_excluding(&src, dst, exclude)?;
        let sha256 = Some(hash_path(dst)?);
        let file_count = u64::from(files);
        let restore_path = self.restore_path_for_source(&src)?;
        self.total_bytes += bytes;
        self.total_files += file_count;
        self.exclusions.credential_keys += excluded;
        self.exclusions.planning_symlinks += planning_excluded;
        self.record_copy_instant();
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: jiff::Zoned::now().to_string(),
            byte_count: bytes,
            status: String::from(STATUS_OK),
            agent_id: None,
            workspace_source_class: None,
            exclusion_reason: None,
            sha256,
        };
        let metadata = EntryManifestMetadata {
            file_count: Some(file_count),
            restore_path: Some(restore_path),
        };
        if optional {
            self.optional_stores.push(entry);
            self.optional_store_metadata.push(metadata);
        } else {
            self.stores.push(entry);
            self.store_metadata.push(metadata);
        }
        Ok(())
    }

    /// Copy a configured agent workspace and record its coverage metadata.
    pub(crate) fn copy_configured_workspace_entry(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        agent_id: String,
        workspace_source_class: String,
    ) -> error::Result<()> {
        // WHY(#7246, #7320): an operator-configured workspace path is the
        // one caller here where `src` itself (not merely something nested
        // under it) could be named `.planning` -- guard the same as a
        // nested one so it is excluded and recorded, not routed into
        // `copy_path` (which would report an empty copy) or `hash_path`
        // (which would error: nothing was ever written to `dst`).
        //
        // WHY(#7320): the guard must key off `resolve_backup_source_entry`
        // (symlink-ness, like the other five call sites fixed by #7246 /
        // PR #7309), not `is_excluded_backup_symlink_name` alone -- that
        // checks only the file name, so a real directory happening to be
        // named `.planning` was excluded and reported as a symlink
        // exclusion when it should be copied like any other workspace.
        if matches!(
            resolve_backup_source_entry(&src, &src)?,
            BackupSourceEntry::ExcludedPlanning
        ) {
            self.exclusions.planning_symlinks += 1;
            self.record_optional_entry(OptionalStoreRecord {
                name: String::from(name),
                source_path: src,
                backup_path,
                restore_path: None,
                status: String::from(STATUS_EXCLUDED),
                agent_id: Some(agent_id),
                workspace_source_class: Some(workspace_source_class),
                exclusion_reason: Some(String::from(
                    "configured workspace is a `.planning` symlink, excluded from backups and \
                     never dereferenced",
                )),
                byte_count: 0,
                file_count: 0,
                sha256: None,
            });
            return Ok(());
        }
        let (bytes, files, planning_excluded) = copy_path(&src, dst)?;
        let sha256 = Some(hash_path(dst)?);
        let file_count = u64::from(files);
        let restore_path = self.restore_path_for_source(&src)?;
        self.total_bytes += bytes;
        self.total_files += file_count;
        self.exclusions.planning_symlinks += planning_excluded;
        self.record_copy_instant();
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: jiff::Zoned::now().to_string(),
            byte_count: bytes,
            status: String::from(STATUS_OK),
            agent_id: Some(agent_id),
            workspace_source_class: Some(workspace_source_class),
            exclusion_reason: None,
            sha256,
        };
        self.optional_stores.push(entry);
        self.optional_store_metadata.push(EntryManifestMetadata {
            file_count: Some(file_count),
            restore_path: Some(restore_path),
        });
        Ok(())
    }

    pub(crate) fn record_optional_entry(&mut self, record: OptionalStoreRecord) {
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
            sha256: record.sha256,
        };
        self.optional_stores.push(entry);
        self.optional_store_metadata.push(EntryManifestMetadata {
            file_count: Some(record.file_count),
            restore_path: record.restore_path,
        });
    }

    pub(crate) fn restore_path_for_source(&self, src: &Path) -> error::Result<PathBuf> {
        let rel = src.strip_prefix(&self.source_root).map_err(|_strip_err| {
            error::MaintenanceInvariantSnafu {
                context: format!(
                    "backup source {} is outside instance root {}",
                    src.display(),
                    self.source_root.display()
                ),
            }
            .build()
        })?;
        ensure_relative_manifest_path(rel, "restore path").map_err(|err| {
            error::MaintenanceInvariantSnafu {
                context: format!("invalid restore path for {}: {err}", src.display()),
            }
            .build()
        })?;
        Ok(rel.to_path_buf())
    }
}
