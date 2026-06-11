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

// ── Workspace save and load roundtrip ──

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

    let ws = ProjectWorkspace::create(&root).expect("create workspace");
    let project = Project::new(
        "Existing".into(),
        "desc".into(),
        ProjectMode::Full,
        "owner".into(),
    );
    ws.save_project(&project).expect("save");

    let ws2 = ProjectWorkspace::open(&root).expect("open existing workspace");
    let loaded = ws2.load_project().expect("load project");
    assert_eq!(loaded.name, "Existing");
}

#[test]
fn workspace_open_nonexistent_fails() {
    let result = ProjectWorkspace::open("/nonexistent/path/to/project");
    assert!(
        result.is_err(),
        "Opening non-existent workspace should fail"
    );

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

// ── Serde roundtrips ──

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

// ── Blocker creation and usage ──

#[test]
fn blocker_creation_and_properties() {
    let plan_id = koina::ulid::Ulid::new();
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
        plan_id: koina::ulid::Ulid::new(),
        detected_at: jiff::Timestamp::now(),
    };

    let json = serde_json::to_string(&blocker).expect("serialize");
    let loaded: Blocker = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(loaded.description, blocker.description);
    assert_eq!(loaded.plan_id, blocker.plan_id);
}

// ── Error variants (via operations that trigger them) ──

#[test]
fn error_invalid_transition() {
    let result = ProjectState::Complete.transition_gated(Transition::StartQuestioning, None);
    assert!(result.is_err());

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
    use dianoia::state::ProjectState;

    let gate = PhaseGate::new(
        ProjectState::Planning,
        vec![GateCondition::TestsPassing, GateCondition::ReviewApproved],
    );

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
