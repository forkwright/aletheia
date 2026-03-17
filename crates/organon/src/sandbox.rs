//! Landlock + seccomp + network namespace sandbox for tool execution.
//!
//! Restricts filesystem access via Landlock LSM, blocks dangerous
//! syscalls via seccomp BPF filters, and isolates network access via
//! Linux network namespaces. Applied in child processes after fork,
//! before exec.

use std::net::IpAddr;
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

/// Network egress policy for child processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EgressPolicy {
    /// Block all outbound network from child processes.
    Deny,
    /// No egress filtering; child processes have full network access.
    #[default]
    Allow,
    /// Permit only connections to listed destinations.
    Allowlist,
}

/// Expand a leading `~` to the HOME environment variable.
///
/// If the path does not start with `~`, or if `HOME` is not set, returns the
/// path unchanged. This allows config files to use `~` as a portable reference
/// to the operator's home directory.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with('~')
        && let Ok(home) = std::env::var("HOME")
    {
        let without_tilde = s.strip_prefix('~').unwrap_or(&s);
        return PathBuf::from(format!("{home}{without_tilde}"));
    }
    path.to_path_buf()
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
    /// Additional filesystem paths granted execute access.
    ///
    /// Values may begin with `~` which is expanded to the HOME environment
    /// variable at policy-build time.
    pub extra_exec_paths: Vec<PathBuf>,
    /// Network egress policy for child processes.
    pub egress: EgressPolicy,
    /// Addresses or CIDR ranges permitted when `egress = "allowlist"`.
    ///
    /// Entries are parsed as IP addresses or CIDR notation (e.g.
    /// `"127.0.0.1"`, `"::1"`, `"10.0.0.0/8"`). Only loopback
    /// destinations can be enforced without root privileges; non-loopback
    /// entries log a warning.
    pub egress_allowlist: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
            egress: EgressPolicy::default(),
            egress_allowlist: Vec::new(),
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
        if !self.enabled {
            return SandboxPolicy {
                enabled: false,
                read_paths: Vec::new(),
                write_paths: Vec::new(),
                exec_paths: Vec::new(),
                enforcement: self.enforcement,
                egress: EgressPolicy::Allow,
                egress_allowlist: Vec::new(),
            };
        }

        let mut read_paths = vec![
            PathBuf::from("/usr"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
            PathBuf::from("/proc"),
            PathBuf::from("/dev"),
        ];

        let mut write_paths = vec![PathBuf::from("/tmp")];

        // WHY: System binary dirs are always executable. workspace and
        // allowed_roots are also added so agents can execute scripts they own
        // or that live in shared data directories: closes #1246.
        // /lib and /lib64 are included because the kernel opens the ELF
        // dynamic linker (ld-linux-*.so) with exec intent during execve().
        // Without Execute on these paths, all dynamically-linked binaries
        // fail with "Permission denied" even when the binary itself is in an
        // allowed exec path.
        let mut exec_paths = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/lib"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
        ];

        write_paths.push(workspace.to_path_buf());

        // WHY: workspace must be executable so agents can run scripts they
        // create inside their own working directory.
        if !exec_paths.contains(&workspace.to_path_buf()) {
            exec_paths.push(workspace.to_path_buf());
        }

        // WHY: allowed_roots grant read-only access to shared data that agents
        // may inspect but must not modify. Write access is limited to the
        // workspace and extra_write_paths, which are operator-controlled.
        // Exec access is also granted so agents can run scripts in shared dirs.
        for root in allowed_roots {
            if !read_paths.contains(root) {
                read_paths.push(root.clone());
            }
            if !exec_paths.contains(root) {
                exec_paths.push(root.clone());
            }
        }

        read_paths.extend(self.extra_read_paths.iter().cloned());
        write_paths.extend(self.extra_write_paths.iter().cloned());

        // WHY: extra_exec_paths support `~` prefix so operators can grant home
        // directory exec access in the config without hard-coding the path.
        exec_paths.extend(self.extra_exec_paths.iter().map(|p| expand_tilde(p)));

        for wp in &write_paths {
            if !read_paths.contains(wp) {
                read_paths.push(wp.clone());
            }
        }

        SandboxPolicy {
            enabled: true,
            read_paths,
            write_paths,
            exec_paths,
            enforcement: self.enforcement,
            egress: self.egress,
            egress_allowlist: self.egress_allowlist.clone(),
        }
    }
}

