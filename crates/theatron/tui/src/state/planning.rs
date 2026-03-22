//! State for the planning dashboard and retrospective views.

/// Which tab of the planning dashboard is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum PlanningTab {
    Overview,
    Requirements,
    Execution,
    Verification,
    Tasks,
    Timeline,
    EditHistory,
}

impl PlanningTab {
    pub(crate) const ALL: [Self; 7] = [
        Self::Overview,
        Self::Requirements,
        Self::Execution,
        Self::Verification,
        Self::Tasks,
        Self::Timeline,
        Self::EditHistory,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Requirements => "Requirements",
            Self::Execution => "Execution",
            Self::Verification => "Verification",
            Self::Tasks => "Tasks",
            Self::Timeline => "Timeline",
            Self::EditHistory => "History",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Overview => Self::Requirements,
            Self::Requirements => Self::Execution,
            Self::Execution => Self::Verification,
            Self::Verification => Self::Tasks,
            Self::Tasks => Self::Timeline,
            Self::Timeline => Self::EditHistory,
            Self::EditHistory => Self::Overview,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Overview => Self::EditHistory,
            Self::Requirements => Self::Overview,
            Self::Execution => Self::Requirements,
            Self::Verification => Self::Execution,
            Self::Tasks => Self::Verification,
            Self::Timeline => Self::Tasks,
            Self::EditHistory => Self::Timeline,
        }
    }
}

/// Risk level for a checkpoint requiring human approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "variants constructed when API populates planning data"
    )
)]
pub(crate) enum CheckpointRisk {
    Low,
    Medium,
    High,
    Critical,
}

impl CheckpointRisk {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MED",
            Self::High => "HIGH",
            Self::Critical => "CRIT",
        }
    }
}

/// Status of a requirement in the requirements table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
// WHY: variant construction depends on API data; subset used in tests.
#[allow(dead_code)]
pub(crate) enum RequirementStatus {
    Pending,
    InProgress,
    Met,
    Failed,
    Deferred,
}

impl RequirementStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In Progress",
            Self::Met => "Met",
            Self::Failed => "Failed",
            Self::Deferred => "Deferred",
        }
    }
}

/// Display-friendly project state (decoupled from dianoia domain types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
// WHY: variant construction depends on API data; some variants are only used in tests.
#[allow(dead_code)]
pub(crate) enum DisplayProjectState {
    Created,
    Planning,
    Executing,
    Verifying,
    Complete,
    Paused,
    Failed,
}

impl DisplayProjectState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Planning => "Planning",
            Self::Executing => "Executing",
            Self::Verifying => "Verifying",
            Self::Complete => "Complete",
            Self::Paused => "Paused",
            Self::Failed => "Failed",
        }
    }
}

/// Display-friendly phase state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
// WHY: variant construction depends on API data; subset used in tests.
#[allow(dead_code)]
pub(crate) enum DisplayPhaseState {
    Pending,
    Active,
    Executing,
    Verifying,
    Complete,
    Failed,
}

impl DisplayPhaseState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Active => "Active",
            Self::Executing => "Executing",
            Self::Verifying => "Verifying",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
        }
    }
}

/// Display-friendly plan/task state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
// WHY: variant construction depends on API data; some variants are only used in tests.
#[allow(dead_code)]
pub(crate) enum DisplayPlanState {
    Pending,
    Ready,
    Executing,
    Complete,
    Failed,
    Skipped,
    Stuck,
}

impl DisplayPlanState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Ready => "Ready",
            Self::Executing => "Executing",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
            Self::Stuck => "Stuck",
        }
    }
}

/// A project for display in the planning dashboard.
#[derive(Debug, Clone)]
pub(crate) struct DisplayProject {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) state: DisplayProjectState,
    pub(crate) phases: Vec<DisplayPhase>,
    pub(crate) checkpoints: Vec<DisplayCheckpoint>,
    pub(crate) requirements: Vec<DisplayRequirement>,
    pub(crate) verifications: Vec<DisplayVerification>,
    pub(crate) tasks: Vec<DisplayTask>,
    pub(crate) milestones: Vec<DisplayMilestone>,
    pub(crate) edit_history: Vec<DisplayEditEntry>,
}

