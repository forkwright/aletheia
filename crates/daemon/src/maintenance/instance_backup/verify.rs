use std::collections::HashMap;
use std::fs;
use std::io::Read as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};
use snafu::ResultExt as _;

use crate::error;

use super::super::fjall_backup::FjallBackup;
use super::{
    EntryManifestMetadata, InstanceBackup, InstanceVerifyResult, ManifestSection, STATUS_EXCLUDED,
    StoreEntry, StoreVerifyReport, dir_size, ensure_manifest_for_verify, join_manifest_backup_path,
    path_to_selector, read_backup_manifest,
};

impl InstanceBackup {
    /// Verify a whole-instance backup set.
    ///
    /// Reads `manifest.json`, confirms the required stores (`knowledge.fjall`
    /// and `sessions.db`) are present as current-format fjall stores, validates
    /// manifest paths/statuses, and compares every included entry against its
    /// recorded byte count, file count, and content digest.
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia backup verify command
    pub fn verify_backup(path: &Path) -> error::Result<InstanceVerifyResult> {
        let (manifest, evidence) = read_backup_manifest(path)?;

        let mut result = InstanceVerifyResult {
            manifest: Some(manifest.clone()),
            store_results: Vec::new(),
            total_keys: 0,
            first_error: None,
            store_generations: HashMap::new(),
        };

        if let Err(err) = ensure_manifest_for_verify(&manifest, &evidence) {
            record_verify_error(&mut result, "manifest", err);
            return Ok(result);
        }

        // Required stores.
        for name in ["knowledge.fjall", "sessions.db"] {
            let Some((index, store)) = manifest
                .stores
                .iter()
                .enumerate()
                .find(|(_, store)| store.name == name)
            else {
                let err = format!("required store missing from manifest: {name}");
                record_verify_error(&mut result, name, err);
                continue;
            };
            let entry_evidence = evidence.entry(ManifestSection::Stores, index);
            let store_path = match join_manifest_backup_path(path, &store.backup_path) {
                Ok(store_path) => store_path,
                Err(err) => {
                    record_verify_error(&mut result, name, err);
                    continue;
                }
            };
            if !store_path.exists() {
                let err = format!("required store directory missing: {}", store_path.display());
                record_verify_error(&mut result, name, err);
                continue;
            }

            match verify_required_store_path(name, &store_path) {
                Ok((total, generations)) => {
                    if let Err(err) =
                        verify_entry_integrity(name, &store_path, store, entry_evidence)
                    {
                        record_verify_error(&mut result, name, err);
                    } else {
                        result.total_keys += total;
                        result.store_results.push((String::from(name), Ok(total)));
                        insert_generations(&mut result.store_generations, generations);
                    }
                }
                Err(err) => {
                    record_verify_error(&mut result, name, err);
                }
            }
        }

        // Verify every remaining manifest entry. This covers config and
        // workspace/data directories, proving the restore set matches the
        // manifest instead of only checking the two required fjall stores.
        for (index, store) in manifest
            .stores
            .iter()
            .enumerate()
            .filter(|(_, store)| !is_required_store_name(&store.name))
        {
            // WHY(#4950): excluded entries were intentionally omitted from the
            // backup set by policy. They are recorded in the manifest but do not
            // represent a verification failure.
            if store.status == STATUS_EXCLUDED {
                continue;
            }

            verify_manifest_entry(
                path,
                store,
                evidence.entry(ManifestSection::Stores, index),
                &mut result,
            );
        }

        for (index, store) in manifest.optional_stores.iter().enumerate() {
            // WHY(#4950): excluded entries were intentionally omitted from the
            // backup set by policy. They are recorded in the manifest but do not
            // represent a verification failure.
            if store.status == STATUS_EXCLUDED {
                continue;
            }

            verify_manifest_entry(
                path,
                store,
                evidence.entry(ManifestSection::OptionalStores, index),
                &mut result,
            );
        }

        Ok(result)
    }

