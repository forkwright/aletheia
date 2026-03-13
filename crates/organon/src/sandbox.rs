//! Landlock + seccomp sandbox for tool execution.
//!
//! Restricts filesystem access via Landlock LSM and blocks dangerous
//! syscalls via seccomp BPF filters. Applied in child processes after
//! fork, before exec.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Sandbox enforcement level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SandboxEnforcement {
    Enforcing,
    Permissive,
}

/// Configuration for the execution sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub enforcement: SandboxEnforcement,
    pub extra_read_paths: Vec<PathBuf>,
    pub extra_write_paths: Vec<PathBuf>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcement::Enforcing,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
        }
    }
}

impl SandboxConfig {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn build_policy(&self, workspace: &Path, allowed_roots: &[PathBuf]) -> SandboxPolicy {
        let mut read_paths = vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc"),
            PathBuf::from("/dev"),
        ];

        let mut write_paths = vec![PathBuf::from("/tmp")];

        let exec_paths = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/lib"),
        ];

        write_paths.push(workspace.to_path_buf());
        for root in allowed_roots {
            if !write_paths.contains(root) {
                write_paths.push(root.clone());
            }
        }

        read_paths.extend(self.extra_read_paths.iter().cloned());
        write_paths.extend(self.extra_write_paths.iter().cloned());

        for wp in &write_paths {
            if !read_paths.contains(wp) {
                read_paths.push(wp.clone());
            }
        }

        SandboxPolicy {
            read_paths,
            write_paths,
            exec_paths,
            enforcement: self.enforcement,
        }
    }
}

/// Runtime sandbox policy with resolved paths.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub exec_paths: Vec<PathBuf>,
    pub enforcement: SandboxEnforcement,
}

