//! Planning verification types for project requirement tracking.

use serde::{Deserialize, Serialize};

/// Verification status for a single requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum VerificationStatus {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RequirementPriority {
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    /// Human-readable label for this evidence.
    pub label: String,
    /// Path or reference to the evidence artifact.
    pub artifact: String,
}

/// A criterion not yet satisfied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationGap {
    /// Description of the missing criteria.
    pub missing_criteria: String,
    /// Suggested action to close the gap.
    pub suggested_action: String,
}

/// Verification result for a single requirement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequirementVerification {
    /// Requirement identifier.
    // kanon:ignore RUST/primitive-for-domain-id — skene verification type mirrors server-side string IDs directly
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub tier: String,
    /// Priority level.
    pub priority: RequirementPriority,
    /// Current verification status.
    pub status: VerificationStatus,
    /// Coverage percentage 0–100.
    pub coverage_pct: u8,
    /// Evidence supporting this requirement.
    pub evidence: Vec<VerificationEvidence>,
    /// Gaps remaining for this requirement.
    pub gaps: Vec<VerificationGap>,
}

/// Full verification result for a project.
///
/// Wire format consumed by the desktop `VerificationView` and served
/// by pylon's `GET /api/v1/planning/projects/{id}/verification`.
///
/// WHY(#4565): `visibility`/`classification`/`redacted` were missing from
/// the initial cut of this type even though pylon's handler always sends
/// them (`crates/pylon/src/handlers/planning.rs::load_project_verification`)
/// -- a caller deserializing the real endpoint into the bare three-field
/// struct silently dropped the privacy labels the desktop view renders.
/// Defaults mirror the desktop's own pre-existing fallback (private /
/// restricted, not redacted) for a server old enough to omit the fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectVerificationResult {
    /// Project identifier.
    // kanon:ignore RUST/primitive-for-domain-id — skene verification type mirrors server-side string IDs directly
    pub project_id: String,
    /// Per-requirement verification results.
    pub requirements: Vec<RequirementVerification>,
    /// ISO 8601 timestamp of the last verification run.
    pub last_verified_at: String,
    /// Project visibility label surfaced for privacy review.
    #[serde(default = "default_visibility")]
    pub visibility: String,
    /// Classification label surfaced for privacy review.
    #[serde(default = "default_classification")]
    pub classification: String,
    /// True when evidence and gap detail have been redacted.
    #[serde(default)]
    pub redacted: bool,
}

fn default_visibility() -> String {
    "private".to_string()
}

fn default_classification() -> String {
    "restricted".to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn verification_result_serde_roundtrip() {
        let result = ProjectVerificationResult {
            project_id: "p1".to_string(),
            requirements: vec![RequirementVerification {
                id: "r1".to_string(),
                title: "Tests pass".to_string(),
                tier: "v1".to_string(),
                priority: RequirementPriority::P0,
                status: VerificationStatus::Verified,
                coverage_pct: 100,
                evidence: vec![VerificationEvidence {
                    label: "CI run".to_string(),
                    artifact: "run-123".to_string(),
                }],
                gaps: vec![],
            }],
            last_verified_at: "2026-01-01T00:00:00Z".to_string(),
            visibility: "public".to_string(),
            classification: "public".to_string(),
            redacted: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ProjectVerificationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn verification_status_snake_case_roundtrip() {
        let statuses = [
            (VerificationStatus::Verified, "\"verified\""),
            (
                VerificationStatus::PartiallyVerified,
                "\"partially_verified\"",
            ),
            (VerificationStatus::Unverified, "\"unverified\""),
            (VerificationStatus::Failed, "\"failed\""),
        ];
        for (status, expected_json) in &statuses {
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(&json, *expected_json);
            let back: VerificationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *status);
        }
    }

    #[test]
    fn requirement_priority_snake_case_roundtrip() {
        let priorities = [
            (RequirementPriority::P0, "\"p0\""),
            (RequirementPriority::P1, "\"p1\""),
            (RequirementPriority::P2, "\"p2\""),
            (RequirementPriority::P3, "\"p3\""),
        ];
        for (priority, expected_json) in &priorities {
            let json = serde_json::to_string(priority).unwrap();
            assert_eq!(&json, *expected_json);
            let back: RequirementPriority = serde_json::from_str(&json).unwrap();
            assert_eq!(back, *priority);
        }
    }

    #[test]
    fn verification_gap_serde() {
        let gap = VerificationGap {
            missing_criteria: "test coverage".to_string(),
            suggested_action: "add integration tests".to_string(),
        };
        let json = serde_json::to_string(&gap).unwrap();
        let back: VerificationGap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gap);
    }

    #[test]
    fn empty_verification_result_deserializes() {
        let json = r#"{
            "project_id": "p1",
            "requirements": [],
            "last_verified_at": "pending"
        }"#;
        let result: ProjectVerificationResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.project_id, "p1");
        assert!(result.requirements.is_empty());
        assert_eq!(result.last_verified_at, "pending");
    }

    /// WHY(#4565): a server old enough to omit the privacy fields must not
    /// fail to deserialize -- defaults mirror the desktop's own
    /// pre-existing fallback (private/restricted, not redacted) verbatim.
    #[test]
    fn verification_result_defaults_privacy_fields_when_absent() {
        let json = r#"{
            "project_id": "p1",
            "requirements": [],
            "last_verified_at": "pending"
        }"#;
        let result: ProjectVerificationResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.visibility, "private");
        assert_eq!(result.classification, "restricted");
        assert!(!result.redacted);
    }

    #[test]
    fn verification_result_reads_privacy_fields_when_present() {
        let json = r#"{
            "project_id": "p1",
            "requirements": [],
            "last_verified_at": "pending",
            "visibility": "internal",
            "classification": "confidential",
            "redacted": true
        }"#;
        let result: ProjectVerificationResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.visibility, "internal");
        assert_eq!(result.classification, "confidential");
        assert!(result.redacted);
    }
}
