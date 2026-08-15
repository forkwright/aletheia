//! Runtime sandbox policy application -- Landlock, seccomp, network namespaces.
//!
//! The full Landlock + seccomp + netns implementation is Linux-only. On other
//! platforms (macOS CI, Windows) most symbols are unreachable via `apply_sandbox`,
//! which short-circuits to a no-op. The attribute below silences `dead_code` /
//! `unused_imports` warnings on those platforms without sprinkling per-item cfgs.
#![cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        unused_imports,
        unused_variables,
        clippy::unused_self,
        clippy::needless_pass_by_value,
        clippy::missing_const_for_fn,
        reason = "Linux-only sandbox machinery; apply_sandbox is a no-op on other platforms"
    )
)]

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use koina::http::{HostResolver, is_private_ip};

use super::config::{EgressPolicy, SandboxConfig, SandboxEnforcement, SandboxPolicy};

/// Status of a sandbox guarantee for operator diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuaranteeStatus {
    /// The guarantee is enforced for this execution.
    Active,
    /// The guarantee is requested but cannot be fully enforced; execution continues.
    Degraded,
    /// The guarantee is requested but unavailable; enforcing mode blocks execution.
    Unavailable,
    /// The guarantee was not requested (e.g., egress=allow).
    Unrestricted,
}

impl std::fmt::Display for GuaranteeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unavailable => write!(f, "unavailable"),
            Self::Unrestricted => write!(f, "unrestricted"),
        }
    }
}

/// Per-guarantee status for a sandbox policy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SandboxGuarantees {
    /// Filesystem access restriction via Landlock.
    pub(crate) landlock: GuaranteeStatus,
    /// Dangerous syscall blocking via seccomp.
    pub(crate) seccomp: GuaranteeStatus,
    /// Network egress restriction.
    pub(crate) egress: GuaranteeStatus,
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
/// treated as non-loopback, so a typo fails closed the same way a genuine
/// non-loopback entry does rather than silently passing as enforceable --
/// callers key startup validation (`SandboxConfig::validate`), runtime
/// diagnostics (`egress_guarantee_status`), and spawn-time rejection
/// (`apply_sandbox`) off this single predicate.
// kanon:ignore RUST/doc-promised-observability -- WHY: caller behavior (validate/reject/log) is prose description of consumers, not an observability contract; function is a pure predicate
pub(crate) fn allowlist_is_loopback_only(entries: &[String]) -> bool {
    entries.iter().all(|entry| {
        let ip_part = entry.split('/').next().unwrap_or(entry);
        ip_part.parse::<IpAddr>().is_ok_and(|a| is_loopback(&a))
    })
}

/// One parsed `egress_allowlist` entry (bare IP or CIDR range).
///
/// Unparseable entries are dropped by [`EgressGate::new`] with a warning
/// rather than causing every check to silently fail closed on a typo, but
/// also never silently match (see `EgressGate` doc).
#[derive(Debug, Clone, Copy)]
struct AllowedNetwork {
    network: IpAddr,
    prefix_len: u32,
}

impl AllowedNetwork {
    /// Parse `entry` as a bare IP address (implicit /32 or /128) or CIDR.
    fn parse(entry: &str) -> Option<Self> {
        let (ip_part, prefix_part) = match entry.split_once('/') {
            Some((ip, prefix)) => (ip, Some(prefix)),
            None => (entry, None),
        };
        let network: IpAddr = ip_part.trim().parse().ok()?;
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix_len = match prefix_part {
            Some(p) => p.trim().parse::<u32>().ok().filter(|p| *p <= max_prefix)?,
            None => max_prefix,
        };
        Some(Self {
            network,
            prefix_len,
        })
    }

    /// Whether `target` falls inside this network.
    fn contains(&self, target: IpAddr) -> bool {
        match (self.network, target) {
            (IpAddr::V4(net), IpAddr::V4(t)) => {
                let mask = mask_u32(self.prefix_len);
                (u32::from(net) & mask) == (u32::from(t) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(t)) => {
                let mask = mask_u128(self.prefix_len);
                (u128::from(net) & mask) == (u128::from(t) & mask)
            }
            // WHY: an IPv4 allowlist entry never matches an IPv6 target and
            // vice versa. `to_ipv4_mapped` normalization is deliberately not
            // applied here -- operators who want to allow a v4 destination
            // reached via a v4-mapped v6 address should list it explicitly.
            _ => false,
        }
    }
}

fn mask_u32(prefix_len: u32) -> u32 {
    if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    }
}

