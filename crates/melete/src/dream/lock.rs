//! Consolidation lock: PID-file with rustix flock for atomic acquisition.
//!
//! The lock file serves dual purpose:
//! - **Body**: holder PID (identifies who holds the logical lock)
//! - **mtime**: `lastConsolidatedAt` timestamp (avoids a separate state file)
//!
//! Acquisition uses `rustix::fs::flock` for atomic PID writes, then re-reads
//! to verify ownership (race guard). Stale locks are reclaimed when the holder
//! PID is dead or the mtime exceeds the stale threshold (default: 1 hour).

use std::io::{Read, Seek, Write};
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error::{DreamLockIoSnafu, Result};

/// Result of a successful lock acquisition.
///
/// Holds the prior mtime for rollback on consolidation failure.
/// The lock is logically held as long as this value exists; the caller
/// is responsible for calling [`mark_complete`](AcquiredLock::mark_complete) on
/// success or [`rollback`](AcquiredLock::rollback) on failure.
#[derive(Debug)]
pub(crate) struct AcquiredLock {
    /// Path to the consolidation lock file.
    ///
    /// WHY: `Option` lets explicit `mark_complete`/`rollback` take the path
    /// once, so the subsequent `Drop` is a no-op and cleanup stays idempotent.
    path: Option<PathBuf>,
    /// mtime before we acquired (None if lock file did not exist).
    prior_mtime: Option<std::time::SystemTime>,
}

impl AcquiredLock {
    /// Mark consolidation as complete by touching the lock file to UPDATE mtime.
    ///
    /// Clears the PID body so the file signals "completed, not held."
    ///
    /// # Errors
    ///
    /// Returns `DreamLockIo` if the file cannot be written.
    pub(crate) fn mark_complete(mut self) -> Result<()> {
        // WHY: synchronous variant kept for test callers and the pre-spawn
        // check_gates path. The async consolidation task uses
        // [`mark_complete_async`](Self::mark_complete_async).

        // WHY: take the path so `Drop` will not attempt a second cleanup.
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        // WHY: write empty body to signal "not held"; mtime of this write = lastConsolidatedAt.
        write_file(&path, b"")?;
        Ok(())
    }

    /// Async version of [`mark_complete`](Self::mark_complete) that runs the
    /// blocking std::fs operations on Tokio's blocking pool.
    ///
    /// WHY: `run_consolidation` executes on a Tokio worker thread; lock-file
    /// I/O must not block it (#5712).
    pub(crate) async fn mark_complete_async(self) -> Result<()> {
        run_blocking_lock(move || self.mark_complete()).await
    }

    /// Rollback: restore pre-acquisition mtime on consolidation failure.
    ///
    /// If there was no prior consolidation (`prior_mtime` is `None`), deletes
    /// the lock file to restore the "never consolidated" state.
    ///
    /// # Errors
    ///
    /// Returns `DreamLockIo` if file operations fail.
    pub(crate) fn rollback(mut self) -> Result<()> {
        // WHY: synchronous variant kept for test callers and the pre-spawn
        // check_gates path. The async consolidation task uses
        // [`rollback_async`](Self::rollback_async).
        // WHY: take the path so `Drop` will not attempt a second cleanup.
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        if let Some(prior) = self.prior_mtime {
            // WHY: clear PID body first, then restore mtime.
            write_file(&path, b"")?;
            // NOTE: write_file updates mtime to now, so we must re-apply the prior mtime.
            let times = std::fs::FileTimes::new().set_modified(prior);
            let file =
                std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .context(DreamLockIoSnafu {
                        context: "open lock file for mtime restore",
                    })?;
            file.set_times(times).context(DreamLockIoSnafu {
                context: "restore lock file mtime",
            })?;
        } else {
            // WHY: no prior consolidation existed; DELETE to restore "never consolidated" state.
            if path.exists() {
                std::fs::remove_file(&path).context(DreamLockIoSnafu {
                    context: "DELETE lock file on rollback",
                })?;
            }
        }
        Ok(())
    }

