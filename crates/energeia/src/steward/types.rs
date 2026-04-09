//! Shared types for the steward CI management pipeline.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A pull request representation for steward classification.
///
/// WHY: Aletheia doesn't have a GitHub wrapper crate. This minimal struct
/// carries the fields the steward needs for classification and merge decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PullRequest {
    /// Pull request number (e.g., 42 for PR #42).
    pub number: u64,
    /// Title of the pull request.
    pub title: String,
    /// Name of the head branch.
    pub head_ref_name: Option<String>,
    /// SHA of the head commit.
    pub head_sha: Option<String>,
    /// State of the PR (e.g., "open", "closed").
    pub state: Option<String>,
    /// Mergeability status from GitHub API.
    pub mergeable: Option<String>,
    /// Body/description of the PR.
    pub body: Option<String>,
    /// ISO 8601 timestamp of last update.
    pub updated_at: Option<String>,
    /// ISO 8601 timestamp when merged, if applicable.
    pub merged_at: Option<String>,
}

/// Merge method for pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MergeMethod {
    /// Squash all commits into a single commit.
    Squash,
    /// Create a merge commit (traditional merge).
    Merge,
    /// Rebase commits onto the target branch.
    Rebase,
}

impl fmt::Display for MergeMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Squash => f.write_str("squash"),
            Self::Merge => f.write_str("merge"),
            Self::Rebase => f.write_str("rebase"),
        }
    }
}

/// A GitHub issue for observation matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Issue {
    /// Issue number.
    pub number: u64,
    /// Title of the issue.
    pub title: String,
    /// Body/description of the issue.
    pub body: Option<String>,
    /// Labels attached to the issue.
    pub labels: Vec<String>,
    /// State of the issue (e.g., "open", "closed").
    pub state: Option<String>,
    /// ISO 8601 timestamp when the issue was created.
    pub created_at: Option<String>,
}

/// A pull request with its CI classification and safety metadata.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ClassifiedPr {
    /// The underlying pull request.
    pub pr: PullRequest,

    /// Aggregate CI status across all checks.
    pub ci_status: CiStatus,

    /// Files changed by this PR (relative paths).
    pub changed_files: Vec<String>,

    /// Prompt number extracted from the PR title or branch name.
    pub prompt_number: Option<u32>,

    /// Whether all changed files fall within the declared blast radius.
    pub blast_radius_ok: bool,

    /// Whether the diff is free of known anti-patterns.
    pub merge_safe: bool,

    /// Whether a `Gate-Passed` trailer was found in any PR commit.
    /// WHY: Local gate enforcement replaces the verify-gate GitHub Action.
    /// When CI checks are absent (minutes exhausted), this field allows the
    /// steward to treat the PR as green without depending on GitHub CI.
    pub has_gate_trailer: bool,

    /// Suppression findings detected in the PR diff.
    /// WHY: Structural suppression detection distinguishes between `#[allow]`
    /// (discouraged) and `#[expect(..., reason = "...")]` (preferred).
    pub suppression_findings: Vec<SuppressionFinding>,

    /// QA verdict for this PR, if available.
    /// WHY: Tiered merge policy requires QA verdict to distinguish
    /// PASS (auto-merge eligible) from PARTIAL/FAIL (hold/block).
    /// Extracted from the PR body `<!-- qa-verdict: PASS -->` marker or DB.
    pub qa_verdict: Option<QaVerdictStatus>,
}

/// A suppression attribute found in the PR diff.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SuppressionFinding {
    /// The file where the suppression was found.
    pub file: String,

    /// The line number where the suppression was found (1-indexed).
    pub line: u32,

    /// The kind of suppression detected.
    pub kind: SuppressionKind,

    /// The lint name being suppressed (e.g., `dead_code`, `clippy::unwrap_used`).
    pub lint_name: Option<String>,

    /// The reason provided for the suppression, if any.
    pub reason: Option<String>,
}

