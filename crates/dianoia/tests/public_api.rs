//! Integration tests for dianoia public API.
//!
//! Covers `Project`, `Phase`, `Plan`, `PhaseGate`, `GateCondition`, `Workspace`,
//! and state machine transitions (`ProjectState`, `PhaseState`, `PlanState`).

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test: index safety verified by assertions")]

use aletheia_dianoia::gate::{
    evaluate_gate, default_gate, GateCondition, GateResult, PhaseGate,
};
use aletheia_dianoia::phase::{Phase, PhaseState};
use aletheia_dianoia::plan::{Blocker, Plan, PlanState};
use aletheia_dianoia::project::{Project, ProjectMode};
use aletheia_dianoia::state::{ProjectState, Transition};
use aletheia_dianoia::workspace::ProjectWorkspace;

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

// =============================================================================
// Project State Machine Transitions
// =============================================================================

#[test]
fn project_state_full_lifecycle() {
    let state = ProjectState::Created;

    let state = state
        .transition(Transition::StartQuestioning)
        .expect("Created -> Questioning");
    assert_eq!(state, ProjectState::Questioning);

    let state = state
        .transition(Transition::StartResearch)
        .expect("Questioning -> Researching");
    assert_eq!(state, ProjectState::Researching);

    let state = state
        .transition(Transition::StartScoping)
        .expect("Researching -> Scoping");
    assert_eq!(state, ProjectState::Scoping);

    let state = state
        .transition(Transition::StartPlanning)
        .expect("Scoping -> Planning");
    assert_eq!(state, ProjectState::Planning);

    let state = state
        .transition(Transition::StartDiscussion)
        .expect("Planning -> Discussing");
    assert_eq!(state, ProjectState::Discussing);

    let state = state
        .transition(Transition::StartExecution)
        .expect("Discussing -> Executing");
    assert_eq!(state, ProjectState::Executing);

    let state = state
        .transition(Transition::StartVerification)
        .expect("Executing -> Verifying");
    assert_eq!(state, ProjectState::Verifying);

    let state = state.transition(Transition::Complete).expect("Verifying -> Complete");
    assert_eq!(state, ProjectState::Complete);
}

#[test]
fn project_state_skip_paths() {
    // Skip from Created directly to Scoping
    let state = ProjectState::Created
        .transition(Transition::SkipToResearch)
        .expect("Created -> Scoping (skip)");
    assert_eq!(state, ProjectState::Scoping);

    // Skip from Questioning to Scoping
    let state = ProjectState::Questioning
        .transition(Transition::SkipResearch)
        .expect("Questioning -> Scoping (skip)");
    assert_eq!(state, ProjectState::Scoping);

    // Skip from Planning to Executing
    let state = ProjectState::Planning
        .transition(Transition::StartExecution)
        .expect("Planning -> Executing (skip discussion)");
    assert_eq!(state, ProjectState::Executing);
}

#[test]
fn project_state_pause_and_resume() {
    let original = ProjectState::Executing;
    let paused = original
        .transition(Transition::Pause)
        .expect("Executing -> Pause");

    assert!(
        matches!(paused, ProjectState::Paused { .. }),
        "Expected Paused state"
    );

    let resumed = paused.transition(Transition::Resume).expect("Paused -> Resume");
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
        let result = state.clone().transition(Transition::Abandon);
        assert!(
            result.is_ok(),
            "Abandon should succeed from {state:?}"
        );
        assert_eq!(
            result.expect("abandon transition"),
            ProjectState::Abandoned,
            "Abandon should reach Abandoned from {state:?}"
        );
    }
}

#[test]
fn project_state_invalid_transition_fails() {
    let result = ProjectState::Complete.transition(Transition::StartQuestioning);
    assert!(result.is_err(), "Should not be able to transition from Complete");

    let result = ProjectState::Abandoned.transition(Transition::Pause);
    assert!(result.is_err(), "Should not be able to pause Abandoned");

    let result = ProjectState::Created.transition(Transition::Complete);
    assert!(result.is_err(), "Should not be able to Complete from Created");
}

