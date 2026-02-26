// OrchestrationCore tests — comprehensive validation of all ORCH requirements
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";
import { OrchestrationCore } from "./orchestration-core.js";
import type { VerificationResult } from "./types.js";
import type { PhasePlan } from "./roadmap.js";

let db: Database.Database;
let store: PlanningStore;
let orchestrator: OrchestrationCore;

const defaultConfig = {
  depth: "standard" as const,
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
  pause_between_phases: false,
};

function makeDb(): Database.Database {
  const d = new Database(":memory:");
  d.pragma("journal_mode = WAL");
  d.pragma("foreign_keys = ON");
  d.exec(PLANNING_V20_DDL);
  d.exec(PLANNING_V21_MIGRATION);
  d.exec(PLANNING_V22_MIGRATION);
  d.exec(PLANNING_V23_MIGRATION);
  d.exec(PLANNING_V24_MIGRATION);
  d.exec(PLANNING_V25_MIGRATION);
  d.exec(PLANNING_V26_MIGRATION);
  d.exec(PLANNING_V27_MIGRATION);
  return d;
}

beforeEach(() => {
  db = makeDb();
  store = new PlanningStore(db);
  orchestrator = new OrchestrationCore(db);
});

afterEach(() => {
  db.close();
});

// ORCH-01: Clean state machine with valid transitions
describe("ORCH-01: State Machine Validation", () => {
  it("executes valid state transitions successfully", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    const result = orchestrator.executeStateTransition(
      project.id,
      "START_QUESTIONING",
      { reason: "User initiated questioning" }
    );

    expect(result.success).toBe(true);
    expect(result.fromState).toBe("idle");
    expect(result.toState).toBe("questioning");
    expect(result.event).toBe("START_QUESTIONING");
    expect(result.metadata?.reason).toBe("User initiated questioning");

    // Verify the state was actually updated in the database
    const updatedProject = store.getProjectOrThrow(project.id);
    expect(updatedProject.state).toBe("questioning");
  });

  it("rejects invalid state transitions", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session", 
      goal: "Test project",
      config: defaultConfig
    });

    const result = orchestrator.executeStateTransition(
      project.id,
      "VERIFY",
      { reason: "Invalid transition attempt" }
    );

    expect(result.success).toBe(false);
    expect(result.fromState).toBe("idle");
    expect(result.toState).toBe("idle"); // No change on failure
    expect(result.metadata?.error).toContain("Invalid transition");

    // Verify the state was NOT changed in the database
    const unchangedProject = store.getProjectOrThrow(project.id);
    expect(unchangedProject.state).toBe("idle");
  });

  it("handles full happy path state transitions", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project", 
      config: defaultConfig
    });

    const transitions = [
      { event: "START_QUESTIONING", expectedState: "questioning" },
      { event: "START_RESEARCH", expectedState: "researching" },
      { event: "RESEARCH_COMPLETE", expectedState: "requirements" },
      { event: "REQUIREMENTS_COMPLETE", expectedState: "roadmap" },
      { event: "ROADMAP_COMPLETE", expectedState: "discussing" },
      { event: "DISCUSSION_COMPLETE", expectedState: "phase-planning" },
      { event: "PLAN_READY", expectedState: "executing" },
      { event: "VERIFY", expectedState: "verifying" },
      { event: "ALL_PHASES_COMPLETE", expectedState: "complete" }
    ];

    for (const transition of transitions) {
      const result = orchestrator.executeStateTransition(project.id, transition.event);
      expect(result.success).toBe(true);
      expect(result.toState).toBe(transition.expectedState);
    }
  });
});

