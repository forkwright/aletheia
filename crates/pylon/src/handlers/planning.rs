//! Planning project verification endpoints.
//!
//! Serves the verification state consumed by the desktop `VerificationView`
//! component. Results are computed through dianoia's verification engine over
//! the persisted planning workspace for the requested project.
//!
//! Privacy guard: project visibility is read from a per-workspace metadata
//! sidecar. Public projects return full verification detail; private or
//! internal projects classify the response and redact evidence, gap, and
//! artifact detail because this handler only validates bearer presence, not
//! project-scoped roles.

use std::path::{Path as StdPath, PathBuf};

use axum::Json;
use axum::extract::{Path as AxumPath, State};

use dianoia::phase::{Phase, PhaseState};
use dianoia::plan::PlanState;
use dianoia::verify::{
    CriterionInput, CriterionResult, CriterionStatus as DianoiaCriterionStatus, Evidence,
    VerificationGap as DianoiaVerificationGap,
};
use dianoia::workspace::ProjectWorkspace;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, BadRequestSnafu, InternalSnafu, NotFoundSnafu};
use crate::extract::Claims;
use crate::state::PlanningState;

#[path = "planning_dto.rs"]
mod planning_dto;
use planning_dto::{
    CriterionEvaluation, CriterionStatusInput, RefreshRequest, RequirementPriority,
    RequirementVerification, VerificationEvidence, VerificationGap, VerificationResult,
    VerificationStatus,
};

/// Name of the per-workspace metadata sidecar that carries privacy labels.
const METADATA_FILE: &str = "planning_meta.json";

/// Project visibility as declared in the workspace metadata sidecar.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Visibility {
    /// Visible to any authenticated caller.
    Public,
    /// Visible only to participants; redacted by default.
    #[default]
    Private,
    /// Organization-visible but still redacted until role checks land.
    Internal,
}

impl Visibility {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Internal => "internal",
        }
    }
}

/// Privacy/classification metadata for a planning workspace.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProjectMetadata {
    #[serde(default)]
    visibility: Visibility,
    #[serde(default)]
    classification: String,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            visibility: Visibility::Private,
            classification: "restricted".to_owned(),
        }
    }
}

impl ProjectMetadata {
    fn classification_label(&self) -> String {
        if self.classification.is_empty() {
            self.visibility.as_str().to_owned()
        } else {
            self.classification.clone()
        }
    }
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
    _claims: Claims,
    AxumPath(project_id): AxumPath<String>,
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
    _claims: Claims,
    AxumPath(project_id): AxumPath<String>,
    body: Option<Json<RefreshRequest>>,
) -> Result<Json<VerificationResult>, ApiError> {
    let criteria = body.map(|Json(request)| request.criteria);
    load_project_verification(state.planning_root, project_id, criteria)
        .await
        .map(Json)
}

pub(crate) async fn load_project_verification(
    planning_root: PathBuf,
    project_id: String,
    criteria: Option<Vec<CriterionEvaluation>>,
) -> Result<VerificationResult, ApiError> {
    validate_project_id(&project_id)?;

    tokio::task::spawn_blocking(move || {
        let workspace_path = resolve_planning_workspace(&planning_root, &project_id)?;
        let metadata = load_project_metadata(&workspace_path);
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

        let redacted = !matches!(metadata.visibility, Visibility::Public);
        let requirements = if redacted {
            requirements
                .into_iter()
                .map(|mut req| {
                    req.evidence.clear();
                    req.gaps.clear();
                    req
                })
                .collect()
        } else {
            requirements
        };

        Ok(VerificationResult {
            project_id,
            requirements,
            last_verified_at,
            visibility: metadata.visibility.as_str().to_owned(),
            classification: metadata.classification_label(),
            redacted,
        })
    })
    .await
    .map_err(ApiError::from)?
}

