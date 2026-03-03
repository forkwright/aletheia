//! Project types and lifecycle management.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::Result;
use crate::phase::Phase;
use crate::state::{ProjectState, Transition};

/// A planning project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Ulid,
    pub name: String,
    pub description: String,
    pub scope: Option<String>,
    pub state: ProjectState,
    pub mode: ProjectMode,
    pub phases: Vec<Phase>,
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
    pub owner: String,
}

/// Operating modes for planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectMode {
    Full,
    Quick { appetite_minutes: u32 },
    Background,
}

impl Project {
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

    pub fn add_phase(&mut self, phase: Phase) {
        self.phases.push(phase);
        self.updated_at = jiff::Timestamp::now();
    }

    /// Get the current active phase (first non-complete phase).
    #[must_use]
    pub fn active_phase(&self) -> Option<&Phase> {
        self.phases
            .iter()
            .find(|p| !p.is_complete())
    }

    /// Get a mutable reference to the current active phase.
    pub fn active_phase_mut(&mut self) -> Option<&mut Phase> {
        self.phases
            .iter_mut()
            .find(|p| !p.is_complete())
    }
}

#[cfg(test)]
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
        // Small sleep to ensure timestamp differs
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
}