/// A phase within a project.
#[derive(Debug, Clone)]
pub(crate) struct DisplayPhase {
    pub(crate) name: String,
    pub(crate) state: DisplayPhaseState,
    pub(crate) completion_pct: u8,
    pub(crate) plans: Vec<DisplayPlan>,
}

/// A plan (unit of work) within a phase.
#[derive(Debug, Clone)]
pub(crate) struct DisplayPlan {
    pub(crate) title: String,
    pub(crate) wave: u32,
    pub(crate) state: DisplayPlanState,
    pub(crate) depends_on: Vec<String>,
}

/// A requirement tracked in the requirements table.
#[derive(Debug, Clone)]
pub(crate) struct DisplayRequirement {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) status: RequirementStatus,
    pub(crate) source: String,
}

/// A verification result.
#[derive(Debug, Clone)]
pub(crate) struct DisplayVerification {
    pub(crate) name: String,
    pub(crate) passed: bool,
    pub(crate) evidence: String,
}

/// A task in the hierarchical task list.
#[derive(Debug, Clone)]
pub(crate) struct DisplayTask {
    pub(crate) title: String,
    pub(crate) state: DisplayPlanState,
    pub(crate) depth: u8,
    pub(crate) blocked_by: Vec<String>,
}

/// A milestone on the timeline.
#[derive(Debug, Clone)]
pub(crate) struct DisplayMilestone {
    pub(crate) label: String,
    pub(crate) timestamp: String,
    pub(crate) completed: bool,
}

/// A checkpoint requiring human approval.
#[derive(Debug, Clone)]
pub(crate) struct DisplayCheckpoint {
    pub(crate) description: String,
    pub(crate) risk: CheckpointRisk,
    pub(crate) approved: bool,
}

/// An entry in the edit history log.
#[derive(Debug, Clone)]
pub(crate) struct DisplayEditEntry {
    pub(crate) timestamp: String,
    pub(crate) description: String,
    pub(crate) author: String,
}

/// Full planning dashboard state.
#[derive(Debug, Clone)]
pub struct PlanningDashboardState {
    pub(crate) project: Option<DisplayProject>,
    pub(crate) tab: PlanningTab,
    pub(crate) selected_row: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) expanded_phases: Vec<bool>,
    pub(crate) checkpoint_cursor: usize,
    pub(crate) loading: bool,
}

impl PlanningDashboardState {
    pub(crate) fn new() -> Self {
        Self {
            project: None,
            tab: PlanningTab::Overview,
            selected_row: 0,
            scroll_offset: 0,
            expanded_phases: Vec::new(),
            checkpoint_cursor: 0,
            loading: false,
        }
    }

    pub(crate) fn tab_next(&mut self) {
        self.tab = self.tab.next();
        self.selected_row = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn tab_prev(&mut self) {
        self.tab = self.tab.prev();
        self.selected_row = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn select_up(&mut self) {
        self.selected_row = self.selected_row.saturating_sub(1);
    }

    pub(crate) fn select_down(&mut self) {
        let max = self.row_count().saturating_sub(1);
        if self.selected_row < max {
            self.selected_row += 1;
        }
    }

    pub(crate) fn toggle_phase(&mut self) {
        if let Some(expanded) = self.expanded_phases.get_mut(self.selected_row) {
            *expanded = !*expanded;
        }
    }

    /// Approve the currently focused checkpoint.
    pub(crate) fn approve_checkpoint(&mut self) -> bool {
        if let Some(ref mut project) = self.project
            && let Some(cp) = project.checkpoints.get_mut(self.checkpoint_cursor)
            && !cp.approved
        {
            cp.approved = true;
            return true;
        }
        false
    }

    /// Whether there are any unapproved checkpoints blocking execution.
    pub(crate) fn has_pending_checkpoints(&self) -> bool {
        self.project
            .as_ref()
            .is_some_and(|p| p.checkpoints.iter().any(|c| !c.approved))
    }

    /// Number of rows in the current tab's data.
    pub(crate) fn row_count(&self) -> usize {
        let Some(ref project) = self.project else {
            return 0;
        };
        match self.tab {
            PlanningTab::Overview => project.phases.len(),
            PlanningTab::Requirements => project.requirements.len(),
            PlanningTab::Execution => project.phases.len(),
            PlanningTab::Verification => project.verifications.len(),
            PlanningTab::Tasks => project.tasks.len(),
            PlanningTab::Timeline => project.milestones.len(),
            PlanningTab::EditHistory => project.edit_history.len(),
        }
    }
}

impl Default for PlanningDashboardState {
    fn default() -> Self {
        Self::new()
    }
}

/// A decision made during project execution, for the retrospective audit trail.
#[derive(Debug, Clone)]
pub(crate) struct RetrospectiveDecision {
    pub(crate) timestamp: String,
    pub(crate) question: String,
    pub(crate) choice: String,
    pub(crate) rationale: String,
}

/// Data for the retrospective post-mortem view.
#[derive(Debug, Clone)]
pub(crate) struct RetrospectiveEntry {
    pub(crate) project_name: String,
    pub(crate) successes: Vec<String>,
    pub(crate) blockers: Vec<String>,
    pub(crate) lessons: Vec<String>,
    pub(crate) decisions: Vec<RetrospectiveDecision>,
}

/// Which section of the retrospective is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum RetrospectiveSection {
    Successes,
    Blockers,
    Lessons,
    Decisions,
}

impl RetrospectiveSection {
    #[allow(dead_code)]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Successes => "Successes",
            Self::Blockers => "Blockers",
            Self::Lessons => "Lessons",
            Self::Decisions => "Decisions",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Successes => Self::Blockers,
            Self::Blockers => Self::Lessons,
            Self::Lessons => Self::Decisions,
            Self::Decisions => Self::Successes,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Successes => Self::Decisions,
            Self::Blockers => Self::Successes,
            Self::Lessons => Self::Blockers,
            Self::Decisions => Self::Lessons,
        }
    }
}