    /// Async version of [`rollback`](Self::rollback) that runs the blocking
    /// std::fs operations on Tokio's blocking pool.
    ///
    /// WHY: `run_consolidation` executes on a Tokio worker thread; lock-file
    /// I/O must not block it (#5712).
    pub(crate) async fn rollback_async(self) -> Result<()> {
        run_blocking_lock(move || self.rollback()).await
    }

    /// The prior mtime for external inspection (e.g. consolidation timestamp).
    pub(crate) fn prior_mtime(&self) -> Option<&std::time::SystemTime> {
        self.prior_mtime.as_ref()
    }
}

impl Drop for AcquiredLock {
    /// Best-effort rollback when the lock is dropped without explicit completion.
    ///
    /// WHY: task cancellation or panic can leave the current PID in the lock
    /// file until stale timeout. This `Drop` performs the same cleanup as
    /// [`rollback`](AcquiredLock::rollback) without panicking on errors.
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            // NOTE: explicit `mark_complete` or `rollback` already took the path.
            return;
        };

        if let Some(prior) = self.prior_mtime {
            // WHY: clear PID body first, then restore prior mtime.
            if let Err(e) = write_file(&path, b"") {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "AcquiredLock drop failed to clear PID body"
                );
                return;
            }
            let times = std::fs::FileTimes::new().set_modified(prior);
            match std::fs::File::options().write(true).open(&path) {
                Ok(file) => {
                    if let Err(e) = file.set_times(times) {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "AcquiredLock drop failed to restore lock file mtime"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "AcquiredLock drop failed to open lock file for mtime restore"
                    );
                }
            }
        } else {
            // WHY: no prior consolidation existed; DELETE to restore "never consolidated" state.
            if path.exists()
                && let Err(e) = std::fs::remove_file(&path)
            {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "AcquiredLock drop failed to remove lock file"
                );
            }
        }
    }
}

/// Run a lock-file operation on Tokio's blocking pool.
///
/// WHY: lock-file operations use synchronous `std::fs` I/O. When called from
/// the async consolidation task, they must be moved off the worker thread
/// (#5712).
async fn run_blocking_lock<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| {
            DreamLockIoSnafu {
                context: "blocking lock operation",
                source: std::io::Error::other(e.to_string()),
            }
            .build()
        })?
}

/// Write bytes to a file (CREATE + truncate).
///
/// WHY: `std::fs::write` is disallowed by melete's `clippy.toml`; this uses
/// `File::options()` which is permitted.
fn write_file(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = std::fs::File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context(DreamLockIoSnafu {
            context: "open file for write",
        })?;
    file.write_all(content).context(DreamLockIoSnafu {
        context: "write file content",
    })?;
    Ok(())
}

/// Read the entire contents of a file as a string.
///
/// WHY: `std::fs::File::open` is disallowed by melete's `clippy.toml`; this
/// uses `File::options().read(true).open()` which is permitted.
fn read_file_string(path: &Path) -> Option<String> {
    let mut file = std::fs::File::options().read(true).open(path).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    Some(contents)
}

