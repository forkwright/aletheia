// GoalBackwardVerifier unit tests — disabled path, met path, not-met path, generateGapPlans
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { GoalBackwardVerifier } from "./verifier.js";
import type { VerificationGap } from "./types.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";

let db: Database.Database;
let store: PlanningStore;

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
  return d;
}

const toolContext = {} as ToolContext;

beforeEach(() => {
  db = makeDb();
  store = new PlanningStore(db);
});

afterEach(() => {
  db.close();
});

describe("GoalBackwardVerifier.verify — verifier disabled", () => {
  it("returns met immediately without dispatching when config.verifier is false", async () => {
    const disabledConfig = { ...defaultConfig, verifier: false };
    const project = store.createProject({
      nousId: "nous-disabled",
      sessionId: "sess-disabled",
      goal: "test project",
      config: disabledConfig,
    });
    const phase = store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "implement feature",
      requirements: [],
      successCriteria: ["Feature works"],
      phaseOrder: 1,
    });

    const mockDispatch = vi.fn() as unknown as ToolHandler;

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const result = await verifier.verify(project.id, phase.id, toolContext);

    expect(result.status).toBe("met");
    expect(result.summary).toBe("Verification disabled.");
    expect(result.gaps).toEqual([]);
    expect(result.verifiedAt).toBeDefined();
    expect(mockDispatch).not.toHaveBeenCalled();

    // Side effect: verification result persisted
    const updated = store.getPhaseOrThrow(phase.id);
    expect(updated.verificationResult?.status).toBe("met");
  });
});

describe("GoalBackwardVerifier.verify — verifier enabled, phase met", () => {
  it("dispatches sub-agent and returns met result", async () => {
    const project = store.createProject({
      nousId: "nous-enabled",
      sessionId: "sess-enabled",
      goal: "build API",
      config: defaultConfig,
    });
    const phase = store.createPhase({
      projectId: project.id,
      name: "Auth Phase",
      goal: "implement authentication",
      requirements: ["AUTH-01"],
      successCriteria: ["Users can login"],
      phaseOrder: 1,
    });

    const dispatchResult = {
      results: [
        {
          index: 0,
          status: "success" as const,
          result: JSON.stringify({
            status: "met",
            summary: "All criteria satisfied.",
            gaps: [],
          }),
          durationMs: 100,
        },
      ],
    };

    const mockDispatch = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object" as const, properties: {} } },
      execute: vi.fn().mockResolvedValue(JSON.stringify(dispatchResult)),
    } as unknown as ToolHandler;

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const result = await verifier.verify(project.id, phase.id, toolContext);

    expect(result.status).toBe("met");
    expect(result.summary).toBe("All criteria satisfied.");
    expect(result.gaps).toEqual([]);
    expect(result.verifiedAt).toBeDefined();
    expect((mockDispatch as { execute: ReturnType<typeof vi.fn> }).execute).toHaveBeenCalledOnce();

    // Dispatch payload contains phase goal and success criteria
    const dispatchCall = (mockDispatch as { execute: ReturnType<typeof vi.fn> }).execute.mock.calls[0]!;
    const dispatchInput = dispatchCall[0] as Record<string, unknown>;
    const tasks = dispatchInput["tasks"] as Array<{ context: string }>;
    expect(tasks[0]?.context).toContain("implement authentication");
    expect(tasks[0]?.context).toContain("Users can login");

    // Side effect: persisted
    const updated = store.getPhaseOrThrow(phase.id);
    expect(updated.verificationResult?.status).toBe("met");
  });
});