#[test]
fn project_state_revert_from_verifying() {
    let verifying = ProjectState::Verifying;

    let reverted = verifying
        .clone()
        .transition(Transition::Revert {
            to: ProjectState::Executing,
        })
        .expect("Revert to Executing");
    assert_eq!(reverted, ProjectState::Executing);

    let reverted = verifying
        .clone()
        .transition(Transition::Revert {
            to: ProjectState::Planning,
        })
        .expect("Revert to Planning");
    assert_eq!(reverted, ProjectState::Planning);

    let reverted = verifying
        .transition(Transition::Revert {
            to: ProjectState::Scoping,
        })
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
        vec![
            GateCondition::TestsPassing,
            GateCondition::ReviewApproved,
        ],
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
// Workspace Save and Load Roundtrip
// =============================================================================

#[test]
fn workspace_save_and_load_project() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let ws = ProjectWorkspace::create(temp_dir.path().join("project")).expect("create workspace");

    let mut project = Project::new(
        "Roundtrip Test".into(),
        "Testing save and load".into(),
        ProjectMode::Full,
        "tester".into(),
    );

    // Add a phase with plans
    let mut phase = Phase::new("Phase 1".into(), "First phase".into(), 1);
    phase.state = PhaseState::Active;
    project.add_phase(phase);

    ws.save_project(&project).expect("save project");
    let loaded = ws.load_project().expect("load project");

    assert_eq!(loaded.id, project.id);
    assert_eq!(loaded.name, "Roundtrip Test");
    assert_eq!(loaded.description, "Testing save and load");
    assert_eq!(loaded.owner, "tester");
    assert_eq!(loaded.phases.len(), 1);
    assert_eq!(loaded.phases[0].state, PhaseState::Active);
}

#[test]
fn workspace_save_and_load_with_state_transitions() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let ws = ProjectWorkspace::create(temp_dir.path().join("project")).expect("create workspace");

    let mut project = Project::new(
        "State Test".into(),
        "Testing state persistence".into(),
        ProjectMode::Full,
        "tester".into(),
    );

    // Advance through some states
    project
        .advance(Transition::StartQuestioning)
        .expect("advance to questioning");
    project
        .advance(Transition::StartResearch)
        .expect("advance to researching");

    ws.save_project(&project).expect("save project");
    let loaded = ws.load_project().expect("load project");

    assert_eq!(loaded.state, ProjectState::Researching);
}

#[test]
fn workspace_open_existing() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let root = temp_dir.path().join("existing-project");

    // Create first
    let ws = ProjectWorkspace::create(&root).expect("create workspace");
    let project = Project::new("Existing".into(), "desc".into(), ProjectMode::Full, "owner".into());
    ws.save_project(&project).expect("save");

    // Then open
    let ws2 = ProjectWorkspace::open(&root).expect("open existing workspace");
    let loaded = ws2.load_project().expect("load project");
    assert_eq!(loaded.name, "Existing");
}

#[test]
fn workspace_open_nonexistent_fails() {
    let result = ProjectWorkspace::open("/nonexistent/path/to/project");
    assert!(result.is_err(), "Opening non-existent workspace should fail");

    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("not found") || err_msg.contains("project"),
                "Error should indicate project not found: {err_msg}"
            );
        }
        Ok(_) => panic!("Expected error for non-existent workspace"),
    }
}

// =============================================================================
// Serde Roundtrips
// =============================================================================

#[test]
fn project_serde_roundtrip() {
    let mut project = Project::new(
        "Serde Test".into(),
        "Testing serialization".into(),
        ProjectMode::Quick {
            appetite_minutes: 45,
        },
        "serde-tester".into(),
    );
    project.scope = Some("test scope".into());

    let phase = Phase::new("P1".into(), "Phase one".into(), 1);
    project.add_phase(phase);

    let json = serde_json::to_string(&project).expect("serialize");
    let loaded: Project = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.id, project.id);
    assert_eq!(loaded.name, "Serde Test");
    assert_eq!(loaded.scope, Some("test scope".into()));
    assert_eq!(loaded.phases.len(), 1);
}

#[test]
fn phase_serde_roundtrip() {
    let mut phase = Phase::new("Test Phase".into(), "Test goal".into(), 2);
    phase.state = PhaseState::Executing;
    phase.requirements = vec!["req1".into(), "req2".into()];

    let json = serde_json::to_string(&phase).expect("serialize");
    let loaded: Phase = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.id, phase.id);
    assert_eq!(loaded.name, "Test Phase");
    assert_eq!(loaded.goal, "Test goal");
    assert_eq!(loaded.order, 2);
    assert_eq!(loaded.state, PhaseState::Executing);
    assert_eq!(loaded.requirements, vec!["req1", "req2"]);
}

#[test]
fn plan_serde_roundtrip() {
    let mut plan = Plan::new("My Plan".into(), "Plan description".into(), 3);
    plan.state = PlanState::Executing;
    plan.iterations = 5;
    plan.achievements = vec!["milestone 1".into(), "milestone 2".into()];

    let json = serde_json::to_string(&plan).expect("serialize");
    let loaded: Plan = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.id, plan.id);
    assert_eq!(loaded.title, "My Plan");
    assert_eq!(loaded.wave, 3);
    assert_eq!(loaded.state, PlanState::Executing);
    assert_eq!(loaded.iterations, 5);
    assert_eq!(loaded.achievements, vec!["milestone 1", "milestone 2"]);
}

#[test]
fn phase_state_serde_roundtrip() {
    let states = [
        PhaseState::Pending,
        PhaseState::Active,
        PhaseState::Executing,
        PhaseState::Verifying,
        PhaseState::Complete,
        PhaseState::Failed { can_retry: true },
        PhaseState::Failed { can_retry: false },
    ];

    for state in &states {
        let json = serde_json::to_string(state).expect("serialize");
        let loaded: PhaseState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&loaded, state, "roundtrip failed for {state:?}");
    }
}