/// Classification of suppression attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SuppressionKind {
    /// `#[allow(...)]` attribute -- discouraged, use `#[expect]` instead.
    Allow,

    /// `#[expect(...)]` attribute without a reason -- should include justification.
    ExpectNoReason,

    /// `#[expect(..., reason = "...")]` -- preferred form.
    ExpectWithReason,

    /// `#[cfg_attr(..., allow(...))]` -- conditional suppression.
    CfgAttrAllow,

    /// New line added to a lint-ignore file.
    LintIgnoreFile,

    /// `// lint-ignore` inline comment.
    LintIgnoreInline,

    /// `// SAFETY:` or `// INVARIANT:` comment added in a lint-fix PR context.
    /// WHY: LLM workers add these to bypass skip patterns without
    /// the code actually needing a safety/invariant justification.
    StructuredCommentBypass,
}

impl fmt::Display for SuppressionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => f.write_str("allow-attribute"),
            Self::ExpectNoReason => f.write_str("expect-no-reason"),
            Self::ExpectWithReason => f.write_str("expect-with-reason"),
            Self::CfgAttrAllow => f.write_str("cfg-attr-allow"),
            Self::LintIgnoreFile => f.write_str("lint-ignore-file"),
            Self::LintIgnoreInline => f.write_str("lint-ignore-inline"),
            Self::StructuredCommentBypass => f.write_str("structured-comment-bypass"),
        }
    }
}

/// QA verdict status for tiered merge policy.
///
/// WHY: Merge tiers are based on QA verdict. The steward needs this at
/// merge-decision time to distinguish auto-merge (PASS) from hold (PARTIAL)
/// and block (FAIL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QaVerdictStatus {
    /// All acceptance criteria passed.
    Pass,
    /// Some criteria passed, some failed.
    Partial,
    /// At least one criterion actively failed.
    Fail,
    /// Verdict could not be determined (no QA data available).
    Unknown,
}

impl fmt::Display for QaVerdictStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => f.write_str("PASS"),
            Self::Partial => f.write_str("PARTIAL"),
            Self::Fail => f.write_str("FAIL"),
            Self::Unknown => f.write_str("UNKNOWN"),
        }
    }
}

/// Merge tier classification for tiered merge policy.
///
/// WHY: 5 merge tiers based on QA verdict, CI status, blast radius scope,
/// and public API surface changes. The steward uses this to decide
/// auto-merge vs hold vs block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeTier {
    /// QA PASS + CI green + single-module blast radius -> auto-merge.
    Tier1AutoMerge,
    /// QA PASS + CI green + multi-module blast radius -> merge + notify architect.
    Tier2MergeNotify,
    /// QA PARTIAL or hold flag -> hold for architect review.
    Tier3Hold,
    /// Touches public API surface -> hold for architect review.
    Tier4PublicApi,
    /// QA FAIL or CI failing -> block.
    Tier5Block,
}

impl fmt::Display for MergeTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tier1AutoMerge => f.write_str("tier-1 (auto-merge)"),
            Self::Tier2MergeNotify => f.write_str("tier-2 (merge+notify)"),
            Self::Tier3Hold => f.write_str("tier-3 (hold)"),
            Self::Tier4PublicApi => f.write_str("tier-4 (public-api hold)"),
            Self::Tier5Block => f.write_str("tier-5 (block)"),
        }
    }
}

/// Aggregate CI status for a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CiStatus {
    /// All checks passed.
    Green,
    /// One or more checks failed.
    Red,
    /// One or more checks are still running.
    Pending,
    /// No checks found on the PR.
    Unknown,
}

impl fmt::Display for CiStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Green => f.write_str("green"),
            Self::Red => f.write_str("red"),
            Self::Pending => f.write_str("pending"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// Decision about what to do with a classified PR.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MergeDecision {
    /// PR number this decision applies to.
    pub pr_number: u64,
    /// The action to take (merge, hold, block, etc.).
    pub action: MergeAction,
    /// Human-readable explanation for the decision.
    pub reason: String,
}

