//! Landlock + seccomp + network namespace sandbox for tool execution.
//!
//! Restricts filesystem access via Landlock LSM, blocks dangerous
//! syscalls via seccomp BPF filters, and isolates network access via
//! Linux network namespaces. Applied in child processes after fork,
//! before exec.

use std::path::{Path, PathBuf};

use koina::system::{Environment, RealSystem};
use serde::{Deserialize, Serialize};

/// Sandbox enforcement level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SandboxEnforcement {
    /// Sandbox violations cause the operation to fail.
    Enforcing,
    /// Sandbox violations are logged but allowed to proceed.
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
pub(crate) fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with('~')
        && let Some(home) = RealSystem.var("HOME")
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
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcement,
    /// Additional filesystem root granted read access, beyond the
    /// workspace and each agent's own `allowed_roots`.
    ///
    /// SECURITY(#5064): empty by default. Read authority is derived from
    /// the resolved agent workspace and its `allowed_roots` (see
    /// [`build_policy`](Self::build_policy)) -- not a blanket grant. Set
    /// this to `~` (or any path) to explicitly opt an agent's sandboxed
    /// subprocesses into reading beyond their own roots; `~` expands to
    /// the HOME environment variable at policy-build time. This is an
    /// explicit widening an operator chooses, not an implicit default, and
    /// [`validate`](Self::validate) flags it when set.
    pub allowed_root: PathBuf,
    /// Additional filesystem paths granted read access.
    pub extra_read_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted read+write access.
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
    /// Maximum number of processes (`RLIMIT_NPROC`) for exec child processes.
    ///
    /// WHY: `RLIMIT_NPROC` counts ALL processes for the user, not just sandbox
    /// children. The previous default of 64 caused EAGAIN failures on systems
    /// running dispatch agents or other background processes. Default: 256.
    pub nproc_limit: u32,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            // SECURITY(#5064): no implicit HOME-wide read grant. See the
            // field doc for how to opt back in explicitly.
            allowed_root: PathBuf::new(),
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
            egress: EgressPolicy::default(),
            egress_allowlist: Vec::new(),
            nproc_limit: 256,
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
    /// Filesystem paths granted read access.
    pub read_paths: Vec<PathBuf>,
    /// Filesystem paths granted read+write access.
    pub write_paths: Vec<PathBuf>,
    /// Filesystem paths granted execute access.
    pub exec_paths: Vec<PathBuf>,
    /// Enforcement level.
    pub enforcement: SandboxEnforcement,
    /// Network egress policy.
    pub egress: EgressPolicy,
    /// Allowed destinations when `egress == Allowlist`.
    pub egress_allowlist: Vec<String>,
}

/// A configuration combination that makes [`SandboxConfig`]'s stated
/// guarantees misleading, found by [`SandboxConfig::validate`].
#[derive(Debug, Clone)]
pub struct SandboxConfigIssue {
    /// Human-readable description, suitable for a startup log line.
    pub message: String,
    /// Whether this issue represents a guarantee the sandbox cannot
    /// actually provide under `enforcement = "enforcing"` (as opposed to
    /// one that only degrades safety under `enforcement = "permissive"`,
    /// where "logged but not blocked" is the documented behavior).
    pub broken_under_enforcing: bool,
}

