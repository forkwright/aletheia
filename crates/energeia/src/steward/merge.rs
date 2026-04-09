//! Merge decision logic for green PRs.
//!
//! Contains the pure decision functions for determining merge tier,
//! detecting public API changes, and making merge decisions. Async
//! functions that execute merges via GitHub API are not included here.

use std::collections::HashSet;

use super::types::{
    ClassifiedPr, MergeAction, MergeDecision, MergeMethod, MergeOptions, MergeTier, QaVerdictStatus,
};

/// Make a merge decision for a single classified PR.
///
/// Implements the tiered merge policy:
/// - Tier 1: QA PASS + CI green + single-module -> auto-merge
/// - Tier 2: QA PASS + CI green + multi-module -> merge + notify architect
/// - Tier 3: QA PARTIAL or hold flag -> hold for architect
/// - Tier 4: Touches public API surface -> hold for architect
/// - Tier 5: QA FAIL or CI failing -> block
#[must_use]
pub fn make_merge_decision(
    classified: &ClassifiedPr,
    opts: &MergeOptions,
    diff: Option<&str>,
) -> MergeDecision {
    let pr_number = classified.pr.number;

    // NOTE: Check if the PR is in a conflicting state.
    if let Some(ref mergeable) = classified.pr.mergeable
        && mergeable == "CONFLICTING"
    {
        return MergeDecision {
            pr_number,
            action: MergeAction::Blocked("merge conflict".to_string()),
            reason: "PR has merge conflicts that must be resolved".to_string(),
        };
    }

    // NOTE: If review is required for all PRs, route to review.
    if opts.require_review {
        return MergeDecision {
            pr_number,
            action: MergeAction::NeedsReview,
            reason: "review required for all merges (--require-review)".to_string(),
        };
    }

    // NOTE: Check blast radius compliance.
    if !classified.blast_radius_ok {
        return MergeDecision {
            pr_number,
            action: MergeAction::NeedsReview,
            reason: "files changed outside declared blast radius".to_string(),
        };
    }

    // NOTE: Classify into merge tier.
    let tier = classify_merge_tier(classified, diff);

    tracing::info!(
        pr_number,
        tier = %tier,
        qa_verdict = ?classified.qa_verdict,
        "merge tier classification"
    );

    match tier {
        MergeTier::Tier5Block => MergeDecision {
            pr_number,
            action: MergeAction::Blocked("QA FAIL -- tier 5 block".to_string()),
            reason: format!(
                "tier-5: QA verdict is {}",
                classified.qa_verdict.unwrap_or(QaVerdictStatus::Unknown)
            ),
        },
        MergeTier::Tier3Hold => MergeDecision {
            pr_number,
            action: MergeAction::HoldForArchitect("QA PARTIAL or hold flag".to_string()),
            reason: "tier-3: QA PARTIAL or hold flag -- held for architect review".to_string(),
        },
        MergeTier::Tier4PublicApi => MergeDecision {
            pr_number,
            action: MergeAction::HoldForArchitect("public API surface change".to_string()),
            reason: "tier-4: touches public API surface -- held for architect review".to_string(),
        },
        MergeTier::Tier1AutoMerge | MergeTier::Tier2MergeNotify => {
            // NOTE: Anti-patterns are a warning, not a blocker.
            // WHY: CI already enforces workspace-level clippy denials (expect_used, unwrap_used).
            // If CI passes, the code meets the project's actual standards.
            if !classified.merge_safe {
                tracing::warn!(
                    pr_number,
                    "anti-patterns detected in diff (CI passed, proceeding)"
                );
            }

            let method = if opts.squash {
                MergeMethod::Squash
            } else {
                MergeMethod::Merge
            };

            let reason = if tier == MergeTier::Tier2MergeNotify {
                "tier-2: green CI, multi-module -- auto-merge + architect notification".to_string()
            } else {
                "tier-1: green CI, single-module -- auto-merge".to_string()
            };

            MergeDecision {
                pr_number,
                action: MergeAction::Merge(method),
                reason,
            }
        }
    }
}