describe("GoalBackwardVerifier.verify — verifier enabled, phase not-met with gaps", () => {
  it("returns not-met result with gaps and persists to store", async () => {
    const project = store.createProject({
      nousId: "nous-notmet",
      sessionId: "sess-notmet",
      goal: "build API",
      config: defaultConfig,
    });
    const phase = store.createPhase({
      projectId: project.id,
      name: "Data Phase",
      goal: "implement data layer",
      requirements: ["DATA-01"],
      successCriteria: ["Data persists", "Queries are fast"],
      phaseOrder: 1,
    });

    const gap: VerificationGap = {
      criterion: "Queries are fast",
      found: "No indexes on query columns",
      expected: "Indexes on frequently queried columns",
      proposedFix: "Add indexes to query columns",
    };

    const dispatchResult = {
      results: [
        {
          index: 0,
          status: "success" as const,
          result: JSON.stringify({
            status: "not-met",
            summary: "Performance criterion not met.",
            gaps: [gap],
          }),
          durationMs: 150,
        },
      ],
    };

    const mockDispatch = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object" as const, properties: {} } },
      execute: vi.fn().mockResolvedValue(JSON.stringify(dispatchResult)),
    } as unknown as ToolHandler;

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const result = await verifier.verify(project.id, phase.id, toolContext);

    expect(result.status).toBe("not-met");
    expect(result.summary).toBe("Performance criterion not met.");
    expect(result.gaps).toHaveLength(1);
    expect(result.gaps[0]).toEqual(gap);

    // Side effect: persisted
    const updated = store.getPhaseOrThrow(phase.id);
    expect(updated.verificationResult?.status).toBe("not-met");
    expect(updated.verificationResult?.gaps).toHaveLength(1);
  });

  it("falls back to partially-met when dispatch result is unparseable", async () => {
    const project = store.createProject({
      nousId: "nous-fallback",
      sessionId: "sess-fallback",
      goal: "build API",
      config: defaultConfig,
    });
    const phase = store.createPhase({
      projectId: project.id,
      name: "Phase X",
      goal: "some goal",
      requirements: [],
      successCriteria: ["Something works"],
      phaseOrder: 1,
    });

    const dispatchResult = {
      results: [
        {
          index: 0,
          status: "success" as const,
          result: "not valid json {{{",
          durationMs: 50,
        },
      ],
    };

    const mockDispatch = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object" as const, properties: {} } },
      execute: vi.fn().mockResolvedValue(JSON.stringify(dispatchResult)),
    } as unknown as ToolHandler;

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const result = await verifier.verify(project.id, phase.id, toolContext);

    expect(result.status).toBe("partially-met");
    expect(result.summary).toBe("(verification unavailable)");
    expect(result.gaps).toEqual([]);
  });
});

describe("GoalBackwardVerifier.generateGapPlans", () => {
  it("returns empty array when gaps is empty", () => {
    const mockDispatch = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object" as const, properties: {} } },
      execute: vi.fn(),
    } as unknown as ToolHandler;

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const plans = verifier.generateGapPlans("phase-x", []);
    expect(plans).toEqual([]);
  });

  it("returns one PhasePlan per gap with criterion-derived name and proposedFix acceptance criteria", () => {
    const mockDispatch = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object" as const, properties: {} } },
      execute: vi.fn(),
    } as unknown as ToolHandler;

    const gaps: VerificationGap[] = [
      {
        criterion: "Users can login",
        found: "No login endpoint",
        expected: "POST /auth/login implemented",
        proposedFix: "Implement POST /auth/login",
      },
      {
        criterion: "Data persists across restarts",
        found: "In-memory store only",
        expected: "SQLite persistence",
        proposedFix: "Add SQLite database layer",
      },
    ];

    const verifier = new GoalBackwardVerifier(db, mockDispatch);
    const plans = verifier.generateGapPlans("phase-123", gaps);

    expect(plans).toHaveLength(2);

    expect(plans[0]!.steps).toHaveLength(1);
    expect(plans[0]!.steps[0]!.acceptanceCriteria).toContain("Implement POST /auth/login");
    expect(plans[0]!.id).toBeDefined();
    expect(plans[0]!.id).toMatch(/^vrfy_/);

    expect(plans[1]!.steps).toHaveLength(1);
    expect(plans[1]!.steps[0]!.acceptanceCriteria).toContain("Add SQLite database layer");
  });
});
