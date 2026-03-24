//! Planning state: projects, requirements, roadmap phases, and category proposals.

use serde::{Deserialize, Serialize};

/// Project lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum ProjectStatus {
    /// Initial planning phase.
    Planning,
    /// Actively being worked on.
    InProgress,
    /// All work complete.
    Completed,
    /// Temporarily suspended.
    Paused,
}

/// Current phase summary for a project card.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ProjectPhaseInfo {
    pub(crate) name: String,
    pub(crate) number: u32,
    pub(crate) total: u32,
}

/// A planning project returned from `GET /api/planning/projects`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Project {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) status: ProjectStatus,
    #[serde(default)]
    pub(crate) current_phase: Option<ProjectPhaseInfo>,
    #[serde(default)]
    pub(crate) requirements_completed: u32,
    #[serde(default)]
    pub(crate) requirements_total: u32,
    #[serde(default)]
    pub(crate) last_activity: Option<String>,
    #[serde(default)]
    pub(crate) active_agents: Vec<String>,
}

/// Categorization bucket for requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum RequirementCategory {
    /// Must-have for first release.
    V1,
    /// Planned for future releases.
    V2,
    /// Explicitly excluded.
    OutOfScope,
}

impl RequirementCategory {
    /// Human-readable label for display.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::OutOfScope => "Out of Scope",
        }
    }
}

/// Workflow status for a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum RequirementStatus {
    /// Newly proposed, not yet accepted.
    Proposed,
    /// Accepted into scope.
    Accepted,
    /// Implementation complete.
    Implemented,
    /// Verified as working.
    Verified,
}

impl RequirementStatus {
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Proposed => "Proposed",
            Self::Accepted => "Accepted",
            Self::Implemented => "Implemented",
            Self::Verified => "Verified",
        }
    }

    #[must_use]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Proposed => "#888",
            Self::Accepted => "#4a9aff",
            Self::Implemented => "#f59e0b",
            Self::Verified => "#22c55e",
        }
    }
}

/// Priority tier for requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub(crate) enum RequirementPriority {
    /// Blocking -- highest priority.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
}

impl RequirementPriority {
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
        }
    }

    #[must_use]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::P0 => "#ef4444",
            Self::P1 => "#f59e0b",
            Self::P2 => "#4a9aff",
        }
    }
}

/// A single planning requirement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Requirement {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) category: RequirementCategory,
    pub(crate) status: RequirementStatus,
    pub(crate) priority: RequirementPriority,
    #[serde(default)]
    pub(crate) assigned_agent: Option<String>,
}

/// Status of an agent's category-change proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum ProposalStatus {
    /// Awaiting review.
    Pending,
    /// Category change accepted.
    Accepted,
    /// Category change rejected.
    Rejected,
}

/// Agent-proposed change to a requirement's category.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct CategoryProposal {
    pub(crate) id: String,
    pub(crate) requirement_id: String,
    pub(crate) requirement_title: String,
    pub(crate) current_category: RequirementCategory,
    pub(crate) proposed_category: RequirementCategory,
    pub(crate) agent_name: String,
    pub(crate) rationale: String,
    pub(crate) status: ProposalStatus,
}

/// Phase lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum PhaseStatus {
    /// Not yet started.
    Planned,
    /// Currently in progress.
    Active,
    /// Finished.
    Completed,
}

/// A roadmap phase.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Phase {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) start_date: String,
    #[serde(default)]
    pub(crate) end_date: String,
    pub(crate) status: PhaseStatus,
    #[serde(default)]
    pub(crate) progress: u8,
    #[serde(default)]
    pub(crate) requirements: Vec<String>,
}

/// Dependency edge between two phases.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct PhaseDependency {
    pub(crate) from_phase_id: String,
    pub(crate) to_phase_id: String,
}