fn mask_u128(prefix_len: u32) -> u128 {
    if prefix_len == 0 {
        0
    } else {
        u128::MAX << (128 - prefix_len)
    }
}

/// Network egress was refused by [`EgressGate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDenied(String);

impl std::fmt::Display for EgressDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "network egress denied by sandbox policy: {}", self.0)
    }
}

impl std::error::Error for EgressDenied {}

/// The single network-egress checkpoint every in-process, network-capable
/// tool (`http_request`, `web_fetch`, `web_search`) must call before -- and
/// after -- opening a connection.
///
/// SECURITY(#5071): `sandbox.egress` previously only gated child processes
/// spawned through [`SubprocessRunner`](crate::subprocess::SubprocessRunner);
/// these three tools build their own `reqwest` clients directly and never
/// consulted it, so `egress = "deny"` reassured operators while doing
/// nothing to stop them. Every egress decision for in-process tools now
/// flows through this type so the three call sites cannot drift apart the
/// way three separate ad hoc checks would.
///
/// SECURITY(#5232): `egress = "allowlist"` is enforced for real here --
/// every candidate address is checked against parsed CIDR ranges, not
/// treated as a synonym for `deny` the way the child-process network
/// namespace path does (loopback-only there, see `allowlist_is_loopback_only`).
#[derive(Debug, Clone)]
pub struct EgressGate {
    policy: EgressPolicy,
    allowlist: Vec<AllowedNetwork>,
}

impl EgressGate {
    /// Build a gate from an explicit policy and allowlist entries.
    #[must_use]
    pub fn new(policy: EgressPolicy, allowlist_entries: &[String]) -> Self {
        let mut allowlist = Vec::with_capacity(allowlist_entries.len());
        let mut invalid = Vec::new();
        for entry in allowlist_entries {
            match AllowedNetwork::parse(entry) {
                Some(net) => allowlist.push(net),
                None => invalid.push(entry.clone()),
            }
        }
        if !invalid.is_empty() {
            tracing::warn!(
                entries = ?invalid,
                "sandbox egress_allowlist entries could not be parsed as an IP address or \
                 CIDR range; they will never match and every destination will be denied"
            );
        }
        Self { policy, allowlist }
    }

    /// Build a gate from a [`SandboxConfig`]'s egress policy and allowlist.
    #[must_use]
    pub fn from_config(config: &SandboxConfig) -> Self {
        Self::new(config.egress, &config.egress_allowlist)
    }

    /// The egress policy this gate enforces.
    #[must_use]
    pub fn policy(&self) -> EgressPolicy {
        self.policy
    }

    /// Reject outright when no destination is known yet.
    ///
    /// Only `Deny` can be decided before DNS resolution; call this first so
    /// a denied tool never performs a DNS lookup at all, matching the
    /// child-process path where `egress = "deny"` blocks socket creation
    /// before any connection attempt.
    ///
    /// # Errors
    /// Returns [`EgressDenied`] when the policy is `Deny`.
    pub fn check_before_connect(&self) -> Result<(), EgressDenied> {
        if self.policy == EgressPolicy::Deny {
            return Err(EgressDenied(
                "egress = \"deny\": network access is blocked".to_owned(),
            ));
        }
        Ok(())
    }

    /// Check one resolved or connected address against policy.
    ///
    /// Called for every DNS-resolved candidate before connecting, and again
    /// for the address a connection actually landed on -- so a rebinding
    /// answer that differs between the two checks cannot slip through.
    ///
    /// # Errors
    /// Returns [`EgressDenied`] when the policy is `Deny`, or when the
    /// policy is `Allowlist` and `addr` matches no configured entry.
    pub fn check_addr(&self, addr: IpAddr) -> Result<(), EgressDenied> {
        match self.policy {
            EgressPolicy::Deny => Err(EgressDenied(
                "egress = \"deny\": network access is blocked".to_owned(),
            )),
            EgressPolicy::Allow => Ok(()),
            EgressPolicy::Allowlist => {
                if self.allowlist.iter().any(|net| net.contains(addr)) {
                    Ok(())
                } else {
                    Err(EgressDenied(format!(
                        "egress = \"allowlist\": {addr} is not in egress_allowlist"
                    )))
                }
            }
            // WHY: EgressPolicy is `#[non_exhaustive]` (single-owned by
            // taxis, ARCHITECTURE #4846); fail closed like Deny for an
            // unrecognized future variant rather than silently allowing
            // network access this function was never told is safe.
            _ => Err(EgressDenied(
                "egress policy is not one this build recognizes; denying by default".to_owned(),
            )),
        }
    }
}

