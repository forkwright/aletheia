//! Merge conflict resolution types and prompt construction.
//!
//! When a PR has merge conflicts, the steward uses a three-tier strategy:
//! 1. **API rebase** -- lightweight server-side merge (fast, free)
//! 2. **Structured rebase** -- local merge + file-type strategies (Cargo.lock
//!    regeneration, take-theirs for CHANGELOG/STATE/manifests, JSONL append)
//! 3. **LLM rebase agent** -- semantic conflict resolution for code conflicts
//!
//! The structured rebase tier avoids spawning expensive LLM sessions
//! ($0.80+, 3-5 min) for mechanical conflicts. The heuristic gate checks
//! whether ALL conflicting files have structural strategies before attempting
//! resolution -- if any file requires code-level conflict resolution, the entire
//! merge is aborted and the LLM agent handles it.
//!
//! This module contains the pure prompt construction and types. Actual
//! conflict resolution (which requires git subprocess calls and API access)
//! is implemented by backend trait implementations.

use std::path::Path;

/// Construct the prompt for the LLM rebase agent.
///
/// WHY: Separating prompt construction from execution allows testing
/// the prompt template without spawning agents.
#[must_use]
pub fn build_rebase_prompt(pr_number: u64, branch_name: &str, repo_dir: &Path) -> String {
    format!(
        "Resolve merge conflicts on PR #{number}.\n\
         \n\
         Branch: {branch}\n\
         Repo: {repo}\n\
         \n\
         ## Instructions\n\
         \n\
         1. `git fetch origin`\n\
         2. `git checkout {branch}`\n\
         3. `git rebase origin/main`\n\
         4. Resolve any conflicts preserving the PR's intent\n\
         5. Run the validation gate:\n\
            - `cargo fmt --all -- --check`\n\
            - `cargo clippy --workspace --all-targets -- -D warnings`\n\
            - `cargo test --workspace`\n\
         6. `git push origin {branch} --force-with-lease`\n",
        number = pr_number,
        branch = branch_name,
        repo = repo_dir.display(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::steward::types::{ConflictResult, ConflictStrategy};

    #[test]
    fn build_rebase_prompt_includes_all_details() {
        let prompt = build_rebase_prompt(42, "feat/branch-42", Path::new("/repo/aletheia"));

        assert!(prompt.contains("PR #42"));
        assert!(prompt.contains("feat/branch-42"));
        assert!(prompt.contains("/repo/aletheia"));
        assert!(prompt.contains("git rebase origin/main"));
        assert!(prompt.contains("--force-with-lease"));
        assert!(prompt.contains("cargo fmt"));
        assert!(prompt.contains("cargo test"));
    }

    #[test]
    fn conflict_result_resolved() {
        let result = ConflictResult {
            pr_number: 10,
            resolved: true,
            strategy: ConflictStrategy::ApiRebase,
            details: "success".to_string(),
        };

        assert!(result.resolved);
        assert!(matches!(result.strategy, ConflictStrategy::ApiRebase));
    }

    #[test]
    fn conflict_result_unresolved() {
        let result = ConflictResult {
            pr_number: 10,
            resolved: false,
            strategy: ConflictStrategy::LlmRebase,
            details: "failed".to_string(),
        };

        assert!(!result.resolved);
        assert!(matches!(result.strategy, ConflictStrategy::LlmRebase));
    }

    #[test]
    fn conflict_result_skipped_closed_pr() {
        let result = ConflictResult {
            pr_number: 311,
            resolved: false,
            strategy: ConflictStrategy::Skipped,
            details: "skipped: PR is CLOSED".to_string(),
        };

        assert!(!result.resolved);
        assert!(matches!(result.strategy, ConflictStrategy::Skipped));
        assert!(result.details.contains("CLOSED"));
    }

    #[test]
    fn conflict_result_skipped_merged_pr() {
        let result = ConflictResult {
            pr_number: 311,
            resolved: false,
            strategy: ConflictStrategy::Skipped,
            details: "skipped: PR is MERGED".to_string(),
        };

        assert!(!result.resolved);
        assert!(matches!(result.strategy, ConflictStrategy::Skipped));
        assert!(result.details.contains("MERGED"));
    }

    #[test]
    fn conflict_result_structured_rebase_resolved() {
        let result = ConflictResult {
            pr_number: 42,
            resolved: true,
            strategy: ConflictStrategy::StructuredRebase,
            details: "all 2 conflicts resolved structurally: Cargo.lock: cargo-lock, CHANGELOG.md: take-theirs".to_string(),
        };

        assert!(result.resolved);
        assert!(matches!(
            result.strategy,
            ConflictStrategy::StructuredRebase
        ));
        assert!(result.details.contains("cargo-lock"));
        assert!(result.details.contains("take-theirs"));
    }

    #[test]
    fn conflict_result_structured_rebase_needs_llm() {
        let result = ConflictResult {
            pr_number: 42,
            resolved: false,
            strategy: ConflictStrategy::StructuredRebase,
            details: "1 file(s) require LLM resolution: src/main.rs".to_string(),
        };

        assert!(!result.resolved);
        assert!(matches!(
            result.strategy,
            ConflictStrategy::StructuredRebase
        ));
        assert!(result.details.contains("LLM resolution"));
    }
}
