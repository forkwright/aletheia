use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use snafu::ResultExt as _;
use tracing::warn;

use crate::error;

use super::{BackupManifest, SYMLINK_POLICY};

/// Resolve a configured workspace string against the instance root.
///
/// WHY: relative workspace paths resolve to the instance root; absolute
/// paths are taken as-is so operators can point outside the oikos.
pub(crate) fn resolve_workspace_source(instance_root: &Path, workspace: &str) -> PathBuf {
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
pub(crate) fn classify_workspace_source(
    instance_root: &Path,
    workspace: &str,
    source: &Path,
) -> String {
    if !Path::new(workspace).is_absolute() {
        return String::from("in-root");
    }
    if source.starts_with(instance_root) {
        String::from("absolute-inside-root")
    } else {
        String::from("absolute-outside-root")
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "synchronous maintenance utility invoked from spawn_blocking outside the async runtime"
)]
pub(crate) fn write_text_file(path: &Path, contents: &str) -> error::Result<()> {
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
pub(crate) fn copy_path(src: &Path, dst: &Path) -> error::Result<(u64, u32)> {
    reject_symlinks_in_backup_source(src, src)?;
    copy_path_checked(src, dst, src)
}

pub(crate) fn copy_path_checked(
    src: &Path,
    dst: &Path,
    source_root: &Path,
) -> error::Result<(u64, u32)> {
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
pub(crate) fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    source_root: &Path,
) -> error::Result<(u64, u32)> {
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

pub(crate) fn reject_symlinks_in_backup_source(
    path: &Path,
    source_root: &Path,
) -> error::Result<()> {
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

pub(crate) fn refuse_backup_source_entry<T>(
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

pub(crate) fn traversal_relative_path(path: &Path, source_root: &Path) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        String::from(".")
    } else {
        relative.to_string_lossy().replace('\\', "/")
    }
}

/// Calculate total size of a directory tree.
pub(crate) fn dir_size(path: &Path) -> u64 {
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

pub(crate) fn default_symlink_policy() -> String {
    String::from(SYMLINK_POLICY)
}

/// Parse the `created_at` field from a backup set's manifest into a
/// [`SystemTime`]. Returns `None` if the manifest is missing, unreadable, or
/// the timestamp cannot be parsed. (#5138)
pub(crate) fn manifest_created_time(backup_path: &Path) -> Option<std::time::SystemTime> {
    let manifest_json = fs::read_to_string(backup_path.join("manifest.json")).ok()?;
    let manifest: BackupManifest = serde_json::from_str(&manifest_json).ok()?;
    let zoned: jiff::Zoned = manifest.created_at.parse().ok()?;
    Some(std::time::SystemTime::from(zoned.timestamp()))
}

/// Set owner-only (0o700) permissions on a directory. No-op on non-Unix. (#5140)
#[cfg(unix)]
pub(crate) fn set_dir_restrictive(path: &Path) {
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
pub(crate) fn set_dir_restrictive(_path: &Path) {}

/// Set owner-only (0o600) permissions on every regular file under `dir`,
/// recursing into subdirectories. No-op on non-Unix. (#5140)
#[cfg(unix)]
pub(crate) fn set_files_restrictive(dir: &Path) {
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
pub(crate) fn set_files_restrictive(_dir: &Path) {}

/// Copy a file or directory tree, skipping any source file for which
/// `exclude` returns `true`. Returns `(bytes_copied, files_copied,
/// files_excluded)`.
///
/// WHY(#5353): a deliberate second copy path alongside [`copy_path`] rather
/// than an `Option<exclude>` parameter threaded onto it -- `copy_path` has
/// six call sites that have no exclusion need, and this lets an entry's
/// copy step drop specific files (credential decryption-key sidecars)
/// while still hashing/counting the tree it actually produced. Copying
/// everything and deleting afterward would leave the manifest's byte
/// count, file count, and content hash describing a tree that no longer
/// exists on disk, since [`super::BackupBuild::copy_entry`] hashes
/// immediately after copying.
pub(crate) fn copy_path_excluding<F: Fn(&Path) -> bool>(
    src: &Path,
    dst: &Path,
    exclude: &F,
) -> error::Result<(u64, u32, u32)> {
    reject_symlinks_in_backup_source(src, src)?;
    copy_path_checked_excluding(src, dst, src, exclude)
}

fn copy_path_checked_excluding<F: Fn(&Path) -> bool>(
    src: &Path,
    dst: &Path,
    source_root: &Path,
    exclude: &F,
) -> error::Result<(u64, u32, u32)> {
    let metadata = fs::symlink_metadata(src).context(error::MaintenanceIoSnafu {
        context: format!("reading source metadata {}", src.display()),
    })?;
    if metadata.file_type().is_symlink() {
        return refuse_backup_source_entry("symbolic link", src, source_root);
    }

    if metadata.is_dir() {
        return copy_dir_recursive_excluding(src, dst, source_root, exclude);
    }

    if !metadata.is_file() {
        return refuse_backup_source_entry("unsupported file type", src, source_root);
    }

    if exclude(src) {
        return Ok((0, 0, 1));
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).context(error::MaintenanceIoSnafu {
            context: format!("creating backup dir {}", parent.display()),
        })?;
    }
    let bytes = fs::copy(src, dst).context(error::MaintenanceIoSnafu {
        context: format!("copying {} to {}", src.display(), dst.display()),
    })?;
    Ok((bytes, 1, 0))
}

/// Recursively copy a directory, skipping entries `exclude` matches.
/// Returns `(bytes_copied, files_copied, files_excluded)`. See
/// [`copy_path_excluding`] for why this exists alongside
/// [`copy_dir_recursive`].
fn copy_dir_recursive_excluding<F: Fn(&Path) -> bool>(
    src: &Path,
    dst: &Path,
    source_root: &Path,
    exclude: &F,
) -> error::Result<(u64, u32, u32)> {
    fs::create_dir_all(dst).context(error::MaintenanceIoSnafu {
        context: format!("creating backup dir {}", dst.display()),
    })?;

    let mut total_bytes = 0u64;
    let mut total_files = 0u32;
    let mut total_excluded = 0u32;

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
            let (bytes, files, excluded) =
                copy_dir_recursive_excluding(&src_path, &dst_path, source_root, exclude)?;
            total_bytes += bytes;
            total_files += files;
            total_excluded += excluded;
        } else if metadata.is_file() {
            if exclude(&src_path) {
                total_excluded += 1;
                continue;
            }
            let bytes = fs::copy(&src_path, &dst_path).context(error::MaintenanceIoSnafu {
                context: format!("copying {} to {}", src_path.display(), dst_path.display()),
            })?;
            total_bytes += bytes;
            total_files += 1;
        } else {
            return refuse_backup_source_entry("unsupported file type", &src_path, source_root);
        }
    }

    Ok((total_bytes, total_files, total_excluded))
}

/// Whether `path` is a symbolon credential decryption-key sidecar
/// (`<name>.key` under a `credentials` directory). (#5353)
///
/// Scoped to the `credentials` path component so `config/tls/*.key` (TLS
/// private keys, a different artifact with a different risk profile) is
/// unaffected.
pub(crate) fn is_credential_key_sidecar(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "key")
        && path.components().any(|c| c.as_os_str() == "credentials")
}
