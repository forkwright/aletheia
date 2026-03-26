//! RAII guard for subprocess lifecycle management.
//!
//! `ProcessGuard` wraps a [`std::process::Child`] and guarantees the child
//! process is killed and reaped when the guard is dropped: including on
//! panic.  This prevents both orphan processes (child outlives parent) and
//! zombie accumulation (child exited but not reaped).
//!
//! # Usage pattern
//!
//! Wrap the child immediately after `spawn()`, then interact via
//! `get_mut()`:
//!
//! ```ignore
//! let child = Command::new("sh").arg("-c").arg(cmd).spawn()?;
//! let mut guard = ProcessGuard::new(child);
//!
//! // Poll, read I/O, enforce a deadline…
//! match guard.get_mut().try_wait() { ... }
//!
//! // Timeout path: just return early: drop kills the child automatically.
//!
//! // Normal exit path: detach to take ownership back and read stdio.
//! let mut child = guard.detach();
//! child.wait()?;
//! ```

/// RAII guard that kills and reaps a child process on drop.
///
/// Drop calls `kill()` followed by `wait()` on the inner
/// [`Child`][std::process::Child].  Both calls ignore errors: `kill()` fails
/// if the process has already exited (safe), and `wait()` fails if the OS
/// has already reaped the zombie (safe).
pub struct ProcessGuard {
    child: Option<std::process::Child>,
}

impl ProcessGuard {
    /// Wrap a spawned child process in a kill-on-drop guard.
    pub fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    /// Borrow the inner [`Child`][std::process::Child] for polling or I/O.
    ///
    /// # Panics
    ///
    /// Panics if the guard has already been consumed via
    /// [`detach()`][Self::detach].
    #[expect(
        clippy::expect_used,
        reason = "panics intentionally when called after detach() — documented invariant"
    )]
    pub fn get_mut(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("ProcessGuard already consumed") // kanon:ignore RUST/expect
    }

    /// Take ownership of the child, disarming the kill-on-drop.
    ///
    /// The caller is responsible for reaping the process (calling `wait()` or
    /// `try_wait()`).  Use this after the process has exited normally and you
    /// need the exit status or remaining stdio output.
    ///
    /// # Panics
    ///
    /// Panics if called on an already-detached guard.
    #[expect(
        clippy::expect_used,
        reason = "panics intentionally when called twice — documented invariant"
    )]
    pub(crate) fn detach(mut self) -> std::process::Child {
        self.child.take().expect("ProcessGuard already consumed") // kanon:ignore RUST/expect
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // WHY: kill() may return ESRCH if the process already exited;
            // safe to ignore: we are cleaning up, not asserting liveness.
            let _ = child.kill();
            // INVARIANT: wait() after kill() prevents zombie accumulation.
            // If try_wait() already reaped the zombie, wait() returns ECHILD
            // which is safe to ignore: the goal (no zombie) is already met.
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::*;

    /// Spawn a long-running process, drop the guard, and verify the process
    /// is no longer alive.
    #[test]
    fn kills_child_on_drop() {
        let child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        let guard = ProcessGuard::new(child);
        drop(guard);

        std::thread::sleep(Duration::from_millis(50)); // kanon:ignore TESTING/sleep-in-test

        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .expect("kill -0")
            .status
            .success();
        assert!(!alive, "process {pid} should have been killed on drop");
    }

    /// After `detach()`, the guard no longer kills the child.
    #[test]
    fn detach_prevents_kill() {
        let child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        let guard = ProcessGuard::new(child);
        let mut child = guard.detach(); // disarms kill-on-drop

        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .expect("kill -0")
            .status
            .success();
        assert!(alive, "process {pid} should still be alive after detach");

        let _ = child.kill();
        let _ = child.wait();
    }

    /// `get_mut()` exposes the child for polling.
    #[test]
    fn get_mut_allows_try_wait() {
        let child = Command::new("true").spawn().expect("spawn true");
        let mut guard = ProcessGuard::new(child);

        let mut status = None;
        for _ in 0..50 {
            if let Ok(Some(s)) = guard.get_mut().try_wait() {
                status = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(10)); // kanon:ignore TESTING/sleep-in-test
        }
        let status = status.expect("process should have exited");
        assert!(status.success(), "expected status.success() to be true");
    }

    /// Dropping a guard whose child has already exited is safe (no panic,
    /// no double-kill errors propagated).
    #[test]
    fn drop_of_already_exited_child_is_safe() {
        let child = Command::new("true").spawn().expect("spawn true");
        let mut guard = ProcessGuard::new(child);

        loop {
            if let Ok(Some(_)) = guard.get_mut().try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10)); // kanon:ignore TESTING/sleep-in-test
        }

        // INVARIANT: Drop must not panic even though the process has already exited.
        drop(guard);
    }

    /// `detach()` followed by `wait()` is the normal success path: get the
    /// exit status without triggering the kill-on-drop.
    #[test]
    fn detach_then_wait_gives_exit_status() {
        let child = Command::new("sh")
            .args(["-c", "exit 7"])
            .spawn()
            .expect("spawn");
        let guard = ProcessGuard::new(child);
        let mut child = guard.detach();
        let status = child.wait().expect("wait");
        assert_eq!(
            status.code(),
            Some(7),
            "expected status.code() to equal Some(7)"
        );
    }

    /// A guard that is constructed but immediately dropped (zero-size path)
    /// does not crash.
    #[test]
    fn guard_with_short_lived_process_does_not_crash() {
        let child = Command::new("echo")
            .arg("hello")
            .spawn()
            .expect("spawn echo");
        let _guard = ProcessGuard::new(child);
    }

    /// Panic while a guard is live causes Drop to run and kill the child.
    ///
    /// This verifies the invariant without needing `UnwindSafe` on `Child`:
    /// we spawn the child, record its PID, then in a separate thread (which
    /// can unwind without propagating to the test thread) we create a guard
    /// and panic.
    #[test]
    fn guard_cleans_up_on_thread_panic() {
        use std::sync::mpsc;

        let child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        let (tx, rx) = mpsc::channel::<std::process::Child>();
        tx.send(child).unwrap();

        let handle = std::thread::spawn(move || {
            let child = rx.recv().unwrap();
            let _guard = ProcessGuard::new(child);
            panic!("intentional panic to test guard cleanup");
        });

        let _ = handle.join(); // join is Ok(Err(panic)), we don't care

        std::thread::sleep(Duration::from_millis(100)); // kanon:ignore TESTING/sleep-in-test

        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .expect("kill -0")
            .status
            .success();
        assert!(
            !alive,
            "process {pid} should have been killed by guard drop on panic"
        );
    }

    /// Two sequential guards for two different processes both clean up
    /// independently.
    #[test]
    fn multiple_guards_are_independent() {
        let c1 = Command::new("sleep").arg("60").spawn().expect("spawn 1");
        let c2 = Command::new("sleep").arg("60").spawn().expect("spawn 2");
        let pid1 = c1.id();
        let pid2 = c2.id();

        let g1 = ProcessGuard::new(c1);
        let g2 = ProcessGuard::new(c2);
        drop(g1);
        drop(g2);

        std::thread::sleep(Duration::from_millis(50)); // kanon:ignore TESTING/sleep-in-test

        for pid in [pid1, pid2] {
            let alive = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .expect("kill -0")
                .status
                .success();
            assert!(!alive, "process {pid} should have been killed");
        }
    }
}
