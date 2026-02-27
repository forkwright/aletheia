// orchestrator-context.test.ts — tests for orchestrator context assembly
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import Database from "better-sqlite3";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { assembleOrchestratorContext, discoverProjectDirs } from "./orchestrator-context.js";
import { PlanningStore } from "./store.js";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
  PLANNING_V27_MIGRATION,
  PLANNING_V28_MIGRATION,
  PLANNING_V29_MIGRATION,
  PLANNING_V31_MIGRATION,
} from "./schema.js";

const DEFAULT_CONFIG = {
  mode: "interactive" as const,
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  pause_between_phases: false,
};

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  db.exec(PLANNING_V26_MIGRATION);
  db.exec(PLANNING_V27_MIGRATION);
  db.exec(PLANNING_V28_MIGRATION);
  db.exec(PLANNING_V29_MIGRATION);
  db.exec(PLANNING_V31_MIGRATION);
  return db;
}

describe("assembleOrchestratorContext", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let workspaceRoot: string;

  beforeEach(() => {
    db = makeDb();
    store = new PlanningStore(db);
    workspaceRoot = mkdtempSync(join(tmpdir(), "dianoia-orch-ctx-"));
  });

  afterEach(() => {
    db.close();
    try { rmSync(workspaceRoot, { recursive: true, force: true }); } catch { /* best-effort */ }
  });

  it("returns empty context when no projects exist", () => {
    const result = assembleOrchestratorContext({ workspaceRoot, db });
    expect(result.context).toBe("");
    expect(result.projectCount).toBe(0);
    expect(result.activeProjectIds).toEqual([]);
  });

  it("includes active project with goal and state", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Build a widget system",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(project.id, "executing");

    const result = assembleOrchestratorContext({ workspaceRoot, db });
    expect(result.projectCount).toBe(1);
    expect(result.context).toContain("Build a widget system");
    expect(result.context).toContain("executing");
    expect(result.context).toContain(project.id);
    expect(result.activeProjectIds).toContain(project.id);
  });

  it("excludes completed/abandoned projects when activeOnly=true", () => {
    const active = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Active project",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(active.id, "executing");

    const completed = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Completed project",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(completed.id, "complete");

    const result = assembleOrchestratorContext({ workspaceRoot, db, activeOnly: true });
    expect(result.projectCount).toBe(1);
    expect(result.context).toContain("Active project");
    expect(result.context).not.toContain("Completed project");
  });

  it("includes all projects when activeOnly=false", () => {
    const active = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Active project",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(active.id, "executing");

    const completed = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Done project",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(completed.id, "complete");

    const result = assembleOrchestratorContext({ workspaceRoot, db, activeOnly: false });
    expect(result.projectCount).toBe(2);
    expect(result.context).toContain("Active project");
    expect(result.context).toContain("Done project");
  });

  it("includes phase overview", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Multi-phase project",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(project.id, "executing");

    store.createPhase({
      projectId: project.id,
      name: "Foundation",
      goal: "Set up project",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });

    store.createPhase({
      projectId: project.id,
      name: "Implementation",
      goal: "Build the thing",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });

    const result = assembleOrchestratorContext({ workspaceRoot, db });
    expect(result.context).toContain("Foundation");
    expect(result.context).toContain("Implementation");
    expect(result.context).toContain("Phases:");
  });

  it("marks executing phase as current", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Current phase test",
      config: DEFAULT_CONFIG,
    });
    store.updateProjectState(project.id, "executing");

    const phase1 = store.createPhase({
      projectId: project.id,
      name: "Done Phase",
      goal: "Already done",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });
    store.updatePhaseStatus(phase1.id, "complete");

    const phase2 = store.createPhase({
      projectId: project.id,
      name: "Active Phase",
      goal: "In progress",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    store.updatePhaseStatus(phase2.id, "executing");

    const result = assembleOrchestratorContext({ workspaceRoot, db });
    expect(result.context).toContain("Active Phase");
    expect(result.context).toContain("← current");
  });

  it("stays under token budget", () => {
    const result = assembleOrchestratorContext({ workspaceRoot, db, maxTokens: 4000 });
    expect(result.estimatedTokens).toBeLessThanOrEqual(4000);
  });
});

describe("discoverProjectDirs", () => {
  let workspaceRoot: string;

  beforeEach(() => {
    workspaceRoot = mkdtempSync(join(tmpdir(), "dianoia-discover-"));
  });

  afterEach(() => {
    try { rmSync(workspaceRoot, { recursive: true, force: true }); } catch { /* best-effort */ }
  });

  it("returns empty array when no .dianoia directory exists", () => {
    expect(discoverProjectDirs(workspaceRoot)).toEqual([]);
  });

  it("discovers project directories", () => {
    const { mkdirSync } = require("node:fs");
    mkdirSync(join(workspaceRoot, ".dianoia", "projects", "proj_abc123"), { recursive: true });
    mkdirSync(join(workspaceRoot, ".dianoia", "projects", "proj_def456"), { recursive: true });
    
    const dirs = discoverProjectDirs(workspaceRoot);
    expect(dirs).toHaveLength(2);
    expect(dirs).toContain("proj_abc123");
    expect(dirs).toContain("proj_def456");
  });

  it("ignores non-project directories", () => {
    const { mkdirSync } = require("node:fs");
    mkdirSync(join(workspaceRoot, ".dianoia", "projects", "proj_abc123"), { recursive: true });
    mkdirSync(join(workspaceRoot, ".dianoia", "projects", "not_a_project"), { recursive: true });
    
    const dirs = discoverProjectDirs(workspaceRoot);
    expect(dirs).toHaveLength(1);
    expect(dirs).toContain("proj_abc123");
  });
});