/// Resolve `host:port` and check every candidate address against `gate`.
///
/// Called before connecting so `Deny` and a non-matching `Allowlist`
/// destination are rejected before any bytes leave the process. `Allow`
/// short-circuits without resolving: no per-address egress decision is
/// needed, and the independent SSRF guard (private/internal address
/// rejection) still applies regardless of egress policy.
///
/// # Errors
/// Returns a message describing why egress was denied, or a DNS resolution
/// failure surfaced by `resolver`.
pub async fn check_egress<R>(
    gate: &EgressGate,
    host: &str,
    port: u16,
    resolver: &R,
) -> Result<(), String>
where
    R: HostResolver + ?Sized,
{
    gate.check_before_connect().map_err(|e| e.to_string())?;
    if gate.policy() == EgressPolicy::Allow {
        return Ok(());
    }
    let addrs = resolver.resolve_host(host, port).await?;
    for addr in &addrs {
        gate.check_addr(addr.ip()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Re-check the address a connection actually landed on, after connecting.
///
/// SECURITY(#5229): DNS can change between [`check_egress`]'s resolution and
/// the HTTP client's own connect-time resolution (rebinding). This closes
/// that gap by validating the real peer instead of trusting the pre-connect
/// answer, and applies unconditionally (not only under `egress =
/// "allowlist"`): a private/internal peer is never an acceptable outcome for
/// these tools regardless of egress policy, matching the pre-connect SSRF
/// guard's behavior.
///
/// `addr` is `None` when the transport does not expose a peer address (some
/// mocked clients in tests); there is nothing to re-validate in that case,
/// the pre-connect check already ran.
///
/// # Errors
/// Returns a message when the peer is a private/internal address, or when
/// `gate`'s policy additionally rejects it (`Deny`, or `Allowlist` with no
/// matching entry).
pub fn check_egress_remote_addr(gate: &EgressGate, addr: Option<SocketAddr>) -> Result<(), String> {
    let Some(addr) = addr else {
        return Ok(());
    };
    if is_private_ip(&addr.ip()) {
        return Err(format!(
            "connection landed on private/internal address {} after DNS validation passed \
             (possible DNS rebinding); rejecting",
            addr.ip()
        ));
    }
    gate.check_addr(addr.ip()).map_err(|e| e.to_string())
}

impl SandboxPolicy {
    /// Apply Landlock + seccomp + egress restrictions to the current process.
    ///
    /// Designed to run in a child process via `pre_exec`. Returns `io::Error`
    /// on failure; on unsupported kernels, logs and continues based on
    /// enforcement mode.
    pub(crate) fn apply(&self) -> std::io::Result<()> {
        // WHY: Egress, Landlock, and seccomp are independent controls. A failure
        // in one must not short-circuit the others; otherwise a missing kernel
        // feature (e.g. Landlock) could leave the child fully unsandboxed.
        let egress = self.apply_egress();
        let landlock = self.apply_landlock();
        let seccomp = self.apply_seccomp();

        let mut failures: Vec<(&str, String)> = Vec::new();
        if let Err(e) = egress {
            failures.push(("egress", e.to_string()));
        }
        if let Err(e) = landlock {
            failures.push(("landlock", e.to_string()));
        }
        if let Err(e) = seccomp {
            failures.push(("seccomp", e.to_string()));
        }

        if failures.is_empty() || self.enforcement == SandboxEnforcement::Permissive {
            // WHY: Permissive mode deliberately continues after logging the
            // degradation in the parent. All requested controls were still
            // attempted above, so any that could be installed are active.
            return Ok(());
        }

        // INVARIANT: In enforcing mode, any sandbox setup failure is fatal.
        // Return the first failure with a clear name so operators can tell
        // which guarantee blocked execution.
        if let Some((name, err)) = failures.into_iter().next() {
            return Err(std::io::Error::other(format!(
                "{name} sandbox setup failed: {err}"
            )));
        }

        Ok(())
    }

    /// Apply network egress restrictions via Linux network namespaces.
    ///
    /// WHY: `unshare(CLONE_NEWUSER | CLONE_NEWNET)` creates an isolated
    /// network namespace containing only a loopback interface. This blocks
    /// all outbound connections to external hosts without requiring root
    /// privileges. The user namespace is required because `CLONE_NEWNET`
    /// alone requires `CAP_SYS_ADMIN`.
    ///
    /// WHY `Deny` and `Allowlist` share this arm (#4997): neither `unshare`
    /// nor the seccomp fallback below can inspect a destination address --
    /// they can only isolate the child to loopback or block sockets
    /// outright, so there is no way to honor a specific `egress_allowlist`
    /// entry at this layer. Selectivity is enforced upstream instead:
    /// `SandboxConfig::validate` and `egress_guarantee_status` (this
    /// module's `probe_guarantees` path) both key off
    /// `allowlist_is_loopback_only` to refuse (enforcing) or degrade
    /// (permissive) an `Allowlist` policy this arm cannot actually satisfy,
    /// rather than silently letting `Allowlist` report the same guarantee
    /// `Deny` gets. Do not add per-destination logic here without also
    /// giving the child a real route to those destinations (e.g. a
    /// configured veth + firewall rules) -- inspecting an address with no
    /// path to reach it changes nothing observable.
    #[cfg(target_os = "linux")]
    fn apply_egress(&self) -> std::io::Result<()> {
        // WHY: `!=` rather than matching `Deny | Allowlist` explicitly --
        // EgressPolicy is `#[non_exhaustive]` (single-owned by taxis,
        // ARCHITECTURE #4846); an unrecognized future variant falls into
        // the same restrictive isolate-or-block path below as
        // Deny/Allowlist, never the permissive Allow return above. (Also
        // sidesteps clippy::single_match_else, which a two-armed
        // `match self.egress { Allow => .., _ => <long block> }` here
        // triggered.)
        if self.egress == EgressPolicy::Allow {
            return Ok(());
        }

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
                rustix::thread::UnshareFlags::NEWUSER | rustix::thread::UnshareFlags::NEWNET,
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
            arch_syscalls::SYS_SOCKET,
            vec![
                SeccompRule::new(vec![block_inet])
                    .map_err(|e| std::io::Error::other(format!("seccomp rule failed: {e}")))?,
                SeccompRule::new(vec![block_inet6])
                    .map_err(|e| std::io::Error::other(format!("seccomp rule failed: {e}")))?,
            ],
        )]);

        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        let arch = target_arch();
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let arch = {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "seccomp target architecture unsupported",
            ));
        };
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
    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the Linux implementation; non-Linux stub is a no-op"
    )]
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
    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the Linux implementation; non-Linux stub is a no-op"
    )]
    fn apply_landlock(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn apply_seccomp(&self) -> std::io::Result<()> {
        use std::collections::BTreeMap;

        use seccompiler::{SeccompAction, SeccompFilter, SeccompRule};

        let blocked_syscalls: &[i64] = blocked_syscalls();

        let rules: BTreeMap<i64, Vec<SeccompRule>> =
            blocked_syscalls.iter().map(|&nr| (nr, vec![])).collect();

        let action = if self.enforcement == SandboxEnforcement::Permissive {
            SeccompAction::Log
        } else {
            SeccompAction::Errno(1u32 /* EPERM */)
        };

        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        let arch = target_arch();
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let arch = {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "seccomp target architecture unsupported",
            ));
        };

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
    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the Linux implementation; non-Linux stub is a no-op"
    )]
    fn apply_seccomp(&self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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

/// Architecture-specific syscall numbers used in seccomp BPF filters.
///
/// WHY: seccompiler validates the filter against a target architecture, but the
/// rule keys are raw syscall numbers. Using `x86_64` numbers on `aarch64` (or
/// vice versa) would silently fail to block the intended syscalls.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) mod arch_syscalls {
    pub const SYS_PTRACE: i64 = 101;
    pub const SYS_MOUNT: i64 = 165;
    pub const SYS_UMOUNT2: i64 = 166;
    pub const SYS_REBOOT: i64 = 169;
    pub const SYS_KEXEC_LOAD: i64 = 246;
    pub const SYS_INIT_MODULE: i64 = 175;
    pub const SYS_DELETE_MODULE: i64 = 176;
    pub const SYS_FINIT_MODULE: i64 = 313;
    pub const SYS_PIVOT_ROOT: i64 = 155;
    pub const SYS_CHROOT: i64 = 161;
    pub const SYS_SOCKET: i64 = 41;
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) mod arch_syscalls {
    pub const SYS_PTRACE: i64 = 117;
    pub const SYS_MOUNT: i64 = 40;
    pub const SYS_UMOUNT2: i64 = 39;
    pub const SYS_REBOOT: i64 = 142;
    pub const SYS_KEXEC_LOAD: i64 = 104;
    pub const SYS_INIT_MODULE: i64 = 105;
    pub const SYS_DELETE_MODULE: i64 = 106;
    pub const SYS_FINIT_MODULE: i64 = 273;
    pub const SYS_PIVOT_ROOT: i64 = 41;
    pub const SYS_CHROOT: i64 = 51;
    pub const SYS_SOCKET: i64 = 198;
}