/// Action the steward should take on a PR.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum MergeAction {
    /// Safe to merge with the given method.
    Merge(MergeMethod),
    /// Requires LLM review before merging.
    NeedsReview,
    /// Held for architect review (tiered merge policy).
    /// WHY: QA PARTIAL, hold flag, or public API changes require human review.
    HoldForArchitect(String),
    /// CI failed -- queue for automated fixing.
    NeedsFix,
    /// Cannot merge (e.g. merge conflict).
    Blocked(String),
    /// Do not touch (manual PR, external contributor, etc.).
    Skip(String),
}

impl fmt::Display for MergeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Merge(method) => write!(f, "merge ({method})"),
            Self::NeedsReview => f.write_str("needs-review"),
            Self::HoldForArchitect(reason) => write!(f, "hold-for-architect: {reason}"),
            Self::NeedsFix => f.write_str("needs-fix"),
            Self::Blocked(reason) => write!(f, "blocked: {reason}"),
            Self::Skip(reason) => write!(f, "skip: {reason}"),
        }
    }
}

/// Options controlling merge behavior.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MergeOptions {
    /// Report what would happen without making changes.
    pub dry_run: bool,

    /// Require LLM review for all merges (not just flagged ones).
    pub require_review: bool,

    /// Use squash merge (default true).
    pub squash: bool,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            require_review: false,
            squash: true,
        }
    }
}

/// Result of attempting to merge a single PR.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MergeResult {
    /// PR number.
    pub pr_number: u64,
    /// The decision that was made.
    pub decision: MergeDecision,
    /// Whether the merge (if attempted) succeeded.
    pub success: bool,
    /// Error message if the merge failed.
    pub error: Option<String>,
}

/// Aggregate result of a single steward pass.
///
/// NOTE: Does not include `health_entries` -- that is a kanon-specific
/// health dashboard concern, not part of the core steward logic.
#[derive(Debug)]
#[non_exhaustive]
pub struct StewardResult {
    /// All classified PRs from this pass.
    pub classified: Vec<ClassifiedPr>,

    /// Merge results for PRs that were attempted.
    pub merged: Vec<MergeResult>,

    /// PRs that need CI fixes (red status).
    pub needs_fix: Vec<ClassifiedPr>,

    /// PRs that are blocked with reasons.
    pub blocked: Vec<(u64, String)>,

    /// CI status of the main branch (from pre-flight check).
    /// WHY: Callers need visibility into whether PR fixes were skipped
    /// due to a broken base branch.
    pub main_ci_status: CiStatus,

    /// Whether a mechanical fix was attempted on the main branch.
    pub main_fix_attempted: bool,
}

/// A single CI check result.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct CheckRun {
    /// Name of the check (e.g. "build", "test").
    pub name: String,

    /// Status of the check (e.g. "completed", `in_progress`, "queued").
    /// WHY: GitHub API uses "status" not "state" for check-runs.
    #[serde(default, alias = "state")]
    pub status: String,

    /// Conclusion of the check (e.g. "success", "failure", "neutral").
    #[serde(default)]
    pub conclusion: Option<String>,
}

/// A single file entry from a PR files API response.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct PrFile {
    /// Path of the changed file.
    pub filename: String,
}

// ---------------------------------------------------------------------------
// Fix types
// ---------------------------------------------------------------------------

/// Result of a fix attempt on a single PR.
///
/// NOTE: Does not include `training_records` -- that is a kanon-specific
/// training concern, not part of the core steward logic.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FixResult {
    /// PR number that was fixed.
    pub pr_number: u64,
    /// Individual fixes that were applied.
    pub fixes_applied: Vec<FixApplied>,
    /// Whether CI might still be failing after fixes.
    pub still_failing: bool,
}

