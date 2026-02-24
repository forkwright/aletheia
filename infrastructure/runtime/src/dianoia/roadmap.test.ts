import { describe, it, expect, vi, type MockedFunction } from "vitest";
import Database from "better-sqlite3";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { RoadmapOrchestrator } from "./roadmap.js";
import type { PhaseDefinition, PhasePlan } from "./roadmap.js";
import type { ToolHandler, ToolContext } from "../organon/registry.js";
import { transition } from "./machine.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  return db;
}

const DEFAULT_CONFIG = {
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
};

function makeProject(db: Database.Database): string {
  const store = new PlanningStore(db);
  const project = store.createProject({
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Build a planning tool",
    config: DEFAULT_CONFIG,
  });
  store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));
  store.updateProjectState(project.id, transition("questioning", "START_RESEARCH"));
  store.updateProjectState(project.id, transition("researching", "RESEARCH_COMPLETE"));
  store.updateProjectState(project.id, transition("requirements", "REQUIREMENTS_COMPLETE"));
  return project.id;
}

function makeDispatchTool(responses: string[]): ToolHandler {
  let callCount = 0;
  return {
    definition: {
      name: "sessions_dispatch",
      description: "Dispatch tasks",
      input_schema: { type: "object" as const, properties: {}, required: [] },
    },
    execute: vi.fn(async (_input, _context) => {
      const response = responses[callCount] ?? responses[responses.length - 1]!;
      callCount++;
      return response;
    }) as MockedFunction<ToolHandler["execute"]>,
  };
}

const FAKE_TOOL_CONTEXT: ToolContext = {
  nousId: "test-nous",
  sessionId: "test-session",
  workspace: "/tmp/test",
};

const SAMPLE_PHASES: PhaseDefinition[] = [
  {
    name: "Foundation",
    goal: "Set up auth and storage",
    requirements: ["AUTH-01", "AUTH-02"],
    successCriteria: ["Users can log in", "Passwords are stored hashed"],
    phaseOrder: 1,
  },
  {
    name: "API Layer",
    goal: "Build the REST API",
    requirements: ["API-01"],
    successCriteria: ["API endpoints respond correctly"],
    phaseOrder: 2,
  },
];

function makeV1Req(store: PlanningStore, projectId: string, reqId: string, category: string): void {
  store.createRequirement({
    projectId,
    reqId,
    description: `User can do ${reqId}`,
    category,
    tier: "v1",
  });
}

// --- validateCoverage ---

describe("RoadmapOrchestrator.validateCoverage()", () => {
  it("returns covered=true when phases cover all v1 reqs", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    const phases: PhaseDefinition[] = [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01", "AUTH-02"], successCriteria: [], phaseOrder: 1 },
    ];
    const result = orch.validateCoverage(projectId, phases);
    expect(result.covered).toBe(true);
    expect(result.missing).toEqual([]);
  });

  it("returns covered=false, missing=[reqId] when one v1 req is absent from all phases", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    const phases: PhaseDefinition[] = [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ];
    const result = orch.validateCoverage(projectId, phases);
    expect(result.covered).toBe(false);
    expect(result.missing).toEqual(["AUTH-02"]);
  });

  it("returns covered=true with empty phases if no v1 reqs exist", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);
    // Only v2 reqs
    store.createRequirement({
      projectId,
      reqId: "FEAT-01",
      description: "User can do feature",
      category: "FEAT",
      tier: "v2",
    });

    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    const result = orch.validateCoverage(projectId, []);
    expect(result.covered).toBe(true);
    expect(result.missing).toEqual([]);
  });
});

// --- validateCoverageFromDb ---

describe("RoadmapOrchestrator.validateCoverageFromDb()", () => {
  it("returns covered=true when persisted phases cover all v1 reqs", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01", "AUTH-02"], successCriteria: [], phaseOrder: 1 },
    ]);
    const result = orch.validateCoverageFromDb(projectId);
    expect(result.covered).toBe(true);
    expect(result.missing).toEqual([]);
  });

  it("returns covered=false, missing=[reqId] when a v1 req is absent from all persisted phases", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);
    const result = orch.validateCoverageFromDb(projectId);
    expect(result.covered).toBe(false);
    expect(result.missing).toEqual(["AUTH-02"]);
  });
});

// --- listPhases ---

describe("RoadmapOrchestrator.listPhases()", () => {
  it("returns phases sorted by phase_order ascending", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    orch.commitRoadmap(projectId, [
      { name: "API Layer", goal: "Build API", requirements: [], successCriteria: [], phaseOrder: 2 },
      { name: "Foundation", goal: "Set up auth", requirements: [], successCriteria: [], phaseOrder: 1 },
    ]);

    const phases = orch.listPhases(projectId);
    expect(phases).toHaveLength(2);
    expect(phases[0]!.phaseOrder).toBe(1);
    expect(phases[1]!.phaseOrder).toBe(2);
    expect(phases[0]!.name).toBe("Foundation");
  });

  it("returns [] when no phases exist for project", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    expect(orch.listPhases(projectId)).toEqual([]);
  });
});

