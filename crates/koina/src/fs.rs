//! Restricted filesystem helpers for writing sensitive files.

use std::path::Path;

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