impl SandboxConfig {
    /// Check for configuration combinations that make this config's stated
    /// guarantees misleading, independent of any single tool invocation.
    ///
    /// SECURITY(#5081, #5064, #5232, #4997): `enabled = true` does not by
    /// itself mean "safe" -- a permissive enforcement, an explicit broad
    /// `allowed_root`, or an `egress = "allowlist"` policy the
    /// child-process network-namespace path cannot enforce beyond loopback
    /// are all states an operator could previously only discover by
    /// reading source or scattered per-invocation log lines. Call this once
    /// at startup (see `register_domain_tools`) and log every issue; an
    /// issue with `broken_under_enforcing` set refuses registration outright
    /// under `enforcement = "enforcing"` rather than only logging it.
    #[must_use]
    pub fn validate(&self) -> Vec<SandboxConfigIssue> {
        let mut issues = Vec::new();
        if !self.enabled {
            // WHY: an explicitly disabled sandbox has no guarantees to be
            // misleading about; operators who disabled it know what that means.
            return issues;
        }

        let permissive = self.enforcement == SandboxEnforcement::Permissive;
        if permissive {
            issues.push(SandboxConfigIssue {
                message: "sandbox.enforcement = \"permissive\": filesystem, syscall, and \
                          egress violations are logged but NOT blocked"
                    .to_owned(),
                broken_under_enforcing: false,
            });
        }

        if !self.allowed_root.as_os_str().is_empty() {
            issues.push(SandboxConfigIssue {
                message: format!(
                    "sandbox.allowedRoot = \"{}\" grants every sandboxed subprocess read \
                     access beyond its own workspace and allowed_roots{}",
                    self.allowed_root.display(),
                    if permissive {
                        " (enforcement=permissive means this grant, like everything else, is \
                         logged but not actually enforced)"
                    } else {
                        ""
                    }
                ),
                broken_under_enforcing: false,
            });
        }

        if self.egress == EgressPolicy::Allowlist
            && !super::policy::allowlist_is_loopback_only(&self.egress_allowlist)
        {
            issues.push(SandboxConfigIssue {
                message: format!(
                    "sandbox.egress = \"allowlist\" with non-loopback entries: the \
                     child-process network-namespace path can only enforce loopback \
                     destinations without root privileges; non-loopback entries can never be \
                     reached, not selectively denied like a real allowlist (in-process tools \
                     -- http_request, web_fetch, web_search -- enforce the full allowlist via \
                     their own egress checkpoint, independent of this path).{}",
                    if permissive {
                        " enforcement=permissive logs this and continues, running \
                          subprocess-sandboxed tools as if egress = \"deny\""
                    } else {
                        " enforcement=enforcing refuses to register tools rather than start up \
                          on a guarantee it cannot keep"
                    }
                ),
                broken_under_enforcing: true,
            });
        }

        // SECURITY(#5081): `egress = "allow"` is the compiled default (see
        // `Default` below) -- an operator who never touched `[sandbox]` at
        // all is running it. Every other guarantee gap above surfaces on
        // startup; a wide-open subprocess network was the one silently
        // unstated posture. This is informational rather than
        // `broken_under_enforcing`: nothing is promised and then not kept --
        // egress simply was never restricted.
        if self.egress == EgressPolicy::Allow {
            issues.push(SandboxConfigIssue {
                message: "sandbox.egress = \"allow\" (the default): sandboxed subprocesses have \
                          full outbound network access, unrestricted by CIDR or destination"
                    .to_owned(),
                broken_under_enforcing: false,
            });
        }

        issues
    }