    /// Verify a fjall store or a directory tree containing fjall cohort stores.
    ///
    /// Returns `Ok(None)` when `path` exists but is neither a fjall store nor a
    /// tree containing fjall stores. Callers that require a current-format
    /// store should turn that into a domain-specific verification failure.
    ///
    /// WHY(#5040): migration and whole-instance backup must share the same
    /// read-only fjall traversal so they do not drift on cohort layouts.
    // kanon:ignore RUST/pub-visibility -- consumed by aletheia migrate verification
    pub fn verify_fjall_store_tree(
        logical_name: &str,
        path: &Path,
    ) -> std::result::Result<Option<StoreVerifyReport>, String> {
        verify_fjall_tree(logical_name, path).map(|outcome| {
            outcome.map(|(total_keys, store_generations)| StoreVerifyReport {
                total_keys,
                store_generations,
            })
        })
    }
}

pub(crate) fn verify_manifest_entry(
    backup_root: &Path,
    store: &StoreEntry,
    evidence: Option<&EntryManifestMetadata>,
    result: &mut InstanceVerifyResult,
) {
    let store_path = match join_manifest_backup_path(backup_root, &store.backup_path) {
        Ok(store_path) => store_path,
        Err(err) => {
            record_verify_error(result, &store.name, err);
            return;
        }
    };

    match verify_manifest_store(&store.name, &store_path) {
        Ok((total, generations)) => {
            if let Err(err) = verify_entry_integrity(&store.name, &store_path, store, evidence) {
                record_verify_error(result, &store.name, err);
            } else {
                result.store_results.push((store.name.clone(), Ok(total)));
                insert_generations(&mut result.store_generations, generations);
            }
        }
        Err(err) => record_verify_error(result, &store.name, err),
    }
}

pub(crate) fn verify_entry_integrity(
    name: &str,
    path: &Path,
    store: &StoreEntry,
    evidence: Option<&EntryManifestMetadata>,
) -> std::result::Result<(), String> {
    let expected_file_count = evidence
        .and_then(|entry| entry.file_count)
        .ok_or_else(|| format!("{name}: missing manifest file_count"))?;
    let expected_sha256 = store
        .sha256
        .as_deref()
        .ok_or_else(|| format!("{name}: missing manifest sha256"))?;
    verify_integrity_values(
        name,
        path,
        store.byte_count,
        expected_file_count,
        expected_sha256,
    )
}

pub(crate) fn verify_integrity_values(
    name: &str,
    path: &Path,
    expected_byte_count: u64,
    expected_file_count: u64,
    expected_sha256: &str,
) -> std::result::Result<(), String> {
    let observed =
        path_integrity(path).map_err(|err| format!("{name}: failed to inspect payload: {err}"))?;
    if observed.byte_count != expected_byte_count {
        return Err(format!(
            "{name}: byte_count mismatch for {} (expected {}, got {})",
            path.display(),
            expected_byte_count,
            observed.byte_count
        ));
    }
    if observed.file_count != expected_file_count {
        return Err(format!(
            "{name}: file_count mismatch for {} (expected {}, got {})",
            path.display(),
            expected_file_count,
            observed.file_count
        ));
    }
    if observed.sha256 != expected_sha256 {
        return Err(format!(
            "{name}: sha256 mismatch for {} (expected {expected_sha256}, got {})",
            path.display(),
            observed.sha256
        ));
    }

    Ok(())
}

pub(crate) fn record_verify_error(result: &mut InstanceVerifyResult, name: &str, err: String) {
    result
        .store_results
        .push((String::from(name), Err(err.clone())));
    if result.first_error.is_none() {
        result.first_error = Some(err);
    }
}

/// Verification outcome for a single store: key count and fjall generations.
///
/// WHY(#4950): seqnos (generations) are captured during verify and recorded in
/// the backup manifest so restore can detect mismatched write points. A logical
/// store may contain several fjall cohorts, such as `knowledge.fjall/shared`.
pub(crate) type VerifyStoreOutcome = (usize, Vec<(String, u64)>);

pub(crate) fn is_required_store_name(name: &str) -> bool {
    matches!(name, "knowledge.fjall" | "sessions.db")
}