// ORCH-02: Wave-based execution with dependency tracking  
describe("ORCH-02: Wave-based Execution Status", () => {
  it("correctly identifies execution waves and status", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    // Create phases with dependencies: A -> B -> C, D independent
    const phaseA = store.createPhase({ 
      projectId: project.id, 
      name: "Phase A", 
      goal: "Goal A", 
      requirements: [], 
      successCriteria: ["A done"], 
      phaseOrder: 1 
    });
    const phaseB = store.createPhase({ 
      projectId: project.id, 
      name: "Phase B", 
      goal: "Goal B", 
      requirements: [], 
      successCriteria: ["B done"], 
      phaseOrder: 2 
    });
    const phaseC = store.createPhase({ 
      projectId: project.id, 
      name: "Phase C", 
      goal: "Goal C", 
      requirements: [], 
      successCriteria: ["C done"], 
      phaseOrder: 3 
    });
    const phaseD = store.createPhase({ 
      projectId: project.id, 
      name: "Phase D", 
      goal: "Goal D", 
      requirements: [], 
      successCriteria: ["D done"], 
      phaseOrder: 4 
    });

    // Set up dependencies: B depends on A, C depends on B
    const planB: PhasePlan = { steps: [], dependencies: [phaseA.id], acceptanceCriteria: [] };
    const planC: PhasePlan = { steps: [], dependencies: [phaseB.id], acceptanceCriteria: [] };
    const planD: PhasePlan = { steps: [], dependencies: [], acceptanceCriteria: [] };

    store.updatePhasePlan(phaseB.id, planB);
    store.updatePhasePlan(phaseC.id, planC);
    store.updatePhasePlan(phaseD.id, planD);

    const status = orchestrator.getExecutionStatus(project.id);

    expect(status.totalWaves).toBe(3); // Wave 1: A,D; Wave 2: B; Wave 3: C
    expect(status.currentWave).toBe(0); // First wave not started yet
    expect(status.pendingPhases).toHaveLength(4);
    expect(status.completedPhases).toHaveLength(0);
  });

  it("tracks phase completion and wave progression", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    const phaseA = store.createPhase({ 
      projectId: project.id, 
      name: "Phase A", 
      goal: "Goal A", 
      requirements: [], 
      successCriteria: ["A done"], 
      phaseOrder: 1 
    });
    const phaseB = store.createPhase({ 
      projectId: project.id, 
      name: "Phase B", 
      goal: "Goal B", 
      requirements: [], 
      successCriteria: ["B done"], 
      phaseOrder: 2 
    });

    // Create spawn records to simulate execution
    const recordA = store.createSpawnRecord({ 
      projectId: project.id, 
      phaseId: phaseA.id, 
      waveNumber: 0 
    });
    const _recordB = store.createSpawnRecord({
      projectId: project.id,
      phaseId: phaseB.id,
      waveNumber: 0
    });

    // Mark A as completed
    store.updateSpawnRecord(recordA.id, { status: "done" });

    const status = orchestrator.getExecutionStatus(project.id);

    expect(status.completedPhases).toContain(phaseA.id);
    expect(status.pendingPhases).toContain(phaseB.id);
  });
});

// ORCH-03: Post-execution verification with gap detection
describe("ORCH-03: Verification Analysis", () => {
  it("correctly analyzes successful verification", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    const phase = store.createPhase({ 
      projectId: project.id, 
      name: "Test Phase", 
      goal: "Test goal", 
      requirements: [], 
      successCriteria: ["All tests pass"], 
      phaseOrder: 1 
    });

    const verificationResult: VerificationResult = {
      status: "met",
      summary: "All success criteria met",
      gaps: [],
      verifiedAt: new Date().toISOString()
    };

    const analysis = orchestrator.analyzeVerificationResult(
      project.id,
      phase.id,
      verificationResult
    );

    expect(analysis.overallStatus).toBe("passed");
    expect(analysis.criticalGaps).toHaveLength(0);
    expect(analysis.minorGaps).toHaveLength(0);
    expect(analysis.recommendations).toContain("Phase successfully meets all success criteria");
    expect(analysis.nextActions).toContain("Proceed to next phase");
  });

  it("correctly categorizes verification gaps", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    const phase = store.createPhase({ 
      projectId: project.id, 
      name: "Test Phase", 
      goal: "Test goal", 
      requirements: [], 
      successCriteria: ["Critical functionality works", "Performance optimized"], 
      phaseOrder: 1 
    });

    const verificationResult: VerificationResult = {
      status: "partially-met",
      summary: "Some gaps identified",
      gaps: [
        {
          criterion: "Critical functionality works",
          status: "not-met",
          detail: "Core feature missing",
          proposedFix: "Implement core feature"
        },
        {
          criterion: "Performance optimized", 
          status: "partially-met",
          detail: "Some optimization done",
          proposedFix: "Add caching layer"
        }
      ],
      verifiedAt: new Date().toISOString()
    };

    const analysis = orchestrator.analyzeVerificationResult(
      project.id,
      phase.id,
      verificationResult
    );

    expect(analysis.overallStatus).toBe("failed"); // Critical gaps present
    expect(analysis.criticalGaps).toHaveLength(1);
    expect(analysis.minorGaps).toHaveLength(1);
    expect(analysis.recommendations).toContain("Address 1 critical gap(s) before proceeding");
    expect(analysis.nextActions).toContain("Fix critical issues and re-run verification");
  });
});