    /// Create a disabled sandbox config (no restrictions applied).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "sandbox bypass for test and no-restriction configurations"
        )
    )]
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Build a resolved [`SandboxPolicy`] from this config for the given workspace.
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
            // WHY: Grant only /proc/self, not all of /proc. The sandboxed child
            // may need its own process metadata (e.g. /proc/self/exe, status),
            // but reading /proc/<parent-pid>/environ or cmdline would bypass
            // the environment-scrubbing boundary and leak parent secrets.
            PathBuf::from("/proc/self"),
            PathBuf::from("/dev"),
        ];

        // WHY: Use RealSystem::temp_dir() instead of hardcoded /tmp to support
        // systems where the temp directory differs (e.g. /var/folders on macOS,
        // or a custom TMPDIR).
        let mut write_paths = vec![RealSystem.temp_dir()];

        // WHY: System binary dirs are always executable. workspace and
        // allowed_roots are also added so agents can execute scripts they own
        // or that live in shared data directories.
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

        // SECURITY(#5064): allowed_root is empty by default (no implicit
        // HOME grant); only apply it when an operator has explicitly set
        // one. Expand tilde so config files can use `~` portably.
        if !self.allowed_root.as_os_str().is_empty() {
            let expanded_allowed_root = expand_tilde(&self.allowed_root);
            if !read_paths.contains(&expanded_allowed_root) {
                read_paths.push(expanded_allowed_root);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_replaces_prefix_with_home() {
        // WHY: Read current HOME rather than setting it to avoid env mutation in concurrent tests.
        if let Ok(home) = std::env::var("HOME") {
            let path = Path::new("~/projects");
            let expanded = expand_tilde(path);
            assert_eq!(expanded, PathBuf::from(format!("{home}/projects")));
        }
    }

    #[test]
    fn expand_tilde_leaves_absolute_path_unchanged() {
        let path = Path::new("/usr/local/bin");
        let expanded = expand_tilde(path);
        assert_eq!(expanded, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn expand_tilde_leaves_relative_path_unchanged() {
        let path = Path::new("relative/path");
        let expanded = expand_tilde(path);
        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    #[test]
    fn sandbox_config_disabled_sets_enabled_false() {
        let config = SandboxConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn build_policy_when_disabled_returns_disabled_policy() {
        let config = SandboxConfig::disabled();
        let policy = config.build_policy(Path::new("/workspace"), &[]);
        assert!(!policy.enabled);
        assert!(policy.read_paths.is_empty());
        assert!(policy.write_paths.is_empty());
        assert!(policy.exec_paths.is_empty());
        assert_eq!(policy.egress, EgressPolicy::Allow);
    }

    #[test]
    fn build_policy_includes_workspace_in_write_paths() {
        let config = SandboxConfig {
            enabled: true,
            ..SandboxConfig::default()
        };
        let workspace = Path::new("/tmp/workspace");
        let policy = config.build_policy(workspace, &[]);
        assert!(
            policy.write_paths.contains(&workspace.to_path_buf()),
            "workspace must be writable"
        );
    }

    #[test]
    fn build_policy_includes_extra_read_paths() {
        let config = SandboxConfig {
            enabled: true,
            extra_read_paths: vec![PathBuf::from("/data/shared")],
            ..SandboxConfig::default()
        };
        let policy = config.build_policy(Path::new("/workspace"), &[]);
        assert!(
            policy.read_paths.contains(&PathBuf::from("/data/shared")),
            "extra read path must be in policy"
        );
    }

    #[test]
    fn egress_policy_default_is_allow() {
        assert_eq!(EgressPolicy::default(), EgressPolicy::Allow);
    }

    #[test]
    fn nproc_limit_default_is_256() {
        let config = SandboxConfig::default();
        assert_eq!(
            config.nproc_limit, 256,
            "nproc_limit should default to 256 to accommodate background processes"
        );
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test context — parse failure is a test bug"
    )]
    fn nproc_limit_configurable_via_serde() {
        let json = r#"{"enabled":true,"nprocLimit":512}"#;
        let config: SandboxConfig = serde_json::from_str(json).expect("parse");
        assert_eq!(config.nproc_limit, 512);
    }

    /// SECURITY(#5081): before this fix, `validate()` flagged permissive
    /// enforcement, a broad `allowed_root`, and a non-loopback allowlist, but
    /// said nothing about `egress = "allow"` -- the actual compiled default,
    /// and the widest of the four. An operator relying on `validate()`'s
    /// output to see every guarantee gap saw three of four.
    #[test]
    fn validate_flags_default_open_egress() {
        let config = SandboxConfig::default();
        assert_eq!(
            config.egress,
            EgressPolicy::Allow,
            "must be testing the default"
        );

        let issues = config.validate();
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("egress") && issue.message.contains("allow")),
            "validate() must flag open egress on the default config, got: {issues:?}"
        );
    }

    /// Egress denial and a loopback-only allowlist are both genuine
    /// restrictions; validate() must not flag either as open egress.
    #[test]
    fn validate_does_not_flag_restricted_egress() {
        for (egress, allowlist) in [
            (EgressPolicy::Deny, Vec::new()),
            (EgressPolicy::Allowlist, vec!["127.0.0.1".to_owned()]),
        ] {
            let config = SandboxConfig {
                egress,
                egress_allowlist: allowlist,
                ..SandboxConfig::default()
            };
            let issues = config.validate();
            assert!(
                !issues
                    .iter()
                    .any(|issue| issue.message.contains("egress = \"allow\"")),
                "validate() must not flag {egress:?} as open egress, got: {issues:?}"
            );
        }
    }
}
