#![expect(clippy::expect_used, reason = "test assertions")]

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
            "StartQuestioning from terminal state {terminal:?} should return an error"
        );
        assert!(
            terminal.clone().transition(Transition::Abandon).is_err(),
            "Abandon from terminal state {terminal:?} should return an error"
        );
        assert!(
            terminal.clone().transition(Transition::Pause).is_err(),
            "Pause from terminal state {terminal:?} should return an error"
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

// ── transition_gated ───────────────────────────────────────────────────────

#[test]
fn gated_advance_blocked_when_gate_fails() {
    use crate::gate::{GateCondition, PhaseGate};
    let gate = PhaseGate::new(
        ProjectState::Planning,
        vec![GateCondition::Custom("acceptance_criteria_defined".into())],
    );
    // Gate not satisfied — transition must be rejected.
    let result = ProjectState::Planning
        .transition_gated(Transition::StartExecution, Some(&gate));
    assert!(result.is_err(), "StartExecution should be blocked by an unsatisfied gate");
}

#[test]
fn gated_advance_allowed_when_gate_passes() {
    use crate::gate::{GateCondition, PhaseGate};
    let mut gate = PhaseGate::new(
        ProjectState::Planning,
        vec![GateCondition::Custom("acceptance_criteria_defined".into())],
    );
    gate.mark_satisfied("acceptance_criteria_defined");
    let result = ProjectState::Planning
        .transition_gated(Transition::StartExecution, Some(&gate));
    assert!(result.is_ok(), "StartExecution should succeed when gate is satisfied");
    assert_eq!(result.expect("gated transition should succeed"), ProjectState::Executing);
}

#[test]
fn gated_transition_with_no_gate_passes_through() {
    // No gate supplied — behaves identically to plain transition().
    let result = ProjectState::Executing
        .transition_gated(Transition::StartVerification, None);
    assert!(result.is_ok(), "StartVerification with no gate should succeed");
    assert_eq!(result.expect("ungated transition should succeed"), ProjectState::Verifying);
}

#[test]
fn non_advance_transition_bypasses_gate() {
    use crate::gate::{GateCondition, PhaseGate};
    // Gate is unsatisfied but Abandon is not an advance transition.
    let gate = PhaseGate::new(
        ProjectState::Executing,
        vec![GateCondition::TestsPassing],
    );
    let result = ProjectState::Executing
        .transition_gated(Transition::Abandon, Some(&gate));
    assert!(result.is_ok(), "Abandon should bypass an unsatisfied gate");
    assert_eq!(result.expect("Abandon should succeed"), ProjectState::Abandoned);
}

#[test]
fn complete_transition_blocked_by_unsatisfied_verifying_gate() {
    use crate::gate::{GateCondition, PhaseGate};
    let gate = PhaseGate::new(
        ProjectState::Verifying,
        vec![GateCondition::ReviewApproved],
    );
    let result = ProjectState::Verifying
        .transition_gated(Transition::Complete, Some(&gate));
    assert!(result.is_err(), "Complete should be blocked when review is not approved");
}

#[test]
fn complete_transition_allowed_when_review_approved() {
    use crate::gate::{GateCondition, PhaseGate};
    let mut gate = PhaseGate::new(
        ProjectState::Verifying,
        vec![GateCondition::ReviewApproved],
    );
    gate.mark_satisfied("review_approved");
    let result = ProjectState::Verifying
        .transition_gated(Transition::Complete, Some(&gate));
    assert!(result.is_ok(), "Complete should succeed when review is approved");
    assert_eq!(result.expect("gated Complete should succeed"), ProjectState::Complete);
}

#[test]
fn gated_transition_error_contains_failing_conditions() {
    use crate::gate::{GateCondition, PhaseGate};
    let gate = PhaseGate::new(
        ProjectState::Executing,
        vec![GateCondition::TestsPassing, GateCondition::LintClean],
    );
    let err = ProjectState::Executing
        .transition_gated(Transition::StartVerification, Some(&gate))
        .expect_err("should fail with GateBlocked error");
    let msg = err.to_string();
    assert!(
        msg.contains("tests_passing") || msg.contains("lint_clean"),
        "error message should name failing conditions, got: {msg}"
    );
}
