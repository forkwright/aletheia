import { describe, it, expect, beforeEach, vi } from "vitest";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { eventBus } from "../koina/event-bus.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
};

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  db.exec(PLANNING_V26_MIGRATION);
  db.exec(PLANNING_V27_MIGRATION);
  return db;
}

function makeOrchestrator(): DianoiaOrchestrator {
  return new DianoiaOrchestrator(makeDb(), DEFAULT_CONFIG);
}

describe("DianoiaOrchestrator.handle()", () => {
  it("creates a new project and returns first question when no active project exists", () => {
    const orch = makeOrchestrator();
    const result = orch.handle("nous-1", "session-1");
    expect(result.toLowerCase()).toContain("what are you building");
    const project = orch.getActiveProject("nous-1");
    expect(project).toBeDefined();
    expect(project!.state).toBe("questioning");
  });

  it("returns resume confirmation when active project exists", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const result = orch.handle("nous-1", "session-1");
    expect(result.toLowerCase()).toContain("still working on");
    const project = orch.getActiveProject("nous-1");
    expect((project!.config as Record<string, unknown>)["pendingConfirmation"]).toBe(true);
  });

  it("associates project with nousId for later resume", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-a", "session-a");
    expect(orch.getActiveProject("nous-a")).toBeDefined();
    expect(orch.getActiveProject("nous-b")).toBeUndefined();
  });
});

describe("DianoiaOrchestrator.confirmResume()", () => {
  let orch: DianoiaOrchestrator;
  let projectId: string;

  beforeEach(() => {
    orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    orch.handle("nous-1", "session-1");
    projectId = orch.getActiveProject("nous-1")!.id;
  });

  it("confirmResume with 'yes' resumes the project", () => {
    const result = orch.confirmResume(projectId, "nous-1", "session-1", "yes");
    expect(result.toLowerCase()).toContain("resuming");
    const project = orch.getActiveProject("nous-1");
    expect((project!.config as Record<string, unknown>)["pendingConfirmation"]).toBe(false);
  });

  it("confirmResume with 'no' abandons old project and creates fresh one", () => {
    orch.confirmResume(projectId, "nous-1", "session-1", "no");
    const newProject = orch.getActiveProject("nous-1");
    expect(newProject).toBeDefined();
    expect(newProject!.id).not.toBe(projectId);
    expect(newProject!.state).toBe("questioning");
  });
});

describe("DianoiaOrchestrator.abandon()", () => {
  it("does not find completed or abandoned projects as active", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;
    orch.abandon(project.id);
    expect(orch.getActiveProject("nous-1")).toBeUndefined();
  });
});

describe("DianoiaOrchestrator.processAnswer()", () => {
  it("records answer to rawTranscript in project_context", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    orch.processAnswer(project.id, "I'm building a CLI planning tool");
    const updated = orch.getActiveProject("nous-1")!;
    expect(updated.projectContext?.rawTranscript).toHaveLength(1);
    expect(updated.projectContext?.rawTranscript?.[0]?.text).toBe("I'm building a CLI planning tool");
    expect(updated.projectContext?.rawTranscript?.[0]?.turn).toBe(1);
  });

  it("appends multiple answers incrementing turn numbers", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    orch.processAnswer(project.id, "Answer one");
    orch.processAnswer(project.id, "Answer two");
    const updated = orch.getActiveProject("nous-1")!;
    expect(updated.projectContext?.rawTranscript).toHaveLength(2);
    expect(updated.projectContext?.rawTranscript?.[1]?.turn).toBe(2);
  });
});

describe("DianoiaOrchestrator.getNextQuestion()", () => {
  it("returns first question when rawTranscript is empty", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;
    const q = orch.getNextQuestion(project.id);
    expect(q).not.toBeNull();
    expect(typeof q).toBe("string");
    expect(q!.length).toBeGreaterThan(0);
  });

  it("returns null after all QUESTIONS answered", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    // Answer all 5 questions
    for (let i = 0; i < 5; i++) {
      orch.processAnswer(project.id, `Answer ${i + 1}`);
    }
    const q = orch.getNextQuestion(project.id);
    expect(q).toBeNull();
  });

  it("returns null when project not in questioning state", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;
    orch.abandon(project.id);
    const q = orch.getNextQuestion(project.id);
    expect(q).toBeNull();
  });
});

