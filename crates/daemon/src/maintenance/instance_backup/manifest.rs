use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt as _;

use crate::error;

use super::{
    BackupBuild, BackupManifest, EntryManifestMetadata, MANIFEST_CHECKPOINT_GENERATIONS_FIELD,
    MANIFEST_FILE_COUNT_FIELD, MANIFEST_RESTORE_PATH_FIELD, MANIFEST_TOTAL_FILES_FIELD,
    MANIFEST_VERSION, ManifestEvidence, ManifestSection, SNAPSHOT_PROTOCOL_VERSION,
    STATUS_EXCLUDED, STATUS_OK, SYMLINK_POLICY, StoreEntry,
};

pub(crate) fn join_manifest_backup_path(
    backup_root: &Path,
    backup_path: &Path,
) -> std::result::Result<PathBuf, String> {
    ensure_relative_manifest_path(backup_path, "backup path")?;
    Ok(backup_root.join(backup_path))
}

pub(crate) fn ensure_relative_manifest_path(
    path: &Path,
    label: &str,
) -> std::result::Result<(), String> {
    let mut has_component = false;
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.contains('\\') {
                    return Err(format!(
                        "{label} must not contain alternate path separators: {}",
                        path.display()
                    ));
                }
                has_component = true;
            }
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "{label} must be a clean relative path: {}",
                    path.display()
                ));
            }
        }
    }
    if !has_component {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}

pub(crate) fn path_to_selector(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn path_component_count(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count()
}

pub(crate) fn read_backup_manifest(
    backup_root: &Path,
) -> error::Result<(BackupManifest, ManifestEvidence)> {
    let manifest_path = backup_root.join("manifest.json");
    if !manifest_path.is_file() {
        return error::MaintenanceInvariantSnafu {
            context: format!(
                "not an instance backup set (missing manifest.json): {}",
                backup_root.display()
            ),
        }
        .fail();
    }

    let manifest_json = fs::read_to_string(&manifest_path).context(error::MaintenanceIoSnafu {
        context: format!("reading manifest {}", manifest_path.display()),
    })?;
    let manifest_value: serde_json::Value = serde_json::from_str(&manifest_json)
        .map_err(std::io::Error::other)
        .context(error::MaintenanceIoSnafu {
            context: format!("parsing manifest {}", manifest_path.display()),
        })?;
    let manifest: BackupManifest = serde_json::from_value(manifest_value.clone())
        .map_err(std::io::Error::other)
        .context(error::MaintenanceIoSnafu {
            context: format!("parsing manifest {}", manifest_path.display()),
        })?;
    let evidence = parse_manifest_evidence(&manifest_value);

    Ok((manifest, evidence))
}

pub(crate) fn inject_manifest_evidence(
    manifest_value: &mut serde_json::Value,
    build: &BackupBuild,
    store_generations: &HashMap<String, u64>,
) -> Result<(), serde_json::Error> {
    if let Some(object) = manifest_value.as_object_mut() {
        object.insert(
            String::from(MANIFEST_TOTAL_FILES_FIELD),
            serde_json::Value::from(build.total_files),
        );
    }

    inject_entry_evidence(
        manifest_value,
        "stores",
        &build.stores,
        &build.store_metadata,
        store_generations,
    )?;
    inject_entry_evidence(
        manifest_value,
        "optional_stores",
        &build.optional_stores,
        &build.optional_store_metadata,
        store_generations,
    )
}

pub(crate) fn inject_entry_evidence(
    manifest_value: &mut serde_json::Value,
    field: &str,
    entries: &[StoreEntry],
    metadata: &[EntryManifestMetadata],
    store_generations: &HashMap<String, u64>,
) -> Result<(), serde_json::Error> {
    let Some(array) = manifest_value
        .get_mut(field)
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };

    for ((entry_value, entry), metadata) in array.iter_mut().zip(entries).zip(metadata) {
        let Some(object) = entry_value.as_object_mut() else {
            continue;
        };
        if let Some(file_count) = metadata.file_count {
            object.insert(
                String::from(MANIFEST_FILE_COUNT_FIELD),
                serde_json::Value::from(file_count),
            );
        }
        if let Some(restore_path) = &metadata.restore_path {
            object.insert(
                String::from(MANIFEST_RESTORE_PATH_FIELD),
                serde_json::to_value(restore_path)?,
            );
        }
        let checkpoint_generations =
            checkpoint_generations_for_entry(&entry.name, store_generations);
        if !checkpoint_generations.is_empty() {
            object.insert(
                String::from(MANIFEST_CHECKPOINT_GENERATIONS_FIELD),
                serde_json::to_value(checkpoint_generations)?,
            );
        }
    }

    Ok(())
}