// ORCH-04: Automatic rollback planning for failed verification
describe("ORCH-04: Rollback Plan Generation", () => {
  it("generates comprehensive rollback plan for failed phase", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    // Create phases: A -> B -> C, D independent  
    const phaseA = store.createPhase({ 
      projectId: project.id, 
      name: "Phase A", 
      goal: "Goal A", 
      requirements: [], 
      successCriteria: ["A complete"], 
      phaseOrder: 1 
    });
    const phaseB = store.createPhase({ 
      projectId: project.id, 
      name: "Phase B", 
      goal: "Goal B", 
      requirements: [], 
      successCriteria: ["B complete"], 
      phaseOrder: 2 
    });
    const phaseC = store.createPhase({ 
      projectId: project.id, 
      name: "Phase C", 
      goal: "Goal C", 
      requirements: [], 
      successCriteria: ["C complete"], 
      phaseOrder: 3 
    });
    const phaseD = store.createPhase({ 
      projectId: project.id, 
      name: "Phase D", 
      goal: "Goal D", 
      requirements: [], 
      successCriteria: ["D complete"], 
      phaseOrder: 4 
    });

    // Set dependencies: B -> A, C -> B
    const planB: PhasePlan = { steps: [], dependencies: [phaseA.id], acceptanceCriteria: [] };
    const planC: PhasePlan = { steps: [], dependencies: [phaseB.id], acceptanceCriteria: [] };

    store.updatePhasePlan(phaseB.id, planB);
    store.updatePhasePlan(phaseC.id, planC);

    const failedVerification: VerificationResult = {
      status: "not-met",
      summary: "Critical failures detected",
      gaps: [
        {
          criterion: "A complete",
          status: "not-met",
          detail: "Core implementation missing",
          proposedFix: "Complete core implementation"
        }
      ],
      verifiedAt: new Date().toISOString()
    };

    const rollbackPlan = orchestrator.generateRollbackPlan(
      project.id,
      phaseA.id,
      failedVerification,
      "Phase A verification failed"
    );

    expect(rollbackPlan.failedPhaseId).toBe(phaseA.id);
    expect(rollbackPlan.failureReason).toBe("Phase A verification failed");
    expect(rollbackPlan.skippedPhases).toContain(phaseB.id);
    expect(rollbackPlan.skippedPhases).toContain(phaseC.id);
    expect(rollbackPlan.skippedPhases).not.toContain(phaseD.id); // D is independent

    // Check rollback actions
    const skipActions = rollbackPlan.rollbackActions.filter(a => a.type === "skip_phase");
    const fixActions = rollbackPlan.rollbackActions.filter(a => a.type === "manual_fix");
    const checkpointActions = rollbackPlan.rollbackActions.filter(a => a.type === "checkpoint_required");

    expect(skipActions).toHaveLength(2); // B and C should be skipped
    expect(fixActions).toHaveLength(1); // One gap fix
    expect(checkpointActions).toHaveLength(1); // Manual approval needed
  });

  it("handles phase with no dependents correctly", () => {
    const project = store.createProject({
      nousId: "test-nous", 
      sessionId: "test-session",
      goal: "Test project",
      config: defaultConfig
    });

    const phase = store.createPhase({ 
      projectId: project.id, 
      name: "Independent Phase", 
      goal: "Independent goal", 
      requirements: [], 
      successCriteria: ["Work complete"], 
      phaseOrder: 1 
    });

    const failedVerification: VerificationResult = {
      status: "not-met",
      summary: "Work incomplete",
      gaps: [],
      verifiedAt: new Date().toISOString()
    };

    const rollbackPlan = orchestrator.generateRollbackPlan(
      project.id,
      phase.id,
      failedVerification
    );

    expect(rollbackPlan.skippedPhases).toHaveLength(0);
    expect(rollbackPlan.rollbackActions.filter(a => a.type === "skip_phase")).toHaveLength(0);
    expect(rollbackPlan.rollbackActions.filter(a => a.type === "checkpoint_required")).toHaveLength(0);
  });
});

