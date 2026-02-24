import { describe, it, expect, beforeEach } from "vitest";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL } from "./schema.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
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
