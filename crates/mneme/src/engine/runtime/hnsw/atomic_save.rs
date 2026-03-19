//! Atomic saves for HNSW state persistence.
//!
//! Prevents corruption on crash by writing state to a temporary file, calling
//! `fsync`, then atomically renaming into place. At no point is the target
//! file in a partial-write state visible to readers.
//!
//! The pattern: write → fsync → rename.
#![allow(
    dead_code,
    reason = "infrastructure for future HNSW persistence integration"
)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tracing::debug;

use crate::engine::error::InternalResult as Result;
use crate::engine::runtime::error::InvalidOperationSnafu;

fn save_err(reason: String) -> crate::engine::error::InternalError {
    crate::engine::error::InternalError::Runtime {
        source: InvalidOperationSnafu {
            op: "atomic_save",
            reason,
        }
        .build(),
    }
}

/// Atomically write `data` to `target_path`.
///
/// 1. Write to a temporary file in the same directory as `target_path`.
/// 2. `fsync` the temporary file to ensure data is on disk.
/// 3. `fsync` on Unix platforms to ensure directory entry is durable.
/// 4. Atomically rename the temp file to `target_path`.
///
/// If any step fails, the temp file is cleaned up and `target_path` is
/// untouched.
///
/// # Errors
///
/// Returns an error if the write, fsync, or rename fails.
pub(crate) fn atomic_write(target_path: &Path, data: &[u8]) -> Result<()> {
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let temp_path = temp_path_for(target_path);

    // Step 1: write to temp file.
    let write_result = write_temp(&temp_path, data);
    if let Err(e) = write_result {
        // Clean up temp file on failure.
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    // Step 2: fsync the temp file.
    if let Err(e) = fsync_file(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    // Step 3: atomic rename.
    if let Err(e) = std::fs::rename(&temp_path, target_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(save_err(format!(
            "rename {} → {}: {e}",
            temp_path.display(),
            target_path.display()
        )));
    }

    // Step 4: fsync the parent directory to make the rename durable.
    fsync_dir(parent).ok();

    debug!(
        path = %target_path.display(),
        bytes = data.len(),
        "atomic save completed"
    );

    Ok(())
}

/// Generate a temp file path adjacent to `target` with a `.tmp` suffix.
fn temp_path_for(target: &Path) -> PathBuf {
    let mut temp = target.as_os_str().to_owned();
    temp.push(".tmp");
    PathBuf::from(temp)
}

fn write_temp(path: &Path, data: &[u8]) -> Result<()> {
    let mut file =
        File::create(path).map_err(|e| save_err(format!("create {}: {e}", path.display())))?;
    file.write_all(data)
        .map_err(|e| save_err(format!("write {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| save_err(format!("chmod {}: {e}", path.display())))?;
    }
    Ok(())
}

fn fsync_file(path: &Path) -> Result<()> {
    #[expect(
        clippy::disallowed_methods,
        reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
    )]
    let file = File::open(path)
        .map_err(|e| save_err(format!("open for fsync {}: {e}", path.display())))?;
    file.sync_all()
        .map_err(|e| save_err(format!("fsync {}: {e}", path.display())))?;
    Ok(())
}

fn fsync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        let dir = File::open(path)
            .map_err(|e| save_err(format!("open dir for fsync {}: {e}", path.display())))?;
        dir.sync_all()
            .map_err(|e| save_err(format!("fsync dir {}: {e}", path.display())))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Save serialized HNSW index state atomically.
///
/// Wraps [`atomic_write`] with a higher-level API that serializes the data
/// using MessagePack before writing.
///
/// # Errors
///
/// Returns an error if serialization or the atomic write fails.
pub(crate) fn atomic_save_state<T: serde::Serialize>(path: &Path, state: &T) -> Result<()> {
    let data = rmp_serde::to_vec(state).map_err(|e| save_err(format!("serialize: {e}")))?;
    atomic_write(path, &data)
}

/// Load HNSW index state, returning `None` if the file does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or deserialized.
pub(crate) fn load_state<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    #[expect(
        clippy::disallowed_methods,
        reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
    )]
    match std::fs::read(path) {
        Ok(data) => {
            let state: T =
                rmp_serde::from_slice(&data).map_err(|e| save_err(format!("deserialize: {e}")))?;
            Ok(Some(state))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(save_err(format!("read {}: {e}", path.display()))),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.bin");

        atomic_write(&target, b"hello world").unwrap();

        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        let contents = std::fs::read(&target).unwrap();
        assert_eq!(contents, b"hello world", "file contents match");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.bin");

        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();

        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        let contents = std::fs::read(&target).unwrap();
        assert_eq!(contents, b"second", "overwrite replaces content");
    }

    #[test]
    fn no_temp_file_left_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.bin");

        atomic_write(&target, b"data").unwrap();

        let temp = temp_path_for(&target);
        assert!(!temp.exists(), "temp file must be renamed away on success");
    }

    #[test]
    fn atomic_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.msgpack");

        let state: Vec<u64> = vec![1, 2, 3, 42, 100];
        atomic_save_state(&target, &state).unwrap();

        let loaded: Option<Vec<u64>> = load_state(&target).unwrap();
        assert_eq!(loaded, Some(state), "roundtrip preserves data");
    }

    #[test]
    fn load_state_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("missing.msgpack");

        let loaded: Option<Vec<u8>> = load_state(&target).unwrap();
        assert!(loaded.is_none(), "missing file → None");
    }

    #[test]
    fn crash_recovery_invariant() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.bin");

        // Write initial state.
        atomic_write(&target, b"initial").unwrap();

        // Simulate a "crash" by writing a temp file but not renaming.
        let temp = temp_path_for(&target);
        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        std::fs::write(&temp, b"partial").unwrap();

        // The target should still have the original content.
        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        let contents = std::fs::read(&target).unwrap();
        assert_eq!(
            contents, b"initial",
            "target unchanged despite lingering temp file"
        );

        // A subsequent atomic write should clean up and succeed.
        atomic_write(&target, b"recovered").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "mneme filesystem operations access the embedded DB or model files; synchronous I/O is required in these contexts"
        )]
        let contents = std::fs::read(&target).unwrap();
        assert_eq!(contents, b"recovered", "recovery write succeeds");

        // Temp file should be gone.
        assert!(!temp.exists(), "temp file cleaned up after recovery");
    }
}