pub(crate) fn checkpoint_generations_for_entry(
    entry_name: &str,
    store_generations: &HashMap<String, u64>,
) -> HashMap<String, u64> {
    let child_prefix = format!("{entry_name}/");
    store_generations
        .iter()
        .filter(|(name, _)| *name == entry_name || name.starts_with(&child_prefix))
        .map(|(name, seqno)| (name.clone(), *seqno))
        .collect()
}

pub(crate) fn parse_manifest_evidence(manifest_value: &serde_json::Value) -> ManifestEvidence {
    ManifestEvidence {
        total_files: manifest_value
            .get(MANIFEST_TOTAL_FILES_FIELD)
            .and_then(serde_json::Value::as_u64),
        stores: parse_entry_evidence_array(manifest_value.get("stores")),
        optional_stores: parse_entry_evidence_array(manifest_value.get("optional_stores")),
    }
}

pub(crate) fn parse_entry_evidence_array(
    value: Option<&serde_json::Value>,
) -> Vec<EntryManifestMetadata> {
    match value.and_then(serde_json::Value::as_array) {
        Some(entries) => entries.iter().map(parse_entry_evidence).collect(),
        None => Vec::new(),
    }
}

pub(crate) fn parse_entry_evidence(value: &serde_json::Value) -> EntryManifestMetadata {
    EntryManifestMetadata {
        file_count: value
            .get(MANIFEST_FILE_COUNT_FIELD)
            .and_then(serde_json::Value::as_u64),
        restore_path: value
            .get(MANIFEST_RESTORE_PATH_FIELD)
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from),
    }
}

pub(crate) fn ensure_manifest_for_verify(
    manifest: &BackupManifest,
    evidence: &ManifestEvidence,
) -> std::result::Result<(), String> {
    if manifest.version != MANIFEST_VERSION {
        return Err(format!(
            "unsupported backup manifest version: {}",
            manifest.version
        ));
    }
    if manifest.snapshot_protocol_version != SNAPSHOT_PROTOCOL_VERSION {
        return Err(format!(
            "unsupported backup snapshot protocol version: {}",
            manifest.snapshot_protocol_version
        ));
    }
    if manifest.symlink_policy != SYMLINK_POLICY {
        return Err(format!(
            "unsupported backup symlink policy: {}",
            manifest.symlink_policy
        ));
    }

    ensure_manifest_entry_set(manifest)?;
    ensure_required_manifest_stores(manifest)?;
    ensure_manifest_entry_evidence(manifest, evidence)?;
    ensure_manifest_totals(manifest, evidence)
}

pub(crate) fn ensure_manifest_entry_set(
    manifest: &BackupManifest,
) -> std::result::Result<(), String> {
    let mut names = HashSet::new();
    let mut backup_paths = HashSet::new();

    for store in manifest
        .stores
        .iter()
        .chain(manifest.optional_stores.iter())
    {
        if store.name.trim().is_empty() {
            return Err(String::from(
                "manifest entry logical name must not be empty",
            ));
        }
        if !names.insert(store.name.clone()) {
            return Err(format!("duplicate manifest logical name: {}", store.name));
        }
        ensure_relative_manifest_path(&store.backup_path, "backup path")?;
        let backup_selector = path_to_selector(&store.backup_path);
        if !backup_paths.insert(backup_selector.clone()) {
            return Err(format!("duplicate manifest backup path: {backup_selector}"));
        }
        if !matches!(store.status.as_str(), STATUS_OK | STATUS_EXCLUDED) {
            return Err(format!(
                "{}: invalid manifest status {}",
                store.name, store.status
            ));
        }
    }

    Ok(())
}

