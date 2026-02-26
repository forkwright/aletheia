import { describe, expect, it, vi } from "vitest";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
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
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  db.exec(PLANNING_V26_MIGRATION);
  db.exec(PLANNING_V27_MIGRATION);
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

function makeOrchestrator(db: Database.Database, mockDispatch: ToolHandler): ResearchOrchestrator {
  const orch = new ResearchOrchestrator(db, mockDispatch);
  orch.retryDelayMs = 0; // No delay in tests
  return orch;
}

/** Default research result wrapped in ```json block, matching validateResearcherResponse expectations */
function defaultResearchResult(i: number): string {
  return "```json\n" + JSON.stringify({ summary: `summary ${i}`, details: `details ${i}`, confidence: "high" }) + "\n```";
}

function makeDispatchResult(overrides: Array<Partial<{ status: string; result: string; error: string; durationMs: number }>>) {
  const results = overrides.map((o, i) => ({
    index: i,
    status: o.status ?? "success",
    result: o.result ?? defaultResearchResult(i),
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
        // First call: batch dispatch of all 4 dimensions
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "success" },
          ]),
        )
        // Second call: synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Consolidated synthesis" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
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

  it("retries timed-out dimension individually; stores partial if retry also times out", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        // Batch dispatch: features times out
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "timeout", durationMs: 90000 },
            { status: "success" },
            { status: "success" },
          ]),
        )
        // Individual retry for features — also times out
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "timeout", durationMs: 90000 }]),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Partial synthesis" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
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
  });

  it("retries errored dimension; stores failed if retry also errors", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        // Batch dispatch: architecture errors
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "error", error: "agent crashed", durationMs: 500 },
            { status: "success" },
          ]),
        )
        // Individual retry for architecture — also errors
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "error", error: "agent crashed again" }]),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Error synthesis" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(3);
    expect(result.partial).toBe(0);
    expect(result.failed).toBe(1);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    expect(rows).toHaveLength(5); // 4 dimensions + synthesis

    const errored = rows.find(r => r.dimension === "architecture");
    expect(errored?.status).toBe("failed");
  });

  it("retries errored dimension and succeeds on retry", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        // Batch dispatch: pitfalls errors
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "error", error: "agent crashed" },
          ]),
        )
        // Individual retry for pitfalls — succeeds this time
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success" }]),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Full synthesis" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(4);
    expect(result.partial).toBe(0);
    expect(result.failed).toBe(0);
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

    const orchestrator = makeOrchestrator(db, mockDispatch);
    await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    const synthesis = rows.find(r => r.dimension === "synthesis");

    expect(synthesis).toBeDefined();
    expect(synthesis?.status).toBe("complete");
    expect(synthesis?.content).toContain("## Stack");
  });

  it("skips already-completed dimensions on re-run", async () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);

    // Pre-populate 2 completed dimensions
    store.createResearch({ projectId, phase: "research", dimension: "stack", content: '{"summary":"done","details":"done","confidence":"high"}', status: "complete" });
    store.createResearch({ projectId, phase: "research", dimension: "features", content: '{"summary":"done","details":"done","confidence":"high"}', status: "complete" });

    const mockDispatch: ToolHandler = {
      definition: { name: "sessions_dispatch", description: "", input_schema: { type: "object", properties: {}, required: [] } },
      execute: vi.fn()
        // Only 2 dimensions dispatched (architecture + pitfalls)
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
          ]),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Complete synthesis" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    // 2 pre-existing + 2 newly dispatched
    expect(result.stored).toBe(4);
    expect(result.partial).toBe(0);
    expect(result.failed).toBe(0);

    // Dispatch should have been called with only 2 tasks (not 4)
    const firstCall = (mockDispatch.execute as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(firstCall[0].tasks).toHaveLength(2);
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
        // Batch: pitfalls times out
        .mockResolvedValueOnce(
          makeDispatchResult([
            { status: "success" },
            { status: "success" },
            { status: "success" },
            { status: "timeout", durationMs: 90000 },
          ]),
        )
        // Individual retry for pitfalls — also times out
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "timeout", durationMs: 90000 }]),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Synthesis with partial data" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
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

describe("ResearchOrchestrator - CTX-04 enhancements", () => {
  it("validates JSON response structure and marks invalid as partial", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "mock", description: "", input_schema: {} },
      execute: vi.fn()
        // Batch dispatch: 1 valid, 3 invalid (no JSON / wrong schema / malformed)
        .mockResolvedValueOnce(
          JSON.stringify({
            results: [
              {
                index: 0,
                status: "success",
                result: `Valid JSON response: \`\`\`json\n{"summary":"good summary","details":"good details","confidence":"high"}\n\`\`\``,
                durationMs: 1000,
              },
              {
                index: 1,
                status: "success",
                result: "Invalid response with no JSON block at all",
                durationMs: 1000,
              },
              {
                index: 2,
                status: "success",
                result: `Invalid JSON structure: \`\`\`json\n{"wrong":"fields","missing":"required"}\n\`\`\``,
                durationMs: 1000,
              },
              {
                index: 3,
                status: "success",
                result: `Malformed JSON: \`\`\`json\n{"summary":"test", bad json\n\`\`\``,
                durationMs: 1000,
              },
            ],
          }),
        )
        // Synthesis
        .mockResolvedValueOnce(
          makeDispatchResult([{ status: "success", result: "Synthesis of valid + partial data" }]),
        ),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
    const result = await orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT);

    expect(result.stored).toBe(1); // Only first one valid
    expect(result.partial).toBe(3); // Others marked as partial due to validation failures

    const store = new PlanningStore(db);
    const rows = store.listResearch(projectId);
    
    const stackRow = rows.find(r => r.dimension === "stack");
    expect(stackRow?.status).toBe("complete");
    expect(stackRow?.content).toContain('"confidence": "high"'); // Structured JSON

    const featuresRow = rows.find(r => r.dimension === "features");
    expect(featuresRow?.status).toBe("partial");
    expect(featuresRow?.content).toContain("Invalid response with no JSON block"); // Raw text

    const architectureRow = rows.find(r => r.dimension === "architecture");
    expect(architectureRow?.status).toBe("partial");
    expect(architectureRow?.content).toContain("Invalid JSON structure"); // Raw text
  });

  it("throws error when all dimensions fail after retries", async () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const mockDispatch: ToolHandler = {
      definition: { name: "mock", description: "", input_schema: {} },
      execute: vi.fn()
        // Batch dispatch: all error
        .mockResolvedValueOnce(
          JSON.stringify({
            results: [
              { index: 0, status: "error", error: "Failed", durationMs: 1000 },
              { index: 1, status: "error", error: "Failed", durationMs: 1000 },
              { index: 2, status: "error", error: "Failed", durationMs: 1000 },
              { index: 3, status: "error", error: "Failed", durationMs: 1000 },
            ],
          }),
        )
        // Individual retries — all fail again
        .mockResolvedValueOnce(makeDispatchResult([{ status: "error", error: "Failed" }]))
        .mockResolvedValueOnce(makeDispatchResult([{ status: "error", error: "Failed" }]))
        .mockResolvedValueOnce(makeDispatchResult([{ status: "error", error: "Failed" }]))
        .mockResolvedValueOnce(makeDispatchResult([{ status: "error", error: "Failed" }])),
    };

    const orchestrator = makeOrchestrator(db, mockDispatch);
    
    await expect(
      orchestrator.runResearch(projectId, "Build a planning tool", TOOL_CONTEXT)
    ).rejects.toThrow("Research failed: No dimensions completed successfully");
  });
});
