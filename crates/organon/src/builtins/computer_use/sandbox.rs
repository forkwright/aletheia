//! Landlock sandbox session configuration and sandboxed action execution.

use std::path::PathBuf;

use koina::system::{Environment, RealSystem};

use super::actions::dispatch_action;
use super::capture::{capture_screen, compute_diff_region, describe_change, read_frame};
use super::process::CommandDiagnostics;
use super::types::{ActionResult, ComputerAction};
use crate::sandbox::{EgressPolicy, SandboxConfig, SandboxEnforcement, SandboxPolicy};
use crate::subprocess::SubprocessRunner;
use crate::types::{ToolContext, ToolDiagnostics};

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
                // WHY: Same minimal /proc grant as the execution sandbox.
                PathBuf::from("/proc/self"),
                PathBuf::from("/dev"),
            ],
            allowed_write_paths: vec![RealSystem.temp_dir()],
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

    /// Build the shared runner sandbox config used for resource limits.
    #[must_use]
    pub(crate) fn to_runner_sandbox_config(&self) -> SandboxConfig {
        SandboxConfig {
            enabled: true,
            enforcement: self.enforcement,
            egress: EgressPolicy::Deny,
            ..SandboxConfig::default()
        }
    }

    /// Build the per-request display-aware sandbox policy.
    #[must_use]
    pub(crate) fn to_display_sandbox_policy(&self) -> SandboxPolicy {
        let mut policy = self.to_sandbox_policy();
        add_display_paths(&mut policy);
        policy
    }
}

/// Result from a sandboxed computer-use action plus subprocess diagnostics.
pub(super) struct SandboxedActionResult {
    pub(super) action_result: ActionResult,
    pub(super) diagnostics: ToolDiagnostics,
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
    ctx: &ToolContext,
) -> std::io::Result<SandboxedActionResult> {
    let temp_dir = RealSystem.temp_dir();
    let before_path = temp_dir.join("aletheia_cu_before.png");
    let after_path = temp_dir.join("aletheia_cu_after.png");
    let runner = SubprocessRunner::new(session_config.to_runner_sandbox_config());
    let policy = session_config.to_display_sandbox_policy();
    let mut diagnostics = CommandDiagnostics::default();

    let before_capture = capture_screen(&before_path, &runner, ctx, &temp_dir, &policy)?;
    diagnostics.record(&before_capture);
    let before_bytes = read_frame(&before_path)?;

    let action_result = dispatch_action(action, &runner, ctx, &temp_dir, &policy)?;
    for output in &action_result.outputs {
        diagnostics.record(output);
    }

    // WHY: give the screen time to update after the action.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let after_capture = capture_screen(&after_path, &runner, ctx, &temp_dir, &policy)?;
    diagnostics.record(&after_capture);
    let after_bytes = read_frame(&after_path)?;

    let diff_region = compute_diff_region(&before_bytes, &after_bytes);
    let change_description = describe_change(action, diff_region.as_ref());

    let frame_base64 = Some(koina::base64::encode(&after_bytes));

    if let Err(err) = std::fs::remove_file(&before_path) {
        tracing::debug!(path = %before_path.display(), error = %err, "computer_use cleanup failed");
    }
    if let Err(err) = std::fs::remove_file(&after_path) {
        tracing::debug!(path = %after_path.display(), error = %err, "computer_use cleanup failed");
    }

    let action_result = ActionResult {
        success: true,
        action: action.to_string(),
        diff_region,
        change_description,
        frame_base64,
    };

    Ok(SandboxedActionResult {
        action_result,
        diagnostics: diagnostics.into_tool_diagnostics(),
    })
}

fn add_display_paths(policy: &mut SandboxPolicy) {
    add_env_path(&mut policy.read_paths, "XAUTHORITY");
    add_env_path(&mut policy.read_paths, "ICEAUTHORITY");
    add_default_home_auth_path(&mut policy.read_paths, ".Xauthority");
    add_default_home_auth_path(&mut policy.read_paths, ".ICEauthority");

    if let Some(runtime_dir) = RealSystem.var("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime_dir);
        push_unique(&mut policy.read_paths, path.clone());
        push_unique(&mut policy.write_paths, path);
    }
}

fn add_env_path(paths: &mut Vec<PathBuf>, var: &str) {
    if let Some(value) = RealSystem.var(var) {
        push_unique(paths, PathBuf::from(value));
    }
}

fn add_default_home_auth_path(paths: &mut Vec<PathBuf>, file_name: &str) {
    if let Some(home) = RealSystem.var("HOME") {
        push_unique(paths, PathBuf::from(home).join(file_name));
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() || paths.contains(&path) {
        return;
    }
    paths.push(path);
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

    #[test]
    fn display_sandbox_policy_includes_display_auth_paths() {
        let Ok(_guard) = crate::subprocess::SUBPROCESS_ENV_LOCK.lock() else {
            panic!("env lock poisoned");
        };
        #[expect(
            unsafe_code,
            reason = "set_var requires unsafe in Rust 2024; test controls env"
        )]
        unsafe {
            std::env::set_var("XAUTHORITY", "/tmp/aletheia-test-xauthority");
            std::env::set_var("ICEAUTHORITY", "/tmp/aletheia-test-iceauthority");
            std::env::set_var("XDG_RUNTIME_DIR", "/tmp/aletheia-test-runtime");
        }

        let config = ComputerUseSessionConfig::default();
        let policy = config.to_display_sandbox_policy();

        #[expect(
            unsafe_code,
            reason = "remove_var requires unsafe in Rust 2024; test cleanup"
        )]
        unsafe {
            std::env::remove_var("XAUTHORITY");
            std::env::remove_var("ICEAUTHORITY");
            std::env::remove_var("XDG_RUNTIME_DIR");
        }

        assert!(
            policy
                .read_paths
                .contains(&PathBuf::from("/tmp/aletheia-test-xauthority")),
            "XAUTHORITY should be granted read access"
        );
        assert!(
            policy
                .read_paths
                .contains(&PathBuf::from("/tmp/aletheia-test-iceauthority")),
            "ICEAUTHORITY should be granted read access"
        );
        assert!(
            policy
                .read_paths
                .contains(&PathBuf::from("/tmp/aletheia-test-runtime")),
            "XDG_RUNTIME_DIR should be granted read access for display sockets"
        );
        assert!(
            policy
                .write_paths
                .contains(&PathBuf::from("/tmp/aletheia-test-runtime")),
            "XDG_RUNTIME_DIR should be granted write access for display sockets"
        );
    }
}
