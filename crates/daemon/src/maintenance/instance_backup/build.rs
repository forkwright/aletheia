use std::path::{Path, PathBuf};

use crate::error;

use super::{
    BackupBuild, EntryManifestMetadata, OptionalStoreRecord, STATUS_OK, StoreEntry, copy_path,
    ensure_relative_manifest_path, hash_path,
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
            snapshot_time: jiff::Zoned::now().to_string(),
        }
    }

    pub(crate) fn copy_entry(
        &mut self,
        name: &str,
        src: PathBuf,
        dst: &Path,
        backup_path: PathBuf,
        optional: bool,
    ) -> error::Result<()> {
        let (bytes, files) = copy_path(&src, dst)?;
        let sha256 = Some(hash_path(dst)?);
        let file_count = u64::from(files);
        let restore_path = self.restore_path_for_source(&src)?;
        self.total_bytes += bytes;
        self.total_files += file_count;
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: self.snapshot_time.clone(),
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
        let (bytes, files) = copy_path(&src, dst)?;
        let sha256 = Some(hash_path(dst)?);
        let file_count = u64::from(files);
        let restore_path = self.restore_path_for_source(&src)?;
        self.total_bytes += bytes;
        self.total_files += file_count;
        let entry = StoreEntry {
            name: String::from(name),
            source_path: src,
            backup_path,
            snapshot_time: self.snapshot_time.clone(),
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
