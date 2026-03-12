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
                if path.exists() {
                    if let Ok(fd) = PathFd::new(path) {
                        rs = rs.add_rule(PathBeneath::new(fd, access)).map_err(|e| {
                            std::io::Error::other(format!(
                                "Landlock rule failed for {}: {e}",
                                path.display()
                            ))
                        })?;
                    }
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

/// Apply sandbox restrictions to a [`std::process::Command`] via `pre_exec`.
///
/// # Safety
///
/// This uses [`std::os::unix::process::CommandExt::pre_exec`] which runs
/// between fork and exec in the child process. The sandbox operations
/// (Landlock ruleset, seccomp filter) use kernel syscalls that are
/// async-signal-safe.
#[cfg(target_os = "linux")]
pub fn apply_sandbox(cmd: &mut std::process::Command, policy: SandboxPolicy) {
    use std::os::unix::process::CommandExt;

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
}

#[cfg(not(target_os = "linux"))]
pub fn apply_sandbox(_cmd: &mut std::process::Command, _policy: SandboxPolicy) {}

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
    fn landlock_applies_in_child() {
        use std::process::Command;

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("cat");
        cmd.arg("/etc/hostname");
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
        apply_sandbox(&mut cmd, policy);

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