/// A single fix that was applied to a PR.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FixApplied {
    /// What kind of fix was applied (format, clippy, etc.).
    pub kind: FixKind,
    /// Files that were changed by this fix.
    pub files_changed: Vec<String>,
    /// Human-readable description of what was done.
    pub details: String,
}

/// Category of fix applied.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FixKind {
    /// `cargo fmt --all`
    Format,
    /// `cargo clippy --fix`
    ClippyFix,
    /// Removed unfulfilled `#[expect(...)]` attributes.
    ExpectRemoval,
    /// Trailing whitespace / missing final newline.
    Whitespace,
    /// Injected `Gate-Passed` trailer into HEAD commit.
    GateTrailer,
    /// Regenerated `Cargo.lock` via `cargo generate-lockfile`.
    LockfileRegen,
    /// Resolved training file conflicts via take-theirs.
    TrainingTakeTheirs,
    /// LLM agent applied a semantic fix.
    LlmFix,
}

impl fmt::Display for FixKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format => f.write_str("cargo-fmt"),
            Self::ClippyFix => f.write_str("clippy-fix"),
            Self::ExpectRemoval => f.write_str("unfulfilled-expect"),
            Self::Whitespace => f.write_str("whitespace"),
            Self::GateTrailer => f.write_str("gate-trailer"),
            Self::LockfileRegen => f.write_str("lockfile-regen"),
            Self::TrainingTakeTheirs => f.write_str("training-take-theirs"),
            Self::LlmFix => f.write_str("llm-fix"),
        }
    }
}

/// A CI check failure with log details.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CiFailure {
    /// Name of the failing check (e.g. "build", "test").
    pub check_name: String,
    /// Conclusion string (e.g. "failure", "timed_out").
    pub conclusion: String,
    /// Relevant portion of the CI log showing the failure.
    pub log_excerpt: String,
}

/// Classification of a CI failure by fixability.
///
/// WHY: Avoids dispatching expensive LLM agents ($0.50-2.00 each) for
/// failures that are deterministic or cannot benefit from reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CiFailureKind {
    /// Deterministic fix exists (fmt, clippy --fix, trailer injection).
    Mechanical,
    /// Requires LLM reasoning (type errors, test failures, logic bugs).
    Semantic,
}

/// Result of a merge conflict resolution attempt.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConflictResult {
    /// PR number.
    pub pr_number: u64,
    /// Whether the conflict was resolved.
    pub resolved: bool,
    /// Strategy used to resolve the conflict.
    pub strategy: ConflictStrategy,
    /// Human-readable details about the resolution.
    pub details: String,
}

/// Strategy used to resolve a merge conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictStrategy {
    /// Resolved via API rebase (fast, free).
    ApiRebase,
    /// Resolved by file-type-specific merge strategies (Rust, TOML, Markdown, JSON).
    FileTypeStrategy,
    /// Resolved by local structured rebase (merge + file-type strategies + push).
    StructuredRebase,
    /// Resolved by an LLM rebase agent.
    LlmRebase,
    /// Skipped because the PR was closed or merged before resolution.
    Skipped,
}