/// Classify a PR into a merge tier.
///
/// The tier determines the merge action:
/// - Tier 1: Auto-merge (single-module, QA PASS)
/// - Tier 2: Merge + notify (multi-module, QA PASS)
/// - Tier 3: Hold (QA PARTIAL or hold flag in body)
/// - Tier 4: Hold (public API surface changes)
/// - Tier 5: Block (QA FAIL)
#[must_use]
pub fn classify_merge_tier(classified: &ClassifiedPr, diff: Option<&str>) -> MergeTier {
    // NOTE: Tier 5 -- QA FAIL blocks merge.
    if classified.qa_verdict == Some(QaVerdictStatus::Fail) {
        return MergeTier::Tier5Block;
    }

    // NOTE: Tier 3 -- QA PARTIAL holds for architect.
    if classified.qa_verdict == Some(QaVerdictStatus::Partial) {
        return MergeTier::Tier3Hold;
    }

    // NOTE: Tier 3 -- hold flag in PR body.
    if has_hold_flag(classified.pr.body.as_deref()) {
        return MergeTier::Tier3Hold;
    }

    // NOTE: Tier 4 -- public API surface changes hold for architect.
    if let Some(diff_text) = diff
        && has_public_api_changes(diff_text)
    {
        return MergeTier::Tier4PublicApi;
    }

    // NOTE: Tier 1 vs Tier 2 -- based on blast radius scope.
    if is_multi_module(&classified.changed_files) {
        MergeTier::Tier2MergeNotify
    } else {
        MergeTier::Tier1AutoMerge
    }
}

/// Check if changed files span multiple crate modules.
///
/// WHY: Multi-module changes have higher risk and should notify the architect
/// even when auto-merging (tier 2).
#[must_use]
pub(crate) fn is_multi_module(changed_files: &[String]) -> bool {
    let mut crates: HashSet<&str> = HashSet::new();

    for path in changed_files {
        if let Some(rest) = path.strip_prefix("crates/")
            && let Some(crate_name) = rest.split('/').next()
            && !crate_name.is_empty()
        {
            crates.insert(crate_name);
        }
    }

    // WHY: A single crate or zero crates (root-level files only) is single-module.
    // Two or more crates is multi-module.
    crates.len() > 1
}

/// Check if a diff introduces public API surface changes.
///
/// WHY: Tier 4 -- changes to public API surface require architect
/// review regardless of QA verdict. Detects added `pub fn`, `pub struct`,
/// `pub enum`, `pub trait`, `pub type`, `pub mod`, `pub use` in Rust files.
#[must_use]
pub fn has_public_api_changes(diff: &str) -> bool {
    // WHY: Only check added lines (starting with `+`) in Rust files.
    let mut in_rust_file = false;

    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            in_rust_file = std::path::Path::new(path.trim())
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
            continue;
        }

        if !in_rust_file {
            continue;
        }

        // NOTE: Only check added lines (not context or removed).
        if let Some(added) = line.strip_prefix('+') {
            if line.starts_with("+++") {
                continue;
            }
            let trimmed = added.trim();

            // WHY: Detect public item declarations. These represent API surface
            // changes that need architect review.
            if trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("pub type ")
                || trimmed.starts_with("pub mod ")
                || trimmed.starts_with("pub use ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("pub const ")
                || trimmed.starts_with("pub static ")
            {
                return true;
            }
        }
    }

    false
}