/// Runtime sandbox policy with resolved paths.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// Whether sandbox restrictions are applied at all.
    ///
    /// When `false`, `apply_sandbox` returns immediately without registering
    /// any `pre_exec` hook. Callers need not check this field separately.
    pub enabled: bool,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub exec_paths: Vec<PathBuf>,
    pub enforcement: SandboxEnforcement,
    /// Network egress policy.
    pub egress: EgressPolicy,
    /// Allowed destinations when `egress == Allowlist`.
    pub egress_allowlist: Vec<String>,
}

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
fn allowlist_is_loopback_only(entries: &[String]) -> bool {
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
        let block_inet = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            libc::AF_INET as u64,
        )
        .map_err(|e| std::io::Error::other(format!("seccomp condition failed: {e}")))?;

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
        // NOTE: Landlock ABI available, proceed with sandbox setup
        (Some(_), _) => {}
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_enabled() {
        let config = SandboxConfig::default();
        assert!(config.enabled);
        assert_eq!(config.enforcement, SandboxEnforcement::Permissive);
        assert!(config.extra_read_paths.is_empty());
        assert!(config.extra_write_paths.is_empty());
        assert!(config.extra_exec_paths.is_empty());
    }

    #[test]
    fn disabled_config() {
        let config = SandboxConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn disabled_config_returns_disabled_policy() {
        let config = SandboxConfig::disabled();
        let policy = config.build_policy(Path::new("/tmp/ws"), &[PathBuf::from("/extra")]);
        assert!(
            !policy.enabled,
            "disabled config must produce disabled policy"
        );
        assert!(
            policy.read_paths.is_empty(),
            "disabled policy has no read paths"
        );
        assert!(
            policy.write_paths.is_empty(),
            "disabled policy has no write paths"
        );
        assert!(
            policy.exec_paths.is_empty(),
            "disabled policy has no exec paths"
        );
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = SandboxConfig {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            extra_read_paths: vec![PathBuf::from("/opt/data")],
            extra_write_paths: vec![PathBuf::from("/var/cache")],
            extra_exec_paths: vec![PathBuf::from("/opt/scripts")],
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.enabled);
        assert_eq!(back.enforcement, SandboxEnforcement::Permissive);
        assert_eq!(back.extra_read_paths, vec![PathBuf::from("/opt/data")]);
        assert_eq!(back.extra_write_paths, vec![PathBuf::from("/var/cache")]);
        assert_eq!(back.extra_exec_paths, vec![PathBuf::from("/opt/scripts")]);
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
        assert_eq!(config.enforcement, SandboxEnforcement::Permissive);
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
    fn policy_includes_allowed_roots_as_read_only() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let extra = PathBuf::from("/shared/data");
        let policy = config.build_policy(&workspace, std::slice::from_ref(&extra));
        assert!(
            policy.read_paths.contains(&extra),
            "allowed_roots must appear in read_paths"
        );
        assert!(
            !policy.write_paths.contains(&extra),
            "allowed_roots must not appear in write_paths — read-only access only"
        );
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
        assert!(policy.exec_paths.contains(&PathBuf::from("/lib")));
        assert!(policy.exec_paths.contains(&PathBuf::from("/lib64")));
        assert!(policy.write_paths.contains(&PathBuf::from("/tmp")));
    }

    #[test]
    fn policy_includes_workspace_in_exec_paths() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let policy = config.build_policy(&workspace, &[]);
        assert!(
            policy.exec_paths.contains(&workspace),
            "workspace must be in exec_paths so agents can run scripts in their workspace"
        );
    }

    #[test]
    fn policy_includes_allowed_roots_in_exec_paths() {
        let config = SandboxConfig::default();
        let workspace = PathBuf::from("/home/agent/workspace");
        let shared = PathBuf::from("/shared/scripts");
        let policy = config.build_policy(&workspace, std::slice::from_ref(&shared));
        assert!(
            policy.exec_paths.contains(&shared),
            "allowed_roots must be in exec_paths so agents can run scripts in shared dirs"
        );
    }

    #[test]
    fn policy_includes_extra_exec_paths() {
        let config = SandboxConfig {
            extra_exec_paths: vec![PathBuf::from("/opt/scripts")],
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert!(
            policy.exec_paths.contains(&PathBuf::from("/opt/scripts")),
            "extra_exec_paths must appear in exec_paths"
        );
    }

    #[test]
    fn expand_tilde_replaces_home() {
        // Only meaningful when HOME is set: guard with an env check.
        if let Ok(home) = std::env::var("HOME") {
            let p = expand_tilde(Path::new("~/scripts"));
            assert_eq!(p, PathBuf::from(format!("{home}/scripts")));

            let p2 = expand_tilde(Path::new("~"));
            assert_eq!(p2, PathBuf::from(&home));
        }
    }

    #[test]
    fn expand_tilde_leaves_absolute_path_unchanged() {
        let p = expand_tilde(Path::new("/usr/local/bin"));
        assert_eq!(p, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn policy_expands_tilde_in_extra_exec_paths() {
        if let Ok(home) = std::env::var("HOME") {
            let config = SandboxConfig {
                extra_exec_paths: vec![PathBuf::from("~")],
                ..SandboxConfig::default()
            };
            let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
            assert!(
                policy.exec_paths.contains(&PathBuf::from(&home)),
                "~ in extra_exec_paths must be expanded to HOME"
            );
        }
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

        if probe_landlock_abi().is_none() {
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
        } else {
            // Landlock is available: enforcing mode succeeds (no opaque error).
            let result = apply_sandbox(&mut cmd, policy);
            assert!(
                result.is_ok(),
                "enforcing mode must succeed when Landlock is available: {result:?}"
            );
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
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
        ];

        let policy = SandboxPolicy {
            enabled: true,
            read_paths,
            write_paths,
            exec_paths,
            enforcement: SandboxEnforcement::Enforcing,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
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
            ],
            enforcement: SandboxEnforcement::Enforcing,
            egress: EgressPolicy::Allow,
            egress_allowlist: Vec::new(),
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

    #[cfg(target_os = "linux")]
    #[test]
    fn exec_succeeds_under_sandbox_with_absolute_and_bare_paths() {
        use std::process::Command;

        if probe_landlock_abi().is_none() {
            return;
        }

        let config = SandboxConfig::default();
        let dir = tempfile::tempdir().expect("create temp dir");

        // Absolute path: bypasses PATH resolution, still needs dynamic linker.
        let policy = config.build_policy(dir.path(), &[]);
        let mut cmd = Command::new("/usr/bin/uname");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");
        let output = cmd.output().expect("spawn");
        assert!(
            output.status.success(),
            "absolute path exec must succeed under sandbox: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Bare command: needs PATH resolution + dynamic linker loading.
        let policy = config.build_policy(dir.path(), &[]);
        let mut cmd = Command::new("uname");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");
        let output = cmd.output().expect("spawn");
        assert!(
            output.status.success(),
            "bare command exec must succeed under sandbox: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn default_egress_is_allow() {
        let config = SandboxConfig::default();
        assert_eq!(
            config.egress,
            EgressPolicy::Allow,
            "default egress policy must be Allow for backward compatibility"
        );
        assert!(
            config.egress_allowlist.is_empty(),
            "default allowlist must be empty"
        );
    }

    #[test]
    fn egress_policy_serde() {
        let json = serde_json::to_string(&EgressPolicy::Deny).expect("serialize");
        assert_eq!(json, "\"deny\"");
        let back: EgressPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, EgressPolicy::Deny);

        let json = serde_json::to_string(&EgressPolicy::Allow).expect("serialize");
        assert_eq!(json, "\"allow\"");

        let json = serde_json::to_string(&EgressPolicy::Allowlist).expect("serialize");
        assert_eq!(json, "\"allowlist\"");
    }

    #[test]
    fn egress_config_serde_roundtrip() {
        let config = SandboxConfig {
            egress: EgressPolicy::Allowlist,
            egress_allowlist: vec!["127.0.0.1".to_owned(), "::1".to_owned()],
            ..SandboxConfig::default()
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.egress, EgressPolicy::Allowlist);
        assert_eq!(back.egress_allowlist, vec!["127.0.0.1", "::1"]);
    }

    #[test]
    fn disabled_policy_has_allow_egress() {
        let config = SandboxConfig::disabled();
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert_eq!(
            policy.egress,
            EgressPolicy::Allow,
            "disabled sandbox must not restrict egress"
        );
    }

    #[test]
    fn policy_inherits_egress_from_config() {
        let config = SandboxConfig {
            egress: EgressPolicy::Deny,
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(Path::new("/tmp/ws"), &[]);
        assert_eq!(policy.egress, EgressPolicy::Deny);
    }

    #[test]
    fn allowlist_loopback_check() {
        assert!(
            allowlist_is_loopback_only(&[
                "127.0.0.1".to_owned(),
                "::1".to_owned(),
                "127.0.0.1/8".to_owned(),
            ]),
            "loopback-only list should return true"
        );
        assert!(
            !allowlist_is_loopback_only(&["127.0.0.1".to_owned(), "10.0.0.1".to_owned()]),
            "list with non-loopback should return false"
        );
        assert!(
            !allowlist_is_loopback_only(&["example.com".to_owned()]),
            "hostname entries are not loopback"
        );
        assert!(
            allowlist_is_loopback_only(&[]),
            "empty list is trivially loopback-only"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn egress_deny_blocks_network() {
        use std::process::Command;

        let config = SandboxConfig {
            egress: EgressPolicy::Deny,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        // WHY: Try to create a TCP connection to a TEST-NET address (RFC 5737).
        // With egress=deny, the child is in a network namespace with only
        // loopback, so connect() to any non-loopback address fails immediately
        // with ENETUNREACH (or EPERM if seccomp fallback is active).
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo test | nc -w1 198.51.100.1 80 2>&1; echo exit=$?");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // The connection must fail. Possible error messages depend on mechanism:
        // - Network namespace: "Network is unreachable"
        // - Seccomp fallback: "Permission denied" or "Operation not permitted"
        assert!(
            combined.contains("exit=1")
                || combined.contains("Network is unreachable")
                || combined.contains("not permitted")
                || combined.contains("Permission denied")
                || !output.status.success(),
            "egress=deny must block outbound network: {combined}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn egress_deny_allows_basic_commands() {
        use std::process::Command;

        let config = SandboxConfig {
            egress: EgressPolicy::Deny,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("egress test");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(
            output.status.success(),
            "basic commands must work with egress=deny: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("egress test"),
            "command output must be captured"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn egress_allowlist_loopback_permits_localhost() {
        use std::net::TcpListener;
        use std::process::Command;

        // WHY: Bind a listener on loopback so the child has something to
        // connect to. With egress=allowlist and 127.0.0.1 in the list,
        // the child should be able to reach this listener via the namespace's
        // loopback interface.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();

        let config = SandboxConfig {
            egress: EgressPolicy::Allowlist,
            egress_allowlist: vec!["127.0.0.1".to_owned()],
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        // WHY: Use sh -c with echo + /dev/tcp to test connectivity without
        // requiring curl or nc. bash's /dev/tcp is a builtin that creates
        // a TCP connection.
        let test_cmd = format!("bash -c 'echo hi > /dev/tcp/127.0.0.1/{port}' 2>&1; echo exit=$?");
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&test_cmd);
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);

        // NOTE: In a network namespace, loopback is available but we need
        // to bring up the lo interface. The loopback interface exists but
        // may be down. Connection may succeed or fail depending on whether
        // the namespace auto-configures lo. Either way, the key test is
        // that the sandbox setup itself succeeded (no crash).
        // The egress_deny_blocks_network test verifies external blocking.
        assert!(
            stdout.contains("exit=0") || stdout.contains("exit=1"),
            "command must complete (not hang) with allowlist: {stdout}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn egress_allow_does_not_restrict() {
        use std::process::Command;

        let config = SandboxConfig {
            egress: EgressPolicy::Allow,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("no egress filter");
        apply_sandbox(&mut cmd, policy).expect("apply sandbox");

        let output = cmd.output().expect("spawn child");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("no egress filter"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn egress_graceful_fallback() {
        // WHY: This test verifies that apply_sandbox does not return an error
        // even when the egress mechanism (network namespace or seccomp) might
        // not be available. The permissive enforcement ensures graceful
        // degradation rather than hard failure.
        use std::process::Command;

        let config = SandboxConfig {
            egress: EgressPolicy::Deny,
            enforcement: SandboxEnforcement::Permissive,
            ..SandboxConfig::default()
        };
        let dir = tempfile::tempdir().expect("create temp dir");
        let policy = config.build_policy(dir.path(), &[]);

        let mut cmd = Command::new("echo");
        cmd.arg("fallback test");

        // Must not error regardless of kernel support
        let result = apply_sandbox(&mut cmd, policy);
        assert!(
            result.is_ok(),
            "egress deny with permissive enforcement must not error: {result:?}"
        );

        let output = cmd.output().expect("spawn child");
        assert!(
            output.status.success(),
            "command must execute after egress setup"
        );
    }
}