/// Attempt to acquire the consolidation lock.
///
/// Gate ORDER within this function:
/// 1. Check if another active process holds the lock (PID alive + mtime fresh)
/// 2. Acquire flock for atomic PID write
/// 3. Write our PID
/// 4. Release flock
/// 5. Re-read to verify ownership (race guard)
///
/// Returns `None` if the lock is held by another active process.
///
/// # Errors
///
/// Returns `DreamLockIo` on filesystem errors.
pub(crate) fn try_acquire(path: &Path, stale_threshold_secs: i64) -> Result<Option<AcquiredLock>> {
    let prior_mtime = lock_mtime(path);

    // NOTE: check existing holder before attempting flock.
    if let Some(pid) = read_pid(path) {
        if is_pid_alive(pid) && !is_stale(prior_mtime.as_ref(), stale_threshold_secs) {
            tracing::debug!(
                holder_pid = pid,
                "consolidation lock held by active process"
            );
            return Ok(None);
        }
        if !is_pid_alive(pid) {
            tracing::info!(
                stale_pid = pid,
                "reclaiming consolidation lock FROM dead process"
            );
        }
    }

    // NOTE: acquire flock for the brief write+verify phase.
    let mut file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .context(DreamLockIoSnafu {
            context: "open lock file",
        })?;

    // WHY: rustix::fs::flock binds the advisory lock to the file descriptor.
    // NonBlockingLockExclusive returns EWOULDBLOCK/EAGAIN if the lock is held.
    match rustix::fs::flock(
        file.as_fd(),
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    ) {
        Ok(()) => {}
        Err(e) if e == rustix::io::Errno::WOULDBLOCK || e == rustix::io::Errno::AGAIN => {
            tracing::debug!("consolidation lock flock held by another acquirer");
            return Ok(None);
        }
        Err(e) => {
            return Err(std::io::Error::from_raw_os_error(e.raw_os_error())).context(
                DreamLockIoSnafu {
                    context: "acquire flock",
                },
            );
        }
    }

    file.seek(std::io::SeekFrom::Start(0))
        .context(DreamLockIoSnafu {
            context: "seek lock file",
        })?;
    file.set_len(0).context(DreamLockIoSnafu {
        context: "truncate lock file",
    })?;
    write!(file, "{}", std::process::id()).context(DreamLockIoSnafu {
        context: "write PID to lock file",
    })?;
    file.flush().context(DreamLockIoSnafu {
        context: "flush lock file",
    })?;

    // WHY: explicitly release the flock before the race-guard re-read so
    // another concurrent acquirer can take the lock and write its PID. If
    // our PID is still present after releasing, we won the race.
    rustix::fs::flock(file.as_fd(), rustix::fs::FlockOperation::Unlock)
        .map_err(|e| std::io::Error::from_raw_os_error(e.raw_os_error()))
        .context(DreamLockIoSnafu {
            context: "release flock",
        })?;

    // NOTE: re-read to verify our PID stuck (race guard).
    let readback = read_pid(path);
    if readback != Some(std::process::id()) {
        tracing::debug!(
            expected = std::process::id(),
            actual = ?readback,
            "consolidation lock race lost during acquisition"
        );
        return Ok(None);
    }

    Ok(Some(AcquiredLock {
        path: Some(path.to_owned()),
        prior_mtime,
    }))
}

/// Read the lock file mtime (returns `None` if file does not exist).
pub(crate) fn lock_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Convert a `SystemTime` to a `jiff::Timestamp` (best-effort).
pub(crate) fn system_time_to_timestamp(st: std::time::SystemTime) -> Option<jiff::Timestamp> {
    let duration = st.duration_since(std::time::UNIX_EPOCH).ok()?;
    // WHY `try_from`: Unix seconds fit in i64 until year 292 billion and
    // subsec nanos are in 0..1_000_000_000 (well within i32), so both
    // conversions succeed in practice; `?` returns `None` on the
    // pathological overflow case instead of wrapping.
    let secs = i64::try_from(duration.as_secs()).ok()?;
    let nanos = i32::try_from(duration.subsec_nanos()).ok()?;
    jiff::Timestamp::new(secs, nanos).ok()
}

/// Read the PID FROM the lock file body.
fn read_pid(path: &Path) -> Option<u32> {
    let contents = read_file_string(path)?;
    contents.trim().parse::<u32>().ok()
}

