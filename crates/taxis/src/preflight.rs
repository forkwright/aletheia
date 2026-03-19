//! Resource precondition checks at startup.
//!
//! Verifies that required resources are available before any service starts:
//!
//! 1. **Disk space** — the data directory filesystem must have at least
//!    `MIN_REQUIRED_MB` megabytes free.
//! 2. **Port bindability** — the configured gateway TCP port must be available.
//! 3. **Directory permissions** — `config/` must be readable and `data/` must
//!    be writable.
//!
//! All checks are collected so that a single startup attempt reports every
//! problem, not just the first one.

use std::net::TcpListener;
use std::path::Path;

use snafu::Snafu;

use aletheia_koina::disk_space;

use crate::config::AletheiaConfig;
use crate::oikos::Oikos;

/// Minimum free disk space required on the data directory filesystem (megabytes).
pub const MIN_REQUIRED_MB: u64 = 50;

const BYTES_PER_MB: u64 = 1024 * 1024;

/// Collected startup precondition failures.
///
/// Returned by [`check_preconditions`] when one or more resource checks fail.
/// All failures are listed so the operator can fix them in a single startup cycle.
#[derive(Debug, Snafu)]
#[snafu(display("startup precondition checks failed:\n  - {}", failures.join("\n  - ")))]
pub struct PreconditionError {
    /// Human-readable failure messages identifying each failing resource.
    pub failures: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}

/// Run all resource precondition checks before service initialization.
///
/// Checks disk space on the data directory, gateway port availability, and
/// read/write permissions on key instance directories.
///
/// Call this after [`crate::oikos::Oikos::validate`] and config loading, but
/// before starting the HTTP server or any actors.
///
/// # Errors
///
/// Returns [`PreconditionError`] with all collected failures when any check
/// does not pass. The error message is human-readable and actionable.
pub fn check_preconditions(
    config: &AletheiaConfig,
    oikos: &Oikos,
) -> Result<(), PreconditionError> {
    let mut failures: Vec<String> = Vec::new();

    check_disk_space(oikos.data().as_path(), &mut failures);
    check_port(config.gateway.port, &mut failures);
    check_config_readable(oikos.config().as_path(), &mut failures);
    check_data_writable(oikos.data().as_path(), &mut failures);

    if failures.is_empty() {
        Ok(())
    } else {
        PreconditionSnafu { failures }.fail()
    }
}

/// Check that the data directory filesystem has at least `MIN_REQUIRED_MB` MB free.
fn check_disk_space(data_dir: &Path, failures: &mut Vec<String>) {
    let required_bytes = MIN_REQUIRED_MB * BYTES_PER_MB;

    match disk_space::available_space(data_dir) {
        Ok(available) if available < required_bytes => {
            let available_mb = available / BYTES_PER_MB;
            failures.push(format!(
                "insufficient disk space on {}: {available_mb} MB available, \
                 {MIN_REQUIRED_MB} MB required\n    \
                 help: free up disk space or reduce data retention limits",
                data_dir.display()
            ));
        }
        Ok(_) => {
            // NOTE: sufficient space — no failure recorded
        }
        Err(e) => {
            failures.push(format!(
                "cannot check disk space on {}: {e}\n    \
                 help: ensure the data directory exists and is accessible",
                data_dir.display()
            ));
        }
    }
}

/// Check that the configured TCP port can be bound on the loopback interface.
///
/// Uses `127.0.0.1` to avoid requiring elevated privileges when testing
/// `0.0.0.0`. A port that is already in use will still fail to bind on any
/// interface.
fn check_port(port: u16, failures: &mut Vec<String>) {
    // WHY: bind on 127.0.0.1 (not 0.0.0.0) so that the check itself
    // does not require CAP_NET_BIND_SERVICE for ports < 1024.
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => {
            // NOTE: listener dropped immediately — just checking bindability
        }
        Err(e) => {
            failures.push(format!(
                "gateway port {port} is not available: {e}\n    \
                 help: check if another process is already using port {port}, \
                 or change gateway.port in aletheia.toml"
            ));
        }
    }
}

/// Check that the config directory can be read.
fn check_config_readable(config_dir: &Path, failures: &mut Vec<String>) {
    match std::fs::read_dir(config_dir) {
        Ok(_) => {
            // NOTE: readable — no failure recorded
        }
        Err(e) => {
            failures.push(format!(
                "config directory is not readable: {}: {e}\n    \
                 help: check permissions on {} (must be readable by the aletheia user)",
                config_dir.display(),
                config_dir.display()
            ));
        }
    }
}

