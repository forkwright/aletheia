//! Trust boundaries for KAIROS autonomous operation.
//!
//! Every tool call and dispatch action must pass through a trust check before
//! execution. The `kairos.toml` file in each project workspace declares what
//! operations are permitted for autonomous execution.
//!
//! WHY: Autonomous agents must not take actions the operator hasn't approved.
//! Without explicit trust boundaries, a misconfigured daemon could push to
//! wrong remotes, delete data, or restart services. The trust boundary makes
//! the permission model explicit and auditable.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Trust policy
// ---------------------------------------------------------------------------

/// Trust policy loaded from `kairos.toml` in a project workspace.
///
/// Declares which operations the KAIROS daemon is permitted to perform
/// autonomously. Operations not listed are denied by default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TrustPolicy {
    /// Whether KAIROS is enabled for this project at all.
    pub enabled: bool,
    /// Tools permitted for autonomous use.
    pub allowed_tools: HashSet<String>,
    /// Git operations permitted (push, branch, merge, tag).
    pub allowed_git_ops: HashSet<GitOp>,
    /// File paths that may be modified (glob patterns).
    pub writable_paths: Vec<String>,
    /// File paths that must never be modified.
    pub protected_paths: Vec<String>,
    /// Maximum cost per dispatch cycle (USD). 0 = unlimited.
    pub max_cost_per_cycle: f64,
    /// Maximum total turns per dispatch cycle.
    pub max_turns_per_cycle: u32,
    /// Allowed remote names for git push (default: origin only).
    pub allowed_remotes: HashSet<String>,
    /// Whether to require operator approval before merging PRs.
    pub require_merge_approval: bool,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_tools: HashSet::new(),
            allowed_git_ops: HashSet::new(),
            writable_paths: Vec::new(),
            protected_paths: vec![
                // WHY: secrets and credentials must never be autonomously modified.
                "**/.env".to_owned(),
                "**/secrets/**".to_owned(),
                "**/credentials/**".to_owned(),
            ],
            max_cost_per_cycle: 0.0,
            max_turns_per_cycle: 200,
            allowed_remotes: {
                let mut s = HashSet::new();
                s.insert("origin".to_owned());
                s
            },
            require_merge_approval: true,
        }
    }
}

/// Git operations that can be individually permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GitOp {
    /// Create branches.
    Branch,
    /// Push to permitted remotes.
    Push,
    /// Merge PRs (subject to `require_merge_approval`).
    Merge,
    /// Create tags.
    Tag,
    /// Commit changes.
    Commit,
}

// ---------------------------------------------------------------------------
// Trust check
// ---------------------------------------------------------------------------

/// Result of a trust boundary check.
#[derive(Debug, Clone)]
pub enum TrustCheck {
    /// Action is permitted.
    Allowed,
    /// Action is denied with reason.
    Denied {
        /// Why the action was denied.
        reason: String,
    },
}

