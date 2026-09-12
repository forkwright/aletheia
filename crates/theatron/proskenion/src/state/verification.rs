//! Goal-backward verification state for the planning project detail view.
//!
//! WHY(#4565): the wire types (`VerificationStatus`, `RequirementPriority`,
//! `RequirementVerification`, `ProjectVerificationResult`) used to be
//! duplicated here rather than sourced from skene -- the desktop client's
//! own `GET`/`POST /api/v1/planning/projects/{id}/verification` caller now
//! goes through `skene::api::client::ApiClient`, whose methods already
//! return skene's typed `ProjectVerificationResult`, so this module keeps
//! only the view-local `VerificationStore` wrapper and its coverage math.

pub(crate) use skene::api::types::{
    ProjectVerificationResult as VerificationResult, RequirementPriority, RequirementVerification,
    VerificationStatus,
};

/// Store for verification results of the active project.
#[derive(Debug, Clone, Default)]
pub(crate) struct VerificationStore {
    pub(crate) result: Option<VerificationResult>,
}

impl VerificationStore {
    /// Overall coverage as `verified_count * 100 / total_count`.
    ///
    /// Returns `None` when no requirements are defined.
    #[must_use]
    pub(crate) fn overall_coverage(&self) -> Option<u8> {
        let reqs = self.result.as_ref()?.requirements.as_slice();
        if reqs.is_empty() {
            return None;
        }
        let verified = reqs
            .iter()
            .filter(|r| r.status == VerificationStatus::Verified)
            .count();
        Some({
            #[expect(clippy::as_conversions, reason = "percentage clamped to 0–100 fits u8")]
            let pct = ((verified * 100) / reqs.len()).min(100) as u8;
            pct
        })
    }

    /// Coverage percentage for a specific tier (e.g., `"v1"`, `"v2"`).
    ///
    /// Returns `None` when the tier has no requirements.
    #[must_use]
    pub(crate) fn tier_coverage(&self, tier: &str) -> Option<u8> {
        let reqs = self.result.as_ref()?.requirements.as_slice();
        let tier_reqs: Vec<_> = reqs.iter().filter(|r| r.tier == tier).collect();
        if tier_reqs.is_empty() {
            return None;
        }
        let verified = tier_reqs
            .iter()
            .filter(|r| r.status == VerificationStatus::Verified)
            .count();
        Some({
            #[expect(clippy::as_conversions, reason = "percentage clamped to 0–100 fits u8")]
            let pct = ((verified * 100) / tier_reqs.len()).min(100) as u8;
            pct
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(
        id: &str,
        tier: &str,
        priority: RequirementPriority,
        status: VerificationStatus,
    ) -> RequirementVerification {
        RequirementVerification {
            id: id.to_string(),
            title: id.to_string(),
            tier: tier.to_string(),
            priority,
            status,
            coverage_pct: match status {
                VerificationStatus::Verified => 100,
                VerificationStatus::PartiallyVerified => 50,
                _ => 0,
            },
            evidence: vec![],
            gaps: vec![],
        }
    }

    fn store_with(requirements: Vec<RequirementVerification>) -> VerificationStore {
        VerificationStore {
            result: Some(VerificationResult {
                project_id: "p1".to_string(),
                requirements,
                last_verified_at: "2026-01-01T00:00:00Z".to_string(),
                visibility: "public".to_string(),
                classification: "public".to_string(),
                redacted: false,
            }),
        }
    }

    #[test]
    fn overall_coverage_none_when_no_result() {
        assert_eq!(VerificationStore::default().overall_coverage(), None);
    }

    #[test]
    fn overall_coverage_none_when_no_requirements() {
        assert_eq!(store_with(vec![]).overall_coverage(), None);
    }

    #[test]
    fn overall_coverage_calculates_verified_fraction() {
        let store = store_with(vec![
            req(
                "r1",
                "v1",
                RequirementPriority::P0,
                VerificationStatus::Verified,
            ),
            req(
                "r2",
                "v1",
                RequirementPriority::P1,
                VerificationStatus::Unverified,
            ),
            req(
                "r3",
                "v1",
                RequirementPriority::P1,
                VerificationStatus::Verified,
            ),
            req(
                "r4",
                "v1",
                RequirementPriority::P2,
                VerificationStatus::Failed,
            ),
        ]);
        // 2 verified out of 4 = 50%
        assert_eq!(store.overall_coverage(), Some(50));
    }

    #[test]
    fn tier_coverage_none_for_missing_tier() {
        let store = store_with(vec![req(
            "r1",
            "v1",
            RequirementPriority::P0,
            VerificationStatus::Verified,
        )]);
        assert_eq!(store.tier_coverage("v2"), None);
    }
}