/// Return the list of syscalls blocked by the seccomp filter for this arch.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[must_use]
pub(crate) fn blocked_syscalls() -> &'static [i64] {
    use arch_syscalls::{
        SYS_CHROOT, SYS_DELETE_MODULE, SYS_FINIT_MODULE, SYS_INIT_MODULE, SYS_KEXEC_LOAD,
        SYS_MOUNT, SYS_PIVOT_ROOT, SYS_PTRACE, SYS_REBOOT, SYS_UMOUNT2,
    };
    &[
        SYS_PTRACE,
        SYS_MOUNT,
        SYS_UMOUNT2,
        SYS_REBOOT,
        SYS_KEXEC_LOAD,
        SYS_INIT_MODULE,
        SYS_DELETE_MODULE,
        SYS_FINIT_MODULE,
        SYS_PIVOT_ROOT,
        SYS_CHROOT,
    ]
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
        tracing::debug!(landlock_abi = v, "Landlock ABI v{v} available");
    } else {
        tracing::debug!("Landlock not available on this kernel");
    }
    abi
});

/// Probe the status of each sandbox guarantee for the given policy.
///
/// Returns a snapshot suitable for operator diagnostics. The result reflects
/// kernel capabilities and platform support; it does not guarantee that every
/// child-side mechanism will succeed at runtime.
#[cfg(target_os = "linux")]
#[must_use]
fn probe_guarantees(policy: &SandboxPolicy) -> SandboxGuarantees {
    let landlock = match *LANDLOCK_ABI {
        Some(_) => GuaranteeStatus::Active,
        None => {
            if policy.enforcement == SandboxEnforcement::Enforcing {
                GuaranteeStatus::Unavailable
            } else {
                GuaranteeStatus::Degraded
            }
        }
    };
    let seccomp = seccomp_guarantee_status(policy);
    let egress = egress_guarantee_status(policy, seccomp);
    SandboxGuarantees {
        landlock,
        seccomp,
        egress,
    }
}

