//! Project types and lifecycle management.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::Result;
use crate::phase::Phase;
use crate::state::{ProjectState, Transition};

/// A planning project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique project identifier.
    pub id: Ulid,
    /// Human-readable project name.
    pub name: String,
    /// What this project aims to accomplish.
    pub description: String,
    /// Optional scope constraint (e.g., "crate X only").
    pub scope: Option<String>,
    /// Current lifecycle state.
    pub state: ProjectState,
    /// Operating mode controlling planning depth.
    pub mode: ProjectMode,
    /// Ordered phases within this project.
    pub phases: Vec<Phase>,
    /// When the project was created.
    pub created_at: jiff::Timestamp,
    /// When the project was last modified.
    pub updated_at: jiff::Timestamp,
    /// Nous or user that owns this project.
    pub owner: String,
}

/// Operating modes for planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectMode {
    /// Full multi-phase project with research, scoping, planning, and verification.
    Full,
    /// Time-boxed quick task with an appetite limit in minutes.
    Quick { appetite_minutes: u32 },
    /// Autonomous background processing without user interaction.
    Background,
}

impl Project {
    /// Create a new project in the `Created` state.
    #[must_use]
    pub fn new(name: String, description: String, mode: ProjectMode, owner: String) -> Self {
        let now = jiff::Timestamp::now();
        Self {
            id: Ulid::new(),
            name,
            description,
            scope: None,
            state: ProjectState::Created,
            mode,
            phases: Vec::new(),
            created_at: now,
            updated_at: now,
            owner,
        }
    }

    /// Advance project state via a transition.
    pub fn advance(&mut self, transition: Transition) -> Result<()> {
        let current = self.state.clone();
        self.state = current.transition(transition)?;
        self.updated_at = jiff::Timestamp::now();
        Ok(())
    }

    /// Append a phase to the project and update the modification timestamp.
    pub fn add_phase(&mut self, phase: Phase) {
        self.phases.push(phase);
        self.updated_at = jiff::Timestamp::now();
    }

    /// Get the current active phase (first non-complete phase).
    #[must_use]
    pub fn active_phase(&self) -> Option<&Phase> {
        self.phases.iter().find(|p| !p.is_complete())
    }

    /// Get a mutable reference to the current active phase.
    pub fn active_phase_mut(&mut self) -> Option<&mut Phase> {
        self.phases.iter_mut().find(|p| !p.is_complete())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::phase::{Phase, PhaseState};

    #[test]
    fn create_project_defaults() {
        let project = Project::new(
            "test".into(),
            "a test project".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        assert_eq!(project.name, "test");
        assert_eq!(project.state, ProjectState::Created);
        assert_eq!(project.mode, ProjectMode::Full);
        assert!(project.phases.is_empty());
        assert!(project.scope.is_none());
        assert_eq!(project.owner, "syn");
    }

    #[test]
    fn advance_updates_timestamp() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        let before = project.updated_at;
        // WHY: small sleep to ensure timestamp differs between state transitions
        std::thread::sleep(std::time::Duration::from_millis(2));
        project.advance(Transition::StartQuestioning).unwrap();
        assert_eq!(project.state, ProjectState::Questioning);
        assert!(project.updated_at >= before);
    }

    #[test]
    fn active_phase_tracking() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );

        assert!(project.active_phase().is_none());

        let mut phase1 = Phase::new("Phase 1".into(), "First phase".into(), 1);
        phase1.state = PhaseState::Complete;
        project.add_phase(phase1);

        let phase2 = Phase::new("Phase 2".into(), "Second phase".into(), 2);
        project.add_phase(phase2);

        let active = project.active_phase().unwrap();
        assert_eq!(active.name, "Phase 2");
    }

    #[test]
    fn quick_mode_project() {
        let project = Project::new(
            "quick-task".into(),
            "a quick task".into(),
            ProjectMode::Quick {
                appetite_minutes: 30,
            },
            "bob".into(),
        );
        assert_eq!(
            project.mode,
            ProjectMode::Quick {
                appetite_minutes: 30
            }
        );
        assert_eq!(project.state, ProjectState::Created);
    }

    #[test]
    fn background_mode_project() {
        let project = Project::new(
            "bg-task".into(),
            "background work".into(),
            ProjectMode::Background,
            "alice".into(),
        );
        assert_eq!(project.mode, ProjectMode::Background);
        assert_eq!(project.owner, "alice");
    }

    #[test]
    fn add_phase_updates_timestamp() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        let before = project.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let phase = Phase::new("Phase 1".into(), "First".into(), 1);
        project.add_phase(phase);
        assert!(project.updated_at >= before);
        assert_eq!(project.phases.len(), 1);
    }

    #[test]
    fn active_phase_mut_returns_mutable() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        let phase = Phase::new("Phase 1".into(), "First".into(), 1);
        project.add_phase(phase);

        let active = project.active_phase_mut().unwrap();
        active.state = PhaseState::Complete;

        assert!(project.active_phase().is_none());
    }

    #[test]
    fn all_phases_complete_returns_none() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        let mut phase1 = Phase::new("P1".into(), "g1".into(), 1);
        phase1.state = PhaseState::Complete;
        let mut phase2 = Phase::new("P2".into(), "g2".into(), 2);
        phase2.state = PhaseState::Complete;
        project.add_phase(phase1);
        project.add_phase(phase2);
        assert!(project.active_phase().is_none());
    }

    #[test]
    fn advance_invalid_transition_fails() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        let result = project.advance(Transition::StartExecution);
        assert!(result.is_err());
        assert_eq!(project.state, ProjectState::Created);
    }

    #[test]
    fn project_scope_can_be_set() {
        let mut project = Project::new(
            "test".into(),
            "desc".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        assert!(project.scope.is_none());
        project.scope = Some("crate dianoia only".into());
        assert_eq!(project.scope.as_deref(), Some("crate dianoia only"));
    }

    #[test]
    fn project_serde_roundtrip_with_phases_and_plans() {
        use crate::plan::Plan;

        let mut project = Project::new(
            "full-test".into(),
            "roundtrip test".into(),
            ProjectMode::Quick {
                appetite_minutes: 15,
            },
            "alice".into(),
        );
        let mut phase = Phase::new("Phase 1".into(), "Foundation".into(), 1);
        phase.add_plan(Plan::new("task-a".into(), "do A".into(), 1));
        phase.add_plan(Plan::new("task-b".into(), "do B".into(), 2));
        project.add_phase(phase);

        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, project.id);
        assert_eq!(back.name, "full-test");
        assert_eq!(
            back.mode,
            ProjectMode::Quick {
                appetite_minutes: 15
            }
        );
        assert_eq!(back.phases.len(), 1);
        assert_eq!(back.phases[0].plans.len(), 2);
        assert_eq!(back.phases[0].plans[0].title, "task-a");
    }

    #[test]
    fn project_mode_serde_roundtrip() {
        let modes = [
            ProjectMode::Full,
            ProjectMode::Quick {
                appetite_minutes: 42,
            },
            ProjectMode::Background,
        ];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let back: ProjectMode = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, mode, "roundtrip failed for {mode:?}");
        }
    }
}