// ORCH-05: Gray-area discussion question generation
describe("ORCH-05: Discussion Question Generation", () => {
  it("generates relevant questions based on requirements", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session", 
      goal: "Build a web application",
      config: defaultConfig
    });

    const phase = store.createPhase({ 
      projectId: project.id, 
      name: "Frontend Phase", 
      goal: "Build frontend", 
      requirements: ["UI-01", "PERF-01", "SEC-01"], 
      successCriteria: ["UI complete", "Performance acceptable"], 
      phaseOrder: 1 
    });

    // Create requirements with different categories
    store.createRequirement({
      projectId: project.id,
      phaseId: phase.id,
      reqId: "UI-01", 
      description: "Build responsive web interface using modern framework",
      category: "UI",
      tier: "v1"
    });
    store.createRequirement({
      projectId: project.id,
      phaseId: phase.id,
      reqId: "PERF-01",
      description: "Ensure fast page load performance and scalability",
      category: "Performance", 
      tier: "v1"
    });
    store.createRequirement({
      projectId: project.id,
      phaseId: phase.id,
      reqId: "SEC-01",
      description: "Implement user authentication and authorization",
      category: "Security",
      tier: "v1"
    });

    const questions = orchestrator.generateDiscussionQuestions(
      project.id,
      phase.id
    );

    // Should generate questions for technology, performance, security, and testing
    expect(questions.length).toBeGreaterThan(0);
    
    const categories = questions.map(q => q.category);
    expect(categories).toContain("technology"); // Based on framework requirement
    expect(categories).toContain("performance"); // Based on performance requirement  
    expect(categories).toContain("security"); // Based on auth requirement
    expect(categories).toContain("quality"); // Testing strategy (always included)

    // Check question structure
    for (const question of questions) {
      expect(question.question).toBeTruthy();
      expect(question.options.length).toBeGreaterThan(1);
      expect(question.recommendation).toBeTruthy();
      expect(["high", "medium", "low"]).toContain(question.priority);
      
      // All options should have label and rationale
      for (const option of question.options) {
        expect(option.label).toBeTruthy();
        expect(option.rationale).toBeTruthy();
      }
    }
  });

  it("generates basic questions even without specific requirements", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Simple project",
      config: defaultConfig
    });

    const phase = store.createPhase({ 
      projectId: project.id, 
      name: "Basic Phase", 
      goal: "Basic work", 
      requirements: [], 
      successCriteria: ["Work done"], 
      phaseOrder: 1 
    });

    const questions = orchestrator.generateDiscussionQuestions(
      project.id,
      phase.id
    );

    // Should always generate at least the testing strategy question
    expect(questions.length).toBeGreaterThan(0);
    const testingQuestion = questions.find(q => q.category === "quality");
    expect(testingQuestion).toBeTruthy();
    expect(testingQuestion?.question).toContain("testing strategy");
  });
});

describe("OrchestrationCore Integration", () => {
  it("coordinates all orchestration capabilities together", () => {
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Full integration test",
      config: defaultConfig
    });

    // 1. State machine progression
    let result = orchestrator.executeStateTransition(project.id, "START_QUESTIONING");
    expect(result.success).toBe(true);

    result = orchestrator.executeStateTransition(project.id, "START_RESEARCH");
    expect(result.success).toBe(true);

    // 2. Create phases for execution tracking
    const phaseA = store.createPhase({ 
      projectId: project.id, 
      name: "Phase A", 
      goal: "Goal A", 
      requirements: ["REQ-01"], 
      successCriteria: ["A complete"], 
      phaseOrder: 1 
    });

    // 3. Get execution status
    const execStatus = orchestrator.getExecutionStatus(project.id);
    expect(execStatus.pendingPhases).toContain(phaseA.id);

    // 4. Generate discussion questions  
    const questions = orchestrator.generateDiscussionQuestions(project.id, phaseA.id);
    expect(questions.length).toBeGreaterThan(0);

    // 5. Simulate verification failure and rollback
    const failedVerification: VerificationResult = {
      status: "not-met",
      summary: "Integration test failure",
      gaps: [{ 
        criterion: "A complete", 
        status: "not-met", 
        detail: "Not implemented",
        proposedFix: "Implement missing functionality"
      }],
      verifiedAt: new Date().toISOString()
    };

    const rollbackPlan = orchestrator.generateRollbackPlan(
      project.id,
      phaseA.id,
      failedVerification
    );

    expect(rollbackPlan.failedPhaseId).toBe(phaseA.id);
    // Should have at least a manual fix action since we provided a proposedFix
    expect(rollbackPlan.rollbackActions.length).toBeGreaterThan(0);

    // 6. Verification analysis
    const analysis = orchestrator.analyzeVerificationResult(
      project.id,
      phaseA.id,
      failedVerification
    );

    expect(analysis.overallStatus).toBe("failed");
    expect(analysis.nextActions.length).toBeGreaterThan(0);
  });
});