import { describe, expect, it, type MockedFunction, vi } from "vitest";
import Database from "better-sqlite3";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
  PLANNING_V27_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { RoadmapOrchestrator } from "./roadmap.js";
import { createPlanRoadmapTool } from "./roadmap-tool.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { PlanningPhase } from "./types.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { transition } from "./machine.js";

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

const INTERACTIVE_CONFIG = {
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: false,
  verifier: true,
  mode: "interactive" as const,
};

const YOLO_CONFIG = {
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: false,
  verifier: true,
  mode: "yolo" as const,
};

function makeProject(db: Database.Database, config = INTERACTIVE_CONFIG): string {
  const store = new PlanningStore(db);
  const project = store.createProject({
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Build a planning tool",
    config,
  });
  store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));
  store.updateProjectState(project.id, transition("questioning", "START_RESEARCH"));
  store.updateProjectState(project.id, transition("researching", "RESEARCH_COMPLETE"));
  store.updateProjectState(project.id, transition("requirements", "REQUIREMENTS_COMPLETE"));
  return project.id;
}

function makeV1Req(store: PlanningStore, projectId: string, reqId: string, category: string): void {
  store.createRequirement({
    projectId,
    reqId,
    description: `User can do ${reqId}`,
    category,
    tier: "v1",
  });
}

function makeDispatchTool(responses: string[]): ToolHandler {
  let callCount = 0;
  return {
    definition: {
      name: "sessions_dispatch",
      description: "Dispatch tasks",
      input_schema: { type: "object" as const, properties: {}, required: [] },
    },
    execute: vi.fn((_input, _context) => {
      const response = responses[callCount] ?? responses[responses.length - 1]!;
      callCount++;
      return Promise.resolve(response);
    }) as MockedFunction<ToolHandler["execute"]>,
  };
}

function makeMockOrchestrator(db: Database.Database): DianoiaOrchestrator {
  const store = new PlanningStore(db);

  const mock = {
    getProject: vi.fn((id: string) => store.getProject(id)),
    completeRoadmap: vi.fn((_projectId: string, _nousId: string, _sessionId: string) => "Roadmap committed. Starting phase planning."),
    advanceToExecution: vi.fn((_projectId: string, _nousId: string, _sessionId: string) => "All phase plans ready. Moving to execution."),
    listPhases: vi.fn((projectId: string) => store.listPhases(projectId)),
    getPhase: vi.fn((phaseId: string) => store.getPhase(phaseId)),
  } as unknown as DianoiaOrchestrator;

  return mock;
}

const FAKE_CONTEXT: ToolContext = {
  nousId: "test-nous",
  sessionId: "test-session",
  workspace: "/tmp/test",
};

function makeRoadmapDispatchResponse(phases: object[]): string {
  return JSON.stringify({
    results: [
      {
        index: 0,
        status: "success",
        result: "```json\n" + JSON.stringify(phases) + "\n```",
        durationMs: 100,
      },
    ],
  });
}

// --- generate action (interactive mode) ---

describe("plan_roadmap generate action (interactive mode)", () => {
  it("returns draft=true and display containing 'Generated Roadmap' when v1 coverage met", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    const phases = [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01", "AUTH-02"], successCriteria: [], phaseOrder: 1 },
    ];
    const dispatchTool = makeDispatchTool([makeRoadmapDispatchResponse(phases)]);
    const roadmapOrch = new RoadmapOrchestrator(db, dispatchTool);
    const mockOrch = makeMockOrchestrator(db);

    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(await tool.execute({ action: "generate", projectId }, FAKE_CONTEXT));

    expect(result.draft).toBe(true);
    expect(result.display).toContain("Generated Roadmap");
    expect(result.message).toContain("Review the roadmap");
  });

  it("returns error with missing array when agent phases miss a v1 req", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    // Phases only cover AUTH-01, not AUTH-02
    const phases = [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ];
    const dispatchTool = makeDispatchTool([makeRoadmapDispatchResponse(phases)]);
    const roadmapOrch = new RoadmapOrchestrator(db, dispatchTool);
    const mockOrch = makeMockOrchestrator(db);

    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(await tool.execute({ action: "generate", projectId }, FAKE_CONTEXT));

    expect(result.error).toBe("Coverage validation failed");
    expect(result.missing).toContain("AUTH-02");
  });
});

// --- generate action (autonomous / yolo mode) ---

