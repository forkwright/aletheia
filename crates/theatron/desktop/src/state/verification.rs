//! Goal-backward verification state for the planning project detail view.

use serde::Deserialize;

/// Verification status for a single requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum RequirementPriority {
    /// Blocking -- must be verified before release.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
    /// Low or nice-to-have.
    P3,
}

/// A piece of evidence demonstrating a requirement.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct VerificationEvidence {
    pub(crate) label: String,
    pub(crate) artifact: String,
}

/// A criterion not yet satisfied.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct VerificationGap {
    pub(crate) missing_criteria: String,
    pub(crate) suggested_action: String,
}

/// Verification result for a single requirement.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct RequirementVerification {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub(crate) tier: String,
    pub(crate) priority: RequirementPriority,
    pub(crate) status: VerificationStatus,
    /// Coverage percentage 0--100.
    pub(crate) coverage_pct: u8,
    pub(crate) evidence: Vec<VerificationEvidence>,
    pub(crate) gaps: Vec<VerificationGap>,
}

/// Full verification result for a project.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct VerificationResult {
    pub(crate) project_id: String,
    pub(crate) requirements: Vec<RequirementVerification>,
    pub(crate) last_verified_at: String,
}

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
        Some(((verified * 100) / reqs.len()).min(100) as u8)
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
        Some(((verified * 100) / tier_reqs.len()).min(100) as u8)
    }

    /// Requirements that have gaps (unverified, partially verified, or failed).
    #[must_use]
    pub(crate) fn gaps(&self) -> Vec<&RequirementVerification> {
        match &self.result {
            None => Vec::new(),
            Some(r) => r
                .requirements
                .iter()
                .filter(|req| {
                    matches!(
                        req.status,
                        VerificationStatus::Unverified
                            | VerificationStatus::PartiallyVerified
                            | VerificationStatus::Failed
                    )
                })
                .collect(),
        }
    }

    /// Blocking (P0) requirements with gaps.
    #[must_use]
    pub(crate) fn blocking_gaps(&self) -> Vec<&RequirementVerification> {
        self.gaps()
            .into_iter()
            .filter(|r| r.priority == RequirementPriority::P0)
            .collect()
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

    #[test]
    fn gaps_excludes_verified() {
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
                "v2",
                RequirementPriority::P0,
                VerificationStatus::Failed,
            ),
            req(
                "r4",
                "v2",
                RequirementPriority::P2,
                VerificationStatus::PartiallyVerified,
            ),
        ]);
        let gaps = store.gaps();
        assert_eq!(gaps.len(), 3, "unverified + failed + partially_verified");
        assert!(
            gaps.iter()
                .all(|r| r.status != VerificationStatus::Verified)
        );
    }

    #[test]
    fn blocking_gaps_returns_only_p0() {
        let store = store_with(vec![
            req(
                "r1",
                "v1",
                RequirementPriority::P0,
                VerificationStatus::Unverified,
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
                RequirementPriority::P0,
                VerificationStatus::Failed,
            ),
            req(
                "r4",
                "v1",
                RequirementPriority::P0,
                VerificationStatus::Verified,
            ),
        ]);
        let blocking = store.blocking_gaps();
        assert_eq!(blocking.len(), 2, "P0 unverified + P0 failed only");
        assert!(
            blocking
                .iter()
                .all(|r| r.priority == RequirementPriority::P0)
        );
    }
}