/// Retrospective view state.
#[derive(Debug, Clone)]
pub struct RetrospectiveState {
    pub(crate) entry: Option<RetrospectiveEntry>,
    pub(crate) scroll_offset: usize,
    pub(crate) selected_section: RetrospectiveSection,
    pub(crate) loading: bool,
}

impl RetrospectiveState {
    pub(crate) fn new() -> Self {
        Self {
            entry: None,
            scroll_offset: 0,
            selected_section: RetrospectiveSection::Successes,
            loading: false,
        }
    }

    pub(crate) fn section_next(&mut self) {
        self.selected_section = self.selected_section.next();
    }

    pub(crate) fn section_prev(&mut self) {
        self.selected_section = self.selected_section.prev();
    }
}

impl Default for RetrospectiveState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_tab_cycles_forward() {
        let mut tab = PlanningTab::Overview;
        for _ in 0..7 {
            tab = tab.next();
        }
        assert_eq!(tab, PlanningTab::Overview);
    }

    #[test]
    fn planning_tab_cycles_backward() {
        let mut tab = PlanningTab::Overview;
        for _ in 0..7 {
            tab = tab.prev();
        }
        assert_eq!(tab, PlanningTab::Overview);
    }

    #[test]
    fn planning_tab_next_prev_inverse() {
        for tab in PlanningTab::ALL {
            assert_eq!(tab.next().prev(), tab);
        }
    }

    #[test]
    fn planning_tab_all_has_seven_entries() {
        assert_eq!(PlanningTab::ALL.len(), 7);
    }

    #[test]
    fn planning_tab_labels_non_empty() {
        for tab in PlanningTab::ALL {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn dashboard_state_default() {
        let state = PlanningDashboardState::new();
        assert!(state.project.is_none());
        assert_eq!(state.tab, PlanningTab::Overview);
        assert_eq!(state.selected_row, 0);
        assert!(!state.loading);
    }

    #[test]
    fn select_up_saturates_at_zero() {
        let mut state = PlanningDashboardState::new();
        state.select_up();
        assert_eq!(state.selected_row, 0);
    }

    #[test]
    fn select_down_clamps_at_max() {
        let mut state = PlanningDashboardState::new();
        state.project = Some(sample_project());
        let max = state.row_count().saturating_sub(1);
        for _ in 0..100 {
            state.select_down();
        }
        assert_eq!(state.selected_row, max);
    }

    #[test]
    fn tab_next_resets_selection() {
        let mut state = PlanningDashboardState::new();
        state.selected_row = 5;
        state.tab_next();
        assert_eq!(state.selected_row, 0);
        assert_eq!(state.tab, PlanningTab::Requirements);
    }

    #[test]
    fn approve_checkpoint_flips_flag() {
        let mut state = PlanningDashboardState::new();
        state.project = Some(sample_project());
        assert!(state.has_pending_checkpoints());
        assert!(state.approve_checkpoint());
        assert!(!state.has_pending_checkpoints());
    }

    #[test]
    fn approve_already_approved_returns_false() {
        let mut state = PlanningDashboardState::new();
        state.project = Some(sample_project());
        state.approve_checkpoint();
        assert!(!state.approve_checkpoint());
    }

    #[test]
    fn toggle_phase_toggles_expanded() {
        let mut state = PlanningDashboardState::new();
        state.expanded_phases = vec![false, true];
        state.selected_row = 0;
        state.toggle_phase();
        assert!(state.expanded_phases[0]);
        state.toggle_phase();
        assert!(!state.expanded_phases[0]);
    }

    #[test]
    fn row_count_with_no_project_is_zero() {
        let state = PlanningDashboardState::new();
        assert_eq!(state.row_count(), 0);
    }

    #[test]
    fn retrospective_section_cycles() {
        let mut section = RetrospectiveSection::Successes;
        for _ in 0..4 {
            section = section.next();
        }
        assert_eq!(section, RetrospectiveSection::Successes);
    }

    #[test]
    fn retrospective_section_next_prev_inverse() {
        let sections = [
            RetrospectiveSection::Successes,
            RetrospectiveSection::Blockers,
            RetrospectiveSection::Lessons,
            RetrospectiveSection::Decisions,
        ];
        for section in sections {
            assert_eq!(section.next().prev(), section);
        }
    }

    #[test]
    fn retrospective_state_default() {
        let state = RetrospectiveState::new();
        assert!(state.entry.is_none());
        assert_eq!(state.selected_section, RetrospectiveSection::Successes);
    }

    #[test]
    fn checkpoint_risk_labels() {
        assert_eq!(CheckpointRisk::Low.label(), "LOW");
        assert_eq!(CheckpointRisk::Medium.label(), "MED");
        assert_eq!(CheckpointRisk::High.label(), "HIGH");
        assert_eq!(CheckpointRisk::Critical.label(), "CRIT");
    }

    #[test]
    fn requirement_status_labels() {
        assert_eq!(RequirementStatus::Pending.label(), "Pending");
        assert_eq!(RequirementStatus::Met.label(), "Met");
        assert_eq!(RequirementStatus::Failed.label(), "Failed");
    }

    #[test]
    fn display_project_state_labels() {
        assert_eq!(DisplayProjectState::Created.label(), "Created");
        assert_eq!(DisplayProjectState::Executing.label(), "Executing");
        assert_eq!(DisplayProjectState::Complete.label(), "Complete");
    }

    fn sample_project() -> DisplayProject {
        DisplayProject {
            name: "Test Project".into(),
            description: "A test project".into(),
            state: DisplayProjectState::Executing,
            phases: vec![DisplayPhase {
                name: "Phase 1".into(),
                state: DisplayPhaseState::Active,
                completion_pct: 50,
                plans: vec![DisplayPlan {
                    title: "Task A".into(),
                    wave: 1,
                    state: DisplayPlanState::Executing,
                    depends_on: vec![],
                }],
            }],
            checkpoints: vec![DisplayCheckpoint {
                description: "Approve deployment".into(),
                risk: CheckpointRisk::High,
                approved: false,
            }],
            requirements: vec![DisplayRequirement {
                id: "REQ-001".into(),
                description: "Must pass tests".into(),
                status: RequirementStatus::Pending,
                source: "spec".into(),
            }],
            verifications: vec![DisplayVerification {
                name: "Unit tests".into(),
                passed: true,
                evidence: "All 42 tests passed".into(),
            }],
            tasks: vec![DisplayTask {
                title: "Implement feature".into(),
                state: DisplayPlanState::Executing,
                depth: 0,
                blocked_by: vec![],
            }],
            milestones: vec![DisplayMilestone {
                label: "MVP".into(),
                timestamp: "2026-03-20T10:00:00Z".into(),
                completed: false,
            }],
            edit_history: vec![DisplayEditEntry {
                timestamp: "2026-03-19T09:00:00Z".into(),
                description: "Initial plan created".into(),
                author: "agent".into(),
            }],
        }
    }
}