/// Full roadmap returned from `GET /api/planning/projects/{id}/roadmap`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Roadmap {
    pub(crate) phases: Vec<Phase>,
    #[serde(default)]
    pub(crate) dependencies: Vec<PhaseDependency>,
}

/// Request body for accepting or rejecting a category proposal.
#[derive(Debug, Serialize)]
pub(crate) struct ProposalActionRequest {
    pub(crate) action: ProposalAction,
}

/// Action to take on a category proposal.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProposalAction {
    Accept,
    Reject,
}

/// Request body for updating a requirement field.
#[derive(Debug, Serialize)]
pub(crate) struct RequirementUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) category: Option<RequirementCategory>,
}

// --- Stores ---

/// Store for the project list.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectStore {
    pub(crate) projects: Vec<Project>,
}

impl ProjectStore {
    /// Progress percentage for a project (completed / total * 100).
    ///
    /// Returns 0 when the project has no requirements.
    #[must_use]
    pub(crate) fn progress_pct(project: &Project) -> u8 {
        if project.requirements_total == 0 {
            return 0;
        }
        // SAFETY: clamped to 0..=100, guaranteed to fit in u8.
        let pct = (u64::from(project.requirements_completed) * 100)
            / u64::from(project.requirements_total);
        u8::try_from(pct.min(100)).unwrap_or(0)
    }
}

/// Store for requirements and proposals of the active project.
#[derive(Debug, Clone, Default)]
pub(crate) struct RequirementStore {
    pub(crate) requirements: Vec<Requirement>,
    pub(crate) proposals: Vec<CategoryProposal>,
}

impl RequirementStore {
    /// Requirements in a specific category.
    #[must_use]
    pub(crate) fn by_category(&self, category: RequirementCategory) -> Vec<&Requirement> {
        self.requirements
            .iter()
            .filter(|r| r.category == category)
            .collect()
    }

    /// Pending category-change proposals.
    #[must_use]
    pub(crate) fn pending_proposals(&self) -> Vec<&CategoryProposal> {
        self.proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Pending)
            .collect()
    }

    /// Filter requirements by text query (matches title or description, case-insensitive).
    #[must_use]
    pub(crate) fn search(&self, query: &str) -> Vec<&Requirement> {
        if query.is_empty() {
            return self.requirements.iter().collect();
        }
        let lower = query.to_lowercase();
        self.requirements
            .iter()
            .filter(|r| {
                r.title.to_lowercase().contains(&lower)
                    || r.description.to_lowercase().contains(&lower)
            })
            .collect()
    }
}

/// Store for roadmap phases and dependencies.
#[derive(Debug, Clone, Default)]
pub(crate) struct RoadmapStore {
    pub(crate) roadmap: Option<Roadmap>,
}

impl RoadmapStore {
    /// Currently active phase.
    #[must_use]
    pub(crate) fn active_phase(&self) -> Option<&Phase> {
        self.roadmap
            .as_ref()?
            .phases
            .iter()
            .find(|p| p.status == PhaseStatus::Active)
    }

    /// All phases, if loaded.
    #[must_use]
    pub(crate) fn phases(&self) -> &[Phase] {
        match &self.roadmap {
            Some(r) => &r.phases,
            None => &[],
        }
    }

    /// All dependencies, if loaded.
    #[must_use]
    pub(crate) fn dependencies(&self) -> &[PhaseDependency] {
        match &self.roadmap {
            Some(r) => &r.dependencies,
            None => &[],
        }
    }
}

// --- Date helpers ---

/// Approximate day count between two ISO date strings (`YYYY-MM-DD`).
///
/// Returns a minimum of 1 day. Falls back to 30 days if parsing fails.
#[must_use]
pub(crate) fn days_between(start: &str, end: &str) -> u32 {
    match (date_to_days(start), date_to_days(end)) {
        (Some(s), Some(e)) if e > s => (e - s) as u32,
        (Some(_), Some(_)) => 1,
        _ => 30,
    }
}