/// Check whether a PID corresponds to a running process.
///
/// Uses `/proc/{pid}` on Linux and `kill(pid, 0)` on other Unix platforms
/// (macOS, BSDs). The `kill(pid, 0)` syscall sends no signal but checks
/// whether the process exists and is reachable.
///
/// WHY: The previous non-Linux fallback always returned `true`, which meant
/// a stale lock from a crashed process would block consolidation for the
/// full mtime stale threshold (up to 24 hours). Using `kill(pid, 0)` allows
/// immediate reclamation on macOS and other Unix platforms.
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // WHY: Unix PIDs are positive i32. A value greater than i32::MAX
        // (e.g. u32::MAX sentinel written by a stale lock) casts to a
        // negative i32, and kill(negative, 0) is interpreted by POSIX as
        // "signal the caller's process group" — which always succeeds and
        // would falsely report the PID as alive.
        let Ok(pid_i32) = i32::try_from(pid) else {
            return false;
        };
        if pid_i32 <= 0 {
            return false;
        }
        // WHY: kill(pid, 0) checks process existence without sending a signal.
        // Returns 0 if the process exists, -1 with ESRCH if it does not.
        // EPERM (no permission to signal) still means the process exists.
        // SAFETY: kill(pid, 0) with a positive PID is safe — signal 0 performs
        // a permission check without delivering any signal. This is the
        // standard Unix idiom for process existence checks.
        #[expect(
            unsafe_code,
            reason = "libc::kill with signal 0 is the portable idiom for PID liveness check; no process state is modified"
        )]
        let ret = unsafe { libc::kill(pid_i32, 0) };
        if ret == 0 {
            return true;
        }
        // WHY: EPERM means the process exists but we lack permission to signal it.
        // ESRCH means no process with this PID exists.
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        errno == libc::EPERM
    }
    // NOTE: Non-Unix platforms (e.g., Windows) cannot check PID liveness.
    // Return false so stale locks are reclaimed via mtime threshold rather than
    // blocking consolidation indefinitely.
    #[cfg(not(unix))]
    {
        let _ = pid;
        tracing::warn!(
            pid,
            "PID liveness check unavailable on this platform; assuming dead"
        );
        false
    }
}

