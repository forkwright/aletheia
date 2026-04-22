#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: index safety verified by assertions"
)]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs files share the same import block"
)]

use dianoia::gate::{GateCondition, GateResult, PhaseGate, default_gate, evaluate_gate};
use dianoia::phase::{Phase, PhaseState};
use dianoia::plan::{Blocker, Plan, PlanState};
use dianoia::project::{Project, ProjectMode};
use dianoia::research::{FindingStatus, ResearchDomain, ResearchFinding, ResearchOutput};
use dianoia::state::{ProjectState, Transition};
use dianoia::workspace::ProjectWorkspace;

// Split: Project/Phase/Plan/State-transition tests.

// =============================================================================
// Project Constructors and Basic Properties
// =============================================================================

#[test]
fn project_new_full_mode() {
    let project = Project::new(
        "Test Project".into(),
        "A test project description".into(),
        ProjectMode::Full,
        "test-owner".into(),
    );

    assert_eq!(project.name, "Test Project");
    assert_eq!(project.description, "A test project description");
    assert_eq!(project.mode, ProjectMode::Full);
    assert_eq!(project.owner, "test-owner");
    assert_eq!(project.state, ProjectState::Created);
    assert!(project.phases.is_empty());
    assert!(project.scope.is_none());
}

#[test]
fn project_new_quick_mode() {
    let project = Project::new(
        "Quick Task".into(),
        "A quick task".into(),
        ProjectMode::Quick {
            appetite_minutes: 30,
        },
        "owner".into(),
    );

    assert_eq!(project.name, "Quick Task");
    assert_eq!(
        project.mode,
        ProjectMode::Quick {
            appetite_minutes: 30
        }
    );
}

#[test]
fn project_new_background_mode() {
    let project = Project::new(
        "Background Task".into(),
        "Autonomous background work".into(),
        ProjectMode::Background,
        "system".into(),
    );

    assert_eq!(project.mode, ProjectMode::Background);
    assert_eq!(project.owner, "system");
}

#[test]
fn project_add_phase_updates_state() {
    let mut project = Project::new(
        "Test".into(),
        "desc".into(),
        ProjectMode::Full,
        "owner".into(),
    );

    let phase = Phase::new("Phase 1".into(), "First phase".into(), 1);
    project.add_phase(phase);

    assert_eq!(project.phases.len(), 1);
    assert_eq!(project.phases[0].name, "Phase 1");
}

// =============================================================================
// Phase Constructors and State
// =============================================================================

#[test]
fn phase_new_defaults() {
    let phase = Phase::new("Foundation".into(), "Build the foundation".into(), 1);

    assert_eq!(phase.name, "Foundation");
    assert_eq!(phase.goal, "Build the foundation");
    assert_eq!(phase.order, 1);
    assert_eq!(phase.state, PhaseState::Pending);
    assert!(phase.plans.is_empty());
    assert!(phase.requirements.is_empty());
}

#[test]
fn phase_state_variants_equality() {
    assert_eq!(PhaseState::Pending, PhaseState::Pending);
    assert_eq!(PhaseState::Complete, PhaseState::Complete);
    assert_eq!(
        PhaseState::Failed { can_retry: true },
        PhaseState::Failed { can_retry: true }
    );
    assert_ne!(
        PhaseState::Failed { can_retry: true },
        PhaseState::Failed { can_retry: false }
    );
}

// =============================================================================
// Plan Constructors and State
// =============================================================================

#[test]
fn plan_new_defaults() {
    let plan = Plan::new("Do Something".into(), "Detailed description".into(), 1);

    assert_eq!(plan.title, "Do Something");
    assert_eq!(plan.description, "Detailed description");
    assert_eq!(plan.wave, 1);
    assert_eq!(plan.state, PlanState::Pending);
    assert_eq!(plan.iterations, 0);
    assert_eq!(plan.max_iterations, 10); // default
    assert!(plan.depends_on.is_empty());
    assert!(plan.blockers.is_empty());
    assert!(plan.achievements.is_empty());
}

#[test]
fn plan_state_variants() {
    assert_ne!(PlanState::Pending, PlanState::Ready);
    assert_ne!(PlanState::Executing, PlanState::Complete);
    assert_ne!(PlanState::Failed, PlanState::Skipped);
    assert_ne!(PlanState::Stuck, PlanState::Complete);
}