/// Approximate days since epoch for a `YYYY-MM-DD` string.
fn date_to_days(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    // WHY: Approximation sufficient for pixel-based layout; exact calendaring not needed.
    Some(y * 365 + y / 4 - y / 100 + y / 400 + m * 30 + d)
}

/// Status badge style colors.
#[must_use]
pub(crate) fn status_badge_style(status: ProjectStatus) -> (&'static str, &'static str) {
    match status {
        ProjectStatus::Planning => ("#1a2a3a", "#4a9aff"),
        ProjectStatus::InProgress => ("#2a2a1a", "#f59e0b"),
        ProjectStatus::Completed => ("#1a3a1a", "#22c55e"),
        ProjectStatus::Paused => ("#2a2a3a", "#888"),
    }
}

/// Status label for display.
#[must_use]
pub(crate) fn status_label(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Planning => "Planning",
        ProjectStatus::InProgress => "In Progress",
        ProjectStatus::Completed => "Completed",
        ProjectStatus::Paused => "Paused",
    }
}

/// Phase status color.
#[must_use]
pub(crate) fn phase_status_color(status: PhaseStatus) -> &'static str {
    match status {
        PhaseStatus::Planned => "#2a2a3a",
        PhaseStatus::Active => "#1a2a3a",
        PhaseStatus::Completed => "#1a3a1a",
    }
}