/// Reject project identifiers that could be used for path traversal or that
/// name hidden files.
fn validate_project_id(project_id: &str) -> Result<&str, ApiError> {
    if project_id.is_empty() {
        return BadRequestSnafu {
            message: "project_id must not be empty".to_owned(),
        }
        .fail();
    }
    if project_id.contains('/') || project_id.contains('\\') {
        return BadRequestSnafu {
            message: "project_id must not contain path separators".to_owned(),
        }
        .fail();
    }
    if project_id == "." || project_id == ".." || project_id.starts_with('.') {
        return BadRequestSnafu {
            message: "project_id must not be a dot segment or hidden name".to_owned(),
        }
        .fail();
    }
    Ok(project_id)
}

/// Canonicalize the planning root and the requested workspace, then verify the
/// workspace stays inside the root.
fn resolve_planning_workspace(
    planning_root: &StdPath,
    project_id: &str,
) -> Result<PathBuf, ApiError> {
    let canonical_root = std::fs::canonicalize(planning_root).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            return NotFoundSnafu {
                path: format!("planning/projects/{project_id}"),
            }
            .build();
        }
        tracing::error!(
            error = %e,
            path = %planning_root.display(),
            "planning root is unusable"
        );
        InternalSnafu {
            message: format!("planning root is unusable: {e}"),
        }
        .build()
    })?;

    let workspace_path = canonical_root.join(project_id);

    let Ok(canonical_workspace) = std::fs::canonicalize(&workspace_path) else {
        return Err(NotFoundSnafu {
            path: format!("planning/projects/{project_id}"),
        }
        .build());
    };

    if !canonical_workspace.starts_with(&canonical_root) {
        tracing::warn!(
            project_id = %project_id,
            workspace = %canonical_workspace.display(),
            root = %canonical_root.display(),
            "rejected planning workspace outside planning root"
        );
        return Err(NotFoundSnafu {
            path: format!("planning/projects/{project_id}"),
        }
        .build());
    }

    Ok(canonical_workspace)
}

