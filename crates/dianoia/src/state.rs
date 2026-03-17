//! Project lifecycle state machine.

use serde::{Deserialize, Serialize};
use snafu::ensure;

use crate::error::{self, Result};

/// Project lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProjectState {
    /// Project has been created but work has not started.
    Created,
    /// Gathering clarifying questions about the project goals.
    Questioning,
    /// Researching the domain, codebase, or prior art.
    Researching,
    /// Defining the project scope and boundaries.
    Scoping,
    /// Breaking work into phases and plans.
    Planning,
    /// Reviewing and discussing the plan before execution.
    Discussing,
    /// Actively executing plans.
    Executing,
    /// Verifying that execution outcomes meet acceptance criteria.
    Verifying,
    /// All work completed and verified (terminal).
    Complete,
    /// Project was abandoned (terminal).
    Abandoned,
    /// Project is paused, remembering which state to resume to.
    Paused { previous: Box<ProjectState> },
}

/// Valid transitions between project states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transition {
    /// Move from Created to Questioning.
    StartQuestioning,
    /// Skip questioning and research, go directly to Scoping.
    SkipToResearch,
    /// Move from Questioning to Researching.
    StartResearch,
    /// Skip research, go directly to Scoping.
    SkipResearch,
    /// Move from Researching to Scoping.
    StartScoping,
    /// Move from Scoping to Planning.
    StartPlanning,
    /// Move from Planning to Discussing.
    StartDiscussion,
    /// Move from Planning or Discussing to Executing.
    StartExecution,
    /// Move from Executing to Verifying.
    StartVerification,
    /// Move from Verifying to Complete (terminal).
    Complete,
    /// Abandon the project from any non-terminal state (terminal).
    Abandon,
    /// Pause the project, preserving the current state for later resume.
    Pause,
    /// Resume a paused project to its previous state.
    Resume,
    /// Revert from Verifying to an earlier phase (Scoping, Planning, or Executing).
    Revert { to: ProjectState },
}

impl ProjectState {
    /// Attempt a state transition. Returns the new state or an error
    /// if the transition is invalid from the current state.
    pub fn transition(self, t: Transition) -> Result<Self> {
        match (&self, &t) {
            (Self::Created, Transition::StartQuestioning) => Ok(Self::Questioning),
            (Self::Questioning, Transition::StartResearch) => Ok(Self::Researching),
            (Self::Scoping, Transition::StartPlanning) => Ok(Self::Planning),
            (Self::Planning, Transition::StartDiscussion) => Ok(Self::Discussing),
            (Self::Executing, Transition::StartVerification) => Ok(Self::Verifying),
            (Self::Verifying, Transition::Complete) => Ok(Self::Complete),

            (Self::Created, Transition::SkipToResearch)
            | (Self::Questioning, Transition::SkipResearch)
            | (Self::Researching, Transition::StartScoping) => Ok(Self::Scoping),

            (Self::Planning | Self::Discussing, Transition::StartExecution) => Ok(Self::Executing),

            (
                Self::Verifying,
                Transition::Revert {
                    to: to @ (Self::Scoping | Self::Planning | Self::Executing),
                },
            ) => Ok(to.clone()),

            (
                Self::Researching
                | Self::Scoping
                | Self::Planning
                | Self::Discussing
                | Self::Executing,
                Transition::Pause,
            ) => Ok(Self::Paused {
                previous: Box::new(self),
            }),

            (Self::Paused { previous }, Transition::Resume) => Ok(*previous.clone()),

            (
                Self::Created
                | Self::Questioning
                | Self::Researching
                | Self::Scoping
                | Self::Planning
                | Self::Discussing
                | Self::Executing
                | Self::Verifying
                | Self::Paused { .. },
                Transition::Abandon,
            ) => Ok(Self::Abandoned),

            _ => {
                ensure!(
                    false,
                    error::InvalidTransitionSnafu {
                        state: self,
                        transition: t,
                    }
                );
                unreachable!()
            }
        }
    }