#[test]
fn plan_state_serde_roundtrip() {
    let states = [
        PlanState::Pending,
        PlanState::Ready,
        PlanState::Executing,
        PlanState::Complete,
        PlanState::Failed,
        PlanState::Skipped,
        PlanState::Stuck,
    ];

    for state in &states {
        let json = serde_json::to_string(state).expect("serialize");
        let loaded: PlanState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&loaded, state, "roundtrip failed for {state:?}");
    }
}

#[test]
fn project_state_serde_roundtrip() {
    let states = [
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
        ProjectState::Paused {
            previous: Box::new(ProjectState::Executing),
        },
    ];

    for state in &states {
        let json = serde_json::to_string(state).expect("serialize");
        let loaded: ProjectState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&loaded, state, "roundtrip failed for {state:?}");
    }
}

#[test]
fn phase_gate_serde_roundtrip() {
    let mut gate = PhaseGate::new(
        ProjectState::Executing,
        vec![
            GateCondition::TestsPassing,
            GateCondition::Custom("custom_check".into()),
        ],
    );
    gate.mark_satisfied("tests_passing");

    let json = serde_json::to_string(&gate).expect("serialize");
    let loaded: PhaseGate = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.from, ProjectState::Executing);
    assert_eq!(loaded.conditions, gate.conditions);
    assert_eq!(loaded.satisfied, vec!["tests_passing"]);
}

#[test]
fn gate_condition_serde_roundtrip() {
    let conditions = [
        GateCondition::TestsPassing,
        GateCondition::LintClean,
        GateCondition::DocsUpdated,
        GateCondition::ReviewApproved,
        GateCondition::Custom("my_condition".into()),
    ];

    for condition in &conditions {
        let json = serde_json::to_string(condition).expect("serialize");
        let loaded: GateCondition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&loaded, condition, "roundtrip failed for {condition:?}");
    }
}

#[test]
fn project_mode_serde_roundtrip() {
    let modes = [
        ProjectMode::Full,
        ProjectMode::Quick {
            appetite_minutes: 60,
        },
        ProjectMode::Background,
    ];

    for mode in &modes {
        let json = serde_json::to_string(mode).expect("serialize");
        let loaded: ProjectMode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&loaded, mode, "roundtrip failed for {mode:?}");
    }
}

// =============================================================================
// Blocker Creation and Usage
// =============================================================================

#[test]
fn blocker_creation_and_properties() {
    let plan_id = aletheia_koina::ulid::Ulid::new();
    let now = jiff::Timestamp::now();

    let blocker = Blocker {
        description: "Waiting for API approval".into(),
        plan_id,
        detected_at: now,
    };

    assert_eq!(blocker.description, "Waiting for API approval");
    assert_eq!(blocker.plan_id, plan_id);
    assert_eq!(blocker.detected_at, now);
}

#[test]
fn blocker_serde_roundtrip() {
    let blocker = Blocker {
        description: "Test blocker".into(),
        plan_id: aletheia_koina::ulid::Ulid::new(),
        detected_at: jiff::Timestamp::now(),
    };

    let json = serde_json::to_string(&blocker).expect("serialize");
    let loaded: Blocker = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.description, blocker.description);
    assert_eq!(loaded.plan_id, blocker.plan_id);
}

// =============================================================================
// Error Variants (via operations that trigger them)
// =============================================================================

#[test]
fn error_invalid_transition() {
    let result = ProjectState::Complete.transition(Transition::StartQuestioning);
    assert!(result.is_err());

    // The error should be InvalidTransition
    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("invalid transition"),
                "Error should indicate invalid transition: {err_msg}"
            );
        }
        Ok(_) => panic!("Expected error for invalid transition"),
    }
}

#[test]
fn error_project_not_found_on_open() {
    let result = ProjectWorkspace::open("/definitely/does/not/exist");
    assert!(result.is_err());

    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("not found") || err_msg.contains("project"),
                "Error should indicate project not found: {err_msg}"
            );
        }
        Ok(_) => panic!("Expected error for non-existent workspace"),
    }
}

#[test]
fn error_gate_blocked() {
    use aletheia_dianoia::state::ProjectState;

    let gate = PhaseGate::new(
        ProjectState::Planning,
        vec![
            GateCondition::TestsPassing,
            GateCondition::ReviewApproved,
        ],
    );

    // Try to transition through a blocking gate
    let result = ProjectState::Planning.transition_gated(Transition::StartExecution, Some(&gate));
    assert!(result.is_err(), "Should be blocked by unsatisfied gate");

    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("gate blocked") || err_msg.contains("unsatisfied"),
                "Error should indicate gate blocked: {err_msg}"
            );
        }
        Ok(_) => panic!("Expected gate blocked error"),
    }
}
