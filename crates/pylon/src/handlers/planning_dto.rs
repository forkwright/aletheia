// WHY: wire DTO
//! Planning verification endpoint wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Verification status for a single requirement.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerificationStatus {
    /// Requirement fully demonstrated.
    Verified,
    /// Some but not all criteria demonstrated.
    PartiallyVerified,
    /// No verification evidence found.
    Unverified,
    /// Verification attempted but explicitly failed.
    Failed,
}

/// Priority tier for a requirement.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequirementPriority {
    /// Blocking: must be verified before release.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
    /// Low or nice-to-have.
    P3,
}

/// A piece of evidence demonstrating a requirement.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct VerificationEvidence {
    /// Human-readable label for this evidence.
    pub(crate) label: String,
    /// Path or reference to the evidence artifact.
    pub(crate) artifact: String,
}

/// A criterion not yet satisfied.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct VerificationGap {
    /// Description of the missing criteria.
    pub(crate) missing_criteria: String,
    /// Suggested action to close the gap.
    pub(crate) suggested_action: String,
}

/// Verification result for a single requirement.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RequirementVerification {
    /// Requirement identifier.
    pub(crate) id: String,
    /// Human-readable title.
    pub(crate) title: String,
    /// Version tier.
    pub(crate) tier: String,
    /// Priority level.
    pub(crate) priority: RequirementPriority,
    /// Current verification status.
    pub(crate) status: VerificationStatus,
    /// Coverage percentage 0-100.
    pub(crate) coverage_pct: u8,
    /// Evidence supporting this requirement.
    pub(crate) evidence: Vec<VerificationEvidence>,
    /// Gaps remaining for this requirement.
    pub(crate) gaps: Vec<VerificationGap>,
}

/// Full verification result for a project, matching the desktop
/// `VerificationResult` deserialization target.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct VerificationResult {
    /// Project identifier.
    pub(crate) project_id: String,
    /// Per-requirement verification results.
    pub(crate) requirements: Vec<RequirementVerification>,
    /// ISO 8601 timestamp of the last verification run.
    pub(crate) last_verified_at: String,
}

/// Request body accepted by `POST .../verification/refresh`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub(crate) struct RefreshRequest {
    /// Optional caller-supplied criterion evaluations. If omitted, pylon
    /// derives criterion inputs from the persisted dianoia phase and plan state.
    #[serde(default)]
    pub(crate) criteria: Vec<CriterionEvaluation>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct CriterionEvaluation {
    /// Optional phase identifier. Required when a project has multiple phases.
    pub(crate) phase_id: Option<String>,
    /// Criterion text to evaluate.
    pub(crate) criterion: String,
    /// Verification status for the criterion.
    pub(crate) status: CriterionStatusInput,
    /// Evidence supporting this criterion evaluation.
    #[serde(default)]
    pub(crate) evidence: Vec<EvidenceInput>,
    /// Human-readable evaluation detail.
    pub(crate) detail: String,
    /// Suggested fix when the criterion is not met.
    pub(crate) proposed_fix: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CriterionStatusInput {
    /// Criterion is satisfied.
    Met,
    /// Criterion is partly satisfied.
    PartiallyMet,
    /// Criterion is not satisfied.
    NotMet,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct EvidenceInput {
    /// Evidence type, such as `test` or `file`.
    pub(crate) kind: String,
    /// Evidence content or artifact reference.
    pub(crate) content: String,
}
