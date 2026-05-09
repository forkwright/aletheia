//! Planning project verification endpoints.
//!
//! Serves the verification state consumed by the desktop `VerificationView`
//! component. Results are computed through dianoia's verification engine over
//! the persisted planning workspace for the requested project.

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use dianoia::phase::{Phase, PhaseState};
use dianoia::plan::PlanState;
use dianoia::verify::{
    CriterionInput, CriterionResult, CriterionStatus as DianoiaCriterionStatus, Evidence,
    VerificationGap as DianoiaVerificationGap,
};
use dianoia::workspace::ProjectWorkspace;

use crate::error::{ApiError, InternalSnafu, NotFoundSnafu};
use crate::state::PlanningState;

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
    criteria: Vec<CriterionEvaluation>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct CriterionEvaluation {
    /// Optional phase identifier. Required when a project has multiple phases.
    phase_id: Option<String>,
    /// Criterion text to evaluate.
    criterion: String,
    /// Verification status for the criterion.
    status: CriterionStatusInput,
    /// Evidence supporting this criterion evaluation.
    #[serde(default)]
    evidence: Vec<EvidenceInput>,
    /// Human-readable evaluation detail.
    detail: String,
    /// Suggested fix when the criterion is not met.
    proposed_fix: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum CriterionStatusInput {
    /// Criterion is satisfied.
    Met,
    /// Criterion is partly satisfied.
    PartiallyMet,
    /// Criterion is not satisfied.
    NotMet,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct EvidenceInput {
    /// Evidence type, such as `test` or `file`.
    kind: String,
    /// Evidence content or artifact reference.
    content: String,
}

/// `GET /api/v1/planning/projects/{project_id}/verification`
#[utoipa::path(
    get,
    path = "/api/v1/planning/projects/{project_id}/verification",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
    ),
    responses(
        (status = 200, description = "Current verification result"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Project not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_verification(
    State(state): State<PlanningState>,
    Path(project_id): Path<String>,
) -> Result<Json<VerificationResult>, ApiError> {
    load_project_verification(state.planning_root, project_id, None)
        .await
        .map(Json)
}

/// `POST /api/v1/planning/projects/{project_id}/verification/refresh`
#[utoipa::path(
    post,
    path = "/api/v1/planning/projects/{project_id}/verification/refresh",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
    ),
    responses(
        (status = 200, description = "Refreshed verification result"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Project not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn refresh_verification(
    State(state): State<PlanningState>,
    Path(project_id): Path<String>,
    body: Option<Json<RefreshRequest>>,
) -> Result<Json<VerificationResult>, ApiError> {
    let criteria = body.map(|Json(request)| request.criteria);
    load_project_verification(state.planning_root, project_id, criteria)
        .await
        .map(Json)
}

async fn load_project_verification(
    planning_root: PathBuf,
    project_id: String,
    criteria: Option<Vec<CriterionEvaluation>>,
) -> Result<VerificationResult, ApiError> {
    tokio::task::spawn_blocking(move || {
        let workspace_path = planning_root.join(&project_id);
        let workspace = ProjectWorkspace::open(&workspace_path).map_err(|e| {
            tracing::debug!(error = %e, path = %workspace_path.display(), "planning project not found");
            NotFoundSnafu {
                path: format!("planning/projects/{project_id}"),
            }
            .build()
        })?;
        let project = workspace.load_project().map_err(|e| {
            InternalSnafu {
                message: format!("failed to load planning project: {e}"),
            }
            .build()
        })?;

        let overrides = criteria.unwrap_or_default();
        let mut requirements = Vec::new();
        let mut last_verified_at = jiff::Timestamp::now().to_string();

        for phase in &project.phases {
            let inputs = inputs_for_phase(phase, &overrides, project.phases.len());
            let result = dianoia::verify::verify_phase(phase, &inputs);
            last_verified_at = result.verified_at.to_string();
            requirements.extend(requirements_from_phase(phase, &result.criteria, &result.gaps));
        }

        Ok(VerificationResult {
            project_id,
            requirements,
            last_verified_at,
        })
    })
    .await
    .map_err(ApiError::from)?
}

fn inputs_for_phase(
    phase: &Phase,
    overrides: &[CriterionEvaluation],
    phase_count: usize,
) -> Vec<CriterionInput> {
    let phase_id = phase.id.to_string();
    let explicit: Vec<_> = overrides
        .iter()
        .filter(|input| {
            input.phase_id.as_deref() == Some(phase_id.as_str())
                || (phase_count == 1 && input.phase_id.is_none())
        })
        .map(CriterionInput::from)
        .collect();
    if !explicit.is_empty() {
        return explicit;
    }

    let criteria = if phase.requirements.is_empty() {
        vec![phase.goal.clone()]
    } else {
        phase.requirements.clone()
    };
    let status = inferred_phase_status(phase);
    let evidence = phase_evidence(phase);
    let detail = inferred_detail(phase, status);

    criteria
        .into_iter()
        .map(|criterion| CriterionInput {
            criterion,
            status,
            evidence: evidence.clone(),
            detail: detail.clone(),
            proposed_fix: (status != DianoiaCriterionStatus::Met)
                .then(|| "complete the remaining phase plans and record evidence".to_owned()),
        })
        .collect()
}

fn inferred_phase_status(phase: &Phase) -> DianoiaCriterionStatus {
    if phase_failed(phase) {
        return DianoiaCriterionStatus::NotMet;
    }
    if phase.state == PhaseState::Complete
        || (!phase.plans.is_empty()
            && phase
                .plans
                .iter()
                .all(|plan| matches!(plan.state, PlanState::Complete | PlanState::Skipped)))
    {
        return DianoiaCriterionStatus::Met;
    }
    if phase
        .plans
        .iter()
        .any(|plan| plan.state == PlanState::Complete || !plan.achievements.is_empty())
    {
        return DianoiaCriterionStatus::PartiallyMet;
    }
    DianoiaCriterionStatus::NotMet
}

fn phase_failed(phase: &Phase) -> bool {
    matches!(phase.state, PhaseState::Failed { .. })
        || phase
            .plans
            .iter()
            .any(|plan| matches!(plan.state, PlanState::Failed | PlanState::Stuck))
}

fn phase_evidence(phase: &Phase) -> Vec<Evidence> {
    let mut evidence = Vec::new();
    for plan in &phase.plans {
        if plan.state == PlanState::Complete {
            evidence.push(Evidence {
                kind: "plan".to_owned(),
                content: format!("completed plan: {}", plan.title),
            });
        }
        evidence.extend(plan.achievements.iter().map(|achievement| Evidence {
            kind: "achievement".to_owned(),
            content: achievement.clone(),
        }));
    }
    if evidence.is_empty() && phase.state == PhaseState::Complete {
        evidence.push(Evidence {
            kind: "phase".to_owned(),
            content: format!("phase '{}' is complete", phase.name),
        });
    }
    evidence
}

fn inferred_detail(phase: &Phase, status: DianoiaCriterionStatus) -> String {
    match status {
        DianoiaCriterionStatus::Met => format!("phase '{}' satisfies its criteria", phase.name),
        DianoiaCriterionStatus::PartiallyMet => {
            format!("phase '{}' has partial completion evidence", phase.name)
        }
        DianoiaCriterionStatus::NotMet => {
            format!("phase '{}' still has unmet planning criteria", phase.name)
        }
        _ => format!("phase '{}' has an unknown verification state", phase.name),
    }
}

fn requirements_from_phase(
    phase: &Phase,
    criteria: &[CriterionResult],
    gaps: &[DianoiaVerificationGap],
) -> Vec<RequirementVerification> {
    let phase_failed = phase_failed(phase);
    criteria
        .iter()
        .enumerate()
        .map(|(idx, criterion)| {
            let matching_gaps: Vec<_> = gaps
                .iter()
                .filter(|gap| gap.criterion == criterion.criterion)
                .map(|gap| VerificationGap {
                    missing_criteria: gap.detail.clone(),
                    suggested_action: gap.proposed_fix.clone(),
                })
                .collect();
            RequirementVerification {
                id: format!("{}:{}", phase.id, idx + 1),
                title: criterion.criterion.clone(),
                tier: format!("phase-{}", phase.order),
                priority: priority_for_index(idx),
                status: status_for_criterion(criterion.status, phase_failed),
                coverage_pct: coverage_for_status(criterion.status),
                evidence: criterion
                    .evidence
                    .iter()
                    .map(|evidence| VerificationEvidence {
                        label: evidence.kind.clone(),
                        artifact: evidence.content.clone(),
                    })
                    .collect(),
                gaps: matching_gaps,
            }
        })
        .collect()
}

fn status_for_criterion(status: DianoiaCriterionStatus, phase_failed: bool) -> VerificationStatus {
    match status {
        DianoiaCriterionStatus::Met => VerificationStatus::Verified,
        DianoiaCriterionStatus::PartiallyMet => VerificationStatus::PartiallyVerified,
        DianoiaCriterionStatus::NotMet if phase_failed => VerificationStatus::Failed,
        _ => VerificationStatus::Unverified,
    }
}

fn coverage_for_status(status: DianoiaCriterionStatus) -> u8 {
    match status {
        DianoiaCriterionStatus::Met => 100,
        DianoiaCriterionStatus::PartiallyMet => 50,
        _ => 0,
    }
}

fn priority_for_index(index: usize) -> RequirementPriority {
    match index {
        0 => RequirementPriority::P0,
        1 => RequirementPriority::P1,
        2 => RequirementPriority::P2,
        _ => RequirementPriority::P3,
    }
}

impl From<&CriterionEvaluation> for CriterionInput {
    fn from(input: &CriterionEvaluation) -> Self {
        Self {
            criterion: input.criterion.clone(),
            status: match input.status {
                CriterionStatusInput::Met => DianoiaCriterionStatus::Met,
                CriterionStatusInput::PartiallyMet => DianoiaCriterionStatus::PartiallyMet,
                CriterionStatusInput::NotMet => DianoiaCriterionStatus::NotMet,
            },
            evidence: input
                .evidence
                .iter()
                .map(|evidence| Evidence {
                    kind: evidence.kind.clone(),
                    content: evidence.content.clone(),
                })
                .collect(),
            detail: input.detail.clone(),
            proposed_fix: input.proposed_fix.clone(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON keys and first requirement are known-present"
)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use axum::{Router, body};
    use tower::ServiceExt as _;

    use dianoia::phase::{Phase, PhaseState};
    use dianoia::plan::{Plan, PlanState};
    use dianoia::project::{Project, ProjectMode};

    use super::*;

    fn planning_router(root: PathBuf) -> Router {
        Router::new()
            .route(
                "/api/v1/planning/projects/{project_id}/verification",
                get(get_verification),
            )
            .route(
                "/api/v1/planning/projects/{project_id}/verification/refresh",
                post(refresh_verification),
            )
            .with_state(PlanningState {
                planning_root: root,
            })
    }

    fn write_project(root: &std::path::Path, complete: bool) -> String {
        let mut project = Project::new(
            "synthetic".to_owned(),
            "synthetic planning project".to_owned(),
            ProjectMode::Full,
            "alice".to_owned(),
        );
        let mut phase = Phase::new(
            "Verification".to_owned(),
            "prove the API returns dianoia verification".to_owned(),
            1,
        );
        phase.requirements = vec!["endpoint returns plan-validity result".to_owned()];
        let mut plan = Plan::new(
            "wire endpoint".to_owned(),
            "call the dianoia verifier".to_owned(),
            1,
        );
        if complete {
            plan.state = PlanState::Complete;
            phase.state = PhaseState::Complete;
        }
        phase.add_plan(plan);
        project.add_phase(phase);

        let project_id = project.id.to_string();
        let workspace = ProjectWorkspace::create(root.join(&project_id)).unwrap();
        workspace.save_project(&project).unwrap();
        project_id
    }

    #[tokio::test]
    async fn get_verification_returns_real_result() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), true);
        let app = planning_router(dir.path().to_path_buf());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/planning/projects/{project_id}/verification"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["project_id"], project_id);
        assert_eq!(json["requirements"][0]["status"], "verified");
        assert_eq!(json["requirements"][0]["coverage_pct"], 100);
    }

    #[tokio::test]
    async fn refresh_verification_accepts_synthetic_criterion_input() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), false);
        let app = planning_router(dir.path().to_path_buf());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/planning/projects/{project_id}/verification/refresh"
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "criteria": [{
                                "criterion": "endpoint returns plan-validity result",
                                "status": "met",
                                "evidence": [{
                                    "kind": "test",
                                    "content": "synthetic endpoint test"
                                }],
                                "detail": "verified by synthetic request"
                            }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["requirements"][0]["status"], "verified");
        assert_eq!(
            json["requirements"][0]["evidence"][0]["artifact"],
            "synthetic endpoint test"
        );
    }

    #[tokio::test]
    async fn get_verification_returns_not_found_for_missing_project() {
        let dir = tempfile::tempdir().unwrap();
        let app = planning_router(dir.path().to_path_buf());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/planning/projects/missing/verification")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