pub(crate) fn verify_required_store_path(
    name: &str,
    path: &Path,
) -> std::result::Result<VerifyStoreOutcome, String> {
    if let Some(outcome) = verify_fjall_tree(name, path)? {
        return Ok(outcome);
    }

    Err(format!(
        "{name}: required store is not a current fjall store or fjall cohort root: {}",
        path.display()
    ))
}

pub(crate) fn verify_manifest_store(
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

#[derive(Debug, Clone)]
pub(crate) struct PathIntegrity {
    pub(crate) byte_count: u64,
    pub(crate) file_count: u64,
    pub(crate) sha256: String,
}

pub(crate) fn hash_path(path: &Path) -> error::Result<String> {
    path_integrity(path).map(|integrity| integrity.sha256)
}

pub(crate) fn path_integrity(path: &Path) -> error::Result<PathIntegrity> {
    let mut hasher = Sha256::new();
    let mut byte_count = 0u64;
    let mut file_count = 0u64;
    hash_path_inner(path, path, &mut hasher, &mut byte_count, &mut file_count)?;
    let digest = hasher.finalize();
    Ok(PathIntegrity {
        byte_count,
        file_count,
        sha256: hex_digest(&digest),
    })
}

#[expect(
    clippy::disallowed_methods,
    reason = "WHY(#4951): restore hashing is synchronous maintenance/CLI work and must read regular files by path"
)]
pub(crate) fn hash_path_inner(
    root: &Path,
    path: &Path,
    hasher: &mut Sha256,
    byte_count: &mut u64,
    file_count: &mut u64,
) -> error::Result<()> {
    let metadata = fs::symlink_metadata(path).context(error::MaintenanceIoSnafu {
        context: format!("reading metadata for hashing {}", path.display()),
    })?;
    if metadata.file_type().is_symlink() {
        return error::MaintenanceInvariantSnafu {
            context: format!(
                "backup payload contains symbolic link, violating manifest symlink policy: {}",
                path.display()
            ),
        }
        .fail();
    }

    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_selector = path_to_selector(rel);

    if metadata.is_dir() {
        hasher.update(b"dir\0");
        hasher.update(rel_selector.as_bytes());
        hasher.update(b"\0");

        let mut entries = fs::read_dir(path)
            .context(error::MaintenanceIoSnafu {
                context: format!("reading directory for hashing {}", path.display()),
            })?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .context(error::MaintenanceIoSnafu {
                        context: format!("reading directory entry for hashing {}", path.display()),
                    })
            })
            .collect::<error::Result<Vec<_>>>()?;
        entries.sort();
        for entry in entries {
            hash_path_inner(root, &entry, hasher, byte_count, file_count)?;
        }
        return Ok(());
    }

    if !metadata.is_file() {
        return error::MaintenanceInvariantSnafu {
            context: format!(
                "backup payload contains unsupported file type: {}",
                path.display()
            ),
        }
        .fail();
    }

    hasher.update(b"file\0");
    hasher.update(rel_selector.as_bytes());
    hasher.update(b"\0");
    *byte_count = byte_count.saturating_add(metadata.len());
    *file_count = file_count.saturating_add(1);

    let mut file = fs::File::open(path).context(error::MaintenanceIoSnafu {
        context: format!("opening file for hashing {}", path.display()),
    })?;
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer).context(error::MaintenanceIoSnafu {
            context: format!("reading file for hashing {}", path.display()),
        })?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or(&[]));
    }
    Ok(())
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    const HEX: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let high = usize::from(byte >> 4);
        let low = usize::from(byte & 0x0f);
        out.push(HEX.get(high).copied().unwrap_or('0'));
        out.push(HEX.get(low).copied().unwrap_or('0'));
    }
    out
}

pub(crate) fn verify_fjall_tree(
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

pub(crate) fn collect_fjall_tree(
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

pub(crate) fn insert_generations(
    target: &mut HashMap<String, u64>,
    generations: Vec<(String, u64)>,
) {
    for (name, seqno) in generations {
        target.insert(name, seqno);
    }
}

pub(crate) fn logical_child_name(logical_name: &str, root: &Path, path: &Path) -> String {
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