#[cfg(not(target_os = "linux"))]
#[must_use]
fn probe_guarantees(policy: &SandboxPolicy) -> SandboxGuarantees {
    let status = if policy.enforcement == SandboxEnforcement::Enforcing {
        GuaranteeStatus::Unavailable
    } else {
        GuaranteeStatus::Degraded
    };
    SandboxGuarantees {
        landlock: status,
        seccomp: status,
        egress: match policy.egress {
            EgressPolicy::Allow => GuaranteeStatus::Unrestricted,
            _ => status,
        },
    }
}

#[cfg(target_os = "linux")]
#[must_use]
fn seccomp_guarantee_status(policy: &SandboxPolicy) -> GuaranteeStatus {
    if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
        GuaranteeStatus::Active
    } else if policy.enforcement == SandboxEnforcement::Enforcing {
        GuaranteeStatus::Unavailable
    } else {
        GuaranteeStatus::Degraded
    }
}

/// Compute the `egress` guarantee status, accounting for whether an
/// `Allowlist` policy's entries are within what the child-process
/// network-namespace/seccomp mechanism can actually provide.
///
/// SECURITY(#4997): `apply_egress` (below) has exactly two real outcomes for
/// a non-`Allow` policy -- isolate the child to loopback-only network access,
/// or block network entirely -- it never inspects `egress_allowlist` to honor
/// a specific destination. An allowlist confined to loopback entries
/// (`allowlist_is_loopback_only`) is within that real capability and tracks
/// the same kernel/arch support `Deny` does. An allowlist with any
/// non-loopback entry is NOT: those entries can never be reached, so
/// reporting the same status `Deny` gets (as a bare `_ => seccomp` match
/// once did here) would tell an operator their listed destinations are
/// enforced when the mechanism can never provide that -- indistinguishable
/// from `deny` in every observable way except the name.
#[cfg(target_os = "linux")]
#[must_use]
fn egress_guarantee_status(policy: &SandboxPolicy, seccomp: GuaranteeStatus) -> GuaranteeStatus {
    match policy.egress {
        EgressPolicy::Allow => GuaranteeStatus::Unrestricted,
        EgressPolicy::Allowlist => {
            if allowlist_is_loopback_only(&policy.egress_allowlist) {
                seccomp
            } else if policy.enforcement == SandboxEnforcement::Enforcing {
                GuaranteeStatus::Unavailable
            } else {
                GuaranteeStatus::Degraded
            }
        }
        // WHY: covers both `Deny` and any unrecognized future variant --
        // a separate `EgressPolicy::Deny => seccomp` arm is identical to
        // this one and clippy::match_same_arms rejects it. EgressPolicy
        // is `#[non_exhaustive]` (single-owned by taxis, ARCHITECTURE
        // #4846), so an unrecognized future variant gets the same
        // conservative `seccomp` guarantee status `Deny` does, rather
        // than claiming `Allow`'s `Unrestricted` guarantee for something
        // never actually verified.
        _ => seccomp,
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
        #[cfg(target_arch = "x86_64")]
        {
            let r: isize;
            core::arch::asm!(
                "syscall",
                inlateout("rax") 444isize => r, // SYS_landlock_create_ruleset
                in("rdi") 0usize,                // null ruleset
                in("rsi") 0usize,                // size 0
                in("rdx") LANDLOCK_CREATE_RULESET_VERSION,
                lateout("rcx") _,
                lateout("r11") _,
            );
            r
        }
        #[cfg(target_arch = "aarch64")]
        {
            let mut x0: usize = 0;
            core::arch::asm!(
                "svc 0",
                in("x8") 444usize, // SYS_landlock_create_ruleset
                inlateout("x0") 0usize => x0,
                in("x1") 0usize,
                in("x2") LANDLOCK_CREATE_RULESET_VERSION,
                options(nostack, preserves_flags)
            );
            x0 as isize
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            -1isize
        }
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

#[cfg(target_os = "linux")]
fn warn_sandbox_degradation(guarantees: SandboxGuarantees, policy: &SandboxPolicy) {
    match (guarantees.landlock, policy.enforcement) {
        (GuaranteeStatus::Degraded, SandboxEnforcement::Permissive) => {
            // WHY: pre_exec cannot safely log; warn in the parent where tracing works.
            tracing::warn!(
                enforcement = "permissive",
                "Landlock unavailable, filesystem sandbox degraded; \
                 set enforcement=enforcing and ensure kernel supports Landlock (5.13+)"
            );
        }
        // WHY: Warn ONCE per process when Landlock is available but enforcement is permissive,
        // so operators know syscall violations are only logged, not blocked.
        (GuaranteeStatus::Active, SandboxEnforcement::Permissive) => {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    enforcement = "permissive",
                    "sandbox enforcement=permissive: policy violations are logged but not \
                     blocked. Set enforcement=enforcing for production deployments."
                );
            }
        }
        _ => {}
    }
    if guarantees.seccomp == GuaranteeStatus::Degraded {
        tracing::warn!(
            enforcement = "permissive",
            "seccomp unavailable on this architecture; syscall sandbox degraded"
        );
    }
    if guarantees.egress == GuaranteeStatus::Degraded {
        tracing::warn!(
            enforcement = "permissive",
            egress = ?policy.egress,
            "egress filtering degraded on this architecture"
        );
    }
    warn_egress_policy(policy);
}