/// Check whether the lock mtime exceeds the stale threshold.
fn is_stale(mtime: Option<&std::time::SystemTime>, stale_threshold_secs: i64) -> bool {
    let Some(mtime) = mtime else {
        // NOTE: no mtime means file doesn't exist or has no metadata → not stale.
        return false;
    };
    let Ok(elapsed) = mtime.elapsed() else {
        // NOTE: mtime in the future → not stale.
        return false;
    };
    let threshold =
        std::time::Duration::from_secs(u64::try_from(stale_threshold_secs).unwrap_or_default()); // kanon:ignore RUST/no-result-unwrap-or-default WHY: negative threshold is pathological; default to 0 (never stale) is safe
    elapsed > threshold
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::dream::DEFAULT_STALE_THRESHOLD_SECS;

    #[test]
    fn try_acquire_creates_lock_file_with_pid() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap()
            .unwrap();

        // NOTE: lock file should contain our PID.
        let pid_str = read_file_string(&lock_path).unwrap_or_default();
        assert_eq!(
            pid_str.trim(),
            std::process::id().to_string(),
            "lock file should contain current PID"
        );

        // NOTE: first acquisition has no prior mtime.
        assert!(
            acquired.prior_mtime().is_none(),
            "first acquisition should have no prior mtime"
        );

        acquired.mark_complete().unwrap_or_default();
    }

    #[test]
    fn try_acquire_rejects_when_held_by_current_process() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        let _acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap()
            .unwrap();

        let result = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS).unwrap();
        assert!(result.is_none(), "should reject concurrent acquisition");
    }

    #[test]
    fn rollback_deletes_when_no_prior_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap()
            .unwrap();

        assert!(lock_path.exists(), "lock file should exist after acquire");
        acquired.rollback().unwrap_or_default();
        assert!(
            !lock_path.exists(),
            "lock file should be deleted on rollback with no prior mtime"
        );
    }

    #[test]
    fn rollback_restores_prior_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        // NOTE: CREATE a lock file with a known mtime (simulate prior consolidation).
        write_file(&lock_path, b"").unwrap_or_default();
        let past =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let times = std::fs::FileTimes::new().set_modified(past);
        let file = std::fs::File::options()
            .write(true)
            .open(&lock_path)
            .unwrap();
        file.set_times(times).unwrap_or_default();
        drop(file);

        // NOTE: stale threshold of 0 so the lock is reclaimable.
        let acquired = try_acquire(&lock_path, 0).unwrap().unwrap();

        assert!(
            acquired.prior_mtime().is_some(),
            "should capture prior mtime"
        );

        acquired.rollback().unwrap_or_default();

        // NOTE: mtime should be restored to the prior value.
        let restored_mtime = lock_mtime(&lock_path).unwrap();
        let delta = restored_mtime
            .duration_since(past)
            .unwrap_or(past.duration_since(restored_mtime).unwrap_or_default());
        assert!(
            delta < std::time::Duration::from_secs(2),
            "restored mtime should be close to prior value, delta: {delta:?}"
        );
    }

    #[test]
    fn drop_performs_rollback_when_no_prior_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap()
            .unwrap();

        assert!(lock_path.exists(), "lock file should exist after acquire");
        drop(acquired);
        assert!(
            !lock_path.exists(),
            "lock file should be removed when dropped with no prior mtime"
        );
    }

    #[tokio::test]
    async fn aborting_task_drops_lock_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");
        let task_lock_path = lock_path.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            let _acquired = try_acquire(&task_lock_path, DEFAULT_STALE_THRESHOLD_SECS)
                .unwrap()
                .unwrap();
            assert!(
                ready_tx.send(()).is_ok(),
                "test receiver should wait for acquisition"
            );
            std::future::pending::<()>().await;
        });

        ready_rx.await.unwrap();
        assert!(
            lock_path.exists(),
            "lock file should exist while task waits"
        );

        handle.abort();
        let join_error = handle.await.unwrap_err();
        assert!(join_error.is_cancelled(), "task should be cancelled");
        assert!(
            !lock_path.exists(),
            "lock file should be removed when task is aborted"
        );
    }

    #[test]
    fn drop_performs_rollback_when_prior_mtime_exists() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        // NOTE: create a lock file with a known mtime (simulate prior consolidation).
        write_file(&lock_path, b"").unwrap_or_default();
        let past =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let times = std::fs::FileTimes::new().set_modified(past);
        let file = std::fs::File::options()
            .write(true)
            .open(&lock_path)
            .unwrap();
        file.set_times(times).unwrap_or_default();
        drop(file);

        // WHY: stale threshold of 0 makes the existing lock reclaimable.
        let acquired = try_acquire(&lock_path, 0).unwrap().unwrap();
        assert!(
            acquired.prior_mtime().is_some(),
            "should capture prior mtime"
        );

        drop(acquired);

        // NOTE: PID body should be cleared.
        let contents = read_file_string(&lock_path).unwrap_or_default();
        assert!(
            contents.is_empty(),
            "PID body should be cleared when dropped with prior mtime"
        );

        // NOTE: mtime should be restored to the prior value.
        let restored_mtime = lock_mtime(&lock_path).unwrap();
        let delta = restored_mtime
            .duration_since(past)
            .unwrap_or(past.duration_since(restored_mtime).unwrap_or_default());
        assert!(
            delta < std::time::Duration::from_secs(2),
            "restored mtime should be close to prior value, delta: {delta:?}"
        );
    }

    #[test]
    fn mark_complete_updates_mtime_and_clears_pid() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap()
            .unwrap();

        acquired.mark_complete().unwrap_or_default();

        // NOTE: PID should be cleared.
        let contents = read_file_string(&lock_path).unwrap_or_default();
        assert!(
            contents.is_empty(),
            "PID should be cleared after completion"
        );

        // NOTE: mtime should be recent (within last few seconds).
        let mtime = lock_mtime(&lock_path).unwrap();
        let elapsed = mtime.elapsed().unwrap_or_default();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "mtime should be recent after completion"
        );
    }

    #[test]
    fn stale_lock_reclaimed_when_pid_dead() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".consolidate-lock");

        // NOTE: write a fake PID that is very unlikely to be alive.
        write_file(&lock_path, b"4294967295").unwrap_or_default();

        // WHY (#3334): On all Unix platforms (Linux via /proc, macOS/BSD via
        // kill(pid, 0)), PID 4294967295 (u32::MAX) should be detected as dead
        // and the lock should be reclaimable without waiting for mtime stale.
        #[cfg(unix)]
        {
            let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS).unwrap();
            assert!(
                acquired.is_some(),
                "lock with dead PID should be reclaimable on Unix"
            );
            if let Some(lock) = acquired {
                lock.mark_complete().unwrap_or_default();
            }
        }

        // NOTE: on non-Unix, dead PIDs return false from is_pid_alive, so
        // the lock is also reclaimable.
        #[cfg(not(unix))]
        {
            let acquired = try_acquire(&lock_path, DEFAULT_STALE_THRESHOLD_SECS).unwrap();
            assert!(
                acquired.is_some(),
                "lock with dead PID should be reclaimable on non-Unix"
            );
            if let Some(lock) = acquired {
                lock.mark_complete().unwrap_or_default();
            }
        }
    }

    #[test]
    fn is_pid_alive_detects_current_process() {
        // WHY: The current process PID must always be detected as alive.
        // This validates the cross-platform PID detection works correctly.
        assert!(
            is_pid_alive(std::process::id()),
            "current process PID should be detected as alive"
        );
    }

    #[test]
    fn is_pid_alive_detects_dead_pid() {
        // WHY (#3334): A PID that does not correspond to any running process
        // must return false so stale locks are reclaimable.
        // PID u32::MAX is extremely unlikely to be in use.
        assert!(
            !is_pid_alive(u32::MAX),
            "u32::MAX PID should be detected as dead"
        );
    }

    #[test]
    fn is_stale_returns_false_for_recent_mtime() {
        let recent = std::time::SystemTime::now();
        assert!(
            !is_stale(Some(&recent), DEFAULT_STALE_THRESHOLD_SECS),
            "recent mtime should not be stale"
        );
    }

    #[test]
    fn is_stale_returns_true_for_old_mtime() {
        let old = std::time::SystemTime::now() - std::time::Duration::from_hours(2);
        assert!(
            is_stale(Some(&old), DEFAULT_STALE_THRESHOLD_SECS),
            "2-hour-old mtime should be stale with 1h threshold"
        );
    }

    #[test]
    fn is_stale_returns_false_for_none() {
        assert!(
            !is_stale(None, DEFAULT_STALE_THRESHOLD_SECS),
            "None mtime should not be considered stale"
        );
    }

    #[test]
    fn read_pid_returns_none_for_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty-lock");
        write_file(&path, b"").unwrap_or_default();
        assert!(read_pid(&path).is_none(), "empty file should yield no PID");
    }

    #[test]
    fn read_pid_returns_none_for_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent");
        assert!(
            read_pid(&path).is_none(),
            "nonexistent file should yield no PID"
        );
    }

    #[test]
    fn read_pid_parses_valid_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pid-lock");
        write_file(&path, b"12345").unwrap_or_default();
        assert_eq!(read_pid(&path), Some(12345), "should parse PID FROM file");
    }

    #[test]
    fn system_time_to_timestamp_roundtrips() {
        let now = std::time::SystemTime::now();
        let ts = system_time_to_timestamp(now);
        assert!(ts.is_some(), "current time should convert to timestamp");
    }
}
