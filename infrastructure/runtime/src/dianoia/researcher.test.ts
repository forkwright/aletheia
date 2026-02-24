import { describe, it, expect, vi } from "vitest";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";
import { ResearchOrchestrator } from "./researcher.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { transition } from "./machine.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  return db;
}

const TOOL_CONTEXT: ToolContext = {
  nousId: "test-nous",
  sessionId: "test-session",
  workspace: "/tmp",
};

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
  return project.id;
}

function makeDispatchResult(overrides: Array<Partial<{ status: string; result: string; error: string; durationMs: number }>>) {
  const results = overrides.map((o, i) => ({
    index: i,
    status: o.status ?? "success",
    result: o.result ?? `{"summary":"summary ${i}","details":"details ${i}","confidence":"high"}`,
    error: o.error,
    durationMs: o.durationMs ?? 1000,
  }));
  return JSON.stringify({ taskCount: overrides.length, succeeded: overrides.filter(o => o.status !== "error" && o.status !== "timeout").length, failed: 0, results });
}

describe("ResearchOrchestrator.runResearch()", () => {
  it("stores 4 complete rows when all dispatched tasks succeed", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "success" },
          ]),
        )
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Consolidated synthesis" }]),
        ),
    };

    const orchestrator = new ResearchOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(4);
    expect(result.partial).toBe(0);
    expect(result.failed).toBe(0);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    expect(rows).toHaveLength(5); // 4 dimensions + synthesis

    const dimRows = rows.filter(r => r.dimension !== "synthesis");
    for (const row of dimRows) {
      expect(row.status).toBe("complete");
    }

    const dimensions = dimRows.map(r => r.dimension);
    expect(dimensions).toContain("stack");
    expect(dimensions).toContain("features");
    expect(dimensions).toContain("architecture");
    expect(dimensions).toContain("pitfalls");
  });

  it("stores partial row for timed-out dimension; others remain complete", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "timeout", durationMs: 90000 },
            { status: "success" },
            { status: "success" },
          ]),
        )
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Partial synthesis" }]),
        ),
    };

    const orchestrator = new ResearchOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(3);
    expect(result.partial).toBe(1);
    expect(result.failed).toBe(0);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    expect(rows).toHaveLength(5); // 4 dimensions + synthesis

    const timedOut = rows.find(r => r.dimension === "features");
    expect(timedOut?.status).toBe("partial");
    const timedOutContent = JSON.parse(timedOut!.content) as { reason: string };
    expect(timedOutContent.reason).toBe("timeout");

    const complete = rows.filter(r => r.dimension !== "features" && r.dimension !== "synthesis");
    for (const row of complete) {
      expect(row.status).toBe("complete");
    }
  });

  it("stores failed row for errored dimension; others remain complete", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "error", error: "agent crashed", durationMs: 500 },
            { status: "success" },
          ]),
        )
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Error synthesis" }]),
        ),
    };

    const orchestrator = new ResearchOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(3);
    expect(result.partial).toBe(0);
    expect(result.failed).toBe(1);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    expect(rows).toHaveLength(5); // 4 dimensions + synthesis

    const errored = rows.find(r => r.dimension === "architecture");
    expect(errored?.status).toBe("failed");
    const erroredContent = JSON.parse(errored!.content) as { reason: string; error: string };
    expect(erroredContent.reason).toBe("error");
    expect(erroredContent.error).toBe("agent crashed");

    const complete = rows.filter(r => r.dimension !== "architecture" && r.dimension !== "synthesis");
    for (const row of complete) {
      expect(row.status).toBe("complete");
    }
  });

  it("stores a synthesis row with dimension='synthesis' after all dimensions complete", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "success" },
          ]),
        )
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "## Stack\nNode.js\n## Features\nCore features" }]),
        ),
    };

    const orchestrator = new ResearchOrchestrator(db, mockDispatch);
    await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    const synthesis = rows.find(r => r.dimension === "synthesis");

    expect(synthesis).toBeDefined();
    expect(synthesis?.status).toBe("complete");
    expect(synthesis?.content).toContain("## Stack");
  });
});

describe("DianoiaOrchestrator.skipResearch()", () => {
  it("transitions project from researching to requirements without creating research rows", () => {
    const db = makeDb();
    const store = new PlanningStore(db);
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Build something",
      config: DEFAULT_CONFIG,
    });

    store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));
    store.updateProjectState(project.id, transition("questioning", "START_RESEARCH"));

    const orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    const message = orchestrator.skipResearch(project.id, "nous-1", "session-1");

    const updated = store.getProjectOrThrow(project.id);
    expect(updated.state).toBe("requirements");
    expect(message).toBe("Research skipped. Proceeding to requirements definition.");

    const researchRows = store.listResearch(project.id);
    expect(researchRows).toHaveLength(0);
  });
});

describe("ResearchOrchestrator partial result surfacing", () => {
  it("returns partial=1 when one dimension times out and the row has status=partial", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "timeout", durationMs: 90000 },
          ]),
        )
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Synthesis with partial data" }]),
        ),
    };

    const orchestrator = new ResearchOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.partial).toBe(1);
    expect(result.stored).toBe(3);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    const partialRow = rows.find(r => r.status === "partial");
    expect(partialRow).toBeDefined();
    expect(partialRow?.dimension).toBe("pitfalls");
  });
});
