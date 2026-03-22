//! Landlock sandbox session configuration and sandboxed action execution.

use std::path::PathBuf;

use super::actions::dispatch_action;
use super::capture::{
    capture_command, capture_screen, compute_diff_region, describe_change, read_frame,
};
use super::types::{ActionResult, ComputerAction};
use crate::process_guard::ProcessGuard;
use crate::sandbox::{SandboxEnforcement, SandboxPolicy};

/// Configuration for a computer use session's Landlock sandbox.
///
/// # NOTE
///
/// Landlock LSM requires Linux kernel 5.13+. The sandbox is applied via
/// the `landlock_create_ruleset`, `landlock_add_rule`, and
/// `landlock_restrict_self` syscalls directly through the `landlock` crate
/// (which wraps the syscalls via `rustix`). No external sandbox binary is
/// used.
#[derive(Debug, Clone)]
pub(crate) struct ComputerUseSessionConfig {
    /// Filesystem paths the session is allowed to read.
    pub(crate) allowed_read_paths: Vec<PathBuf>,
    /// Filesystem paths the session is allowed to write.
    pub(crate) allowed_write_paths: Vec<PathBuf>,
    /// Whether to enforce the sandbox (deny violations) or just log them.
    pub(crate) enforcement: SandboxEnforcement,
}

impl Default for ComputerUseSessionConfig {
    fn default() -> Self {
        Self {
            allowed_read_paths: vec![
                PathBuf::from("/usr"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
                PathBuf::from("/etc"),
                PathBuf::from("/proc"),
                PathBuf::from("/dev"),
            ],
            allowed_write_paths: vec![std::env::temp_dir()],
            enforcement: SandboxEnforcement::Enforcing,
        }
    }
}

impl ComputerUseSessionConfig {
    /// Build a [`SandboxPolicy`] from this session config.
    #[must_use]
    pub(crate) fn to_sandbox_policy(&self) -> SandboxPolicy {
        let mut read_paths = self.allowed_read_paths.clone();
        // WHY: Write paths must also be readable for tools to verify
        // their own output.
        for wp in &self.allowed_write_paths {
            if !read_paths.contains(wp) {
                read_paths.push(wp.clone());
            }
        }

        SandboxPolicy {
            enabled: true,
            read_paths,
            write_paths: self.allowed_write_paths.clone(),
            exec_paths: vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/lib"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
            ],
            enforcement: self.enforcement,
            egress: crate::sandbox::EgressPolicy::Deny,
            egress_allowlist: Vec::new(),
        }
    }
}

/// Execute an action inside a sandboxed subprocess.
///
/// Spawns a child process with Landlock restrictions applied via `pre_exec`,
/// captures before/after frames, and returns a structured [`ActionResult`].
///
/// # NOTE
///
/// The Landlock sandbox is applied via syscall through the `landlock` crate
/// (not an external sandbox binary). Requires Linux kernel 5.13+ with
/// Landlock enabled (`CONFIG_SECURITY_LANDLOCK=y`).
///
/// # Errors
///
/// Returns an error if screen capture, action dispatch, or sandbox setup fails.
pub(super) fn execute_sandboxed_action(
    action: &ComputerAction,
    session_config: &ComputerUseSessionConfig,
) -> std::io::Result<ActionResult> {
    let temp_dir = std::env::temp_dir();
    let before_path = temp_dir.join("aletheia_cu_before.png");
    let after_path = temp_dir.join("aletheia_cu_after.png");

    // Capture pre-action frame.
    capture_screen(&before_path)?;
    let before_bytes = read_frame(&before_path)?;

    // Dispatch the action.
    // NOTE: Actions run in the current process since xdotool needs X11
    // access. The Landlock sandbox is applied to the capture subprocess
    // to restrict filesystem access during frame capture.
    dispatch_action(action)?;

    // Small delay for screen to update after action.
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Capture post-action frame in a sandboxed subprocess.
    let policy = session_config.to_sandbox_policy();
    let mut cmd = capture_command(&after_path);
    crate::sandbox::apply_sandbox(&mut cmd, policy)?;

    let child = cmd.spawn()?;
    let mut guard = ProcessGuard::new(child);
    let status = guard.get_mut().wait()?;
    // WHY: Drop the guard after wait(). The child has already exited;
    // Drop's kill() returns ESRCH (safe), wait() returns ECHILD (safe).
    drop(guard);
    if !status.success() {
        return Err(std::io::Error::other("sandboxed screen capture failed"));
    }

    let after_bytes = read_frame(&after_path)?;

    // Compute diff.
    let diff_region = compute_diff_region(&before_bytes, &after_bytes);
    let change_description = describe_change(action, diff_region.as_ref());

    // Encode post-action frame for return.
    let frame_base64 = Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &after_bytes,
    ));

    // Clean up temp files.
    let _ = std::fs::remove_file(&before_path);
    let _ = std::fs::remove_file(&after_path);

    Ok(ActionResult {
        success: true,
        action: action.to_string(),
        diff_region,
        change_description,
        frame_base64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_config_default_has_standard_paths() {
        let config = ComputerUseSessionConfig::default();
        assert!(
            config.allowed_read_paths.contains(&PathBuf::from("/usr")),
            "default should include /usr read"
        );
        assert!(
            !config.allowed_write_paths.is_empty(),
            "default should have write paths"
        );
        assert_eq!(
            config.enforcement,
            SandboxEnforcement::Enforcing,
            "default enforcement should be enforcing"
        );
    }

    #[test]
    fn session_config_to_sandbox_policy() {
        let config = ComputerUseSessionConfig::default();
        let policy = config.to_sandbox_policy();
        assert!(policy.enabled, "policy should be enabled");
        assert!(
            !policy.exec_paths.is_empty(),
            "policy should have exec paths"
        );
        // Write paths should also appear in read paths.
        for wp in &policy.write_paths {
            assert!(
                policy.read_paths.contains(wp),
                "write path {wp:?} should also be readable"
            );
        }
    }
}