    /// List valid transitions from this state.
    #[must_use]
    pub fn valid_transitions(&self) -> Vec<Transition> {
        match self {
            Self::Created => vec![
                Transition::StartQuestioning,
                Transition::SkipToResearch,
                Transition::Abandon,
            ],
            Self::Questioning => vec![
                Transition::StartResearch,
                Transition::SkipResearch,
                Transition::Abandon,
            ],
            Self::Researching => vec![
                Transition::StartScoping,
                Transition::Abandon,
                Transition::Pause,
            ],
            Self::Scoping => vec![
                Transition::StartPlanning,
                Transition::Abandon,
                Transition::Pause,
            ],
            Self::Planning => vec![
                Transition::StartDiscussion,
                Transition::StartExecution,
                Transition::Abandon,
                Transition::Pause,
            ],
            Self::Discussing => vec![
                Transition::StartExecution,
                Transition::Abandon,
                Transition::Pause,
            ],
            Self::Executing => vec![
                Transition::StartVerification,
                Transition::Abandon,
                Transition::Pause,
            ],
            Self::Verifying => vec![Transition::Complete, Transition::Abandon],
            Self::Complete | Self::Abandoned => vec![],
            Self::Paused { .. } => vec![Transition::Resume, Transition::Abandon],
        }
    }

