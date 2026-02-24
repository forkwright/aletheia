// ExecutionOrchestrator unit tests — in-memory SQLite, wave computation, cascade-skip, resume detection
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";
import { computeWaves, directDependents, findResumeWave, ExecutionOrchestrator } from "./execution.js";
import type { PlanningPhase } from "./types.js";
import type { SpawnRecord } from "./types.js";
import type { PhasePlan } from "./roadmap.js";

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

function makePhase(id: string, name: string, deps: string[] = []): PlanningPhase {
  const plan: PhasePlan = {
    steps: [],
    dependencies: deps,
    acceptanceCriteria: [],
  };
  return {
    id,
    projectId: "proj-test",
    name,
    goal: `Goal for ${name}`,
    requirements: [],
    successCriteria: [],
    plan,
    status: "pending",
    phaseOrder: 0,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
}

beforeEach(() => {
  db = makeDb();
  store = new PlanningStore(db);
});

afterEach(() => {
  db.close();
});

// --- computeWaves ---

describe("computeWaves", () => {
  it("groups independent plans into the same wave", () => {
    const a = makePhase("phase-A", "A");
    const c = makePhase("phase-C", "C");
    const b = makePhase("phase-B", "B", ["phase-A"]);
    const waves = computeWaves([a, b, c]);
    expect(waves).toHaveLength(2);
    expect(waves[0]!.map(p => p.id)).toEqual(expect.arrayContaining(["phase-A", "phase-C"]));
    expect(waves[1]!.map(p => p.id)).toEqual(["phase-B"]);
  });

  it("handles a single plan with no dependencies", () => {
    const a = makePhase("phase-A", "A");
    const waves = computeWaves([a]);
    expect(waves).toHaveLength(1);
    expect(waves[0]!.map(p => p.id)).toEqual(["phase-A"]);
  });

  it("detects cycles and returns remaining plans as a single wave", () => {
    const a = makePhase("phase-A", "A", ["phase-B"]);
    const b = makePhase("phase-B", "B", ["phase-A"]);
    const waves = computeWaves([a, b]);
    // Cycle: A depends on B, B depends on A — remaining treated as one wave
    expect(waves.length).toBeGreaterThanOrEqual(1);
    const allInWaves = waves.flat().map(p => p.id);
    expect(allInWaves).toEqual(expect.arrayContaining(["phase-A", "phase-B"]));
  });

  it("handles a linear dependency chain", () => {
    const a = makePhase("phase-A", "A");
    const b = makePhase("phase-B", "B", ["phase-A"]);
    const c = makePhase("phase-C", "C", ["phase-B"]);
    const waves = computeWaves([a, b, c]);
    expect(waves).toHaveLength(3);
    expect(waves[0]!.map(p => p.id)).toEqual(["phase-A"]);
    expect(waves[1]!.map(p => p.id)).toEqual(["phase-B"]);
    expect(waves[2]!.map(p => p.id)).toEqual(["phase-C"]);
  });
});

// --- directDependents ---

describe("directDependents", () => {
  it("returns only direct dependents of failed plan", () => {
    const a = makePhase("phase-A", "A");
    const b = makePhase("phase-B", "B", ["phase-A"]);
    const c = makePhase("phase-C", "C", ["phase-B"]);
    const dependents = directDependents("phase-A", [a, b, c]);
    expect(dependents.map(p => p.id)).toEqual(["phase-B"]);
    // C depends on B (not A directly) — NOT included
  });

  it("returns empty array when no plan directly depends on failed plan", () => {
    const a = makePhase("phase-A", "A");
    const b = makePhase("phase-B", "B", ["phase-X"]);
    const dependents = directDependents("phase-A", [a, b]);
    expect(dependents).toHaveLength(0);
  });

  it("returns multiple direct dependents when several plans depend on failed plan", () => {
    const a = makePhase("phase-A", "A");
    const b = makePhase("phase-B", "B", ["phase-A"]);
    const c = makePhase("phase-C", "C", ["phase-A"]);
    const dependents = directDependents("phase-A", [a, b, c]);
    expect(dependents.map(p => p.id)).toEqual(expect.arrayContaining(["phase-B", "phase-C"]));
    expect(dependents).toHaveLength(2);
  });
});

// --- findResumeWave ---

describe("findResumeWave", () => {
  it("returns 0 when no records exist", () => {
    expect(findResumeWave([])).toBe(0);
  });

  it("returns -1 when all waves are done or skipped", () => {
    const records: SpawnRecord[] = [
      { id: "r1", projectId: "p", phaseId: "ph1", waveNumber: 0, sessionKey: null, status: "done", errorMessage: null, partialOutput: null, startedAt: null, completedAt: null, createdAt: "", updatedAt: "" },
      { id: "r2", projectId: "p", phaseId: "ph2", waveNumber: 1, sessionKey: null, status: "done", errorMessage: null, partialOutput: null, startedAt: null, completedAt: null, createdAt: "", updatedAt: "" },
    ];
    expect(findResumeWave(records)).toBe(-1);
  });

  it("returns wave index of first incomplete wave", () => {
    const records: SpawnRecord[] = [
      { id: "r1", projectId: "p", phaseId: "ph1", waveNumber: 0, sessionKey: null, status: "done", errorMessage: null, partialOutput: null, startedAt: null, completedAt: null, createdAt: "", updatedAt: "" },
      { id: "r2", projectId: "p", phaseId: "ph2", waveNumber: 1, sessionKey: null, status: "running", errorMessage: null, partialOutput: null, startedAt: null, completedAt: null, createdAt: "", updatedAt: "" },
    ];
    expect(findResumeWave(records)).toBe(1);
  });

  it("returns -1 when all records are skipped", () => {
    const records: SpawnRecord[] = [
      { id: "r1", projectId: "p", phaseId: "ph1", waveNumber: 0, sessionKey: null, status: "skipped", errorMessage: null, partialOutput: null, startedAt: null, completedAt: null, createdAt: "", updatedAt: "" },
    ];
    expect(findResumeWave(records)).toBe(-1);
  });
});

// --- PlanningStore spawn record CRUD ---

describe("PlanningStore spawn records", () => {
  it("creates and retrieves a spawn record", () => {
    const project = store.createProject({ nousId: "nous", sessionId: "sess", goal: "test", config: defaultConfig });
    const phase = store.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });
    const record = store.createSpawnRecord({ projectId: project.id, phaseId: phase.id, waveNumber: 0 });
    expect(record.status).toBe("pending");
    expect(record.waveNumber).toBe(0);
    expect(record.phaseId).toBe(phase.id);
    expect(record.projectId).toBe(project.id);
  });

  it("updates spawn record status and fields", () => {
    const project = store.createProject({ nousId: "nous", sessionId: "sess", goal: "test", config: defaultConfig });
    const phase = store.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });
    const record = store.createSpawnRecord({ projectId: project.id, phaseId: phase.id, waveNumber: 0 });
    store.updateSpawnRecord(record.id, { status: "running", startedAt: "2026-01-01T00:00:00.000Z" });
    const updated = store.getSpawnRecordOrThrow(record.id);
    expect(updated.status).toBe("running");
    expect(updated.startedAt).toBe("2026-01-01T00:00:00.000Z");
  });

  it("lists spawn records for a project", () => {
    const project = store.createProject({ nousId: "nous", sessionId: "sess", goal: "test", config: defaultConfig });
    const phase1 = store.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });
    const phase2 = store.createPhase({ projectId: project.id, name: "P2", goal: "g2", requirements: [], successCriteria: [], phaseOrder: 2 });
    store.createSpawnRecord({ projectId: project.id, phaseId: phase1.id, waveNumber: 0 });
    store.createSpawnRecord({ projectId: project.id, phaseId: phase2.id, waveNumber: 1 });
    const records = store.listSpawnRecords(project.id);
    expect(records).toHaveLength(2);
  });
});