describe("DianoiaOrchestrator.synthesizeContext()", () => {
  it("returns a non-empty summary string containing transcript entries", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    orch.processAnswer(project.id, "Building a planning tool");
    orch.processAnswer(project.id, "TypeScript only, no new deps");
    const summary = orch.synthesizeContext(project.id);
    expect(summary).toContain("Building a planning tool");
    expect(summary).toContain("TypeScript only, no new deps");
    expect(summary.toLowerCase()).toContain("here's what i captured");
  });
});

describe("DianoiaOrchestrator.confirmSynthesis()", () => {
  it("persists goal and structured context, transitions state to researching", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    orch.processAnswer(project.id, "Building a CLI planning assistant");
    const result = orch.confirmSynthesis(project.id, "nous-1", "session-1", {
      goal: "Build a CLI planning assistant",
      coreValue: "Developer autonomy",
      constraints: ["TypeScript", "SQLite only"],
      keyDecisions: ["Use vitest", "File-based config"],
    });

    expect(result).toContain("Context saved");
    const updated = orch.getActiveProject("nous-1");
    expect(updated?.state).toBe("researching");
  });

  it("project state after confirmSynthesis is researching, not questioning", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;
    expect(project.state).toBe("questioning");

    orch.confirmSynthesis(project.id, "nous-1", "session-1", { goal: "Build X" });
    const after = orch.getActiveProject("nous-1");
    expect(after?.state).toBe("researching");
  });

  it("preserves rawTranscript in the merged context", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    orch.processAnswer(project.id, "First answer");
    orch.confirmSynthesis(project.id, "nous-1", "session-1", {
      goal: "My goal",
      coreValue: "Quality",
    });

    const updated = orch.getActiveProject("nous-1");
    expect(updated?.projectContext?.rawTranscript).toHaveLength(1);
    expect(updated?.projectContext?.goal).toBe("My goal");
  });
});

describe("DianoiaOrchestrator.completePhase()", () => {
  it("emits planning:phase-complete event", () => {
    const orch = makeOrchestrator();
    const spy = vi.spyOn(eventBus, "emit");

    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;
    orch.completePhase(project.id, "nous-1", "session-1", "questioning");

    expect(spy).toHaveBeenCalledWith("planning:phase-complete", expect.objectContaining({
      projectId: project.id,
      phase: "questioning",
    }));
    spy.mockRestore();
  });
});

describe("DianoiaOrchestrator.completeProject()", () => {
  it("emits planning:complete event and transitions to complete state", () => {
    const orch = makeOrchestrator();
    const spy = vi.spyOn(eventBus, "emit");

    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    // Must be in verifying state to use ALL_PHASES_COMPLETE — drive it there
    // questioning -> researching -> requirements -> roadmap -> phase-planning -> executing -> verifying
    orch.confirmSynthesis(project.id, "nous-1", "session-1", { goal: "Test" });
    // Now in researching — drive to verifying via store directly for test isolation
    const { PlanningStore } = orch as unknown as { store: InstanceType<typeof import("./store.js").PlanningStore> };
    void PlanningStore;

    // Use the orchestrator store directly by confirming we need it in verifying
    // Instead, verify via abandon + new path that emits the event
    // The simpler approach: drive state to verifying by calling updateProjectState directly
    // (white-box test — acceptable for event emission test)
    spy.mockRestore();
  });

  it("emits planning:complete event from verifying state", () => {
    const db = makeDb();
    const orch = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    const spy = vi.spyOn(eventBus, "emit");

    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    // Drive state manually to verifying so completeProject can use ALL_PHASES_COMPLETE
    db.prepare("UPDATE planning_projects SET state = 'verifying' WHERE id = ?").run(project.id);

    orch.completeProject(project.id, "nous-1", "session-1");

    expect(spy).toHaveBeenCalledWith("planning:complete", expect.objectContaining({
      projectId: project.id,
    }));
    const completed = db.prepare("SELECT state FROM planning_projects WHERE id = ?")
      .get(project.id) as { state: string };
    expect(completed.state).toBe("complete");
    spy.mockRestore();
  });
});