// --- adjustPhase ---

describe("RoadmapOrchestrator.adjustPhase()", () => {
  it("renaming a phase updates its name in the DB", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: [], successCriteria: [], phaseOrder: 1 },
    ]);

    orch.adjustPhase(projectId, "rename it", { phaseName: "Foundation", newName: "Auth Layer" });

    const phases = orch.listPhases(projectId);
    expect(phases[0]!.name).toBe("Auth Layer");
  });

  it("updating requirements replaces the requirements array for that phase", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    orch.adjustPhase(projectId, "add AUTH-02", { phaseName: "Foundation", requirements: ["AUTH-01", "AUTH-02"] });

    const phases = orch.listPhases(projectId);
    expect(phases[0]!.requirements).toEqual(["AUTH-01", "AUTH-02"]);
  });

  it("throws PlanningError when phase name not found", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    expect(() => {
      orch.adjustPhase(projectId, "rename nonexistent", { phaseName: "Nonexistent Phase", newName: "Something" });
    }).toThrow("Phase not found");
  });
});

// --- planPhase ---

const SAMPLE_PLAN: PhasePlan = {
  steps: [
    { id: "step-1", description: "Set up schema", subtasks: ["Create tables"], dependsOn: [] },
  ],
  dependencies: ["AUTH-01"],
  acceptanceCriteria: ["Database is initialized"],
};

describe("RoadmapOrchestrator.planPhase() — plan_check=false", () => {
  it("calls generatePlanForPhase exactly once, stores plan, never calls checkPlan", async () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const [phase] = orch.listPhases(projectId);
    const phaseId = phase!.id;

    const planJson = JSON.stringify(SAMPLE_PLAN);
    const dispatchResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: "```json\n" + planJson + "\n```", durationMs: 100 }],
    });

    const dispatchTool = makeDispatchTool([dispatchResponse]);
    const orchWithTool = new RoadmapOrchestrator(db, dispatchTool);

    const result = await orchWithTool.planPhase(projectId, phaseId, { plan_check: false }, FAKE_TOOL_CONTEXT);

    expect(result.steps).toHaveLength(1);
    expect(dispatchTool.execute).toHaveBeenCalledTimes(1);

    // Verify stored in DB
    const store = new PlanningStore(db);
    const stored = store.getPhase(phaseId);
    expect(stored?.plan).not.toBeNull();
  });
});

describe("RoadmapOrchestrator.planPhase() — plan_check=true, checker passes on first try", () => {
  it("calls generatePlanForPhase once, checkPlan once (pass=true), stores plan", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const planJson = JSON.stringify(SAMPLE_PLAN);
    const planResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: "```json\n" + planJson + "\n```", durationMs: 100 }],
    });
    const checkPassResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: JSON.stringify({ pass: true, issues: [] }), durationMs: 50 }],
    });

    const dispatchTool = makeDispatchTool([planResponse, checkPassResponse]);
    const orch = new RoadmapOrchestrator(db, dispatchTool);

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const [phase] = orch.listPhases(projectId);
    const phaseId = phase!.id;

    const result = await orch.planPhase(projectId, phaseId, { plan_check: true }, FAKE_TOOL_CONTEXT);
    expect(result.steps).toHaveLength(1);
    expect(dispatchTool.execute).toHaveBeenCalledTimes(2);
  });
});

describe("RoadmapOrchestrator.planPhase() — plan_check=true, checker fails then passes", () => {
  it("calls generatePlanForPhase, checkPlan (fail), revisePlan, checkPlan (pass), stores revised plan", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const planJson = JSON.stringify(SAMPLE_PLAN);
    const revisedPlan: PhasePlan = { ...SAMPLE_PLAN, acceptanceCriteria: ["All tests pass"] };
    const revisedPlanJson = JSON.stringify(revisedPlan);

    const planResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: "```json\n" + planJson + "\n```", durationMs: 100 }],
    });
    const checkFailResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: JSON.stringify({ pass: false, issues: ["Missing step for AUTH-02"] }), durationMs: 50 }],
    });
    const revisedResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: "```json\n" + revisedPlanJson + "\n```", durationMs: 100 }],
    });
    const checkPassResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: JSON.stringify({ pass: true, issues: [] }), durationMs: 50 }],
    });

    // Calls: generate, checkFail, revise, checkPass
    const dispatchTool = makeDispatchTool([planResponse, checkFailResponse, revisedResponse, checkPassResponse]);
    const orch = new RoadmapOrchestrator(db, dispatchTool);

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const [phase] = orch.listPhases(projectId);
    const phaseId = phase!.id;

    const result = await orch.planPhase(projectId, phaseId, { plan_check: true }, FAKE_TOOL_CONTEXT);
    expect(result.acceptanceCriteria).toEqual(["All tests pass"]);
    expect(dispatchTool.execute).toHaveBeenCalledTimes(4);
  });
});

