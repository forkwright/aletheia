// PhaseExecutor unit tests — task ordering, dependency handling, result aggregation
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION, PLANNING_V28_MIGRATION, PLANNING_V29_MIGRATION } from "./schema.js";
import { TASK_V1_DDL } from "./task-schema.js";
import { PlanningStore } from "./store.js";
import { TaskStore } from "./task-store.js";
import { PhaseExecutor } from "./phase-executor.js";

let db: Database.Database;
let planningStore: PlanningStore;
let taskStore: TaskStore;

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
  d.exec(PLANNING_V28_MIGRATION);
  d.exec(PLANNING_V29_MIGRATION);
  d.exec(TASK_V1_DDL);
  return d;
}

beforeEach(() => {
  db = makeDb();
  planningStore = new PlanningStore(db);
  taskStore = new TaskStore(db);
});

afterEach(() => {
  db.close();
});

describe("PhaseExecutor", () => {
  it("returns empty results when no tasks exist for phase", async () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "test", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 3,
      enableGitCommits: false,
      enableReview: false,
    });

    const result = await executor.executePhase(
      project.id, phase.id,
      async () => { throw new Error("should not be called"); },
    );

    expect(result.taskResults).toHaveLength(0);
    expect(result.succeeded).toBe(0);
  });

  it("executes tasks in order and tracks results", async () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "Build auth", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "Auth endpoints", requirements: [], successCriteria: [], phaseOrder: 1 });

    // Create two independent tasks
    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "Create login endpoint",
      description: "POST /login",
      priority: "high",
    });
    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "Create logout endpoint",
      description: "POST /logout",
      priority: "medium",
    });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 0,
      enableGitCommits: false,
      enableReview: false,
    });

    let dispatchCount = 0;
    const result = await executor.executePhase(
      project.id, phase.id,
      async () => {
        dispatchCount++;
        // Sub-agent returns but no git changes (will fail truths verification)
        return '```json\n{"status":"success","summary":"Done","filesChanged":[],"buildPassed":true,"confidence":0.9}\n```';
      },
    );

    // Both tasks dispatched
    expect(dispatchCount).toBe(2);
    expect(result.taskResults).toHaveLength(2);
    // Both fail at truths level (no real git changes in test env)
    expect(result.failed).toBe(2);
  });

  it("skips tasks blocked by failed dependencies", async () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "test", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });

    const task1 = taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "First task",
      priority: "high",
    });
    // task2 depends on task1 (uses taskId, not UUID id)
    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "Second task (depends on first)",
      priority: "medium",
      blockedBy: [task1.taskId],
    });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 0,
      enableGitCommits: false,
      enableReview: false,
    });

    let dispatchCount = 0;
    const result = await executor.executePhase(
      project.id, phase.id,
      async () => {
        dispatchCount++;
        // No JSON response — parse fails
        return "I analyzed the code and here is my plan...";
      },
    );

    // First task dispatched and failed (no JSON)
    // Second task skipped due to failed dependency
    expect(dispatchCount).toBe(1);
    expect(result.failed).toBe(1);
    expect(result.skipped).toBe(1);
    expect(result.taskResults[1]!.error).toContain("failed dependency");
  });

  it("skips deploy/checkpoint tasks", async () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "test", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });

    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "Deploy to production",
      priority: "high",
    });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 0,
      enableGitCommits: false,
      enableReview: false,
    });

    let dispatchCount = 0;
    const result = await executor.executePhase(
      project.id, phase.id,
      async () => {
        dispatchCount++;
        return "done";
      },
    );

    // Deploy task skipped (checkpoint)
    expect(dispatchCount).toBe(0);
    expect(result.skipped).toBe(1);
    expect(result.taskResults[0]!.error).toContain("checkpoint");
  });

  it("reports hasTasksForPhase correctly", () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "test", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 0,
      enableGitCommits: false,
      enableReview: false,
    });

    expect(executor.hasTasksForPhase(project.id, phase.id)).toBe(false);

    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "A task",
      priority: "medium",
    });

    expect(executor.hasTasksForPhase(project.id, phase.id)).toBe(true);
  });

  it("handles dispatch errors without crashing", async () => {
    const project = planningStore.createProject({ nousId: "n", sessionId: "s", goal: "test", config: defaultConfig });
    const phase = planningStore.createPhase({ projectId: project.id, name: "P1", goal: "g", requirements: [], successCriteria: [], phaseOrder: 1 });

    taskStore.createTask({
      projectId: project.id,
      phaseId: phase.id,
      title: "Task that crashes",
      priority: "high",
    });

    const executor = new PhaseExecutor(db, {
      workspaceRoot: "/nonexistent",
      maxReviewRounds: 0,
      enableGitCommits: false,
      enableReview: false,
    });

    const result = await executor.executePhase(
      project.id, phase.id,
      async () => { throw new Error("Connection refused"); },
    );

    expect(result.failed).toBe(1);
    expect(result.taskResults[0]!.error).toContain("Connection refused");
  });
});
