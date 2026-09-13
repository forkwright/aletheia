//! Named policy: the destructive-git-operation guard.
//!
//! ARCHITECTURE(#7174): composes, as one referenceable declaration, the two
//! facts that together enforce every destructive git operation end-to-end,
//! so a future consumer (a new tool, a new dispatch pipeline, an audit) can
//! check "is this covered by the destructive-git policy" instead of
//! re-deriving the answer by reading `git_ops.rs` and `workspace.rs`
//! separately:
//!
//! 1. `git_ops` never registers a tool for `git commit`, `git push`,
//!    `git reset`, `git rebase`, or `git merge`; the one mutating tool it
//!    does register, `git_checkout`, never passes `--force`/`-f`.
//! 2. The only remaining path to a destructive git operation is the `exec`
//!    tool (`crate::builtins::workspace`'s `exec_def`), which declares
//!    `Reversibility::Irreversible`. `ApprovalRequirement::from` maps that
//!    to `Mandatory` at the single dispatch approval boundary
//!    (`nous::execute::dispatch`) -- fail-closed when no approval gate is
//!    wired or the approver is unreachable.
//!
//! This is a declaration, not new enforcement: both halves already run
//! today (see `destructive_git_policy_composes_registered_refusals_with_exec_approval`
//! below, which checks the declaration against the actual registry rather
//! than merely asserting the constant equals itself). `git_ops.rs` checks
//! the same two facts again at registration time (`register_with_sandbox`),
//! against the live registry it just populated, so drift panics at startup
//! instead of silently invalidating the declaration below.
//!
//! ## Open decision this policy does not cover
//!
//! This policy does not add a "permanently blocked, no override" tier.
//! Force push, `git remote --delete`, and `--mirror` all run only through
//! `exec` (this policy's `fallback_tool`) today, and `exec`'s `Mandatory`
//! approval requirement remains *approvable* -- an operator can grant it
//! for any of those operations. Whether organon should grow a non-approvable
//! tier for that operation class -- one the approval boundary cannot wave
//! through -- is an open operator decision aletheia#7174 records but does
//! not make; nothing below should be read as already covering it.
use crate::types::{ApprovalRequirement, Reversibility};

/// Declaration composed by [`DESTRUCTIVE_GIT_OPERATION_POLICY`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DestructiveGitOperationPolicy {
    /// Stable identifier other code, audits, or docs can reference instead
    /// of re-deriving the policy from this module's doc comment.
    pub(crate) name: &'static str,
    /// Git subcommands `git_ops` never registers a tool for (see
    /// `git_ops.rs`'s module-level "Scope decisions" doc).
    pub(crate) refused_git_subcommands: &'static [&'static str],
    /// Flags `git_checkout` refuses regardless of caller input.
    pub(crate) checkout_denied_flags: &'static [&'static str],
    /// Name of the one remaining tool through which a destructive git
    /// operation can run.
    pub(crate) fallback_tool: &'static str,
    /// Reversibility declared on `fallback_tool`; the dispatch approval
    /// boundary maps this to [`ApprovalRequirement::Mandatory`] via
    /// `ApprovalRequirement::from`.
    pub(crate) fallback_reversibility: Reversibility,
}

impl DestructiveGitOperationPolicy {
    /// The approval requirement this policy composes to at the dispatch
    /// boundary (`nous::execute::dispatch`), derived the same way dispatch
    /// derives it: `ApprovalRequirement::from(Reversibility)`.
    #[must_use]
    pub(crate) fn fallback_approval(&self) -> ApprovalRequirement {
        ApprovalRequirement::from(self.fallback_reversibility)
    }
}

pub(crate) const DESTRUCTIVE_GIT_OPERATION_POLICY: DestructiveGitOperationPolicy =
    DestructiveGitOperationPolicy {
        name: "destructive-git-operation",
        refused_git_subcommands: &["commit", "push", "reset", "rebase", "merge"],
        checkout_denied_flags: &["--force", "-f"],
        fallback_tool: "exec",
        fallback_reversibility: Reversibility::Irreversible,
    };

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use koina::id::ToolName;

    use super::*;

    /// WHY: proves `DESTRUCTIVE_GIT_OPERATION_POLICY` is a checked
    /// declaration, not prose that can silently drift -- its refusal list
    /// is checked against the tools actually (not) registered, and its
    /// `fallback_reversibility` is checked against the real `exec` tool
    /// definition and the real `ApprovalRequirement::from` mapping the
    /// dispatch boundary uses, rather than the test merely restating the
    /// constant's own fields back at itself.
    #[test]
    fn destructive_git_policy_composes_registered_refusals_with_exec_approval() {
        let policy = DESTRUCTIVE_GIT_OPERATION_POLICY;
        assert_eq!(policy.name, "destructive-git-operation");

        let mut reg = crate::registry::ToolRegistry::new();
        crate::builtins::git_ops::register(&mut reg).expect("register git_ops");
        crate::builtins::workspace::register(&mut reg, crate::sandbox::SandboxConfig::default())
            .expect("register workspace");

        for subcommand in policy.refused_git_subcommands {
            let candidate = format!("git_{subcommand}");
            let tn = ToolName::new(&candidate).expect("valid tool name");
            assert!(
                reg.get_def(&tn).is_none(),
                "policy declares {subcommand} refused, but {candidate} is registered"
            );
        }

        let fallback = ToolName::new(policy.fallback_tool).expect("valid tool name");
        let def = reg
            .get_def(&fallback)
            .expect("policy's fallback_tool must be registered");
        assert_eq!(
            def.reversibility, policy.fallback_reversibility,
            "policy's fallback_reversibility must match the fallback tool's actual declaration"
        );
        assert_eq!(policy.fallback_approval(), ApprovalRequirement::Mandatory);
    }
}
