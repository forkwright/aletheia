//! Restricted filesystem helpers for writing sensitive files.

use std::path::{Path, PathBuf};

/// Validate that `path` resolves within `root` after canonicalization.
///
/// Follows the security standard's path validation sequence:
/// normalize -> check `allowed_roots` -> canonicalize -> re-check `allowed_roots`.
///
/// For paths that do not yet exist on disk, the parent directory is
/// canonicalized and the final component is appended. This handles the
/// common pattern of validating a file path before creating it.
///
/// # Errors
///
/// Returns [`std::io::Error`] if:
/// - The path contains `..` components (pre-canonicalization check).
/// - The canonicalized path does not start with the canonicalized root.
/// - Canonicalization itself fails (e.g. root directory does not exist).
pub fn validate_within_root(path: &Path, root: &Path) -> std::io::Result<PathBuf> {
    // Pre-canonicalization: reject `..` components to catch obvious traversal
    // attempts before touching the filesystem.
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "path contains '..' component: {}",
                    path.display()
                ),
            ));
        }
    }

    let canonical_root = std::fs::canonicalize(root)?;

    // WHY: the target path may not exist yet (e.g. health check write test,
    // new credential file). Canonicalize the parent, then append the filename.
    let canonical_path = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        let parent = path.parent().unwrap_or(path);
        let canonical_parent = std::fs::canonicalize(parent)?;
        match path.file_name() {
            Some(name) => canonical_parent.join(name),
            None => canonical_parent,
        }
    };

    // Post-canonicalization containment check (catches symlink escapes).
    if !canonical_path.starts_with(&canonical_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "path escapes root: {} is not within {}",
                canonical_path.display(),
                canonical_root.display()
            ),
        ));
    }

    Ok(canonical_path)
}

/// Write `content` to `path` atomically with 0600 permissions.
///
/// 1. Creates parent directories if needed.
/// 2. Writes to a `.tmp` sibling with mode 0600.
/// 3. Renames atomically to the target path.
///
/// The two-step write prevents other processes from reading a partially-written
/// file and ensures the final file is never world-readable.
///
/// # Errors
///
/// Returns an I/O error if any step (dir creation, write, rename) fails.
pub fn write_restricted(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");

    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }

        file.write_all(content)?;
        file.flush()?;
    }

    std::fs::rename(&tmp, path)?;

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn validate_within_root_accepts_child_path() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("config").join("aletheia.toml");
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(&child, b"").unwrap();

        let result = validate_within_root(&child, dir.path());
        assert!(result.is_ok(), "child path should be accepted: {result:?}");
    }

    #[test]
    fn validate_within_root_accepts_nonexistent_file_in_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let nonexistent = dir.path().join("data").join("new-file.txt");

        let result = validate_within_root(&nonexistent, dir.path());
        assert!(
            result.is_ok(),
            "nonexistent file in existing parent should be accepted: {result:?}"
        );
    }

    #[test]
    fn validate_within_root_rejects_dotdot_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let escape = dir.path().join("data").join("..").join("..").join("etc").join("passwd");

        let result = validate_within_root(&escape, dir.path());
        assert!(result.is_err(), "path with '..' should be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn validate_within_root_rejects_path_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        let result = validate_within_root(&outside_file, root.path());
        assert!(result.is_err(), "path outside root should be rejected");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn validate_within_root_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        // Create a symlink inside root that points outside
        let link = root.path().join("escape-link");
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        let result = validate_within_root(&link, root.path());
        assert!(
            result.is_err(),
            "symlink escaping root should be rejected"
        );
    }

    #[test]
    fn validate_within_root_accepts_root_itself() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_within_root(dir.path(), dir.path());
        assert!(result.is_ok(), "root itself should be accepted: {result:?}");
    }
}