    /// Whether this state represents a terminal condition.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete | Self::Abandoned)
    }

    /// Whether work can happen in this state.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.is_terminal() && !matches!(self, Self::Paused { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_full_lifecycle() {
        let state = ProjectState::Created;
        let state = state
            .transition(Transition::StartQuestioning)
            .expect("Created -> StartQuestioning should succeed");
        assert_eq!(
            state,
            ProjectState::Questioning,
            "Created -> StartQuestioning should reach Questioning"
        );
        let state = state
            .transition(Transition::StartResearch)
            .expect("Questioning -> StartResearch should succeed");
        assert_eq!(
            state,
            ProjectState::Researching,
            "Questioning -> StartResearch should reach Researching"
        );
        let state = state
            .transition(Transition::StartScoping)
            .expect("Researching -> StartScoping should succeed");
        assert_eq!(
            state,
            ProjectState::Scoping,
            "Researching -> StartScoping should reach Scoping"
        );
        let state = state
            .transition(Transition::StartPlanning)
            .expect("Scoping -> StartPlanning should succeed");
        assert_eq!(
            state,
            ProjectState::Planning,
            "Scoping -> StartPlanning should reach Planning"
        );
        let state = state
            .transition(Transition::StartDiscussion)
            .expect("Planning -> StartDiscussion should succeed");
        assert_eq!(
            state,
            ProjectState::Discussing,
            "Planning -> StartDiscussion should reach Discussing"
        );
        let state = state
            .transition(Transition::StartExecution)
            .expect("Discussing -> StartExecution should succeed");
        assert_eq!(
            state,
            ProjectState::Executing,
            "Discussing -> StartExecution should reach Executing"
        );
        let state = state
            .transition(Transition::StartVerification)
            .expect("Executing -> StartVerification should succeed");
        assert_eq!(
            state,
            ProjectState::Verifying,
            "Executing -> StartVerification should reach Verifying"
        );
        let state = state
            .transition(Transition::Complete)
            .expect("Verifying -> Complete should succeed");
        assert_eq!(
            state,
            ProjectState::Complete,
            "Verifying -> Complete should reach Complete"
        );
    }

    #[test]
    fn skip_research() {
        let state = ProjectState::Created;
        let state = state
            .transition(Transition::SkipToResearch)
            .expect("Created -> SkipToResearch should succeed");
        assert_eq!(
            state,
            ProjectState::Scoping,
            "SkipToResearch from Created should reach Scoping"
        );
    }

    #[test]
    fn skip_discussion() {
        let state = ProjectState::Planning;
        let state = state
            .transition(Transition::StartExecution)
            .expect("Planning -> StartExecution should succeed");
        assert_eq!(
            state,
            ProjectState::Executing,
            "Planning -> StartExecution should reach Executing"
        );
    }

    #[test]
    fn invalid_transition_returns_error() {
        let state = ProjectState::Executing;
        let result = state.transition(Transition::StartQuestioning);
        assert!(
            result.is_err(),
            "StartQuestioning from Executing should return an error"
        );
    }

    #[test]
    fn pause_and_resume() {
        let state = ProjectState::Executing;
        let state = state
            .transition(Transition::Pause)
            .expect("Executing -> Pause should succeed");
        assert!(
            matches!(state, ProjectState::Paused { .. }),
            "Pausing Executing should produce a Paused state"
        );
        let state = state
            .transition(Transition::Resume)
            .expect("Paused -> Resume should succeed");
        assert_eq!(
            state,
            ProjectState::Executing,
            "Resuming from Paused(Executing) should return to Executing"
        );
    }

    #[test]
    fn pause_preserves_previous_state() {
        let state = ProjectState::Scoping;
        let state = state
            .transition(Transition::Pause)
            .expect("Scoping -> Pause should succeed");
        assert_eq!(
            state,
            ProjectState::Paused {
                previous: Box::new(ProjectState::Scoping)
            },
            "Pausing Scoping should preserve Scoping as previous state"
        );
    }

    #[test]
    fn abandon_from_various_states() {
        for start in [
            ProjectState::Created,
            ProjectState::Researching,
            ProjectState::Executing,
        ] {
            let state = start
                .transition(Transition::Abandon)
                .expect("Abandon should succeed from any non-terminal state");
            assert_eq!(
                state,
                ProjectState::Abandoned,
                "Abandon should always reach Abandoned"
            );
        }
    }

    #[test]
    fn terminal_states_reject_all_transitions() {
        for terminal in [ProjectState::Complete, ProjectState::Abandoned] {
            assert!(
                terminal
                    .clone()
                    .transition(Transition::StartQuestioning)
                    .is_err(),
                "StartQuestioning from terminal state {:?} should return an error",
                terminal
            );
            assert!(
                terminal.clone().transition(Transition::Abandon).is_err(),
                "Abandon from terminal state {:?} should return an error",
                terminal
            );
            assert!(
                terminal.clone().transition(Transition::Pause).is_err(),
                "Pause from terminal state {:?} should return an error",
                terminal
            );
        }
    }

    #[test]
    fn valid_transitions_per_state() {
        let created = ProjectState::Created;
        let transitions = created.valid_transitions();
        assert_eq!(
            transitions.len(),
            3,
            "Created should have exactly 3 valid transitions"
        );
        assert!(
            transitions.contains(&Transition::StartQuestioning),
            "Created valid transitions should include StartQuestioning"
        );
        assert!(
            transitions.contains(&Transition::SkipToResearch),
            "Created valid transitions should include SkipToResearch"
        );
        assert!(
            transitions.contains(&Transition::Abandon),
            "Created valid transitions should include Abandon"
        );

        assert!(
            ProjectState::Complete.valid_transitions().is_empty(),
            "Complete should have no valid transitions"
        );
        assert!(
            ProjectState::Abandoned.valid_transitions().is_empty(),
            "Abandoned should have no valid transitions"
        );

        let paused = ProjectState::Paused {
            previous: Box::new(ProjectState::Executing),
        };
        let transitions = paused.valid_transitions();
        assert_eq!(
            transitions.len(),
            2,
            "Paused should have exactly 2 valid transitions"
        );
        assert!(
            transitions.contains(&Transition::Resume),
            "Paused valid transitions should include Resume"
        );
        assert!(
            transitions.contains(&Transition::Abandon),
            "Paused valid transitions should include Abandon"
        );
    }

    #[test]
    fn revert_from_verifying() {
        let state = ProjectState::Verifying;
        let state = state
            .transition(Transition::Revert {
                to: ProjectState::Executing,
            })
            .expect("Verifying -> Revert(Executing) should succeed");
        assert_eq!(
            state,
            ProjectState::Executing,
            "Reverting from Verifying to Executing should reach Executing"
        );

        let state = ProjectState::Verifying;
        let state = state
            .transition(Transition::Revert {
                to: ProjectState::Planning,
            })
            .expect("Verifying -> Revert(Planning) should succeed");
        assert_eq!(
            state,
            ProjectState::Planning,
            "Reverting from Verifying to Planning should reach Planning"
        );

        let state = ProjectState::Verifying;
        let state = state
            .transition(Transition::Revert {
                to: ProjectState::Scoping,
            })
            .expect("Verifying -> Revert(Scoping) should succeed");
        assert_eq!(
            state,
            ProjectState::Scoping,
            "Reverting from Verifying to Scoping should reach Scoping"
        );
    }

    #[test]
    fn is_terminal() {
        assert!(
            ProjectState::Complete.is_terminal(),
            "Complete should be terminal"
        );
        assert!(
            ProjectState::Abandoned.is_terminal(),
            "Abandoned should be terminal"
        );
        assert!(
            !ProjectState::Executing.is_terminal(),
            "Executing should not be terminal"
        );
        assert!(
            !ProjectState::Created.is_terminal(),
            "Created should not be terminal"
        );
    }

    #[test]
    fn is_active() {
        assert!(
            ProjectState::Created.is_active(),
            "Created should be active"
        );
        assert!(
            ProjectState::Executing.is_active(),
            "Executing should be active"
        );
        assert!(
            !ProjectState::Complete.is_active(),
            "Complete should not be active"
        );
        assert!(
            !ProjectState::Abandoned.is_active(),
            "Abandoned should not be active"
        );
        assert!(
            !ProjectState::Paused {
                previous: Box::new(ProjectState::Executing)
            }
            .is_active(),
            "Paused should not be active"
        );
    }

    #[test]
    fn questioning_skip_research() {
        let state = ProjectState::Questioning;
        let state = state
            .transition(Transition::SkipResearch)
            .expect("Questioning -> SkipResearch should succeed");
        assert_eq!(
            state,
            ProjectState::Scoping,
            "SkipResearch from Questioning should reach Scoping"
        );
    }

    #[test]
    fn researching_to_scoping() {
        let state = ProjectState::Researching;
        let state = state
            .transition(Transition::StartScoping)
            .expect("Researching -> StartScoping should succeed");
        assert_eq!(
            state,
            ProjectState::Scoping,
            "Researching -> StartScoping should reach Scoping"
        );
    }

    #[test]
    fn pause_from_each_pausable_state() {
        for start in [
            ProjectState::Researching,
            ProjectState::Scoping,
            ProjectState::Planning,
            ProjectState::Discussing,
            ProjectState::Executing,
        ] {
            let paused = start
                .clone()
                .transition(Transition::Pause)
                .expect("Pause should succeed from a pausable state");
            assert!(
                matches!(paused, ProjectState::Paused { .. }),
                "Pausing should produce a Paused state"
            );
            if let ProjectState::Paused { previous } = paused {
                assert_eq!(
                    *previous, start,
                    "Paused previous should match the original state"
                );
            }
        }
    }

    #[test]
    fn cannot_pause_from_created() {
        let result = ProjectState::Created.transition(Transition::Pause);
        assert!(result.is_err(), "Pause from Created should return an error");
    }

    #[test]
    fn cannot_pause_from_questioning() {
        let result = ProjectState::Questioning.transition(Transition::Pause);
        assert!(
            result.is_err(),
            "Pause from Questioning should return an error"
        );
    }

    #[test]
    fn revert_to_invalid_state_rejected() {
        let state = ProjectState::Verifying;
        let result = state.transition(Transition::Revert {
            to: ProjectState::Researching,
        });
        assert!(
            result.is_err(),
            "Revert to Researching from Verifying should return an error"
        );
    }

    #[test]
    fn abandon_from_paused() {
        let state = ProjectState::Paused {
            previous: Box::new(ProjectState::Planning),
        };
        let state = state
            .transition(Transition::Abandon)
            .expect("Paused -> Abandon should succeed");
        assert_eq!(
            state,
            ProjectState::Abandoned,
            "Abandon from Paused should reach Abandoned"
        );
    }

    #[test]
    fn double_pause_not_possible() {
        let state = ProjectState::Paused {
            previous: Box::new(ProjectState::Executing),
        };
        let result = state.transition(Transition::Pause);
        assert!(
            result.is_err(),
            "Pausing an already-Paused state should return an error"
        );
    }

    #[test]
    fn complete_is_not_active() {
        assert!(
            !ProjectState::Complete.is_active(),
            "Complete should not be active"
        );
    }

    #[test]
    fn project_state_serde_roundtrip() {
        for state in [
            ProjectState::Created,
            ProjectState::Questioning,
            ProjectState::Researching,
            ProjectState::Scoping,
            ProjectState::Planning,
            ProjectState::Discussing,
            ProjectState::Executing,
            ProjectState::Verifying,
            ProjectState::Complete,
            ProjectState::Abandoned,
        ] {
            let json = serde_json::to_string(&state)
                .expect("serializing ProjectState to JSON should succeed");
            let back: ProjectState = serde_json::from_str(&json)
                .expect("deserializing ProjectState from JSON should succeed");
            assert_eq!(back, state, "roundtrip failed for {state:?}");
        }
    }

    #[test]
    fn paused_state_serde_roundtrip_preserves_previous() {
        let paused = ProjectState::Paused {
            previous: Box::new(ProjectState::Executing),
        };
        let json = serde_json::to_string(&paused)
            .expect("serializing Paused state to JSON should succeed");
        let back: ProjectState = serde_json::from_str(&json)
            .expect("deserializing Paused state from JSON should succeed");
        assert_eq!(
            back,
            ProjectState::Paused {
                previous: Box::new(ProjectState::Executing)
            },
            "serde roundtrip should preserve the Paused previous state"
        );
    }

    #[test]
    fn revert_then_complete_lifecycle() {
        let state = ProjectState::Verifying;
        let state = state
            .transition(Transition::Revert {
                to: ProjectState::Executing,
            })
            .expect("Verifying -> Revert(Executing) should succeed");
        assert_eq!(
            state,
            ProjectState::Executing,
            "Revert to Executing should reach Executing"
        );
        let state = state
            .transition(Transition::StartVerification)
            .expect("Executing -> StartVerification should succeed");
        assert_eq!(
            state,
            ProjectState::Verifying,
            "StartVerification from Executing should reach Verifying"
        );
        let state = state
            .transition(Transition::Complete)
            .expect("Verifying -> Complete should succeed");
        assert_eq!(
            state,
            ProjectState::Complete,
            "Complete from Verifying should reach Complete"
        );
    }

    #[test]
    fn cannot_revert_from_non_verifying_state() {
        let state = ProjectState::Executing;
        let result = state.transition(Transition::Revert {
            to: ProjectState::Scoping,
        });
        assert!(
            result.is_err(),
            "Revert from Executing (non-Verifying) should return an error"
        );
    }

    #[test]
    fn every_non_terminal_state_can_abandon() {
        let states = vec![
            ProjectState::Created,
            ProjectState::Questioning,
            ProjectState::Researching,
            ProjectState::Scoping,
            ProjectState::Planning,
            ProjectState::Discussing,
            ProjectState::Executing,
            ProjectState::Verifying,
            ProjectState::Paused {
                previous: Box::new(ProjectState::Scoping),
            },
        ];
        for state in states {
            let result = state.clone().transition(Transition::Abandon);
            assert!(result.is_ok(), "Abandon should succeed from {state:?}");
            assert_eq!(
                result.expect("Abandon transition result should be Ok"),
                ProjectState::Abandoned,
                "Abandon from {state:?} should reach Abandoned"
            );
        }
    }
}
