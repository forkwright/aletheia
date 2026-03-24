//! Planning project verification endpoints.
//!
//! Serves the verification state consumed by the desktop `VerificationView`
//! component. The Re-verify button triggers `POST .../verification/refresh`
//! which acknowledges the request and returns the current verification snapshot.
//!
//! # TODO(#2034)
//! Wire to the actual `dianoia` verification engine once a `PlanningService`
//! is exposed in pylon's `AppState`. Current handlers return stub data so the
//! desktop UI flow completes without error.

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Serialize;

/// Verification status for a single requirement.
// NOTE: variants are constructed in tests and will be used in production once
// the dianoia verification engine is wired into pylon (#2034).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "API contract types: variants used in tests, production use pending #2034"
    )
)]
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
// NOTE: variants are constructed in tests and will be used in production once
// the dianoia verification engine is wired into pylon (#2034).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "API contract types: variants used in tests, production use pending #2034"
    )
)]
pub(crate) enum RequirementPriority {
    /// Blocking — must be verified before release.
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
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub(crate) tier: String,
    /// Priority level.
    pub(crate) priority: RequirementPriority,
    /// Current verification status.
    pub(crate) status: VerificationStatus,
    /// Coverage percentage 0–100.
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

/// Response for `POST .../verification/refresh`.
#[derive(Debug, Serialize)]
pub(crate) struct RefreshResponse {
    /// Refresh status: `"accepted"`.
    pub(crate) status: &'static str,
    /// Echoed project identifier.
    pub(crate) project_id: String,
}

/// `GET /api/planning/projects/{project_id}/verification`
///
/// Returns the current verification state for a project. Until the
/// verification engine is wired into pylon, returns an empty result
/// so the desktop `VerificationView` renders correctly.
#[utoipa::path(
    get,
    path = "/api/planning/projects/{project_id}/verification",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
    ),
    responses(
        (status = 200, description = "Verification result"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_verification(Path(project_id): Path<String>) -> Json<VerificationResult> {
    // TODO(#2034): read actual verification data from the project workspace
    // once a PlanningService is part of pylon's AppState.
    Json(VerificationResult {
        project_id,
        requirements: Vec::new(),
        last_verified_at: "pending".to_owned(),
    })
}

/// `POST /api/planning/projects/{project_id}/verification/refresh`
///
/// Triggers a re-verification of the project. The desktop Re-verify button
/// calls this endpoint and, on success, re-fetches verification data via GET.
///
/// Until the verification engine is wired in, this acknowledges the request
/// and returns 200 so the UI flow completes.
#[utoipa::path(
    post,
    path = "/api/planning/projects/{project_id}/verification/refresh",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
    ),
    responses(
        (status = 200, description = "Refresh accepted"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn refresh_verification(
    Path(project_id): Path<String>,
) -> (StatusCode, Json<RefreshResponse>) {
    tracing::info!(project_id = %project_id, "verification refresh requested");
    // TODO(#2034): invoke dianoia verification engine and persist result.
    (
        StatusCode::OK,
        Json(RefreshResponse {
            status: "accepted",
            project_id,
        }),
    )
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use axum::{Router, body};
    use tower::ServiceExt as _;

    use super::*;

    fn planning_router() -> Router {
        Router::new()
            .route(
                "/api/planning/projects/{project_id}/verification",
                get(get_verification),
            )
            .route(
                "/api/planning/projects/{project_id}/verification/refresh",
                post(refresh_verification),
            )
    }

    #[tokio::test]
    async fn get_verification_returns_empty_result() {
        let app = planning_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/planning/projects/proj-123/verification")
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
        assert_eq!(json["project_id"], "proj-123");
        assert!(json["requirements"].as_array().unwrap().is_empty());
        assert_eq!(json["last_verified_at"], "pending");
    }

    #[tokio::test]
    async fn refresh_verification_returns_accepted() {
        let app = planning_router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/planning/projects/proj-456/verification/refresh")
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
        assert_eq!(json["status"], "accepted");
        assert_eq!(json["project_id"], "proj-456");
    }

    #[tokio::test]
    async fn verification_result_matches_desktop_shape() {
        // WHY: ensures the serialized JSON matches what the desktop
        // VerificationView component expects to deserialize.
        let result = VerificationResult {
            project_id: "p1".to_owned(),
            requirements: vec![RequirementVerification {
                id: "r1".to_owned(),
                title: "Tests pass".to_owned(),
                tier: "v1".to_owned(),
                priority: RequirementPriority::P0,
                status: VerificationStatus::Verified,
                coverage_pct: 100,
                evidence: vec![VerificationEvidence {
                    label: "CI run".to_owned(),
                    artifact: "run-123".to_owned(),
                }],
                gaps: vec![],
            }],
            last_verified_at: "2026-01-01T00:00:00Z".to_owned(),
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["project_id"], "p1");
        assert_eq!(json["requirements"][0]["status"], "verified");
        assert_eq!(json["requirements"][0]["priority"], "p0");
        assert_eq!(json["requirements"][0]["coverage_pct"], 100);
        assert_eq!(json["requirements"][0]["evidence"][0]["label"], "CI run");
    }

    #[tokio::test]
    async fn all_status_variants_serialize_snake_case() {
        let statuses = [
            (VerificationStatus::Verified, "verified"),
            (VerificationStatus::PartiallyVerified, "partially_verified"),
            (VerificationStatus::Unverified, "unverified"),
            (VerificationStatus::Failed, "failed"),
        ];
        for (status, expected) in &statuses {
            let json = serde_json::to_value(status).unwrap();
            assert_eq!(json.as_str().unwrap(), *expected);
        }
    }

    #[tokio::test]
    async fn all_priority_variants_serialize_snake_case() {
        let priorities = [
            (RequirementPriority::P0, "p0"),
            (RequirementPriority::P1, "p1"),
            (RequirementPriority::P2, "p2"),
            (RequirementPriority::P3, "p3"),
        ];
        for (priority, expected) in &priorities {
            let json = serde_json::to_value(priority).unwrap();
            assert_eq!(json.as_str().unwrap(), *expected);
        }
    }
}