impl TrustCheck {
    /// Returns true if the action is allowed.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Check if a tool invocation is permitted by the trust policy.
#[must_use]
pub fn check_tool(policy: &TrustPolicy, tool_name: &str) -> TrustCheck {
    if !policy.enabled {
        return TrustCheck::Denied {
            reason: "KAIROS not enabled for this project".to_owned(),
        };
    }

    if policy.allowed_tools.is_empty() {
        // WHY: empty allowed_tools = all tools permitted.
        // This is the "trust everything" mode for development.
        return TrustCheck::Allowed;
    }

    if policy.allowed_tools.contains(tool_name) {
        TrustCheck::Allowed
    } else {
        TrustCheck::Denied {
            reason: format!("tool '{tool_name}' not in allowed_tools"),
        }
    }
}

/// Check if a file path is writable under the trust policy.
#[must_use]
pub fn check_path(policy: &TrustPolicy, path: &str) -> TrustCheck {
    if !policy.enabled {
        return TrustCheck::Denied {
            reason: "KAIROS not enabled".to_owned(),
        };
    }

    // Protected paths are always denied.
    for pattern in &policy.protected_paths {
        if glob_match(pattern, path) {
            return TrustCheck::Denied {
                reason: format!("path '{path}' matches protected pattern '{pattern}'"),
            };
        }
    }

    // If writable_paths is empty, all non-protected paths are writable.
    if policy.writable_paths.is_empty() {
        return TrustCheck::Allowed;
    }

    for pattern in &policy.writable_paths {
        if glob_match(pattern, path) {
            return TrustCheck::Allowed;
        }
    }

    TrustCheck::Denied {
        reason: format!("path '{path}' not in writable_paths"),
    }
}

/// Check if a git operation is permitted.
#[must_use]
pub fn check_git_op(policy: &TrustPolicy, op: GitOp) -> TrustCheck {
    if !policy.enabled {
        return TrustCheck::Denied {
            reason: "KAIROS not enabled".to_owned(),
        };
    }

    if policy.allowed_git_ops.contains(&op) {
        TrustCheck::Allowed
    } else {
        TrustCheck::Denied {
            reason: format!("git operation {op:?} not permitted"),
        }
    }
}

/// Check if a git push to a specific remote is permitted.
#[must_use]
pub fn check_remote(policy: &TrustPolicy, remote: &str) -> TrustCheck {
    if !policy.enabled {
        return TrustCheck::Denied {
            reason: "KAIROS not enabled".to_owned(),
        };
    }

    if policy.allowed_remotes.contains(remote) {
        TrustCheck::Allowed
    } else {
        TrustCheck::Denied {
            reason: format!("remote '{remote}' not in allowed_remotes"),
        }
    }
}

/// Load a trust policy from `kairos.toml` in the given workspace.
///
/// Returns the default (disabled) policy if the file doesn't exist.
///
/// # Errors
///
/// Returns an error if the file exists but can't be parsed.
pub fn load_policy(workspace: &Path) -> crate::error::Result<TrustPolicy> {
    let path = workspace.join("kairos.toml");
    if !path.exists() {
        info!(workspace = %workspace.display(), "no kairos.toml — KAIROS disabled");
        return Ok(TrustPolicy::default());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::TaskFailedSnafu {
            task_id: "trust_policy",
            reason: format!("failed to read {}: {e}", path.display()),
        }
        .build()
    })?;

    let policy: TrustPolicy = toml::from_str(&content).map_err(|e| {
        crate::error::TaskFailedSnafu {
            task_id: "trust_policy",
            reason: format!("failed to parse {}: {e}", path.display()),
        }
        .build()
    })?;

    info!(
        workspace = %workspace.display(),
        enabled = policy.enabled,
        tools = policy.allowed_tools.len(),
        git_ops = policy.allowed_git_ops.len(),
        "trust policy loaded"
    );

    Ok(policy)
}