/// Phase status border color.
#[must_use]
pub(crate) fn phase_border_color(status: PhaseStatus) -> &'static str {
    match status {
        PhaseStatus::Planned => "#444",
        PhaseStatus::Active => "#4a9aff",
        PhaseStatus::Completed => "#22c55e",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(id: &str, status: ProjectStatus, completed: u32, total: u32) -> Project {
        Project {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            status,
            current_phase: None,
            requirements_completed: completed,
            requirements_total: total,
            last_activity: None,
            active_agents: vec![],
        }
    }

    fn make_req(
        id: &str,
        category: RequirementCategory,
        status: RequirementStatus,
        priority: RequirementPriority,
    ) -> Requirement {
        Requirement {
            id: id.to_string(),
            title: format!("Req {id}"),
            description: format!("Description for {id}"),
            category,
            status,
            priority,
            assigned_agent: None,
        }
    }

    #[test]
    fn progress_pct_calculates_correctly() {
        let p = make_project("p1", ProjectStatus::InProgress, 3, 10);
        assert_eq!(ProjectStore::progress_pct(&p), 30, "3/10 = 30%");
    }

    #[test]
    fn progress_pct_zero_when_no_requirements() {
        let p = make_project("p1", ProjectStatus::InProgress, 0, 0);
        assert_eq!(ProjectStore::progress_pct(&p), 0, "0 total = 0%");
    }

    #[test]
    fn progress_pct_caps_at_100() {
        let p = make_project("p1", ProjectStatus::InProgress, 150, 100);
        assert_eq!(ProjectStore::progress_pct(&p), 100, "clamped to 100");
    }

    #[test]
    fn by_category_filters_correctly() {
        let store = RequirementStore {
            requirements: vec![
                make_req(
                    "r1",
                    RequirementCategory::V1,
                    RequirementStatus::Proposed,
                    RequirementPriority::P0,
                ),
                make_req(
                    "r2",
                    RequirementCategory::V2,
                    RequirementStatus::Accepted,
                    RequirementPriority::P1,
                ),
                make_req(
                    "r3",
                    RequirementCategory::V1,
                    RequirementStatus::Implemented,
                    RequirementPriority::P1,
                ),
            ],
            proposals: vec![],
        };
        assert_eq!(
            store.by_category(RequirementCategory::V1).len(),
            2,
            "two v1 reqs"
        );
        assert_eq!(
            store.by_category(RequirementCategory::V2).len(),
            1,
            "one v2 req"
        );
        assert_eq!(
            store.by_category(RequirementCategory::OutOfScope).len(),
            0,
            "none out-of-scope"
        );
    }

    #[test]
    fn pending_proposals_filters_only_pending() {
        let store = RequirementStore {
            requirements: vec![],
            proposals: vec![
                CategoryProposal {
                    id: "p1".into(),
                    requirement_id: "r1".into(),
                    requirement_title: "Req 1".into(),
                    current_category: RequirementCategory::V2,
                    proposed_category: RequirementCategory::V1,
                    agent_name: "agent-1".into(),
                    rationale: "critical for launch".into(),
                    status: ProposalStatus::Pending,
                },
                CategoryProposal {
                    id: "p2".into(),
                    requirement_id: "r2".into(),
                    requirement_title: "Req 2".into(),
                    current_category: RequirementCategory::V1,
                    proposed_category: RequirementCategory::OutOfScope,
                    agent_name: "agent-2".into(),
                    rationale: "not feasible".into(),
                    status: ProposalStatus::Rejected,
                },
            ],
        };
        assert_eq!(store.pending_proposals().len(), 1, "only pending proposals");
    }

    #[test]
    fn search_matches_title_and_description() {
        let store = RequirementStore {
            requirements: vec![
                make_req(
                    "r1",
                    RequirementCategory::V1,
                    RequirementStatus::Proposed,
                    RequirementPriority::P0,
                ),
                make_req(
                    "r2",
                    RequirementCategory::V2,
                    RequirementStatus::Accepted,
                    RequirementPriority::P1,
                ),
            ],
            proposals: vec![],
        };
        assert_eq!(store.search("r1").len(), 1, "matches title containing 'r1'");
        assert_eq!(
            store.search("description").len(),
            2,
            "both match description"
        );
        assert_eq!(store.search("").len(), 2, "empty query returns all");
    }

    #[test]
    fn roadmap_active_phase_finds_active() {
        let store = RoadmapStore {
            roadmap: Some(Roadmap {
                phases: vec![
                    Phase {
                        id: "ph1".into(),
                        name: "Design".into(),
                        start_date: "2026-01-01".into(),
                        end_date: "2026-02-01".into(),
                        status: PhaseStatus::Completed,
                        progress: 100,
                        requirements: vec![],
                    },
                    Phase {
                        id: "ph2".into(),
                        name: "Build".into(),
                        start_date: "2026-02-01".into(),
                        end_date: "2026-04-01".into(),
                        status: PhaseStatus::Active,
                        progress: 45,
                        requirements: vec![],
                    },
                ],
                dependencies: vec![],
            }),
        };
        let active = store.active_phase();
        assert!(active.is_some(), "should find active phase");
        assert_eq!(active.map(|p| p.id.as_str()), Some("ph2"), "correct phase");
    }

    #[test]
    fn roadmap_active_phase_none_when_empty() {
        assert!(
            RoadmapStore::default().active_phase().is_none(),
            "no roadmap loaded"
        );
    }

    #[test]
    fn days_between_valid_dates() {
        let days = days_between("2026-01-01", "2026-02-01");
        assert!(days > 20 && days < 40, "roughly 30 days: got {days}");
    }

    #[test]
    fn days_between_same_date_returns_minimum() {
        assert_eq!(days_between("2026-01-01", "2026-01-01"), 1, "min 1 day");
    }

    #[test]
    fn days_between_invalid_falls_back() {
        assert_eq!(days_between("invalid", "2026-01-01"), 30, "fallback to 30");
    }

    #[test]
    fn status_badge_style_returns_pair() {
        let (bg, fg) = status_badge_style(ProjectStatus::InProgress);
        assert!(!bg.is_empty(), "background color set");
        assert!(!fg.is_empty(), "foreground color set");
    }

    #[test]
    fn category_labels_display_correctly() {
        assert_eq!(RequirementCategory::V1.label(), "v1");
        assert_eq!(RequirementCategory::V2.label(), "v2");
        assert_eq!(RequirementCategory::OutOfScope.label(), "Out of Scope");
    }
}