pub(crate) fn ensure_required_manifest_stores(
    manifest: &BackupManifest,
) -> std::result::Result<(), String> {
    for required_name in ["knowledge.fjall", "sessions.db"] {
        let mut matches = manifest
            .stores
            .iter()
            .filter(|store| store.name == required_name);
        let Some(store) = matches.next() else {
            return Err(format!(
                "required store missing from manifest: {required_name}"
            ));
        };
        if matches.next().is_some() {
            return Err(format!(
                "duplicate required store in manifest: {required_name}"
            ));
        }
        if store.status != STATUS_OK {
            return Err(format!("{required_name}: required store status must be ok"));
        }
    }

    Ok(())
}

pub(crate) fn ensure_manifest_entry_evidence(
    manifest: &BackupManifest,
    evidence: &ManifestEvidence,
) -> std::result::Result<(), String> {
    for (index, store) in manifest.stores.iter().enumerate() {
        ensure_single_entry_evidence(store, evidence.entry(ManifestSection::Stores, index))?;
    }
    for (index, store) in manifest.optional_stores.iter().enumerate() {
        ensure_single_entry_evidence(
            store,
            evidence.entry(ManifestSection::OptionalStores, index),
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_single_entry_evidence(
    store: &StoreEntry,
    evidence: Option<&EntryManifestMetadata>,
) -> std::result::Result<(), String> {
    if store.status == STATUS_EXCLUDED {
        return Ok(());
    }

    let Some(evidence) = evidence else {
        return Err(format!(
            "{}: missing manifest integrity evidence",
            store.name
        ));
    };
    if evidence.file_count.is_none() {
        return Err(format!("{}: missing manifest file_count", store.name));
    }
    let Some(restore_path) = &evidence.restore_path else {
        return Err(format!("{}: missing manifest restore_path", store.name));
    };
    ensure_relative_manifest_path(restore_path, "restore path")?;
    if store.sha256.is_none() {
        return Err(format!("{}: missing manifest sha256", store.name));
    }

    Ok(())
}

pub(crate) fn ensure_manifest_totals(
    manifest: &BackupManifest,
    evidence: &ManifestEvidence,
) -> std::result::Result<(), String> {
    let Some(total_files) = evidence.total_files else {
        return Err(String::from("manifest missing total_files"));
    };
    let mut expected_bytes = 0u64;
    let mut expected_files = 0u64;

    for (section, index, store) in manifest_entry_iter(manifest) {
        if store.status == STATUS_EXCLUDED {
            continue;
        }
        expected_bytes = expected_bytes.saturating_add(store.byte_count);
        let file_count = evidence
            .entry(section, index)
            .and_then(|entry| entry.file_count)
            .ok_or_else(|| format!("{}: missing manifest file_count", store.name))?;
        expected_files = expected_files.saturating_add(file_count);
    }

    if manifest.total_bytes != expected_bytes {
        return Err(format!(
            "manifest total_bytes mismatch (expected sum {expected_bytes}, got {})",
            manifest.total_bytes
        ));
    }
    if total_files != expected_files {
        return Err(format!(
            "manifest total_files mismatch (expected sum {expected_files}, got {total_files})"
        ));
    }

    Ok(())
}

pub(crate) fn manifest_entry_iter(
    manifest: &BackupManifest,
) -> impl Iterator<Item = (ManifestSection, usize, &StoreEntry)> {
    manifest
        .stores
        .iter()
        .enumerate()
        .map(|(index, store)| (ManifestSection::Stores, index, store))
        .chain(
            manifest
                .optional_stores
                .iter()
                .enumerate()
                .map(|(index, store)| (ManifestSection::OptionalStores, index, store)),
        )
}