#[test]
fn plan_from_research_generates_pending_plans() {
    let research = ResearchOutput {
        findings: vec![
            ResearchFinding {
                domain: ResearchDomain::Architecture,
                content: "Use actor model".into(),
                status: FindingStatus::Complete,
            },
            ResearchFinding {
                domain: ResearchDomain::Pitfalls,
                content: "Beware of race conditions".into(),
                status: FindingStatus::Partial,
            },
            ResearchFinding {
                domain: ResearchDomain::Stack,
                content: "Timeout".into(),
                status: FindingStatus::TimedOut,
            },
        ],
        markdown: String::new(),
    };

    let plans = Plan::from_research(&research);
    assert_eq!(plans.len(), 2);

    assert_eq!(plans[0].title, "Architecture");
    assert_eq!(plans[0].description, "Use actor model");
    assert_eq!(plans[0].wave, 0);
    assert_eq!(plans[0].state, PlanState::Pending);

    assert_eq!(plans[1].title, "Pitfalls");
    assert_eq!(plans[1].description, "Beware of race conditions");
    assert_eq!(plans[1].wave, 0);
    assert_eq!(plans[1].state, PlanState::Pending);
}

#[test]
fn plan_from_template_copies_fields_and_resets_state() {
    let completed = Plan::new("Refactor auth".into(), "Extract auth module".into(), 1);
    let next = Plan::from_template(&completed, 2);

    assert_eq!(next.title, "Refactor auth");
    assert_eq!(next.description, "Extract auth module");
    assert_eq!(next.wave, 2);
    assert_eq!(next.state, PlanState::Pending);
    assert_eq!(next.iterations, 0);
    assert!(next.depends_on.is_empty());
    assert!(next.blockers.is_empty());
    assert!(next.achievements.is_empty());
    assert_eq!(next.max_iterations, completed.max_iterations);
    assert_ne!(next.id, completed.id);
}

// =============================================================================
// Project State Machine Transitions
// =============================================================================

#[test]
fn project_state_full_lifecycle() {
    let state = ProjectState::Created;

    let state = state
        .transition_gated(Transition::StartQuestioning, None)
        .expect("Created -> Questioning");
    assert_eq!(state, ProjectState::Questioning);

    let state = state
        .transition_gated(Transition::StartResearch, None)
        .expect("Questioning -> Researching");
    assert_eq!(state, ProjectState::Researching);

    let state = state
        .transition_gated(Transition::StartScoping, None)
        .expect("Researching -> Scoping");
    assert_eq!(state, ProjectState::Scoping);

    let state = state
        .transition_gated(Transition::StartPlanning, None)
        .expect("Scoping -> Planning");
    assert_eq!(state, ProjectState::Planning);

    let state = state
        .transition_gated(Transition::StartDiscussion, None)
        .expect("Planning -> Discussing");
    assert_eq!(state, ProjectState::Discussing);

    let state = state
        .transition_gated(Transition::StartExecution, None)
        .expect("Discussing -> Executing");
    assert_eq!(state, ProjectState::Executing);

    let state = state
        .transition_gated(Transition::StartVerification, None)
        .expect("Executing -> Verifying");
    assert_eq!(state, ProjectState::Verifying);

    let state = state
        .transition_gated(Transition::Complete, None)
        .expect("Verifying -> Complete");
    assert_eq!(state, ProjectState::Complete);
}

#[test]
fn project_state_skip_paths() {
    // Skip from Created directly to Scoping
    let state = ProjectState::Created
        .transition_gated(Transition::SkipToResearch, None)
        .expect("Created -> Scoping (skip)");
    assert_eq!(state, ProjectState::Scoping);

    // Skip from Questioning to Scoping
    let state = ProjectState::Questioning
        .transition_gated(Transition::SkipResearch, None)
        .expect("Questioning -> Scoping (skip)");
    assert_eq!(state, ProjectState::Scoping);

    // Skip from Planning to Executing
    let state = ProjectState::Planning
        .transition_gated(Transition::StartExecution, None)
        .expect("Planning -> Executing (skip discussion)");
    assert_eq!(state, ProjectState::Executing);
}

#[test]
fn project_state_pause_and_resume() {
    let original = ProjectState::Executing;
    let paused = original
        .transition_gated(Transition::Pause, None)
        .expect("Executing -> Pause");

    assert!(
        matches!(paused, ProjectState::Paused { .. }),
        "Expected Paused state"
    );

    let resumed = paused
        .transition_gated(Transition::Resume, None)
        .expect("Paused -> Resume");
    assert_eq!(resumed, ProjectState::Executing);
}

#[test]
fn project_state_abandon_from_any_state() {
    for state in [
        ProjectState::Created,
        ProjectState::Questioning,
        ProjectState::Researching,
        ProjectState::Scoping,
        ProjectState::Planning,
        ProjectState::Discussing,
        ProjectState::Executing,
        ProjectState::Verifying,
    ] {
        let result = state.clone().transition_gated(Transition::Abandon, None);
        assert!(result.is_ok(), "Abandon should succeed from {state:?}");
        assert_eq!(
            result.expect("abandon transition"),
            ProjectState::Abandoned,
            "Abandon should reach Abandoned from {state:?}"
        );
    }
}