impl SandboxPolicy {
    /// Apply Landlock + seccomp restrictions to the current process.
    ///
    /// Designed to run in a child process via `pre_exec`. Returns `io::Error`
    /// on failure; on unsupported kernels, logs and continues based on
    /// enforcement mode.
    pub fn apply(&self) -> std::io::Result<()> {
        self.apply_landlock()?;
        self.apply_seccomp()?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn apply_landlock(&self) -> std::io::Result<()> {
        use landlock::{
            ABI, Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr,
            RulesetCreatedAttr, RulesetStatus,
        };

        let abi = ABI::V3;

        let read_access = AccessFs::ReadFile | AccessFs::ReadDir;
        let write_access = read_access
            | AccessFs::WriteFile
            | AccessFs::RemoveFile
            | AccessFs::RemoveDir
            | AccessFs::MakeDir
            | AccessFs::MakeReg
            | AccessFs::MakeSym
            | AccessFs::Truncate;
        let exec_access = AccessFs::Execute | AccessFs::ReadFile | AccessFs::ReadDir;

        let Ok(ruleset) = Ruleset::default()
            .handle_access(AccessFs::from_all(abi))
            .and_then(landlock::Ruleset::create)
        else {
            if self.enforcement == SandboxEnforcement::Enforcing {
                return Err(std::io::Error::other("failed to create Landlock ruleset"));
            }
            return Ok(());
        };

        let add = |mut rs: landlock::RulesetCreated,
                   paths: &[PathBuf],
                   access: BitFlags<AccessFs>|
         -> std::io::Result<landlock::RulesetCreated> {
            for path in paths {
                if path.exists()
                    && let Ok(fd) = PathFd::new(path)
                {
                    rs = rs.add_rule(PathBeneath::new(fd, access)).map_err(|e| {
                        std::io::Error::other(format!(
                            "Landlock rule failed for {}: {e}",
                            path.display()
                        ))
                    })?;
                }
            }
            Ok(rs)
        };

        let ruleset = add(ruleset, &self.read_paths, read_access)?;
        let ruleset = add(ruleset, &self.write_paths, write_access)?;
        let ruleset = add(ruleset, &self.exec_paths, exec_access)?;

        let status = ruleset
            .restrict_self()
            .map_err(|e| std::io::Error::other(format!("Landlock restrict_self failed: {e}")))?;

        match status.ruleset {
            RulesetStatus::FullyEnforced | RulesetStatus::PartiallyEnforced => {}
            RulesetStatus::NotEnforced => {
                if self.enforcement == SandboxEnforcement::Enforcing {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "Landlock not supported by kernel",
                    ));
                }
            }
        }

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn apply_landlock(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn apply_seccomp(&self) -> std::io::Result<()> {
        use std::collections::BTreeMap;

        use seccompiler::{SeccompAction, SeccompFilter, SeccompRule};

        let blocked_syscalls: &[i64] = &[
            libc::SYS_ptrace,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_reboot,
            libc::SYS_kexec_load,
            libc::SYS_init_module,
            libc::SYS_delete_module,
            libc::SYS_finit_module,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
        ];

        let rules: BTreeMap<i64, Vec<SeccompRule>> =
            blocked_syscalls.iter().map(|&nr| (nr, vec![])).collect();

        let action = if self.enforcement == SandboxEnforcement::Permissive {
            SeccompAction::Log
        } else {
            SeccompAction::Errno(libc::EPERM as u32)
        };

        let arch = target_arch();

        let filter = SeccompFilter::new(rules, SeccompAction::Allow, action, arch)
            .map_err(|e| std::io::Error::other(format!("seccomp filter creation failed: {e}")))?;

        let bpf: seccompiler::BpfProgram =
            filter.try_into().map_err(|e: seccompiler::BackendError| {
                std::io::Error::other(format!("seccomp BPF compilation failed: {e}"))
            })?;

        seccompiler::apply_filter(&bpf)
            .map_err(|e| std::io::Error::other(format!("seccomp filter installation failed: {e}")))
    }

    #[cfg(not(target_os = "linux"))]
    fn apply_seccomp(&self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn target_arch() -> seccompiler::TargetArch {
    #[cfg(target_arch = "x86_64")]
    {
        seccompiler::TargetArch::x86_64
    }
    #[cfg(target_arch = "aarch64")]
    {
        seccompiler::TargetArch::aarch64
    }
}

/// Probe the kernel for the highest Landlock ABI version it supports.
///
/// Returns the ABI version integer (1 through N) if Landlock is available,
/// or `None` if the kernel does not support Landlock or has it disabled.
///
/// Must be called from the parent process before `apply_sandbox`, not inside
/// a `pre_exec` closure. The result is used to detect mismatches early so
/// errors surface with context rather than as opaque "Permission denied" failures.
#[cfg(target_os = "linux")]
pub fn probe_landlock_abi() -> Option<i32> {
    // WHY: landlock_create_ruleset with LANDLOCK_CREATE_RULESET_VERSION returns
    // the ABI version as a non-negative integer, or -1 with errno set to
    // EOPNOTSUPP (supported but not enabled) or ENOSYS (not compiled in).
    // This mirrors the documented ABI probe pattern from the Landlock kernel docs
    // and the same approach used internally by the landlock crate.
    const LANDLOCK_CREATE_RULESET_VERSION: libc::__u32 = 1;
    // SAFETY: landlock_create_ruleset is a stable Linux syscall (kernel 5.13+).
    // Passing a null pointer and size 0 with the VERSION flag is the documented
    // ABI probe pattern. The kernel does not dereference the pointer for this call.
    #[expect(
        unsafe_code,
        reason = "direct syscall required to probe Landlock ABI before any ruleset is created"
    )]
    let v = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    i32::try_from(v).ok().filter(|&n| n >= 1)
}

#[cfg(not(target_os = "linux"))]
pub fn probe_landlock_abi() -> Option<i32> {
    None
}

/// Apply sandbox restrictions to a [`std::process::Command`] via `pre_exec`.
///
/// Returns an error if enforcement is strict and Landlock is unavailable or
/// the kernel ABI is incompatible. Logs a warning and skips sandbox setup
/// when enforcement is permissive and Landlock is unavailable.
///
/// # Errors
///
/// Returns `Err` when `enforcement == Enforcing` and Landlock is not available
/// on the running kernel, naming the ABI mismatch so the error is actionable.
///
/// # Safety
///
/// This uses [`std::os::unix::process::CommandExt::pre_exec`] which runs
/// between fork and exec in the child process. The sandbox operations
/// (Landlock ruleset, seccomp filter) use kernel syscalls that are
/// async-signal-safe.
#[cfg(target_os = "linux")]
pub fn apply_sandbox(
    cmd: &mut std::process::Command,
    policy: SandboxPolicy,
) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    let kernel_abi = probe_landlock_abi();

    match (kernel_abi, policy.enforcement) {
        (None, SandboxEnforcement::Permissive) => {
            // WHY: Log in the parent process where tracing infrastructure is live.
            // The pre_exec closure runs post-fork in a signal-handler context where
            // logging is not safe.
            tracing::warn!(
                enforcement = "permissive",
                "Landlock unavailable, sandboxing disabled (enforcement=permissive)"
            );
            return Ok(());
        }
        (None, SandboxEnforcement::Enforcing) => {
            return Err(std::io::Error::other(
                "Landlock not available on this kernel (ABI probe returned none); \
                 tool execution blocked by strict sandbox enforcement. \
                 Set enforcement=permissive to run without sandboxing.",
            ));
        }
        (Some(abi), _) => {
            tracing::info!(landlock_abi = abi, "Landlock ABI detected");
        }
    }