/// Simple glob matching for path patterns.
///
/// Supports `*` (any segment) and `**` (any number of segments).
fn glob_match(pattern: &str, path: &str) -> bool {
    // WHY: simple implementation for trust boundary checks.
    // Not a full glob engine — covers the patterns we actually use:
    // "**" (everything), "**/suffix" (anywhere ending with), "prefix/**" (under prefix),
    // "**/middle/**" (contains middle segment), "*.ext" (extension match).

    if pattern == "**" {
        return true;
    }

    // Handle "**/{middle}/**" — path contains the middle segment.
    if pattern.starts_with("**/") && pattern.ends_with("/**") {
        let middle = &pattern[3..pattern.len() - 3];
        return path.contains(&format!("/{middle}/"))
            || path.starts_with(&format!("{middle}/"))
            || path.ends_with(&format!("/{middle}"));
    }

    // Handle "**/suffix" — path ends with suffix or contains /suffix.
    if let Some(suffix) = pattern.strip_prefix("**/") {
        return path == suffix
            || path.ends_with(&format!("/{suffix}"))
            || path.contains(&format!("/{suffix}/"));
    }

    // Handle "prefix/**" — path starts with prefix.
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path == prefix
            || path.starts_with(&format!("{prefix}/"));
    }

    // Handle "*.ext" — single wildcard.
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return path.starts_with(parts[0]) && path.ends_with(parts[1]);
        }
    }

    pattern == path
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn dev_policy() -> TrustPolicy {
        TrustPolicy {
            enabled: true,
            allowed_tools: HashSet::new(), // empty = all allowed
            allowed_git_ops: [GitOp::Branch, GitOp::Commit, GitOp::Push]
                .into_iter()
                .collect(),
            writable_paths: vec!["crates/**".to_owned(), "docs/**".to_owned()],
            protected_paths: vec!["**/.env".to_owned(), "**/secrets/**".to_owned()],
            max_cost_per_cycle: 10.0,
            max_turns_per_cycle: 200,
            allowed_remotes: ["origin".to_owned()].into_iter().collect(),
            require_merge_approval: false,
        }
    }

    #[test]
    fn disabled_denies_everything() {
        let policy = TrustPolicy::default();
        assert!(!check_tool(&policy, "file_read").is_allowed());
        assert!(!check_path(&policy, "src/main.rs").is_allowed());
        assert!(!check_git_op(&policy, GitOp::Push).is_allowed());
    }

    #[test]
    fn enabled_with_empty_tools_allows_all_tools() {
        let policy = dev_policy();
        assert!(check_tool(&policy, "any_tool").is_allowed());
    }

    #[test]
    fn writable_paths_enforced() {
        let policy = dev_policy();
        assert!(check_path(&policy, "crates/koina/src/lib.rs").is_allowed());
        assert!(check_path(&policy, "docs/README.md").is_allowed());
        assert!(!check_path(&policy, "scripts/deploy.sh").is_allowed());
    }

    #[test]
    fn protected_paths_override_writable() {
        let policy = dev_policy();
        assert!(!check_path(&policy, "crates/.env").is_allowed());
        assert!(!check_path(&policy, "crates/secrets/api.key").is_allowed());
    }

    #[test]
    fn git_ops_enforced() {
        let policy = dev_policy();
        assert!(check_git_op(&policy, GitOp::Branch).is_allowed());
        assert!(check_git_op(&policy, GitOp::Push).is_allowed());
        assert!(!check_git_op(&policy, GitOp::Tag).is_allowed());
        assert!(!check_git_op(&policy, GitOp::Merge).is_allowed());
    }

    #[test]
    fn remote_enforcement() {
        let policy = dev_policy();
        assert!(check_remote(&policy, "origin").is_allowed());
        assert!(!check_remote(&policy, "upstream").is_allowed());
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match("**/.env", "crates/.env"));
        assert!(glob_match("**/.env", ".env"));
        assert!(glob_match("**/secrets/**", "foo/secrets/api.key"));
        assert!(glob_match("crates/**", "crates/koina/src/lib.rs"));
        assert!(!glob_match("crates/**", "scripts/test.sh"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.py"));
    }

    #[test]
    fn policy_toml_roundtrip() {
        let policy = dev_policy();
        let toml = toml::to_string_pretty(&policy).unwrap();
        let deserialized: TrustPolicy = toml::from_str(&toml).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.max_turns_per_cycle, 200);
    }

    #[test]
    fn load_missing_file_returns_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let policy = load_policy(dir.path()).unwrap();
        assert!(!policy.enabled);
    }

    #[test]
    fn load_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let toml_content = r#"
enabled = true
allowed_tools = ["file_read", "file_write"]
max_turns_per_cycle = 100
"#;
        std::fs::write(dir.path().join("kairos.toml"), toml_content).unwrap();
        let policy = load_policy(dir.path()).unwrap();
        assert!(policy.enabled);
        assert_eq!(policy.allowed_tools.len(), 2);
        assert_eq!(policy.max_turns_per_cycle, 100);
    }
}