describe("plan_roadmap generate action (autonomous mode)", () => {
  it("auto-commits roadmap, calls completeRoadmap, returns committed=true", async () => {
    const db = makeDb();
    const projectId = makeProject(db, YOLO_CONFIG);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");

    const phases = [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ];
    const dispatchTool = makeDispatchTool([makeRoadmapDispatchResponse(phases)]);
    const roadmapOrch = new RoadmapOrchestrator(db, dispatchTool);
    const mockOrch = makeMockOrchestrator(db);

    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(await tool.execute({ action: "generate", projectId }, FAKE_CONTEXT));

    expect(result.committed).toBe(true);
    expect(result.phaseCount).toBe(1);
    expect(mockOrch.completeRoadmap).toHaveBeenCalledWith(projectId, FAKE_CONTEXT.nousId, FAKE_CONTEXT.sessionId);
  });
});

// --- adjust_phase action ---

describe("plan_roadmap adjust_phase action", () => {
  it("renames a phase and returns adjusted=true with updated display", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);
    const roadmapOrch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    const mockOrch = makeMockOrchestrator(db);

    roadmapOrch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Set up auth", requirements: [], successCriteria: [], phaseOrder: 1 },
    ]);

    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(
      await tool.execute(
        { action: "adjust_phase", projectId, adjustment: "rename it", phaseName: "Foundation", newName: "Renamed Phase" },
        FAKE_CONTEXT,
      ),
    );

    expect(result.adjusted).toBe(true);
    expect(result.display).toContain("Renamed Phase");
  });
});

// --- commit action ---

describe("plan_roadmap commit action", () => {
  it("calls completeRoadmap and returns committed=true when all v1 reqs are covered", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");

    const roadmapOrch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    roadmapOrch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const mockOrch = makeMockOrchestrator(db);
    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(await tool.execute({ action: "commit", projectId }, FAKE_CONTEXT));

    expect(result.committed).toBe(true);
    expect(mockOrch.completeRoadmap).toHaveBeenCalledWith(projectId, FAKE_CONTEXT.nousId, FAKE_CONTEXT.sessionId);
  });

  it("returns coverage error and does NOT call completeRoadmap when coverage gate not met", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);
    const store = new PlanningStore(db);
    makeV1Req(store, projectId, "AUTH-01", "AUTH");
    makeV1Req(store, projectId, "AUTH-02", "AUTH");

    // Only AUTH-01 covered in phases
    const roadmapOrch = new RoadmapOrchestrator(db, makeDispatchTool([]));
    roadmapOrch.commitRoadmap(projectId, [
      { name: "Foundation", goal: "Auth", requirements: ["AUTH-01"], successCriteria: [], phaseOrder: 1 },
    ]);

    const mockOrch = makeMockOrchestrator(db);
    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);
    const result = JSON.parse(await tool.execute({ action: "commit", projectId }, FAKE_CONTEXT));

    expect(result.error).toBe("Coverage gate not met");
    expect(result.missing).toContain("AUTH-02");
    expect(mockOrch.completeRoadmap).not.toHaveBeenCalled();
  });
});

// --- plan_phases action ---

describe("plan_roadmap plan_phases action", () => {
  it("calls planPhase for each phase sequentially and advanceToExecution once, returns planned count", async () => {
    const db = makeDb();
    const projectId = makeProject(db, INTERACTIVE_CONFIG);

    const samplePlan = {
      steps: [{ id: "step-1", description: "Do thing", subtasks: [], dependsOn: [] }],
      dependencies: [],
      acceptanceCriteria: ["Done"],
    };
    const planResponse = JSON.stringify({
      results: [
        {
          index: 0,
          status: "success",
          result: "```json\n" + JSON.stringify(samplePlan) + "\n```",
          durationMs: 100,
        },
      ],
    });

    const dispatchTool = makeDispatchTool([planResponse, planResponse]);
    const roadmapOrch = new RoadmapOrchestrator(db, dispatchTool);

    roadmapOrch.commitRoadmap(projectId, [
      { name: "Phase A", goal: "First phase", requirements: [], successCriteria: [], phaseOrder: 1 },
      { name: "Phase B", goal: "Second phase", requirements: [], successCriteria: [], phaseOrder: 2 },
    ]);

    const phases = roadmapOrch.listPhases(projectId) as PlanningPhase[];
    const phaseIds = phases.toSorted((a, b) => a.phaseOrder - b.phaseOrder).map((p) => p.id);

    const mockOrch = makeMockOrchestrator(db);
    const tool = createPlanRoadmapTool(mockOrch, roadmapOrch);

    const result = JSON.parse(
      await tool.execute({ action: "plan_phases", projectId, phaseIds }, FAKE_CONTEXT),
    );

    expect(result.planned).toBe(2);
    expect(result.message).toContain("All phase plans generated");
    expect(dispatchTool.execute).toHaveBeenCalledTimes(2);
    expect(mockOrch.advanceToExecution).toHaveBeenCalledWith(projectId, FAKE_CONTEXT.nousId, FAKE_CONTEXT.sessionId);
    expect(mockOrch.advanceToExecution).toHaveBeenCalledTimes(1);
  });
});
