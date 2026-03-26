//! Landlock + seccomp + network namespace sandbox for tool execution.
//!
//! Restricts filesystem access via Landlock LSM, blocks dangerous
//! syscalls via seccomp BPF filters, and isolates network access via
//! Linux network namespaces. Applied in child processes after fork,
//! before exec.

use std::path::{Path, PathBuf};

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
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcement,
    /// Default filesystem root granted read access.
    ///
    /// Defaults to `~` which expands to the HOME environment variable at
    /// policy-build time. Operators can set this to a stricter path to
    /// prevent agents from reading files outside a specific directory.
    ///
    /// WHY: without a home-directory default, agents cannot read user files
    /// (dotfiles, project repos, etc.) even in permissive mode: closes #1823.
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
    /// Closes #1984.
    pub nproc_limit: u32,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcement::Permissive,
            allowed_root: PathBuf::from("~"),
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

impl SandboxConfig {
    /// Create a disabled sandbox config (no restrictions applied).
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Build a resolved [`SandboxPolicy`] from this config for the given workspace.
    #[must_use]
    pub(crate) fn build_policy(
        &self,
        workspace: &Path,
        allowed_roots: &[PathBuf],
    ) -> SandboxPolicy {
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

        // WHY: Use std::env::temp_dir() instead of hardcoded /tmp to support
        // systems where the temp directory differs (e.g. /var/folders on macOS,
        // or a custom TMPDIR). Closes #1697.
        let mut write_paths = vec![std::env::temp_dir()];

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

        // WHY: allowed_root is the operator-configured default read root (defaults
        // to HOME). Expand tilde so config files can use `~` portably: closes #1823.
        let expanded_allowed_root = expand_tilde(&self.allowed_root);
        if !read_paths.contains(&expanded_allowed_root) {
            read_paths.push(expanded_allowed_root);
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
}