// --- isPaused — pause_between_phases config ---

describe("isPaused — pause_between_phases config", () => {
  it("stops before wave 0 when pause_between_phases is true in project config", async () => {
    const pauseConfig = { ...defaultConfig, pause_between_phases: true };
    const project = store.createProject({
      nousId: "nous-pause",
      sessionId: "sess-pause",
      goal: "pause test",
      config: pauseConfig,
    });
    const phaseA = store.createPhase({
      projectId: project.id,
      name: "A",
      goal: "g",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    const phaseB = store.createPhase({
      projectId: project.id,
      name: "B",
      goal: "g2",
      requirements: [],
      successCriteria: [],
      phaseOrder: 2,
    });
    // Set phaseB to depend on phaseA (wave 1)
    store.updatePhasePlan(phaseB.id, { steps: [], dependencies: [phaseA.id], acceptanceCriteria: [] });

    let dispatchCallCount = 0;
    const mockDispatch = {
      execute: async () => {
        dispatchCallCount++;
        return JSON.stringify({
          results: [{ status: "success", result: "done", durationMs: 100 }],
        });
      },
    } as unknown as import("../organon/registry.js").ToolHandler;

    const orch = new ExecutionOrchestrator(db, mockDispatch);
    const toolContext = {} as import("../organon/registry.js").ToolContext;
    await orch.executePhase(project.id, toolContext);

    // pause_between_phases=true: isPaused() fires before wave 0, execution halts immediately
    // No dispatches should occur
    expect(dispatchCallCount).toBe(0);

    const records = store.listSpawnRecords(project.id);
    const doneRecords = records.filter((r) => r.status === "done");
    expect(doneRecords).toHaveLength(0);
  });

  it("does not stop execution when pause_between_phases is false", async () => {
    const project = store.createProject({
      nousId: "nous-nopause",
      sessionId: "sess-nopause",
      goal: "no pause test",
      config: defaultConfig, // pause_between_phases: false
    });
    const phaseA = store.createPhase({
      projectId: project.id,
      name: "A",
      goal: "g",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    const phaseB = store.createPhase({
      projectId: project.id,
      name: "B",
      goal: "g2",
      requirements: [],
      successCriteria: [],
      phaseOrder: 2,
    });
    store.updatePhasePlan(phaseB.id, { steps: [], dependencies: [phaseA.id], acceptanceCriteria: [] });

    let dispatchCallCount = 0;
    const mockDispatch = {
      execute: async () => {
        dispatchCallCount++;
        return JSON.stringify({
          results: [{ status: "success", result: "done", durationMs: 100 }],
        });
      },
    } as unknown as import("../organon/registry.js").ToolHandler;

    const orch = new ExecutionOrchestrator(db, mockDispatch);
    const toolContext = {} as import("../organon/registry.js").ToolContext;
    await orch.executePhase(project.id, toolContext);

    // pause_between_phases=false: both waves execute, 2 dispatches
    expect(dispatchCallCount).toBe(2);
    const records = store.listSpawnRecords(project.id);
    expect(records.filter((r) => r.status === "done")).toHaveLength(2);
  });
});

// --- reapZombies — cascade-skip dependents ---

describe("reapZombies — cascade-skip dependents", () => {
  it("creates skipped spawn records for direct dependents of zombie plans", async () => {
    const project = store.createProject({
      nousId: "nous-zombie",
      sessionId: "sess-zombie",
      goal: "zombie cascade test",
      config: defaultConfig,
    });
    const phaseA = store.createPhase({
      projectId: project.id,
      name: "Zombie",
      goal: "zombie plan",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    const phaseB = store.createPhase({
      projectId: project.id,
      name: "Dependent",
      goal: "dependent plan",
      requirements: [],
      successCriteria: [],
      phaseOrder: 2,
    });
    store.updatePhasePlan(phaseB.id, { steps: [], dependencies: [phaseA.id], acceptanceCriteria: [] });

    // Manually create a "running" spawn record that is older than 600s
    const zombieRecord = store.createSpawnRecord({
      projectId: project.id,
      phaseId: phaseA.id,
      waveNumber: 0,
    });
    const oldTimestamp = new Date(Date.now() - 700_000).toISOString(); // 700s ago > 600s threshold
    store.updateSpawnRecord(zombieRecord.id, {
      status: "running",
      startedAt: oldTimestamp,
    });

    const mockDispatch = {
      execute: async () =>
        JSON.stringify({ results: [{ status: "success", result: "done", durationMs: 100 }] }),
    } as unknown as import("../organon/registry.js").ToolHandler;

    const orch = new ExecutionOrchestrator(db, mockDispatch);
    const toolContext = {} as import("../organon/registry.js").ToolContext;

    // executePhase() calls reapZombies() at the top before the wave loop
    await orch.executePhase(project.id, toolContext);

    const records = store.listSpawnRecords(project.id);

    // phaseA record should be marked zombie
    const zombieRec = records.find((r) => r.phaseId === phaseA.id && r.status === "zombie");
    expect(zombieRec).toBeDefined();

    // phaseB (direct dependent) should have a skipped record created by reapZombies cascade
    const skippedRec = records.find((r) => r.phaseId === phaseB.id && r.status === "skipped");
    expect(skippedRec).toBeDefined();
  });

  it("does not create skipped records for non-dependents of zombie plans", async () => {
    const project = store.createProject({
      nousId: "nous-zombie2",
      sessionId: "sess-zombie2",
      goal: "zombie non-dep test",
      config: defaultConfig,
    });
    const phaseA = store.createPhase({
      projectId: project.id,
      name: "Zombie",
      goal: "zombie plan",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    // phaseB has NO dependency on phaseA
    const phaseB = store.createPhase({
      projectId: project.id,
      name: "Independent",
      goal: "independent plan",
      requirements: [],
      successCriteria: [],
      phaseOrder: 2,
    });

    const zombieRecord = store.createSpawnRecord({
      projectId: project.id,
      phaseId: phaseA.id,
      waveNumber: 0,
    });
    const oldTimestamp = new Date(Date.now() - 700_000).toISOString();
    store.updateSpawnRecord(zombieRecord.id, {
      status: "running",
      startedAt: oldTimestamp,
    });

    const mockDispatch = {
      execute: async () =>
        JSON.stringify({ results: [{ status: "success", result: "done", durationMs: 100 }] }),
    } as unknown as import("../organon/registry.js").ToolHandler;

    const orch = new ExecutionOrchestrator(db, mockDispatch);
    const toolContext = {} as import("../organon/registry.js").ToolContext;
    await orch.executePhase(project.id, toolContext);

    const records = store.listSpawnRecords(project.id);

    // phaseA is zombie
    expect(records.find((r) => r.phaseId === phaseA.id && r.status === "zombie")).toBeDefined();

    // phaseB is independent — should NOT be skipped due to zombie cascade
    const phaseBSkipped = records.find((r) => r.phaseId === phaseB.id && r.status === "skipped");
    expect(phaseBSkipped).toBeUndefined();
  });
});
