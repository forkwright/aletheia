//! Steward CI management pipeline: classify, merge, fix, and manage pull requests.
//!
//! The steward monitors open PRs and performs:
//! 1. Classification by CI status (green/red/pending)
//! 2. Tiered merge decisions (5 tiers from auto-merge to block)
//! 3. Overlap-aware merge ordering
//! 4. Conflict resolution (API rebase -> structured -> LLM)
//! 5. CI failure repair (mechanical -> LLM)
//!
//! # Architecture
//!
//! This module contains the pure decision logic. External interactions
//! (GitHub API, git subprocess calls) are abstracted behind backend traits
//! that callers provide.

/// PR classification by CI status, blast radius, and suppression detection.
pub mod classify;
/// Merge conflict resolution types and prompt construction.
pub mod conflict;
/// CI failure diagnosis and repair pipeline.
pub mod fix;
/// Green PR merge logic with tiered auto-merge policy.
pub mod merge;
/// File overlap detection and merge ordering.
pub mod overlap;
/// Steward service: configurable polling loop.
pub mod service;
/// Shared types for steward operations.
pub mod types;

pub use classify::{
    apply_gate_trailer_override, determine_ci_status, extract_prompt_number,
    extract_qa_verdict_from_body, parse_suppressions,
};
pub use conflict::build_rebase_prompt;
pub use fix::{classify_failure, fix_kind_category};
pub use merge::{classify_merge_tier, has_hold_flag, has_public_api_changes, make_merge_decision};
pub use overlap::{compute_merge_order, file_overlap};
pub use service::{run, run_once, StewardConfig};
pub use types::{
    CheckRun, CiFailure, CiFailureKind, CiStatus, ClassifiedPr, ConflictResult,
    ConflictStrategy, FixApplied, FixKind, FixResult, Issue, MergeAction, MergeDecision,
    MergeMethod, MergeOptions, MergeResult, MergeTier, PrFile, PullRequest, QaVerdictStatus,
    StewardResult, SuppressionFinding, SuppressionKind,
};