#[cfg(target_os = "linux")]
fn warn_egress_policy(policy: &SandboxPolicy) {
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
        // WHY: covers both `Allow` and any unrecognized future variant --
        // a separate `EgressPolicy::Allow => {}` arm is identical to this
        // one and clippy::match_same_arms rejects it. EgressPolicy is
        // `#[non_exhaustive]` (single-owned by taxis, ARCHITECTURE
        // #4846); nothing to log for either case since this function is
        // purely informational.
        _ => {}
    }
}

/// Apply sandbox restrictions to a [`std::process::Command`] via `pre_exec`.
///
/// Returns an error if enforcement is enforcing and a requested sandbox
/// guarantee cannot be provided. Logs guarantee status and degrades safely
/// when enforcement is permissive.
///
/// # Errors
///
/// Returns `Err` when `enforcement == Enforcing` and a guarantee (Landlock,
/// seccomp, or egress filtering) is unavailable on the running kernel or
/// platform.
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
        tracing::warn!("sandbox disabled: tool execution runs without any restrictions");
        return Ok(());
    }

    let guarantees = probe_guarantees(&policy);
    tracing::info!(
        landlock = %guarantees.landlock,
        seccomp = %guarantees.seccomp,
        egress = %guarantees.egress,
        enforcement = ?policy.enforcement,
        "sandbox guarantees"
    );

    if policy.enforcement == SandboxEnforcement::Enforcing {
        if guarantees.landlock == GuaranteeStatus::Unavailable {
            return Err(std::io::Error::other(
                "Landlock not available on this kernel (ABI probe returned none); \
                 tool execution blocked by enforcing sandbox. \
                 Set enforcement=permissive to run without sandboxing.",
            ));
        }
        if guarantees.seccomp == GuaranteeStatus::Unavailable {
            return Err(std::io::Error::other(
                "seccomp target architecture unsupported; \
                 tool execution blocked by enforcing sandbox. \
                 Set enforcement=permissive to run without syscall sandboxing.",
            ));
        }
        if guarantees.egress == GuaranteeStatus::Unavailable {
            // WHY: distinguish the two ways egress can be Unavailable so the
            // operator gets an actionable message instead of a generic one.
            // `register_domain_tools` already refuses to register tools for
            // this exact allowlist shape (SECURITY(#4997)); this is the
            // defense-in-depth check for callers that build a `SandboxPolicy`
            // directly (tests, or a future caller that bypasses
            // `SandboxConfig::validate`) rather than through that path.
            let reason = if policy.egress == EgressPolicy::Allowlist
                && !allowlist_is_loopback_only(&policy.egress_allowlist)
            {
                "egress_allowlist contains non-loopback entries; the child-process sandbox \
                 can only enforce loopback destinations without root privileges, so \
                 \"allowlist\" cannot honor this configuration"
            } else {
                "egress filtering unavailable on this platform"
            };
            return Err(std::io::Error::other(format!(
                "{reason}; tool execution blocked by enforcing sandbox. Restrict \
                 egress_allowlist to loopback entries, set enforcement=permissive, or set \
                 egress=allow to proceed."
            )));
        }
    }

    warn_sandbox_degradation(guarantees, &policy);

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
    if !policy.enabled {
        tracing::warn!("sandbox disabled: tool execution runs without any restrictions");
        return Ok(());
    }

    let guarantees = probe_guarantees(&policy);
    tracing::info!(
        landlock = %guarantees.landlock,
        seccomp = %guarantees.seccomp,
        egress = %guarantees.egress,
        enforcement = ?policy.enforcement,
        "sandbox guarantees"
    );

    if policy.enforcement == SandboxEnforcement::Enforcing {
        return Err(std::io::Error::other(
            "sandbox enforcement unavailable on non-Linux platforms; \
             tool execution blocked by enforcing sandbox. \
             Set enforcement=permissive to run without sandboxing.",
        ));
    }

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

// NOTE: sandbox tests exercise Linux-only syscalls (Landlock, seccomp) so
// the entire test module is gated to Linux. On other platforms every test
// inside would be cfg'd out, leaving the module-level #![expect(...)]
// attributes unfulfilled.
#[cfg(all(test, target_os = "linux"))]
#[path = "policy_tests/mod.rs"]
mod policy_tests;
