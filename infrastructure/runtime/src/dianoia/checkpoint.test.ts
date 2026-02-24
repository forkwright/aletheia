// CheckpointSystem unit tests — 5-branch evaluate() logic, store persistence, eventBus emission
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
import type { PlanningStore } from "./store.js";
import { CheckpointSystem } from "./checkpoint.js";
import { eventBus } from "../koina/event-bus.js";
import type { PlanningConfig } from "./types.js";

let db: Database.Database;

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

const INTERACTIVE_CONFIG: PlanningConfig = {
  depth: "standard",
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
  pause_between_phases: false,
};

const YOLO_CONFIG: PlanningConfig = {
  depth: "standard",
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "yolo",
  pause_between_phases: false,
};

function makeStore(
  mockCreateCheckpoint: ReturnType<typeof vi.fn>,
  mockResolveCheckpoint: ReturnType<typeof vi.fn>,
): PlanningStore {
  return {
    createCheckpoint: mockCreateCheckpoint,
    resolveCheckpoint: mockResolveCheckpoint,
  } as unknown as PlanningStore;
}

const baseOpts = {
  projectId: "proj-test",
  type: "schema-migration",
  question: "Should we run this migration?",
  context: { table: "users" } as Record<string, unknown>,
  nousId: "nous-test",
  sessionId: "sess-test",
};

beforeEach(() => {
  db = makeDb();
  vi.restoreAllMocks();
});

afterEach(() => {
  db.close();
});

describe("CheckpointSystem.evaluate — low risk", () => {
  it("creates and resolves checkpoint with autoApproved:true, emits event, returns approved", async () => {
    const mockCreate = vi.fn().mockReturnValue({ id: "ckpt-low-1" });
    const mockResolve = vi.fn();
    const store = makeStore(mockCreate, mockResolve);
    const emitSpy = vi.spyOn(eventBus, "emit");

    const system = new CheckpointSystem(store, INTERACTIVE_CONFIG);
    const result = await system.evaluate({ ...baseOpts, riskLevel: "low" });

    expect(result).toBe("approved");
    expect(mockCreate).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledWith("ckpt-low-1", "approved", { autoApproved: true });
    expect(emitSpy).toHaveBeenCalledOnce();
    expect(emitSpy).toHaveBeenCalledWith(
      "planning:checkpoint",
      expect.objectContaining({ decision: "approved", autoApproved: true }),
    );
  });
});

describe("CheckpointSystem.evaluate — medium risk", () => {
  it("creates and resolves checkpoint with notified, emits event, returns approved (non-blocking)", async () => {
    const mockCreate = vi.fn().mockReturnValue({ id: "ckpt-med-1" });
    const mockResolve = vi.fn();
    const store = makeStore(mockCreate, mockResolve);
    const emitSpy = vi.spyOn(eventBus, "emit");

    const system = new CheckpointSystem(store, INTERACTIVE_CONFIG);
    const result = await system.evaluate({ ...baseOpts, riskLevel: "medium" });

    expect(result).toBe("approved");
    expect(mockCreate).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledWith("ckpt-med-1", "notified", { autoApproved: false });
    expect(emitSpy).toHaveBeenCalledOnce();
    expect(emitSpy).toHaveBeenCalledWith(
      "planning:checkpoint",
      expect.objectContaining({ decision: "notified", autoApproved: false }),
    );
  });
});

describe("CheckpointSystem.evaluate — high risk in YOLO mode", () => {
  it("creates and resolves checkpoint with autoApproved:true, emits event, returns approved", async () => {
    const mockCreate = vi.fn().mockReturnValue({ id: "ckpt-high-yolo-1" });
    const mockResolve = vi.fn();
    const store = makeStore(mockCreate, mockResolve);
    const emitSpy = vi.spyOn(eventBus, "emit");

    const system = new CheckpointSystem(store, YOLO_CONFIG);
    const result = await system.evaluate({ ...baseOpts, riskLevel: "high" });

    expect(result).toBe("approved");
    expect(mockCreate).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledOnce();
    expect(mockResolve).toHaveBeenCalledWith("ckpt-high-yolo-1", "approved", { autoApproved: true });
    expect(emitSpy).toHaveBeenCalledOnce();
    expect(emitSpy).toHaveBeenCalledWith(
      "planning:checkpoint",
      expect.objectContaining({ decision: "approved", autoApproved: true }),
    );
  });
});

describe("CheckpointSystem.evaluate — high risk in interactive mode", () => {
  it("creates checkpoint but does NOT resolve or emit event, returns blocked", async () => {
    const mockCreate = vi.fn().mockReturnValue({ id: "ckpt-high-int-1" });
    const mockResolve = vi.fn();
    const store = makeStore(mockCreate, mockResolve);
    const emitSpy = vi.spyOn(eventBus, "emit");

    const system = new CheckpointSystem(store, INTERACTIVE_CONFIG);
    const result = await system.evaluate({ ...baseOpts, riskLevel: "high" });

    expect(result).toBe("blocked");
    expect(mockCreate).toHaveBeenCalledOnce();
    expect(mockResolve).not.toHaveBeenCalled();
    expect(emitSpy).not.toHaveBeenCalled();
  });
});

describe("CheckpointSystem.evaluate — true blocker (bypasses YOLO mode)", () => {
  it("creates checkpoint but does NOT resolve or emit event, returns blocked even in YOLO mode", async () => {
    const mockCreate = vi.fn().mockReturnValue({ id: "ckpt-trueblocker-1" });
    const mockResolve = vi.fn();
    const store = makeStore(mockCreate, mockResolve);
    const emitSpy = vi.spyOn(eventBus, "emit");

    const system = new CheckpointSystem(store, YOLO_CONFIG);
    const result = await system.evaluate({
      ...baseOpts,
      riskLevel: "high",
      trueBlockerCategory: "irreversible-data-deletion",
    });

    expect(result).toBe("blocked");
    expect(mockCreate).toHaveBeenCalledOnce();
    expect(mockResolve).not.toHaveBeenCalled();
    expect(emitSpy).not.toHaveBeenCalled();
  });
});