describe("RoadmapOrchestrator.planPhase() — plan_check=true, checker fails 3 times (best-effort)", () => {
  it("runs 3 check+revise iterations and stores best-effort plan anyway", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const planJson = JSON.stringify(SAMPLE_PLAN);
    const planResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: "```json\n" + planJson + "\n```", durationMs: 100 }],
    });
    const checkFailResponse = JSON.stringify({
      results: [{ index: 0, status: "success", result: JSON.stringify({ pass: false, issues: ["Issue"] }), durationMs: 50 }],
    });

    // generate + checkFail + revise + checkFail + revise + checkFail = 6 calls total
    const dispatchTool = makeDispatchTool([
      planResponse,      // generate
      checkFailResponse, // check 1: fail
      planResponse,      // revise 1
      checkFailResponse, // check 2: fail
      planResponse,      // revise 2
      checkFailResponse, // check 3: fail — best-effort
    ]);
    const orch = new RoadmapOrchestrator(db, dispatchTool);

    orch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const [phase] = orch.listPhases(projectId);
    const phaseId = phase!.id;

    const result = await orch.planPhase(projectId, phaseId, { plan_check: true }, FAKE_TOOL_CONTEXT);
    expect(result.steps).toHaveLength(1);
    expect(dispatchTool.execute).toHaveBeenCalledTimes(6);

    // Verify stored in DB despite all checks failing
    const store = new PlanningStore(db);
    const stored = store.getPhase(phaseId);
    expect(stored?.plan).not.toBeNull();
  });
});

// --- depthToInstruction ---

describe("depthToInstruction()", () => {
  it("quick returns instruction containing '1-3'", () => {
    const orch = new RoadmapOrchestrator(makeDb(), makeDispatchTool([]));
    // Access via public or private - test via generateRoadmap indirectly isn't feasible
    // Instead, expose depthToInstruction as a public method
    expect(orch.depthToInstruction("quick")).toContain("1-3");
  });

  it("standard returns instruction containing '3-5'", () => {
    const orch = new RoadmapOrchestrator(makeDb(), makeDispatchTool([]));
    expect(orch.depthToInstruction("standard")).toContain("3-5");
  });

  it("comprehensive returns instruction containing '5-10'", () => {
    const orch = new RoadmapOrchestrator(makeDb(), makeDispatchTool([]));
    expect(orch.depthToInstruction("comprehensive")).toContain("5-10");
  });
});

// --- commitRoadmap atomic transaction ---

describe("RoadmapOrchestrator.commitRoadmap()", () => {
  it("replaces existing phases atomically for a project", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    orch.commitRoadmap(projectId, SAMPLE_PHASES);
    expect(orch.listPhases(projectId)).toHaveLength(2);

    // Commit again with different phases — should replace
    orch.commitRoadmap(projectId, [
      { name: "Only Phase", goal: "Single", requirements: [], successCriteria: [], phaseOrder: 1 },
    ]);
    const phases = orch.listPhases(projectId);
    expect(phases).toHaveLength(1);
    expect(phases[0]!.name).toBe("Only Phase");
  });

  it("defaults requirements and successCriteria to [] when omitted", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RoadmapOrchestrator(db, makeDispatchTool([]));

    // Cast to test guard behavior — omit optional arrays
    const phase = { name: "Minimal", goal: "Goal", phaseOrder: 1 } as unknown as PhaseDefinition;
    orch.commitRoadmap(projectId, [phase]);

    const phases = orch.listPhases(projectId);
    expect(phases[0]!.requirements).toEqual([]);
    expect(phases[0]!.successCriteria).toEqual([]);
  });
});

// --- formatRoadmapDisplay ---

describe("RoadmapOrchestrator.formatRoadmapDisplay()", () => {
  it("returns markdown with phase headers and ends with adjustment prompt", () => {
    const orch = new RoadmapOrchestrator(makeDb(), makeDispatchTool([]));
    const output = orch.formatRoadmapDisplay(SAMPLE_PHASES);
    expect(output).toContain("## Generated Roadmap");
    expect(output).toContain("### Phase 1: Foundation");
    expect(output).toContain("### Phase 2: API Layer");
    expect(output).toContain("**Goal:** Set up auth and storage");
    expect(output).toContain("AUTH-01");
    expect(output).toContain("Adjust anything?");
  });
});