/// Check that the data directory is writable by attempting a test write.
fn check_data_writable(data_dir: &Path, failures: &mut Vec<String>) {
    let probe = data_dir.join(".aletheia-preflight-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::remove_file(&probe);
        }
        Err(e) => {
            failures.push(format!(
                "data directory is not writable: {}: {e}\n    \
                 help: check permissions on {} (must be writable by the aletheia user)",
                data_dir.display(),
                data_dir.display()
            ));
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions index into known-non-empty Vec"
)]
mod tests {
    use super::*;

    // ── disk space ────────────────────────────────────────────────────────

    #[test]
    fn disk_space_check_passes_on_real_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let mut failures = Vec::new();
        check_disk_space(dir.path(), &mut failures);
        assert!(
            failures.is_empty(),
            "a real filesystem should have >{MIN_REQUIRED_MB} MB free: {failures:?}"
        );
    }

    #[test]
    fn disk_space_check_fails_on_missing_path() {
        let mut failures = Vec::new();
        check_disk_space(
            Path::new("/tmp/aletheia-nonexistent-xyz-99999"),
            &mut failures,
        );
        assert!(
            !failures.is_empty(),
            "missing path should produce a disk space failure"
        );
        assert!(
            failures[0].contains("cannot check disk space"),
            "failure message should describe the error: {}",
            failures[0]
        );
    }

    // ── port bindability ─────────────────────────────────────────────────

    #[test]
    fn port_check_passes_on_unoccupied_port() {
        // Bind an ephemeral port to find one that's free, then release it
        // and check that port again.  There is a TOCTOU race, but in tests
        // it is acceptable.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mut failures = Vec::new();
        check_port(port, &mut failures);
        assert!(
            failures.is_empty(),
            "a just-released port should be bindable: {failures:?}"
        );
    }

    #[test]
    fn port_check_fails_on_occupied_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let mut failures = Vec::new();
        check_port(port, &mut failures);

        drop(listener);

        assert!(
            !failures.is_empty(),
            "an occupied port should produce a failure"
        );
        let msg = &failures[0];
        assert!(
            msg.contains(&port.to_string()),
            "failure should mention the port number: {msg}"
        );
        assert!(
            msg.contains("not available"),
            "failure should say port is not available: {msg}"
        );
    }

    // ── directory permissions ─────────────────────────────────────────────

    #[test]
    fn config_readable_passes_on_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut failures = Vec::new();
        check_config_readable(dir.path(), &mut failures);
        assert!(
            failures.is_empty(),
            "readable dir should pass: {failures:?}"
        );
    }

    #[test]
    fn config_readable_fails_on_missing_dir() {
        let mut failures = Vec::new();
        check_config_readable(
            Path::new("/tmp/aletheia-nonexistent-config-xyz"),
            &mut failures,
        );
        assert!(!failures.is_empty(), "missing dir should fail");
        assert!(
            failures[0].contains("not readable"),
            "failure should say not readable: {}",
            failures[0]
        );
    }

    #[test]
    fn data_writable_passes_on_writable_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut failures = Vec::new();
        check_data_writable(dir.path(), &mut failures);
        assert!(
            failures.is_empty(),
            "writable dir should pass: {failures:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn data_writable_fails_on_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        // Skip when running as root: root bypasses permission checks.
        let probe = dir.path().join(".root-check");
        let is_root = std::fs::write(&probe, b"x").is_ok();
        let _ = std::fs::remove_file(&probe);
        if is_root {
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let mut failures = Vec::new();
        check_data_writable(dir.path(), &mut failures);

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            !failures.is_empty(),
            "read-only dir should produce a failure"
        );
        assert!(
            failures[0].contains("not writable"),
            "failure should say not writable: {}",
            failures[0]
        );
    }

    // ── missing workspace file (integration path through check_preconditions) ──

    #[test]
    fn check_preconditions_passes_on_valid_instance() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();

        let oikos = crate::oikos::Oikos::from_root(dir.path());
        let config = crate::config::AletheiaConfig::default();

        let result = check_preconditions(&config, &oikos);
        // NOTE: port 18789 (default) may or may not be free on the test host;
        // we only assert that the disk and permission checks pass by verifying
        // those failure messages are absent.
        if let Err(ref e) = result {
            for f in &e.failures {
                assert!(
                    !f.contains("disk space")
                        && !f.contains("not readable")
                        && !f.contains("not writable"),
                    "unexpected disk/permission failure on a valid instance: {f}"
                );
            }
        }
    }
}