#[test]
fn project_state_invalid_transition_fails() {
    let result = ProjectState::Complete.transition_gated(Transition::StartQuestioning, None);
    assert!(
        result.is_err(),
        "Should not be able to transition from Complete"
    );

    let result = ProjectState::Abandoned.transition_gated(Transition::Pause, None);
    assert!(result.is_err(), "Should not be able to pause Abandoned");

    let result = ProjectState::Created.transition_gated(Transition::Complete, None);
    assert!(
        result.is_err(),
        "Should not be able to Complete from Created"
    );
}

#[test]
fn project_state_revert_from_verifying() {
    let verifying = ProjectState::Verifying;

    let reverted = verifying
        .clone()
        .transition_gated(
            Transition::Revert {
                to: ProjectState::Executing,
            },
            None,
        )
        .expect("Revert to Executing");
    assert_eq!(reverted, ProjectState::Executing);

    let reverted = verifying
        .clone()
        .transition_gated(
            Transition::Revert {
                to: ProjectState::Planning,
            },
            None,
        )
        .expect("Revert to Planning");
    assert_eq!(reverted, ProjectState::Planning);

    let reverted = verifying
        .transition_gated(
            Transition::Revert {
                to: ProjectState::Scoping,
            },
            None,
        )
        .expect("Revert to Scoping");
    assert_eq!(reverted, ProjectState::Scoping);
}

// =============================================================================
// PhaseGate and GateCondition
// =============================================================================

#[test]
fn phase_gate_new() {
    let gate = PhaseGate::new(
        ProjectState::Planning,
        vec![GateCondition::TestsPassing, GateCondition::ReviewApproved],
    );

    assert_eq!(gate.from, ProjectState::Planning);
    assert_eq!(gate.conditions.len(), 2);
    assert!(gate.satisfied.is_empty());
}

#[test]
fn evaluate_gate_empty_conditions_pass() {
    let gate = PhaseGate::new(ProjectState::Planning, vec![]);
    let result = evaluate_gate(&gate);
    assert_eq!(result, GateResult::Pass);
    assert!(result.is_pass());
}

#[test]
fn evaluate_gate_all_satisfied_pass() {
    let mut gate = PhaseGate::new(
        ProjectState::Executing,
        vec![GateCondition::TestsPassing, GateCondition::LintClean],
    );
    gate.mark_satisfied("tests_passing");
    gate.mark_satisfied("lint_clean");

    let result = evaluate_gate(&gate);
    assert_eq!(result, GateResult::Pass);
    assert!(result.is_pass());
}

#[test]
fn evaluate_gate_partially_satisfied_fails() {
    let mut gate = PhaseGate::new(
        ProjectState::Executing,
        vec![GateCondition::TestsPassing, GateCondition::LintClean],
    );
    gate.mark_satisfied("tests_passing");

    let result = evaluate_gate(&gate);
    assert!(!result.is_pass());
    assert_eq!(result, GateResult::Fail(vec!["lint_clean".into()]));
}

#[test]
fn evaluate_gate_custom_condition() {
    let mut gate = PhaseGate::new(
        ProjectState::Planning,
        vec![GateCondition::Custom("acceptance_criteria_defined".into())],
    );

    // Not satisfied yet
    let result = evaluate_gate(&gate);
    assert!(!result.is_pass());

    // Mark as satisfied
    gate.mark_satisfied("acceptance_criteria_defined");
    let result = evaluate_gate(&gate);
    assert!(result.is_pass());
}

#[test]
fn gate_condition_standard_keys() {
    // The gate evaluation uses internal keys - test all standard conditions
    let mut gate = PhaseGate::new(
        ProjectState::Verifying,
        vec![
            GateCondition::TestsPassing,
            GateCondition::LintClean,
            GateCondition::DocsUpdated,
            GateCondition::ReviewApproved,
        ],
    );

    // Satisfy all standard conditions
    gate.mark_satisfied("tests_passing");
    gate.mark_satisfied("lint_clean");
    gate.mark_satisfied("docs_updated");
    gate.mark_satisfied("review_approved");

    let result = evaluate_gate(&gate);
    assert!(result.is_pass());
}

#[test]
fn default_gate_for_states() {
    // Planning has a default gate
    let gate = default_gate(&ProjectState::Planning);
    assert!(gate.is_some());
    let gate = gate.expect("planning gate exists");
    assert_eq!(gate.from, ProjectState::Planning);

    // Executing has a default gate
    let gate = default_gate(&ProjectState::Executing);
    assert!(gate.is_some());
    let gate = gate.expect("executing gate exists");
    assert_eq!(gate.from, ProjectState::Executing);

    // Verifying has a default gate
    let gate = default_gate(&ProjectState::Verifying);
    assert!(gate.is_some());

    // Created has no default gate
    let gate = default_gate(&ProjectState::Created);
    assert!(gate.is_none());

    // Complete has no default gate
    let gate = default_gate(&ProjectState::Complete);
    assert!(gate.is_none());
}

// =============================================================================