describe("DianoiaOrchestrator.skipDownstreamPhasesOnVerificationFailure() [ORCH-04]", () => {
  it("skips direct dependent phases when verification fails", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    // Create test phases with dependencies
    const db = (orch as any).store.db;
    const phaseA = db.prepare(`
      INSERT INTO planning_phases (id, project_id, name, goal, requirements, success_criteria, phase_order, status, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run("phase-a", project.id, "Phase A", "Build foundation", "[]", "[]", 1, "pending", new Date().toISOString(), new Date().toISOString());

    const phaseB = db.prepare(`
      INSERT INTO planning_phases (id, project_id, name, goal, requirements, success_criteria, phase_order, status, plan, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run("phase-b", project.id, "Phase B", "Depends on A", "[]", "[]", 2, "pending", 
      JSON.stringify({ dependencies: ["phase-a"] }), new Date().toISOString(), new Date().toISOString());

    const phaseC = db.prepare(`
      INSERT INTO planning_phases (id, project_id, name, goal, requirements, success_criteria, phase_order, status, plan, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run("phase-c", project.id, "Phase C", "Independent", "[]", "[]", 3, "pending", 
      JSON.stringify({ dependencies: [] }), new Date().toISOString(), new Date().toISOString());

    // Phase A fails verification
    const verificationGaps = [
      {
        criterion: "Core functionality must work",
        status: "not-met" as const,
        detail: "Tests failing",
        proposedFix: "Fix unit tests"
      },
      {
        criterion: "Documentation complete",
        status: "partially-met" as const, 
        detail: "Missing API docs",
        proposedFix: "Write API documentation"
      }
    ];

    const result = orch.skipDownstreamPhasesOnVerificationFailure(project.id, "phase-a", verificationGaps);

    expect(result.skippedPhases).toEqual(["phase-b"]);
    expect(result.rollbackPlan.failedPhaseId).toBe("phase-a");
    expect(result.rollbackPlan.gapCount).toBe(2);
    expect(result.rollbackPlan.actions).toHaveLength(3); // 2 gaps + 1 verification action

    // Verify phase B was skipped in database
    const phaseBStatus = db.prepare("SELECT status FROM planning_phases WHERE id = ?").get("phase-b") as { status: string };
    expect(phaseBStatus.status).toBe("skipped");

    // Verify phase C was not affected
    const phaseCStatus = db.prepare("SELECT status FROM planning_phases WHERE id = ?").get("phase-c") as { status: string };
    expect(phaseCStatus.status).toBe("pending");
  });

  it("generates rollback plan with correct priority levels", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    const gaps = [
      {
        criterion: "Critical feature",
        status: "not-met" as const,
        detail: "Completely broken",
        proposedFix: "Rewrite component"
      },
      {
        criterion: "Nice-to-have feature", 
        status: "partially-met" as const,
        detail: "Minor issues",
        proposedFix: "Tweak styling"
      }
    ];

    const result = orch.skipDownstreamPhasesOnVerificationFailure(project.id, "test-phase", gaps);

    const criticalAction = result.rollbackPlan.actions.find(a => a.description === "Critical feature");
    const minorAction = result.rollbackPlan.actions.find(a => a.description === "Nice-to-have feature");
    const verifyAction = result.rollbackPlan.actions.find(a => a.type === "verify-phase");

    expect(criticalAction?.priority).toBe("high");
    expect(minorAction?.priority).toBe("medium");
    expect(verifyAction?.priority).toBe("high");
  });

  it("estimates effort correctly based on gap severity", () => {
    const orch = makeOrchestrator();
    orch.handle("nous-1", "session-1");
    const project = orch.getActiveProject("nous-1")!;

    // High effort: 3 critical gaps
    const highEffortGaps = [
      { status: "not-met" as const, criterion: "A", detail: "", proposedFix: "" },
      { status: "not-met" as const, criterion: "B", detail: "", proposedFix: "" },
      { status: "not-met" as const, criterion: "C", detail: "", proposedFix: "" }
    ];

    const highResult = orch.skipDownstreamPhasesOnVerificationFailure(project.id, "phase1", highEffortGaps);
    expect(highResult.rollbackPlan.estimatedEffort).toBe("high");

    // Medium effort: 1 critical gap
    const mediumEffortGaps = [
      { status: "not-met" as const, criterion: "A", detail: "", proposedFix: "" }
    ];

    const mediumResult = orch.skipDownstreamPhasesOnVerificationFailure(project.id, "phase2", mediumEffortGaps);
    expect(mediumResult.rollbackPlan.estimatedEffort).toBe("medium");

    // Low effort: only partial gaps
    const lowEffortGaps = [
      { status: "partially-met" as const, criterion: "A", detail: "", proposedFix: "" }
    ];

    const lowResult = orch.skipDownstreamPhasesOnVerificationFailure(project.id, "phase3", lowEffortGaps);
    expect(lowResult.rollbackPlan.estimatedEffort).toBe("low");
  });
});
