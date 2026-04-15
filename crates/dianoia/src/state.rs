//! Project lifecycle state machine.

use serde::{Deserialize, Serialize};
use snafu::ensure;

use crate::error::{self, GateBlockedSnafu, Result};
use crate::gate::{evaluate_gate, GateResult, PhaseGate};

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
    Paused {
        /// The state to resume to when the project is unpaused.
        previous: Box<ProjectState>,
    },
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
    Revert {
        /// The state to revert to.
        to: ProjectState,
    },
}

impl ProjectState {
    /// Attempt a state transition. Returns the new state or an error
    /// if the transition is invalid from the current state.
    pub fn transition(self, t: Transition) -> Result<Self> {
        let from_label = state_label(&self);
        let result = match (&self, &t) {
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
        };

        if let Ok(ref new_state) = result {
            crate::metrics::record_phase_transition(&from_label, &state_label(new_state));
        }

        result
    }

    /// Attempt a state transition, enforcing a [`PhaseGate`] for advance transitions.
    ///
    /// If `gate` is `Some`, it is evaluated before any advance transition
    /// (`StartExecution`, `StartVerification`, `Complete`). The gate must pass;
    /// otherwise the transition is rejected with [`error::Error::GateBlocked`].
    ///
    /// Non-advance transitions (`Abandon`, `Pause`, `Resume`, `Revert`, …) bypass
    /// the gate — gates only guard forward progress, not escape hatches.
    pub fn transition_gated(self, t: Transition, gate: Option<&PhaseGate>) -> Result<Self> {
        let is_advance = matches!(
            &t,
            Transition::StartExecution
                | Transition::StartVerification
                | Transition::Complete
        );

        if is_advance
            && let Some(g) = gate
            && let GateResult::Fail(conditions) = evaluate_gate(g)
        {
            return GateBlockedSnafu {
                state: self,
                conditions,
            }
            .fail();
        }

        self.transition(t)
    }

    /// List valid transitions from this state.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: project state machine")
    )]
    pub(crate) fn valid_transitions(&self) -> Vec<Transition> {
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
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete | Self::Abandoned)
    }

    /// Whether work can happen in this state.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: planning orchestration")
    )]
    pub(crate) fn is_active(&self) -> bool {
        !self.is_terminal() && !matches!(self, Self::Paused { .. })
    }
}

/// Convert a project state to a short lowercase label for metrics.
fn state_label(state: &ProjectState) -> String {
    match state {
        ProjectState::Created => "created".to_owned(),
        ProjectState::Questioning => "questioning".to_owned(),
        ProjectState::Researching => "researching".to_owned(),
        ProjectState::Scoping => "scoping".to_owned(),
        ProjectState::Planning => "planning".to_owned(),
        ProjectState::Discussing => "discussing".to_owned(),
        ProjectState::Executing => "executing".to_owned(),
        ProjectState::Verifying => "verifying".to_owned(),
        ProjectState::Complete => "complete".to_owned(),
        ProjectState::Abandoned => "abandoned".to_owned(),
        ProjectState::Paused { .. } => "paused".to_owned(),
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