/// Check if the PR body contains a hold flag.
///
/// WHY: Tier 3 -- PRs with a hold flag should be held for
/// architect review. Checks for `<!-- hold -->` marker or `[hold]` tag.
#[must_use]
pub fn has_hold_flag(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "<!-- hold -->" || trimmed.to_lowercase().contains("[hold]") {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::steward::types::{CiStatus, PullRequest};

    /// Helper to build a minimal `ClassifiedPr` for merge decision tests.
    pub(crate) fn make_classified(number: u64, blast_ok: bool, safe: bool) -> ClassifiedPr {
        ClassifiedPr {
            pr: PullRequest {
                number,
                title: format!("PR #{number}"),
                head_ref_name: None,
                head_sha: None,
                state: None,
                mergeable: Some("MERGEABLE".to_string()),
                body: None,
                updated_at: None,
                merged_at: None,
            },
            ci_status: CiStatus::Green,
            changed_files: vec!["crates/energeia/src/lib.rs".to_string()],
            prompt_number: Some(u32::try_from(number).expect("test PR number fits in u32")),
            blast_radius_ok: blast_ok,
            merge_safe: safe,
            has_gate_trailer: true,
            suppression_findings: Vec::new(),
            qa_verdict: Some(QaVerdictStatus::Pass),
        }
    }

    #[test]
    fn decision_tier1_auto_merge_when_safe() {
        let pr = make_classified(1, true, true);
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(
            decision.action,
            MergeAction::Merge(MergeMethod::Squash)
        ));
        assert!(decision.reason.contains("tier-1"));
    }

    #[test]
    fn decision_tier2_multi_module_merge() {
        let mut pr = make_classified(1, true, true);
        pr.changed_files = vec![
            "crates/energeia/src/lib.rs".to_string(),
            "crates/koina/src/engine.rs".to_string(),
        ];
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(
            decision.action,
            MergeAction::Merge(MergeMethod::Squash)
        ));
        assert!(decision.reason.contains("tier-2"));
    }

    #[test]
    fn decision_tier3_qa_partial_holds() {
        let mut pr = make_classified(1, true, true);
        pr.qa_verdict = Some(QaVerdictStatus::Partial);
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::HoldForArchitect(_)));
        assert!(decision.reason.contains("tier-3"));
    }

    #[test]
    fn decision_tier3_hold_flag() {
        let mut pr = make_classified(1, true, true);
        pr.pr.body = Some("Some description\n<!-- hold -->\nMore text".to_string());
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::HoldForArchitect(_)));
        assert!(decision.reason.contains("tier-3"));
    }

    #[test]
    fn decision_tier4_public_api_holds() {
        let pr = make_classified(1, true, true);
        let opts = MergeOptions::default();
        let diff = "+++ b/crates/energeia/src/lib.rs\n+pub fn new_api_function() {}\n";
        let decision = make_merge_decision(&pr, &opts, Some(diff));

        assert!(matches!(decision.action, MergeAction::HoldForArchitect(_)));
        assert!(decision.reason.contains("tier-4"));
    }

    #[test]
    fn decision_tier5_qa_fail_blocks() {
        let mut pr = make_classified(1, true, true);
        pr.qa_verdict = Some(QaVerdictStatus::Fail);
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::Blocked(_)));
        assert!(decision.reason.contains("tier-5"));
    }

    #[test]
    fn decision_needs_review_blast_radius() {
        let pr = make_classified(1, false, true);
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::NeedsReview));
        assert!(decision.reason.contains("blast radius"));
    }

    #[test]
    fn decision_merges_despite_anti_patterns_when_ci_green() {
        // WHY: anti-patterns are warnings, not blockers -- CI already validates
        let pr = make_classified(1, true, false);
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::Merge(_)));
    }

    #[test]
    fn decision_blocked_on_conflict() {
        let mut pr = make_classified(1, true, true);
        pr.pr.mergeable = Some("CONFLICTING".to_string());
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::Blocked(_)));
    }

    #[test]
    fn decision_require_review_flag() {
        let pr = make_classified(1, true, true);
        let opts = MergeOptions {
            require_review: true,
            ..Default::default()
        };
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::NeedsReview));
    }

    #[test]
    fn decision_non_squash_merge() {
        let pr = make_classified(1, true, true);
        let opts = MergeOptions {
            squash: false,
            ..Default::default()
        };
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(
            decision.action,
            MergeAction::Merge(MergeMethod::Merge)
        ));
    }

    #[test]
    fn decision_unknown_qa_verdict_auto_merges() {
        // WHY: When QA verdict is unknown (no QA data), default to auto-merge
        // for backward compatibility. The steward should not block all PRs
        // just because QA wasn't run.
        let mut pr = make_classified(1, true, true);
        pr.qa_verdict = None;
        let opts = MergeOptions::default();
        let decision = make_merge_decision(&pr, &opts, None);

        assert!(matches!(decision.action, MergeAction::Merge(_)));
    }

    #[test]
    fn is_multi_module_single_crate() {
        let files = vec![
            "crates/energeia/src/lib.rs".to_string(),
            "crates/energeia/src/steward/mod.rs".to_string(),
        ];
        assert!(!is_multi_module(&files));
    }

    #[test]
    fn is_multi_module_two_crates() {
        let files = vec![
            "crates/energeia/src/lib.rs".to_string(),
            "crates/koina/src/engine.rs".to_string(),
        ];
        assert!(is_multi_module(&files));
    }

    #[test]
    fn is_multi_module_root_files_only() {
        let files = vec!["Cargo.toml".to_string(), "src/main.rs".to_string()];
        assert!(!is_multi_module(&files));
    }

    #[test]
    fn has_public_api_changes_detects_pub_fn() {
        let diff = "+++ b/crates/energeia/src/lib.rs\n+pub fn new_function() {}\n";
        assert!(has_public_api_changes(diff));
    }

    #[test]
    fn has_public_api_changes_ignores_non_rust() {
        let diff = "+++ b/docs/README.md\n+pub fn new_function() {}\n";
        assert!(!has_public_api_changes(diff));
    }

    #[test]
    fn has_public_api_changes_ignores_removed_lines() {
        let diff = "+++ b/crates/energeia/src/lib.rs\n-pub fn old_function() {}\n";
        assert!(!has_public_api_changes(diff));
    }

    #[test]
    fn has_public_api_changes_detects_pub_struct() {
        let diff = "+++ b/crates/energeia/src/types.rs\n+pub struct NewType {\n";
        assert!(has_public_api_changes(diff));
    }

    #[test]
    fn has_public_api_changes_no_pub_items() {
        let diff = "+++ b/crates/energeia/src/lib.rs\n+fn private_function() {}\n";
        assert!(!has_public_api_changes(diff));
    }

    #[test]
    fn hold_flag_detected() {
        assert!(has_hold_flag(Some("Some text\n<!-- hold -->\nMore")));
        assert!(has_hold_flag(Some("description [hold] here")));
    }

    #[test]
    fn hold_flag_absent() {
        assert!(!has_hold_flag(Some("Normal PR description")));
        assert!(!has_hold_flag(None));
    }

    #[test]
    fn tier_precedence_fail_over_partial() {
        // WHY: QA FAIL should block even if hold flag is also present.
        let mut pr = make_classified(1, true, true);
        pr.qa_verdict = Some(QaVerdictStatus::Fail);
        pr.pr.body = Some("<!-- hold -->".to_string());
        let tier = classify_merge_tier(&pr, None);
        assert_eq!(tier, MergeTier::Tier5Block);
    }

    #[test]
    fn tier_precedence_partial_over_public_api() {
        // WHY: QA PARTIAL takes priority over public API detection.
        let mut pr = make_classified(1, true, true);
        pr.qa_verdict = Some(QaVerdictStatus::Partial);
        let diff = "+++ b/crates/energeia/src/lib.rs\n+pub fn new_api() {}\n";
        let tier = classify_merge_tier(&pr, Some(diff));
        assert_eq!(tier, MergeTier::Tier3Hold);
    }
}