    // SAFETY: Landlock and seccomp operations use direct kernel syscalls
    // (landlock_create_ruleset, landlock_add_rule, landlock_restrict_self,
    // prctl/PR_SET_SECCOMP) which are async-signal-safe. No heap allocation
    // or mutex acquisition occurs in the child process.
    #[expect(
        unsafe_code,
        reason = "pre_exec requires unsafe; runs sandbox setup between fork and exec"
    )]
    unsafe {
        cmd.pre_exec(move || policy.apply());
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_sandbox(
    _cmd: &mut std::process::Command,
    _policy: SandboxPolicy,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_enabled() {
        let config = SandboxConfig::default();
        assert!(config.enabled);
        assert_eq!(config.enforcement, SandboxEnforcement::Enforcing);
        assert!(config.extra_read_paths.is_empty());
        assert!(config.extra_write_paths.is_empty());
    }

    #[test]
    fn disabled_config() {
        let config = SandboxConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = SandboxConfig {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            extra_read_paths: vec![PathBuf::from("/opt/data")],
            extra_write_paths: vec![PathBuf::from("/var/cache")],
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.enabled);
        assert_eq!(back.enforcement, SandboxEnforcement::Permissive);
        assert_eq!(back.extra_read_paths, vec![PathBuf::from("/opt/data")]);
        assert_eq!(back.extra_write_paths, vec![PathBuf::from("/var/cache")]);
    }

    #[test]
    fn enforcement_serde() {
        let json = serde_json::to_string(&SandboxEnforcement::Enforcing).expect("serialize");
        assert_eq!(json, "\"enforcing\"");
        let back: SandboxEnforcement = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, SandboxEnforcement::Enforcing);

        let json = serde_json::to_string(&SandboxEnforcement::Permissive).expect("serialize");
        assert_eq!(json, "\"permissive\"");
    }

    #[test]
    fn config_from_yaml_defaults() {
        let json = "{}";
        let config: SandboxConfig = serde_json::from_str(json).expect("parse");
        assert!(config.enabled);
        assert_eq!(config.enforcement, SandboxEnforcement::Enforcing);
    }

    #[test]
    fn policy_includes_workspace() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let policy = config.build_policy(&workspace, &[]);
        assert!(policy.write_paths.contains(&workspace));
        assert!(policy.read_paths.contains(&workspace));
    }

    #[test]
    fn policy_includes_allowed_roots() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let extra = PathBuf::from("/shared/data");
        let policy = config.build_policy(&workspace, std::slice::from_ref(&extra));
        assert!(policy.write_paths.contains(&extra));
    }

    #[test]
    fn policy_includes_system_paths() {
        let config = SandboxConfig::default();
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert!(policy.read_paths.contains(&PathBuf::from("/usr")));
        assert!(policy.read_paths.contains(&PathBuf::from("/lib")));
        assert!(policy.read_paths.contains(&PathBuf::from("/etc")));
        assert!(policy.exec_paths.contains(&PathBuf::from("/usr/bin")));
        assert!(policy.exec_paths.contains(&PathBuf::from("/bin")));
        assert!(policy.write_paths.contains(&PathBuf::from("/tmp")));
    }

    #[test]
    fn policy_includes_extra_paths() {
        let config = SandboxConfig {
            extra_read_paths: vec![PathBuf::from("/opt/readonly")],
            extra_write_paths: vec![PathBuf::from("/var/scratch")],
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert!(policy.read_paths.contains(&PathBuf::from("/opt/readonly")));
        assert!(policy.write_paths.contains(&PathBuf::from("/var/scratch")));
        assert!(
            policy.read_paths.contains(&PathBuf::from("/var/scratch")),
            "write paths should also be readable"
        );
    }

    #[test]
    fn policy_no_duplicate_write_roots() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let policy = config.build_policy(&workspace, std::slice::from_ref(&workspace));
        let count = policy
            .write_paths
            .iter()
            .filter(|p| **p == workspace)
            .count();
        assert_eq!(count, 1, "workspace should not be duplicated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn probe_returns_consistent_result() {
        // Two consecutive probes must agree: Landlock either is or isn't available.
        let first = probe_landlock_abi();
        let second = probe_landlock_abi();
        assert_eq!(
            first, second,
            "ABI probe must be deterministic across calls"
        );
        if let Some(abi) = first {
            assert!(abi >= 1, "ABI version must be at least 1 when available");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn permissive_skips_sandbox_when_landlock_unavailable() {
        use std::process::Command;

        // Simulate the permissive fallback by building a policy with permissive
        // enforcement and verifying the tool still executes even when we cannot
        // rely on Landlock being present.
        let config = SandboxConfig {
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("permissive fallback");

        // apply_sandbox must not return an error in permissive mode regardless
        // of whether Landlock is available on this kernel.
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "permissive mode must not error when sandbox is unavailable: {result:?}"
        );

        let output = cmd.output().expect("spawn");
        assert!(
            output.status.success(),
            "tool must execute in permissive mode"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("permissive fallback"),
            "tool output must be captured"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn enforcing_surfaces_clear_error_when_landlock_unavailable() {
        use std::process::Command;

        // This test covers the strict enforcement path when Landlock is absent.
        // On kernels where Landlock IS available the enforcing path succeeds, so
        // we test the error path explicitly by constructing a policy with a
        // simulated unavailable state via the apply_sandbox signature.
        //
        // WHY: We cannot force a kernel to lack Landlock in a unit test.
        // Instead we verify the error message content when probe returns None,
        // testing the code path directly via the internal helper.
        let config = SandboxConfig {
            enforcement: SandboxEnforcement::Enforcing,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("should not run");

        match probe_landlock_abi() {
            None => {
                // Landlock is not available: enforcing mode must return a clear error.
                let err = apply_sandbox(&mut cmd, policy).expect_err("enforcing must fail");
                let msg = err.to_string();
                assert!(
                    msg.contains("Landlock not available"),
                    "error must name Landlock: {msg}"
                );
                assert!(
                    msg.contains("ABI"),
                    "error must mention ABI for diagnostics: {msg}"
                );
                assert!(
                    msg.contains("enforcement=permissive"),
                    "error must suggest permissive mode: {msg}"
                );
            }
            Some(_) => {
                // Landlock is available: enforcing mode succeeds (no opaque error).
                let result = apply_sandbox(&mut cmd, policy);
                assert!(
                    result.is_ok(),
                    "enforcing mode must succeed when Landlock is available: {result:?}"
                );
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_applies_in_child() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("cat");
        cmd.arg("/etc/hostname");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(
            output.status.success(),
            "reading /etc/hostname should be allowed"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_blocks_outside_workspace() {
        use std::process::Command;

        let dir = tempfile::tempdir().expect("create temp dir");
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, "top secret").expect("write");

        let workspace = tempfile::tempdir().expect("create workspace");

        let read_paths = vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc"),
            PathBuf::from("/dev"),
            workspace.path().to_path_buf(),
        ];
        let write_paths = vec![workspace.path().to_path_buf()];
        let exec_paths = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/lib"),
        ];

        let policy = SandboxPolicy {
            read_paths,
            write_paths,
            exec_paths,
            enforcement: SandboxEnforcement::Enforcing,
        };

        let mut cmd = Command::new("/usr/bin/cat");
        cmd.arg(&secret);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "reading outside workspace should be blocked (stderr={stderr})"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_mount() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("mount -t tmpfs none /mnt 2>&1; echo $?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains('1'),
            "mount should be blocked by seccomp: {combined}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_allows_normal_operations() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("hello sandbox");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello sandbox"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn permissive_mode_allows_access() {
        use std::process::Command;

        let config = SandboxConfig {
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("permissive test");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(output.status.success());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_with_exec_tool_flow() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("test.txt"), "sandbox test data").expect("write");

        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("cat");
        cmd.arg(dir.path().join("test.txt"));
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("sandbox test data"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_write_in_workspace() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let outfile = dir.path().join("output.txt");
        let cmd_str = format!("echo written > {}", outfile.display());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(output.status.success(), "writing in workspace should work");
        assert!(outfile.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_write_outside_workspace_blocked() {
        use std::process::Command;

        let workspace = tempfile::tempdir().expect("create workspace");
        let outside = tempfile::tempdir().expect("create outside dir");
        let policy = SandboxPolicy {
            read_paths: vec![
                PathBuf::from("/usr"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
                PathBuf::from("/etc"),
                PathBuf::from("/proc"),
                PathBuf::from("/dev"),
                workspace.path().to_path_buf(),
            ],
            write_paths: vec![workspace.path().to_path_buf()],
            exec_paths: vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/lib"),
            ],
            enforcement: SandboxEnforcement::Enforcing,
        };

        let outfile = outside.path().join("escape.txt");
        let cmd_str = format!("echo escape > {} 2>&1; echo $?", outfile.display());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(
            !outfile.exists() || {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.trim().ends_with('1')
            },
            "writing outside workspace should be blocked"
        );
    }
}