/// Load privacy metadata from the workspace sidecar, defaulting to the most
/// conservative label when the file is missing or malformed.
fn load_project_metadata(workspace_root: &StdPath) -> ProjectMetadata {
    let path = workspace_root.join(METADATA_FILE);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<ProjectMetadata>(&contents) {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    path = %path.display(),
                    "invalid planning metadata; using conservative defaults"
                );
                ProjectMetadata::default()
            }
        },
        Err(e) => {
            tracing::debug!(
                error = %e,
                path = %path.display(),
                "no planning metadata; defaulting to private"
            );
            ProjectMetadata::default()
        }
    }
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
    use std::io::Write as _;

    use dianoia::phase::{Phase, PhaseState};
    use dianoia::plan::{Plan, PlanState};
    use dianoia::project::{Project, ProjectMode};

    use super::*;

    fn classification_for(visibility: Visibility) -> String {
        visibility.as_str().to_owned()
    }

    fn write_text(path: &std::path::Path, contents: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn write_project(root: &std::path::Path, complete: bool, visibility: Visibility) -> String {
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

        let metadata = ProjectMetadata {
            visibility,
            classification: classification_for(visibility),
        };
        let meta_path = root.join(&project_id).join(METADATA_FILE);
        let metadata_json = serde_json::to_string(&metadata).unwrap();
        write_text(&meta_path, &metadata_json);

        project_id
    }

    #[tokio::test]
    async fn get_verification_returns_real_result() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), true, Visibility::Public);
        let result = load_project_verification(dir.path().to_path_buf(), project_id.clone(), None)
            .await
            .unwrap();

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["project_id"], project_id);
        assert_eq!(json["requirements"][0]["status"], "verified");
        assert_eq!(json["requirements"][0]["coverage_pct"], 100);
        assert_eq!(json["visibility"], "public");
        assert_eq!(json["classification"], "public");
        assert_eq!(json["redacted"], false);
    }

    #[tokio::test]
    async fn refresh_verification_accepts_synthetic_criterion_input() {
        use planning_dto::EvidenceInput;

        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), false, Visibility::Public);
        let criteria = vec![CriterionEvaluation {
            phase_id: None,
            criterion: "endpoint returns plan-validity result".to_owned(),
            status: CriterionStatusInput::Met,
            evidence: vec![EvidenceInput {
                kind: "test".to_owned(),
                content: "synthetic endpoint test".to_owned(),
            }],
            detail: "verified by synthetic request".to_owned(),
            proposed_fix: None,
        }];
        let result =
            load_project_verification(dir.path().to_path_buf(), project_id, Some(criteria))
                .await
                .unwrap();

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["requirements"][0]["status"], "verified");
        assert_eq!(
            json["requirements"][0]["evidence"][0]["artifact"],
            "synthetic endpoint test"
        );
        assert_eq!(json["redacted"], false);
    }

    #[tokio::test]
    async fn get_verification_returns_not_found_for_missing_project() {
        let dir = tempfile::tempdir().unwrap();
        let result =
            load_project_verification(dir.path().to_path_buf(), "missing".to_owned(), None).await;
        let Err(err) = result else {
            panic!("missing project should fail, got Ok");
        };
        assert!(
            matches!(err, ApiError::NotFound { .. }),
            "expected NotFound, got {err:?}"
        );
    }

    #[tokio::test]
    async fn load_verification_rejects_path_traversal_ids() {
        let dir = tempfile::tempdir().unwrap();
        write_project(dir.path(), true, Visibility::Public);

        let malicious = [
            "../project",
            "foo/../bar",
            "..",
            ".",
            "a/b",
            "a\\b",
            "",
            ".hidden",
        ];
        for id in malicious {
            let result =
                load_project_verification(dir.path().to_path_buf(), id.to_owned(), None).await;
            assert!(
                matches!(result, Err(ApiError::BadRequest { .. })),
                "id {id:?} should be rejected before filesystem resolution, got {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn private_project_redacts_evidence_and_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), false, Visibility::Private);
        let result = load_project_verification(dir.path().to_path_buf(), project_id, None)
            .await
            .unwrap();

        assert_eq!(result.visibility, "private");
        assert_eq!(result.classification, "private");
        assert!(result.redacted, "private projects are redacted");
        assert!(
            !result.requirements.is_empty(),
            "requirements remain visible"
        );
        for req in &result.requirements {
            assert!(
                req.evidence.is_empty(),
                "evidence must be redacted for private projects"
            );
            assert!(
                req.gaps.is_empty(),
                "gaps must be redacted for private projects"
            );
        }
    }

    #[tokio::test]
    async fn internal_project_redacts_detail() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), true, Visibility::Internal);
        let result = load_project_verification(dir.path().to_path_buf(), project_id, None)
            .await
            .unwrap();

        assert_eq!(result.visibility, "internal");
        assert!(result.redacted);
        assert!(result.requirements[0].evidence.is_empty());
    }

    #[tokio::test]
    async fn public_project_returns_unredacted_details() {
        let dir = tempfile::tempdir().unwrap();
        let project_id = write_project(dir.path(), true, Visibility::Public);
        let result = load_project_verification(dir.path().to_path_buf(), project_id, None)
            .await
            .unwrap();

        assert_eq!(result.visibility, "public");
        assert!(!result.redacted, "public projects are not redacted");
        assert!(
            !result.requirements[0].evidence.is_empty(),
            "public projects keep evidence"
        );
    }

    #[test]
    fn metadata_defaults_to_private_and_restricted() {
        let meta = ProjectMetadata::default();
        assert!(matches!(meta.visibility, Visibility::Private));
        assert_eq!(meta.classification_label(), "restricted");
    }

    #[test]
    fn metadata_uses_explicit_classification_when_present() {
        let meta = ProjectMetadata {
            visibility: Visibility::Internal,
            classification: "confidential".to_owned(),
        };
        assert_eq!(meta.classification_label(), "confidential");
    }
}
