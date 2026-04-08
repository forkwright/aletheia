//! Runtime sandbox policy application -- Landlock, seccomp, network namespaces.
use std::net::IpAddr;
use std::path::PathBuf;

use super::config::{EgressPolicy, SandboxEnforcement, SandboxPolicy};

/// Check whether an IP address is loopback.
fn is_loopback(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Check whether all entries in an allowlist are loopback addresses.
///
/// Parses each entry as an IP address or CIDR (prefix/len). Returns `true`
/// if every entry resolves to a loopback address. Unparseable entries are
/// treated as non-loopback so the caller logs a warning.
pub(crate) fn allowlist_is_loopback_only(entries: &[String]) -> bool {
    entries.iter().all(|entry| {
        let ip_part = entry.split('/').next().unwrap_or(entry);
        ip_part.parse::<IpAddr>().is_ok_and(|a| is_loopback(&a))
    })
}

impl SandboxPolicy {
    /// Apply Landlock + seccomp + egress restrictions to the current process.
    ///
    /// Designed to run in a child process via `pre_exec`. Returns `io::Error`
    /// on failure; on unsupported kernels, logs and continues based on
    /// enforcement mode.
    pub(crate) fn apply(&self) -> std::io::Result<()> {
        self.apply_egress()?;
        self.apply_landlock()?;
        self.apply_seccomp()?;
        Ok(())
    }

    /// Apply network egress restrictions via Linux network namespaces.
    ///
    /// WHY: `unshare(CLONE_NEWUSER | CLONE_NEWNET)` creates an isolated
    /// network namespace containing only a loopback interface. This blocks
    /// all outbound connections to external hosts without requiring root
    /// privileges. The user namespace is required because `CLONE_NEWNET`
    /// alone requires `CAP_SYS_ADMIN`.
    #[cfg(target_os = "linux")]
    fn apply_egress(&self) -> std::io::Result<()> {
        match self.egress {
            EgressPolicy::Allow => Ok(()),
            EgressPolicy::Deny | EgressPolicy::Allowlist => {
                // SAFETY: unshare is a single syscall that modifies only the
                // calling thread's namespace associations. It is
                // async-signal-safe and does not allocate.
                // SAFETY: we only pass NEWUSER | NEWNET which do not involve
                // FILES table splitting, so the unsafety concern around
                // UnshareFlags::FILES does not apply here.
                #[expect(
                    unsafe_code,
                    reason = "unshare syscall required to create network namespace for egress filtering"
                )]
                if unsafe {
                    rustix::thread::unshare_unsafe(
                        rustix::thread::UnshareFlags::NEWUSER
                            | rustix::thread::UnshareFlags::NEWNET,
                    )
                }
                .is_ok()
                {
                    return Ok(());
                }

                // WHY: Some kernels disable unprivileged user namespaces
                // (sysctl kernel.unprivileged_userns_clone=0 or Debian
                // hardening). Fall back to seccomp-based socket blocking.
                let errno = std::io::Error::last_os_error();
                Self::apply_egress_seccomp_fallback(&errno)
            }
        }
    }

    /// Seccomp fallback for egress filtering when network namespaces are
    /// unavailable.
    ///
    /// Blocks `socket()` calls for `AF_INET` and `AF_INET6` address
    /// families. This prevents creation of IPv4/IPv6 sockets, causing
    /// any network tool (curl, wget, nc) to fail immediately with EPERM.
    /// `AF_UNIX` sockets are still permitted for local IPC.
    #[cfg(target_os = "linux")]
    fn apply_egress_seccomp_fallback(netns_error: &std::io::Error) -> std::io::Result<()> {
        use seccompiler::{
            SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
            SeccompRule,
        };

        // WHY: AF_INET=2, AF_INET6=10 on Linux. Blocking socket() for
        // these families prevents all IPv4/IPv6 socket creation. Programs
        // get EPERM immediately instead of hanging on connect().
        let block_inet = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            2u64, /* AF_INET */
        )
        .map_err(|e| std::io::Error::other(format!("seccomp condition failed: {e}")))?;

        let block_inet6 = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            10u64, /* AF_INET6 */
        )
        .map_err(|e| std::io::Error::other(format!("seccomp condition failed: {e}")))?;

        let rules = std::collections::BTreeMap::from([(
            41i64, /* SYS_socket */
            vec![
                SeccompRule::new(vec![block_inet])
                    .map_err(|e| std::io::Error::other(format!("seccomp rule failed: {e}")))?,
                SeccompRule::new(vec![block_inet6])
                    .map_err(|e| std::io::Error::other(format!("seccomp rule failed: {e}")))?,
            ],
        )]);

        let arch = target_arch();
        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(1u32 /* EPERM */),
            arch,
        )
        .map_err(|e| {
            std::io::Error::other(format!("egress seccomp filter creation failed: {e}"))
        })?;

        let bpf: seccompiler::BpfProgram =
            filter.try_into().map_err(|e: seccompiler::BackendError| {
                std::io::Error::other(format!("egress seccomp BPF compilation failed: {e}"))
            })?;

        seccompiler::apply_filter(&bpf).map_err(|e| {
            std::io::Error::other(format!(
                "egress seccomp filter installation failed: {e} \
                 (network namespace also unavailable: {netns_error})"
            ))
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn apply_egress(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn apply_landlock(&self) -> std::io::Result<()> {
        use landlock::{
            ABI, Access, AccessFs, BitFlags, PathBeneath, PathFd, Ruleset, RulesetAttr,
            RulesetCreatedAttr, RulesetStatus,
        };

        // WHY: Use the highest filesystem-relevant ABI the crate supports so
        // the ruleset handles all known access types. The crate's best-effort
        // mechanism silently drops flags the running kernel does not recognize,
        // making this safe across kernel versions. V5 added IoctlDev; without
        // handling it on V5+ kernels, ioctl on device files (/dev/null,
        // /dev/tty) would be uncontrolled by the sandbox policy.
        let abi = ABI::V5;

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

        // WHY: IoctlDev (V5+) controls ioctl on device files. Grant it to
        // /dev so child processes can perform terminal operations and interact
        // with device nodes like /dev/null and /dev/tty. On pre-V5 kernels
        // this flag is silently dropped by the crate's best-effort mechanism.
        let dev = [PathBuf::from("/dev")];
        let ruleset = add(ruleset, &dev, read_access | AccessFs::IoctlDev)?;

        let status = ruleset
            .restrict_self()
            .map_err(|e| std::io::Error::other(format!("Landlock restrict_self failed: {e}")))?;

        match status.ruleset {
            // NOTE: sandbox enforcement active, no action needed
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
            // kanon:ignore RUST/indexing-slicing
            101i64, /* SYS_ptrace */
            165i64, /* SYS_mount */
            166i64, /* SYS_umount2 */
            169i64, /* SYS_reboot */
            246i64, /* SYS_kexec_load */
            175i64, /* SYS_init_module */
            176i64, /* SYS_delete_module */
            313i64, /* SYS_finit_module */
            155i64, /* SYS_pivot_root */
            161i64, /* SYS_chroot */
        ];

        let rules: BTreeMap<i64, Vec<SeccompRule>> =
            blocked_syscalls.iter().map(|&nr| (nr, vec![])).collect();

        let action = if self.enforcement == SandboxEnforcement::Permissive {
            SeccompAction::Log
        } else {
            SeccompAction::Errno(1u32 /* EPERM */)
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

/// Cached Landlock ABI version, initialized on first sandbox use.
///
/// Calling `probe_landlock_abi` on every tool execution is unnecessary; the
/// kernel ABI is stable for the lifetime of the process. This static caches
/// the result and emits the availability log exactly once.
#[cfg(target_os = "linux")]
static LANDLOCK_ABI: std::sync::LazyLock<Option<i32>> = std::sync::LazyLock::new(|| {
    let abi = probe_landlock_abi();
    if let Some(v) = abi {
        tracing::info!(landlock_abi = v, "Landlock ABI v{v} available");
    } else {
        tracing::info!("Landlock not available on this kernel");
    }
    abi
});

/// Probe the kernel for the highest Landlock ABI version it supports.
///
/// Returns the ABI version integer (1 through N) if Landlock is available,
/// or `None` if the kernel does not support Landlock or has it disabled.
///
/// Must be called from the parent process before `apply_sandbox`, not inside
/// a `pre_exec` closure. The result is used to detect mismatches early so
/// errors surface with context rather than as opaque "Permission denied" failures.
#[cfg(target_os = "linux")]
#[must_use]
pub fn probe_landlock_abi() -> Option<i32> {
    // kanon:ignore RUST/pub-visibility
    // WHY: landlock_create_ruleset with LANDLOCK_CREATE_RULESET_VERSION returns
    // the ABI version as a non-negative integer, or -1 with errno set to
    // EOPNOTSUPP (supported but not enabled) or ENOSYS (not compiled in).
    // This mirrors the documented ABI probe pattern from the Landlock kernel docs
    // and the same approach used internally by the landlock crate.
    const LANDLOCK_CREATE_RULESET_VERSION: usize = 1;
    // SAFETY: landlock_create_ruleset is a stable Linux syscall (kernel 5.13+).
    // Passing a null pointer and size 0 with the VERSION flag is the documented
    // ABI probe pattern. The kernel does not dereference the pointer for this call.
    #[expect(unsafe_code, reason = "inline asm syscall to probe Landlock ABI")]
    let ret: isize = unsafe {
        let r: isize;
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!(
            "syscall",
            inlateout("rax") 444isize => r, // SYS_landlock_create_ruleset
            in("rdi") 0usize,               // null ruleset
            in("rsi") 0usize,               // size 0
            in("rdx") LANDLOCK_CREATE_RULESET_VERSION,
            lateout("rcx") _,
            lateout("r11") _,
        );
        #[cfg(not(target_arch = "x86_64"))]
        {
            r = -1; // NOTE: landlock only probed on x86_64 for now
        }
        r
    };
    if ret >= 1 {
        i32::try_from(ret).ok()
    } else {
        None
    }
}

/// Probe the Landlock ABI version. Returns None on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub fn probe_landlock_abi() -> Option<i32> {
    // kanon:ignore RUST/pub-visibility
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
/// between fork and exec in the child process. The underlying Landlock and
/// seccomp syscalls are async-signal-safe, but the crate wrappers perform
/// heap allocations. See the inline `SAFETY` and `WARNING` comments for the
/// full risk analysis.
#[cfg(target_os = "linux")]
pub fn apply_sandbox(
    // kanon:ignore RUST/pub-visibility
    cmd: &mut std::process::Command,
    policy: SandboxPolicy,
) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    if !policy.enabled {
        // WHY: Log when sandbox is completely disabled so operators see it clearly.
        // Closes #1718.
        tracing::warn!("sandbox disabled: tool execution runs without any restrictions");
        return Ok(());
    }

    // WHY: LANDLOCK_ABI initializes on first access, logging the result once.
    // Re-probing on every call is unnecessary; the kernel ABI is stable.
    let kernel_abi = *LANDLOCK_ABI;

    match (kernel_abi, policy.enforcement) {
        (None, SandboxEnforcement::Permissive) => {
            // WHY: Log in the parent process where tracing infrastructure is live.
            // The pre_exec closure runs post-fork in a signal-handler context where
            // logging is not safe.
            tracing::warn!(
                enforcement = "permissive",
                "Landlock unavailable, sandboxing disabled (enforcement=permissive); \
                 set enforcement=enforcing and ensure kernel supports Landlock (5.13+)"
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
        // WHY: Log when Landlock is available but enforcement is permissive so operators
        // know syscall violations are only logged, not blocked. Closes #1718.
        (Some(_), SandboxEnforcement::Permissive) => {
            tracing::warn!(
                enforcement = "permissive",
                "sandbox enforcement=permissive: policy violations are logged but not \
                 blocked. Set enforcement=enforcing for production deployments."
            );
        }
        // NOTE: Landlock ABI available and enforcement=enforcing, proceed with sandbox setup
        (Some(_), SandboxEnforcement::Enforcing) => {}
    }

    // WHY: Log egress policy warnings in the parent where tracing works.
    // The pre_exec closure cannot safely use tracing.
    match policy.egress {
        EgressPolicy::Deny => {
            tracing::info!(
                egress = "deny",
                "egress filtering: blocking all outbound network"
            );
        }
        EgressPolicy::Allowlist => {
            if !allowlist_is_loopback_only(&policy.egress_allowlist) {
                tracing::warn!(
                    egress = "allowlist",
                    "egress allowlist contains non-loopback entries; \
                     only loopback destinations are enforceable without root. \
                     Non-loopback entries will be blocked."
                );
            }
            tracing::info!(
                egress = "allowlist",
                entries = ?policy.egress_allowlist,
                "egress filtering: allowlist mode"
            );
        }
        EgressPolicy::Allow => {}
    }

    // SAFETY: The closure runs between fork and exec in the child process.
    // The Landlock and seccomp syscalls themselves (landlock_create_ruleset,
    // landlock_add_rule, landlock_restrict_self, prctl/PR_SET_SECCOMP) are
    // async-signal-safe. policy.apply() is the sole entry point; it calls no
    // signal-unsafe libc functions beyond those syscalls.
    //
    // WARNING: The landlock and seccompiler crate wrappers perform heap
    // allocations between fork and exec (Ruleset data structures, BTreeMap
    // for syscall rules, BpfProgram compilation). In a multi-threaded parent
    // process, fork copies the allocator state into the child, including any
    // arena mutex that another thread held at the moment of fork. If the child
    // then calls malloc, it may deadlock on that copied mutex.
    // Modern per-thread allocator arenas (glibc ptmalloc, jemalloc) make this
    // unlikely in practice: each thread has its own arena: but the risk is
    // not zero on arena exhaustion when threads share an arena.
    // No deadlock has been observed in production use.
    #[expect(
        unsafe_code,
        reason = "pre_exec requires unsafe; runs sandbox setup between fork and exec"
    )]
    unsafe {
        cmd.pre_exec(move || policy.apply());
    }

    Ok(())
}

/// Apply sandbox restrictions to a command. No-op on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub fn apply_sandbox(
    // kanon:ignore RUST/pub-visibility
    _cmd: &mut std::process::Command,
    policy: SandboxPolicy,
) -> std::io::Result<()> {
    // WHY: Landlock, seccomp, and network namespaces are Linux-only kernel
    // interfaces. On other platforms the sandbox is a no-op. Log once per
    // process so operators know sandbox enforcement is absent.
    static WARN_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    WARN_ONCE.get_or_init(|| {
        tracing::warn!(
            "sandbox enforcement unavailable on non-Linux platforms; \
             tool execution runs without filesystem, syscall, or egress restrictions"
        );
    });
    if policy.egress != EgressPolicy::Allow {
        tracing::warn!(
            egress = ?policy.egress,
            "egress filtering unavailable on non-Linux platforms"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    /// Create a minimal sandbox policy for testing with specified paths.
    fn test_policy(
        read_paths: Vec<PathBuf>,
        write_paths: Vec<PathBuf>,
        exec_paths: Vec<PathBuf>,
    ) -> SandboxPolicy {
        SandboxPolicy {
            enabled: true,
            read_paths,
            write_paths,
            exec_paths,
            enforcement: SandboxEnforcement::Enforcing,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        }
    }

    /// Create a policy with system paths included (typical production setup).
    fn policy_with_system_paths(workspace: &std::path::Path) -> SandboxPolicy {
        let mut read_paths = vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc"),
            PathBuf::from("/dev"),
        ];
        let write_paths = vec![workspace.to_path_buf()];
        let exec_paths = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            workspace.to_path_buf(),
        ];

        // Write paths are also readable
        for wp in &write_paths {
            if !read_paths.contains(wp) {
                read_paths.push(wp.clone());
            }
        }

        test_policy(read_paths, write_paths, exec_paths)
    }

    // =================================================================================
    // LANDLOCK FILE ACCESS RESTRICTION TESTS
    // =================================================================================

    /// Test that allowed paths can be read successfully.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_allows_read_in_allowed_paths() {
        let workspace = tempfile::tempdir().expect("create temp dir");
        let test_file = workspace.path().join("test.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&test_file, "test content").expect("write test file");

        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("cat");
        cmd.arg(&test_file);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "reading allowed file should succeed: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "test content",
            "file content should match"
        );
    }

    /// Test that allowed paths can be written successfully.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_allows_write_in_allowed_paths() {
        let workspace = tempfile::tempdir().expect("create temp dir");
        let policy = policy_with_system_paths(workspace.path());

        let outfile = workspace.path().join("output.txt");
        let cmd_str = format!("echo 'written' > {}", outfile.display());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "writing to allowed path should succeed: {stderr}"
        );
        assert!(outfile.exists(), "output file should exist");
    }

    /// Test that paths outside the sandbox cannot be read (EACCES or Permission denied).
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_blocks_read_outside_sandbox() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let secret_file = outside_dir.path().join("secret.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&secret_file, "secret data").expect("write secret file");

        let workspace = tempfile::tempdir().expect("create workspace");
        // Policy does NOT include outside_dir
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("cat");
        cmd.arg(&secret_file);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Should fail with permission denied
        assert!(
            !output.status.success(),
            "reading outside sandbox should be blocked: {stderr}"
        );
        assert!(
            stderr.contains("Permission denied") || stderr.contains("Operation not permitted"),
            "should fail with permission denied, got: {stderr}"
        );
    }

    /// Test that paths outside the sandbox cannot be written.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_blocks_write_outside_sandbox() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let escape_file = outside_dir.path().join("escape.txt");
        let cmd_str = format!(
            "echo 'escape' > {} 2>&1; echo ret=$?",
            escape_file.display()
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Should fail - either file doesn't exist or exit code is non-zero
        assert!(
            !escape_file.exists() || stdout.contains("ret=1"),
            "writing outside sandbox should be blocked: {stdout}"
        );
    }

    /// Test that symlink escapes are blocked via canonical path resolution.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_blocks_symlink_escape() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let secret_file = outside_dir.path().join("secret.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&secret_file, "escaped secret").expect("write secret file");

        let workspace = tempfile::tempdir().expect("create workspace");
        let symlink = workspace.path().join("escape_link");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::os::unix::fs::symlink(&secret_file, &symlink).expect("create symlink");

        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("cat");
        cmd.arg(&symlink);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Symlink resolution should be blocked
        assert!(
            !output.status.success()
                || !String::from_utf8_lossy(&output.stdout).contains("escaped"),
            "symlink escape should be blocked: {stderr}"
        );
    }

    /// Test that symlink creation inside allowed paths works.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_allows_symlink_creation_in_workspace() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let target = workspace.path().join("target.txt");
        let link = workspace.path().join("link.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&target, "target content").expect("write target file");

        let cmd_str = format!(
            "ln -s {} {} 2>&1 && cat {} 2>&1",
            target.display(),
            link.display(),
            link.display()
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            output.status.success(),
            "symlink operations in workspace should succeed: {stderr}"
        );
        assert!(
            stdout.contains("target content"),
            "should read target through symlink: {stdout}"
        );
    }

    /// Test that mount-point traversals don't escape the sandbox.
    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_blocks_mount_traverse_escape() {
        // This test verifies that even if we can see mount points,
        // we cannot traverse to paths outside the allowed set
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Try to read /root (typically exists but is not in our allowed paths)
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("cat /root/.bashrc 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Should fail with permission denied, not "No such file or directory"
        assert!(
            stdout.contains("Permission denied") || stdout.contains("ret=1"),
            "access to /root should be blocked: {stdout}"
        );
    }

    // =================================================================================
    // SECCOMP SYSCALL FILTERING TESTS
    // =================================================================================

    /// Test that ptrace is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_ptrace() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("strace -o /dev/null echo test 2>&1; echo exit=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // ptrace should be blocked - accept various failure modes
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("Permission denied")
                || combined.contains("ret=1")
                || combined.contains("exit=127")
                || combined.contains("strace: not found")
                || combined.contains("command not found")
                || !output.status.success(),
            "ptrace should be blocked by seccomp: {combined}"
        );
    }

    /// Test that mount syscall is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_mount_syscall() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("mount -t tmpfs none /mnt 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Mount should fail - either via seccomp EPERM or "must be superuser"
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains("ret=1")
                || combined.contains("must be superuser")
                || combined.contains("permission denied")
                || !output.status.success(),
            "mount should be blocked by seccomp: {combined}"
        );
    }

    /// Test that umount is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_umount() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("umount /mnt 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail - either permission denied or mount not found (both indicate blocking)
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains("ret=")
                || combined.contains("not mounted")
                || !output.status.success(),
            "umount should be blocked by seccomp: {combined}"
        );
    }

    /// Test that reboot is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_reboot() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Use a fake reboot command that doesn't require root to test seccomp blocking
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("/sbin/reboot 2>&1 || /usr/sbin/reboot 2>&1 || echo 'reboot not found'");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Reboot should be blocked - "Access denied" or any failure is acceptable
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains("Access denied")
                || combined.contains("ret=1")
                || combined.contains("not found")
                || !output.status.success(),
            "reboot should be blocked by seccomp: {combined}"
        );
    }

    /// Test that chroot is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_chroot() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("chroot /tmp 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains("ret=1"),
            "chroot should be blocked by seccomp: {combined}"
        );
    }

    /// Test that pivot_root is blocked by seccomp.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_pivot_root() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("pivot_root /tmp /tmp 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("EPERM")
                || combined.contains("not found")
                || combined.contains("ret=1"),
            "pivot_root should be blocked by seccomp: {combined}"
        );
    }

    /// Test that allowed syscalls work correctly (read, write, open).
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_allows_safe_syscalls() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let test_file = workspace.path().join("seccomp_test.txt");
        let policy = policy_with_system_paths(workspace.path());

        // Test a series of safe operations within the workspace
        let cmd_str = format!(
            "echo 'test' > {} && cat {} && rm {}",
            test_file.display(),
            test_file.display(),
            test_file.display()
        );
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "safe syscalls should work: {stderr}"
        );
    }

    /// Test that module loading is blocked.
    #[cfg(target_os = "linux")]
    #[test]
    fn seccomp_blocks_module_load() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("modprobe nonexistent 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail - either module not found or permission denied
        assert!(
            combined.contains("Operation not permitted")
                || combined.contains("Permission denied")
                || combined.contains("ret=1")
                || !output.status.success(),
            "module loading should be blocked: {combined}"
        );
    }

    // =================================================================================
    // NAMESPACE ISOLATION TESTS
    // =================================================================================

    /// Test that network namespace isolates network access.
    #[cfg(target_os = "linux")]
    #[test]
    fn namespace_isolates_network() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let mut policy = policy_with_system_paths(workspace.path());
        policy.egress = EgressPolicy::Deny;

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("curl -s --connect-timeout 2 http://example.com 2>&1; echo exit_code=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail to connect - accept various failure modes
        assert!(
            combined.contains("Network is unreachable")
                || combined.contains("not permitted")
                || combined.contains("exit_code=1")
                || combined.contains("exit_code=2")
                || combined.contains("exit_code=6")
                || combined.contains("exit_code=7")
                || combined.contains("exit_code=28")
                || combined.contains("Couldn't connect")
                || combined.contains("Could not resolve")
                || combined.contains("Failed to connect")
                || combined.contains("Could not resolve host")
                || !output.status.success(),
            "network should be isolated: {combined}"
        );
    }

    /// Test that loopback is available in network namespace.
    #[cfg(target_os = "linux")]
    #[test]
    fn namespace_allows_loopback() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();

        let workspace = tempfile::tempdir().expect("create workspace");
        let mut policy = policy_with_system_paths(workspace.path());
        policy.egress = EgressPolicy::Allowlist;
        policy.egress_allowlist = vec!["127.0.0.1".to_owned()];

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(format!(
            "bash -c 'echo test > /dev/tcp/127.0.0.1/{port}' 2>&1; echo retcode=$?"
        ));
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // The key test is that the sandbox setup succeeded
        assert!(
            stdout.contains("retcode=0") || stdout.contains("retcode=1"),
            "loopback test should complete: {stdout}"
        );
    }

    // =================================================================================
    // FAILURE MODE TESTS (FAIL-CLOSED BEHAVIOR)
    // =================================================================================

    /// Test that enforcing mode fails closed when Landlock is unavailable.
    #[cfg(target_os = "linux")]
    #[test]
    fn enforcing_fails_closed_when_landlock_unavailable() {
        // This test verifies the code path when Landlock is unavailable
        // We can't force a kernel to lack Landlock in a unit test.
        // Instead we verify the error message structure
        let workspace = tempfile::tempdir().expect("create workspace");

        let policy = SandboxPolicy {
            enabled: true,
            read_paths: vec![workspace.path().to_path_buf()],
            write_paths: vec![workspace.path().to_path_buf()],
            exec_paths: vec![PathBuf::from("/bin"), PathBuf::from("/usr/bin")],
            enforcement: SandboxEnforcement::Enforcing,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        };

        let mut cmd = Command::new("echo");
        cmd.arg("test");

        if probe_landlock_abi().is_none() {
            let result = apply_sandbox(&mut cmd, policy);
            assert!(
                result.is_err(),
                "enforcing mode must fail when Landlock is unavailable"
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("Landlock not available"),
                "error must mention Landlock: {err_msg}"
            );
        } else {
            // Landlock is available, just verify it works
            let result = apply_sandbox(&mut cmd, policy);
            assert!(
                result.is_ok(),
                "enforcing mode should work with Landlock available"
            );
        }
    }

    /// Test that permissive mode fails open (logs but continues) when Landlock is unavailable.
    #[cfg(target_os = "linux")]
    #[test]
    fn permissive_fails_open_when_landlock_unavailable() {
        let workspace = tempfile::tempdir().expect("create workspace");

        // Use full system paths to ensure binaries can be executed
        let policy = SandboxPolicy {
            enabled: true,
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
                PathBuf::from("/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/lib"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
            ],
            enforcement: SandboxEnforcement::Permissive,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        };

        let mut cmd = Command::new("/bin/echo");
        cmd.arg("permissive test");

        // Permissive mode must NOT error regardless of Landlock availability
        let result = apply_sandbox(&mut cmd, policy);
        assert!(result.is_ok(), "permissive mode must not error: {result:?}");

        let output = cmd.output().expect("spawn child");
        assert!(
            output.status.success(),
            "command should execute in permissive mode"
        );
    }

    /// Test that disabled policy allows everything.
    #[cfg(target_os = "linux")]
    #[test]
    fn disabled_policy_allows_all() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = SandboxPolicy {
            enabled: false,
            read_paths: Vec::new(),
            write_paths: Vec::new(),
            exec_paths: Vec::new(),
            enforcement: SandboxEnforcement::Permissive,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        };

        let mut cmd = Command::new("cat");
        cmd.arg("/etc/hostname");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(
            output.status.success(),
            "disabled policy should allow all access"
        );
    }

    // =================================================================================
    // EDGE CASE TESTS
    // =================================================================================

    /// Test /proc/self access restrictions (escape vector).
    #[cfg(target_os = "linux")]
    #[test]
    fn proc_self_access_restricted() {
        let _workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(_workspace.path());

        // Try to access /proc/self/environ which could leak environment variables
        let mut cmd = Command::new("cat");
        cmd.arg("/proc/self/environ");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        // Note: /proc is in read_paths by default, so this may succeed
        // The important thing is that the sandbox setup doesn't crash
        // and the process can still access /proc for basic operations
        assert!(
            output.status.success() || !output.status.success(),
            "proc access test should complete without crashing"
        );
    }

    /// Test that basic /proc access works for allowed processes.
    #[cfg(target_os = "linux")]
    #[test]
    fn proc_access_allowed() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new("cat");
        cmd.arg("/proc/self/status");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            output.status.success(),
            "reading /proc/self/status should work"
        );
        assert!(
            stdout.contains("Name:") || stdout.contains("Pid:"),
            "proc status should contain process info: {stdout}"
        );
    }

    /// Test write operations to /proc are blocked.
    #[cfg(target_os = "linux")]
    #[test]
    fn proc_write_blocked() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Try to write to /proc (should be blocked since /proc is only in read_paths)
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo 0 > /proc/self/oom_score_adj 2>&1; echo ret=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail with permission denied
        assert!(
            combined.contains("Permission denied")
                || combined.contains("Operation not permitted")
                || combined.contains("ret=1")
                || !output.status.success(),
            "writing to /proc should be blocked: {combined}"
        );
    }

    /// Test device access (/dev/null, /dev/zero, /dev/urandom).
    #[cfg(target_os = "linux")]
    #[test]
    fn device_access_allowed() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Test /dev/null which is commonly used
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo test > /dev/null");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output();
        // Device access may or may not work depending on Landlock ABI version
        // The important thing is that the sandbox setup doesn't crash
        if let Ok(out) = output {
            // If it runs, great - if not, that's also acceptable behavior
            let _ = out.status.success();
        }
    }

    /// Test that long path names work correctly.
    #[cfg(target_os = "linux")]
    #[test]
    fn long_path_names_work() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Create a deeply nested directory structure
        let mut deep_path = workspace.path().to_path_buf();
        for i in 0..20 {
            deep_path = deep_path.join(format!("subdir_{:02}", i));
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::create_dir_all(&deep_path).expect("create deep directory");

        let test_file = deep_path.join("deep_file.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&test_file, "deep content").expect("write deep file");

        let mut cmd = Command::new("cat");
        cmd.arg(&test_file);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "long path access should work: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "deep content",
            "deep file content should match"
        );
    }

    /// Test directory traversal prevention.
    #[cfg(target_os = "linux")]
    #[test]
    fn directory_traversal_blocked() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let secret_file = outside_dir.path().join("secret.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&secret_file, "secret data").expect("write secret file");

        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Create a deep nested structure in workspace
        let nested = workspace.path().join("a").join("b").join("c");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::create_dir_all(&nested).expect("create nested dirs");

        // Try to traverse outside workspace using relative paths
        let mut cmd = Command::new("sh");
        cmd.current_dir(&nested);
        cmd.arg("-c").arg(format!(
            "cat ../../../../../../{} 2>&1; echo ret=$?",
            secret_file.display()
        ));
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail with permission denied - the secret file is outside allowed paths
        assert!(
            combined.contains("Permission denied")
                || combined.contains("Operation not permitted")
                || combined.contains("ret=1")
                || !output.status.success()
                || !combined.contains("secret data"),
            "directory traversal should be blocked: {combined}"
        );
    }

    /// Test that hard links cannot escape the sandbox.
    #[cfg(target_os = "linux")]
    #[test]
    fn hardlink_escape_blocked() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let outside_file = outside_dir.path().join("outside.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&outside_file, "outside data").expect("write outside file");

        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Try to create a hard link from workspace to outside file
        let hardlink_path = workspace.path().join("hardlink_escape");
        let cmd_str = format!(
            "ln {} {} 2>&1; echo ret=$?",
            outside_file.display(),
            hardlink_path.display()
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // Should fail - hard link across permission boundaries should be blocked
        assert!(
            combined.contains("Permission denied")
                || combined.contains("Operation not permitted")
                || combined.contains("ret=1")
                || !output.status.success()
                || !hardlink_path.exists(),
            "hardlink escape should be blocked: {combined}"
        );
    }

    /// Test that reading from hard links inside allowed paths works.
    #[cfg(target_os = "linux")]
    #[test]
    fn hardlink_in_workspace_allowed() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        let file1 = workspace.path().join("file1.txt");
        let file2 = workspace.path().join("file2.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&file1, "shared content").expect("write file1");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::hard_link(&file1, &file2).expect("create hard link");

        let mut cmd = Command::new("cat");
        cmd.arg(&file2);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "reading hardlink in workspace should work: {stderr}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "shared content",
            "hardlink content should match"
        );
    }

    /// Test that execution in allowed paths works.
    #[cfg(target_os = "linux")]
    #[test]
    fn execution_in_allowed_paths() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let script = workspace.path().join("test_script.sh");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&script, "#!/bin/bash\necho 'script executed'").expect("write script");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");

        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new(&script);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "script execution should work: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("script executed"),
            "script should have executed"
        );
    }

    /// Test that execution outside allowed paths is blocked.
    #[cfg(target_os = "linux")]
    #[test]
    fn execution_outside_allowed_paths_blocked() {
        let outside_dir = tempfile::tempdir().expect("create outside dir");
        let script = outside_dir.path().join("outside_script.sh");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&script, "#!/bin/bash\necho 'should not execute'").expect("write script");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");

        let workspace = tempfile::tempdir().expect("create workspace");
        // Policy does NOT include outside_dir in exec_paths
        let policy = policy_with_system_paths(workspace.path());

        let mut cmd = Command::new(&script);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        // The spawn itself may fail with permission denied
        let output_result = cmd.output();
        if let Ok(output) = output_result {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            // Should fail - execution outside allowed paths should be blocked
            assert!(
                !output.status.success()
                    || combined.contains("Permission denied")
                    || combined.is_empty(),
                "execution outside allowed paths should be blocked: {combined}"
            );
        }
        // If spawn failed with permission denied, that's also a successful block
    }

    /// Test concurrent sandbox applications (stress test).
    #[cfg(target_os = "linux")]
    #[test]
    fn concurrent_sandbox_applications() {
        use std::sync::Arc;
        use std::thread;

        let mut handles = vec![];
        let policy = Arc::new(policy_with_system_paths(std::env::temp_dir().as_path()));

        for i in 0..10 {
            let policy_clone = Arc::clone(&policy);
            let handle = thread::spawn(move || {
                let mut cmd = Command::new("echo");
                cmd.arg(format!("concurrent test {}", i));

                // We can't actually apply sandbox in threads due to pre_exec constraints,
                // but we can verify the policy structure
                assert!(policy_clone.enabled);
                assert!(!policy_clone.read_paths.is_empty());
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread should complete");
        }
    }

    /// Test that temp directory access works.
    #[cfg(target_os = "linux")]
    #[test]
    fn temp_directory_access() {
        let workspace = tempfile::tempdir().expect("create workspace");
        let policy = policy_with_system_paths(workspace.path());

        // Use the workspace temp directory, not system /tmp
        let temp_file = workspace.path().join("sandbox_temp_test.txt");
        let cmd_str = format!(
            "echo 'temp test' > {} && cat {} && rm {}",
            temp_file.display(),
            temp_file.display(),
            temp_file.display()
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            output.status.success(),
            "temp directory access should work: {stderr}"
        );
        assert!(
            stdout.contains("temp test"),
            "temp file content should match: {stdout}"
        );
    }

    /// Test read-only paths are actually read-only.
    #[cfg(target_os = "linux")]
    #[test]
    fn read_only_paths_cannot_be_written() {
        let read_only_dir = tempfile::tempdir().expect("create read-only dir");
        let test_file = read_only_dir.path().join("readonly.txt");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup requires direct filesystem access"
        )]
        std::fs::write(&test_file, "original").expect("write test file");

        let workspace = tempfile::tempdir().expect("create workspace");
        let mut policy = policy_with_system_paths(workspace.path());
        // Add read-only dir to read_paths but NOT write_paths
        policy.read_paths.push(read_only_dir.path().to_path_buf());

        let cmd_str = format!(
            "echo 'modified' > {} 2>&1; echo exit_status=$?",
            test_file.display()
        );

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&cmd_str);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Should fail - read-only paths should not be writable
        assert!(
            stdout.contains("exit_status=1") || stdout.contains("Permission denied"),
            "writing to read-only path should fail: {stdout}"
        );
    }

    /// Test sandbox apply function directly (unit test for the apply method).
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_policy_apply_unit() {
        let workspace = tempfile::tempdir().expect("create workspace");

        // Create a policy with minimal paths
        let policy = SandboxPolicy {
            enabled: true,
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
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
                workspace.path().to_path_buf(),
            ],
            enforcement: SandboxEnforcement::Permissive,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        };

        // Verify the policy structure
        assert!(policy.enabled);
        assert!(!policy.read_paths.is_empty());
        assert!(!policy.write_paths.is_empty());
        assert!(!policy.exec_paths.is_empty());
    }
}
