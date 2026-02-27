// Tests for RetrospectiveGenerator (Spec 32 Phase 4)
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
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
import { RetrospectiveGenerator } from "./retrospective.js";

let db: Database.Database;
let store: PlanningStore;
let retro: RetrospectiveGenerator;
let workspaceRoot: string;

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
  retro = new RetrospectiveGenerator(db);
  workspaceRoot = join(tmpdir(), `dianoia-retro-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
});

afterEach(() => {
  db.close();
  if (existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true, force: true });
  }
});

describe("RetrospectiveGenerator.generate", () => {
  it("generates retrospective for a completed project", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Build auth system",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "complete");

    const phase = store.createPhase({
      projectId: project.id,
      name: "Auth",
      goal: "Implement OAuth",
      requirements: ["AUTH-01"],
      successCriteria: ["Users can login"],
      phaseOrder: 0,
    });
    store.updatePhaseStatus(phase.id, "complete");
    store.updatePhaseVerificationResult(phase.id, {
      status: "met",
      summary: "All good",
      gaps: [],
      verifiedAt: new Date().toISOString(),
    });

    const result = retro.generate(project.id);

    expect(result.projectId).toBe(project.id);
    expect(result.goal).toBe("Build auth system");
    expect(result.outcome).toBe("complete");
    expect(result.phases).toHaveLength(1);
    expect(result.phases[0]!.name).toBe("Auth");
    expect(result.phases[0]!.status).toBe("complete");
    expect(result.phases[0]!.verificationStatus).toBe("met");
    expect(result.patterns.length).toBeGreaterThan(0);
    expect(result.patterns.some((p) => p.type === "success")).toBe(true);
  });

  it("marks abandoned projects correctly", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Abandoned project",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "abandoned");

    const result = retro.generate(project.id);
    expect(result.outcome).toBe("abandoned");
  });

  it("detects failure patterns", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Failing project",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "blocked");

    const phase = store.createPhase({
      projectId: project.id,
      name: "Data Layer",
      goal: "Build data",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });
    store.updatePhaseStatus(phase.id, "failed");

    const result = retro.generate(project.id);
    expect(result.outcome).toBe("partial");
    expect(result.patterns.some((p) => p.type === "failure")).toBe(true);
  });

  it("detects cascade skip antipattern", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Cascade project",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "complete");

    const p1 = store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "First",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });
    store.updatePhaseStatus(p1.id, "failed");

    const p2 = store.createPhase({
      projectId: project.id,
      name: "Phase 2",
      goal: "Second",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    store.updatePhaseStatus(p2.id, "skipped");

    const result = retro.generate(project.id);
    expect(result.patterns.some((p) => p.type === "antipattern" && p.summary.includes("Cascade"))).toBe(true);
  });

  it("includes discussion counts per phase", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Discussed project",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "complete");

    const phase = store.createPhase({
      projectId: project.id,
      name: "Auth",
      goal: "Implement OAuth",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });
    store.updatePhaseStatus(phase.id, "complete");

    // Add discussions
    store.createDiscussionQuestion({
      projectId: project.id,
      phaseId: phase.id,
      question: "Q1?",
      options: [],
    });
    store.createDiscussionQuestion({
      projectId: project.id,
      phaseId: phase.id,
      question: "Q2?",
      options: [],
    });

    const result = retro.generate(project.id);
    expect(result.phases[0]!.discussionCount).toBe(2);
  });
});

describe("RetrospectiveGenerator file output", () => {
  it("writes RETRO.md and retro.json", () => {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "File output test",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "complete");

    store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "Goal 1",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });

    const result = retro.generate(project.id);
    const projectDirValue = join(workspaceRoot, ".dianoia", "projects", project.id);
    retro.writeRetroFile(projectDirValue, result);
    retro.writeRetroJson(projectDirValue, result);

    const retroMdPath = join(workspaceRoot, ".dianoia", "projects", project.id, "RETRO.md");
    const retroJsonPath = join(workspaceRoot, ".dianoia", "projects", project.id, "retro.json");

    expect(existsSync(retroMdPath)).toBe(true);
    expect(existsSync(retroJsonPath)).toBe(true);

    const md = readFileSync(retroMdPath, "utf-8");
    expect(md).toContain("Retrospective:");
    expect(md).toContain("File output test");

    const json = JSON.parse(readFileSync(retroJsonPath, "utf-8"));
    expect(json.projectId).toBe(project.id);
    expect(json.outcome).toBe("complete");
  });
});
