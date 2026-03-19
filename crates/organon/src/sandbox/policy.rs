//! Runtime sandbox policy application — Landlock, seccomp, network namespaces.
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
    pub fn apply(&self) -> std::io::Result<()> {
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
                #[expect(
                    unsafe_code,
                    reason = "unshare syscall required to create network namespace for egress filtering"
                )]
                let ret = unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) };
                if ret == 0 {
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
        #[expect(
            clippy::as_conversions,
            reason = "libc::AF_INET is i32; seccomp API requires u64"
        )]
        let block_inet = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            libc::AF_INET as u64,
        )
        .map_err(|e| std::io::Error::other(format!("seccomp condition failed: {e}")))?;

        #[expect(
            clippy::as_conversions,
            reason = "libc::AF_INET6 is i32; seccomp API requires u64"
        )]
        let block_inet6 = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            libc::AF_INET6 as u64,
        )
        .map_err(|e| std::io::Error::other(format!("seccomp condition failed: {e}")))?;

        let rules = std::collections::BTreeMap::from([(
            libc::SYS_socket,
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
            #[expect(
                clippy::as_conversions,
                reason = "libc::EPERM is i32; seccomp API requires u32"
            )]
            SeccompAction::Errno(libc::EPERM as u32),
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
            #[expect(
                clippy::as_conversions,
                reason = "libc::EPERM is i32; seccomp API requires u32"
            )]
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
/// between fork and exec in the child process. The underlying Landlock and
/// seccomp syscalls are async-signal-safe, but the crate wrappers perform
/// heap allocations. See the inline `SAFETY` and `WARNING` comments for the
/// full risk analysis.
#[cfg(target_os = "linux")]
pub fn apply_sandbox(
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
    // TODO(#1140): pre-compile seccomp BPF in the parent and use raw Landlock
    // syscalls in the child to eliminate all post-fork allocations.
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