impl fmt::Display for ConflictStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiRebase => f.write_str("api-rebase"),
            Self::FileTypeStrategy => f.write_str("file-type-strategy"),
            Self::StructuredRebase => f.write_str("structured-rebase"),
            Self::LlmRebase => f.write_str("llm-rebase"),
            Self::Skipped => f.write_str("skipped"),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn ci_status_display() {
        assert_eq!(CiStatus::Green.to_string(), "green");
        assert_eq!(CiStatus::Red.to_string(), "red");
        assert_eq!(CiStatus::Pending.to_string(), "pending");
        assert_eq!(CiStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn merge_action_display() {
        assert_eq!(
            MergeAction::Merge(MergeMethod::Squash).to_string(),
            "merge (squash)"
        );
        assert_eq!(MergeAction::NeedsReview.to_string(), "needs-review");
        assert_eq!(
            MergeAction::HoldForArchitect("QA PARTIAL".to_string()).to_string(),
            "hold-for-architect: QA PARTIAL"
        );
        assert_eq!(MergeAction::NeedsFix.to_string(), "needs-fix");
        assert_eq!(
            MergeAction::Blocked("conflict".to_string()).to_string(),
            "blocked: conflict"
        );
        assert_eq!(
            MergeAction::Skip("manual".to_string()).to_string(),
            "skip: manual"
        );
    }

    #[test]
    fn qa_verdict_status_display() {
        assert_eq!(QaVerdictStatus::Pass.to_string(), "PASS");
        assert_eq!(QaVerdictStatus::Partial.to_string(), "PARTIAL");
        assert_eq!(QaVerdictStatus::Fail.to_string(), "FAIL");
        assert_eq!(QaVerdictStatus::Unknown.to_string(), "UNKNOWN");
    }

    #[test]
    fn merge_tier_display() {
        assert_eq!(MergeTier::Tier1AutoMerge.to_string(), "tier-1 (auto-merge)");
        assert_eq!(
            MergeTier::Tier2MergeNotify.to_string(),
            "tier-2 (merge+notify)"
        );
        assert_eq!(MergeTier::Tier3Hold.to_string(), "tier-3 (hold)");
        assert_eq!(
            MergeTier::Tier4PublicApi.to_string(),
            "tier-4 (public-api hold)"
        );
        assert_eq!(MergeTier::Tier5Block.to_string(), "tier-5 (block)");
    }

    #[test]
    fn merge_options_default() {
        let opts = MergeOptions::default();
        assert!(!opts.dry_run);
        assert!(!opts.require_review);
        assert!(opts.squash);
    }

    #[test]
    fn check_run_deserialize() {
        let json = r#"{"name": "build", "state": "completed", "conclusion": "success"}"#;
        let check: CheckRun =
            serde_json::from_str(json).expect("check run deserialization should succeed");
        assert_eq!(check.name, "build");
        assert_eq!(check.status, "completed");
        assert_eq!(check.conclusion, Some("success".to_string()));
    }

    #[test]
    fn check_run_deserialize_missing_conclusion() {
        let json = r#"{"name": "build", "state": "in_progress"}"#;
        let check: CheckRun =
            serde_json::from_str(json).expect("check run deserialization should succeed");
        assert_eq!(check.name, "build");
        assert!(check.conclusion.is_none());
    }

    #[test]
    fn fix_kind_display() {
        assert_eq!(FixKind::Format.to_string(), "cargo-fmt");
        assert_eq!(FixKind::ClippyFix.to_string(), "clippy-fix");
        assert_eq!(FixKind::ExpectRemoval.to_string(), "unfulfilled-expect");
        assert_eq!(FixKind::Whitespace.to_string(), "whitespace");
        assert_eq!(FixKind::GateTrailer.to_string(), "gate-trailer");
        assert_eq!(FixKind::LockfileRegen.to_string(), "lockfile-regen");
        assert_eq!(
            FixKind::TrainingTakeTheirs.to_string(),
            "training-take-theirs"
        );
        assert_eq!(FixKind::LlmFix.to_string(), "llm-fix");
    }

    #[test]
    fn conflict_strategy_display() {
        assert_eq!(ConflictStrategy::ApiRebase.to_string(), "api-rebase");
        assert_eq!(
            ConflictStrategy::FileTypeStrategy.to_string(),
            "file-type-strategy"
        );
        assert_eq!(
            ConflictStrategy::StructuredRebase.to_string(),
            "structured-rebase"
        );
        assert_eq!(ConflictStrategy::LlmRebase.to_string(), "llm-rebase");
        assert_eq!(ConflictStrategy::Skipped.to_string(), "skipped");
    }

    #[test]
    fn fix_result_empty_fixes() {
        let result = FixResult {
            pr_number: 42,
            fixes_applied: Vec::new(),
            still_failing: true,
        };
        assert!(result.fixes_applied.is_empty());
        assert!(result.still_failing);
    }

    #[test]
    fn fix_applied_with_files() {
        let fix = FixApplied {
            kind: FixKind::Format,
            files_changed: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            details: "cargo fmt --all".to_string(),
        };
        assert_eq!(fix.files_changed.len(), 2);
        assert_eq!(fix.kind, FixKind::Format);
    }

    #[test]
    fn ci_failure_fields() {
        let failure = CiFailure {
            check_name: "build".to_string(),
            conclusion: "failure".to_string(),
            log_excerpt: "error[E0308]: mismatched types".to_string(),
        };
        assert_eq!(failure.check_name, "build");
        assert!(failure.log_excerpt.contains("mismatched"));
    }

    #[test]
    fn ci_failure_kind_equality() {
        assert_eq!(CiFailureKind::Mechanical, CiFailureKind::Mechanical);
        assert_eq!(CiFailureKind::Semantic, CiFailureKind::Semantic);
        assert_ne!(CiFailureKind::Mechanical, CiFailureKind::Semantic);
    }

    #[test]
    fn suppression_kind_display() {
        assert_eq!(SuppressionKind::Allow.to_string(), "allow-attribute");
        assert_eq!(
            SuppressionKind::ExpectNoReason.to_string(),
            "expect-no-reason"
        );
        assert_eq!(
            SuppressionKind::ExpectWithReason.to_string(),
            "expect-with-reason"
        );
        assert_eq!(SuppressionKind::CfgAttrAllow.to_string(), "cfg-attr-allow");
        assert_eq!(
            SuppressionKind::LintIgnoreFile.to_string(),
            "lint-ignore-file"
        );
        assert_eq!(
            SuppressionKind::LintIgnoreInline.to_string(),
            "lint-ignore-inline"
        );
        assert_eq!(
            SuppressionKind::StructuredCommentBypass.to_string(),
            "structured-comment-bypass"
        );
    }

    #[test]
    fn suppression_finding_fields() {
        let finding = SuppressionFinding {
            file: "src/lib.rs".to_string(),
            line: 42,
            kind: SuppressionKind::CfgAttrAllow,
            lint_name: Some("dead_code".to_string()),
            reason: None,
        };
        assert_eq!(finding.file, "src/lib.rs");
        assert_eq!(finding.line, 42);
        assert_eq!(finding.kind, SuppressionKind::CfgAttrAllow);
        assert_eq!(finding.lint_name.as_deref(), Some("dead_code"));
        assert!(finding.reason.is_none());
    }

    #[test]
    fn merge_method_display() {
        assert_eq!(MergeMethod::Squash.to_string(), "squash");
        assert_eq!(MergeMethod::Merge.to_string(), "merge");
        assert_eq!(MergeMethod::Rebase.to_string(), "rebase");
    }

    #[test]
    fn pull_request_roundtrip() {
        let pr = PullRequest {
            number: 42,
            title: "feat: add steward".to_string(),
            head_ref_name: Some("feat/steward".to_string()),
            head_sha: Some("abc123".to_string()),
            state: Some("open".to_string()),
            mergeable: Some("MERGEABLE".to_string()),
            body: Some("description".to_string()),
            updated_at: None,
            merged_at: None,
        };
        let json = serde_json::to_string(&pr).unwrap();
        let deserialized: PullRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.number, 42);
        assert_eq!(deserialized.title, "feat: add steward");
    }

    #[test]
    fn issue_roundtrip() {
        let issue = Issue {
            number: 100,
            title: "Bug report".to_string(),
            body: Some("It crashes".to_string()),
            labels: vec!["bug".to_string()],
            state: Some("open".to_string()),
            created_at: None,
        };
        let json = serde_json::to_string(&issue).unwrap();
        let deserialized: Issue = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.number, 100);
        assert_eq!(deserialized.labels, vec!["bug"]);
    }
}
