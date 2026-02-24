// Tests for plan_discuss tool (Spec 32 Phase 3)
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { createPlanDiscussTool } from "./discuss-tool.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

let db: Database.Database;
let store: PlanningStore;
let orchestrator: DianoiaOrchestrator;
let tool: ToolHandler;

const defaultConfig: PlanningConfigSchema = {
  depth: "standard",
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
  pause_between_phases: false,
};

const toolContext = { nousId: "test-nous", sessionId: "test-session" } as ToolContext;

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
  return d;
}

function createTestProject(): { projectId: string; phaseId: string } {
  const project = store.createProject({
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Build auth system",
    config: defaultConfig,
  });
  // Advance to discussing state
  store.updateProjectState(project.id, "discussing");

  const phase = store.createPhase({
    projectId: project.id,
    name: "Authentication",
    goal: "Implement OAuth2",
    requirements: ["AUTH-01", "AUTH-02"],
    successCriteria: ["Users can login via Google", "Sessions persist"],
    phaseOrder: 0,
  });

  return { projectId: project.id, phaseId: phase.id };
}

beforeEach(() => {
  db = makeDb();
  store = new PlanningStore(db);
  orchestrator = new DianoiaOrchestrator(db, defaultConfig);
  tool = createPlanDiscussTool(orchestrator, db);
});

afterEach(() => {
  db.close();
});

describe("plan_discuss — generate", () => {
  it("generates questions for a phase", async () => {
    const { projectId, phaseId } = createTestProject();

    const result = JSON.parse(
      await tool.execute({ action: "generate", projectId, phaseId }, toolContext),
    );

    expect(result.generated).toBeGreaterThan(0);
    expect(result.questions).toBeDefined();
    expect(result.questions.length).toBeGreaterThan(0);
    expect(result.questions[0]).toHaveProperty("id");
    expect(result.questions[0]).toHaveProperty("question");
    expect(result.questions[0]).toHaveProperty("options");
  });

  it("returns error without phaseId", async () => {
    const { projectId } = createTestProject();

    const result = JSON.parse(
      await tool.execute({ action: "generate", projectId }, toolContext),
    );

    expect(result.error).toContain("phaseId required");
  });
});

describe("plan_discuss — add", () => {
  it("adds a custom question", async () => {
    const { projectId, phaseId } = createTestProject();

    const result = JSON.parse(
      await tool.execute({
        action: "add",
        projectId,
        phaseId,
        question: "Should we use JWT or session cookies?",
        options: [
          { label: "JWT", rationale: "Stateless, works across services" },
          { label: "Sessions", rationale: "Server-controlled, revocable" },
        ],
        recommendation: "JWT",
      }, toolContext),
    );

    expect(result.questionId).toBeDefined();
    expect(result.question).toBe("Should we use JWT or session cookies?");
    expect(result.options).toHaveLength(2);
    expect(result.recommendation).toBe("JWT");
  });
});

describe("plan_discuss — answer", () => {
  it("answers a question with decision and note", async () => {
    const { projectId, phaseId } = createTestProject();

    // Add a question first
    const addResult = JSON.parse(
      await tool.execute({
        action: "add",
        projectId,
        phaseId,
        question: "Test question?",
        options: [{ label: "A", rationale: "Option A" }],
      }, toolContext),
    );

    const result = JSON.parse(
      await tool.execute({
        action: "answer",
        projectId,
        questionId: addResult.questionId,
        decision: "A",
        userNote: "Because of X",
      }, toolContext),
    );

    expect(result.answered).toBe(true);
    expect(result.decision).toBe("A");
  });

  it("returns error without questionId", async () => {
    const { projectId } = createTestProject();

    const result = JSON.parse(
      await tool.execute({ action: "answer", projectId, decision: "A" }, toolContext),
    );

    expect(result.error).toContain("questionId required");
  });
});

describe("plan_discuss — skip", () => {
  it("skips a question", async () => {
    const { projectId, phaseId } = createTestProject();

    const addResult = JSON.parse(
      await tool.execute({
        action: "add",
        projectId,
        phaseId,
        question: "Skippable question?",
        options: [],
      }, toolContext),
    );

    const result = JSON.parse(
      await tool.execute({
        action: "skip",
        projectId,
        questionId: addResult.questionId,
      }, toolContext),
    );

    expect(result.skipped).toBe(true);
  });
});

describe("plan_discuss — list", () => {
  it("lists questions with status breakdown", async () => {
    const { projectId, phaseId } = createTestProject();

    // Generate some questions
    await tool.execute({ action: "generate", projectId, phaseId }, toolContext);

    const result = JSON.parse(
      await tool.execute({ action: "list", projectId, phaseId }, toolContext),
    );

    expect(result.total).toBeGreaterThan(0);
    expect(result.pending).toBe(result.total); // All pending initially
    expect(result.answered).toBe(0);
    expect(result.skipped).toBe(0);
    expect(result.questions).toBeDefined();
  });
});

describe("plan_discuss — complete", () => {
  it("completes discussion when all questions resolved", async () => {
    const { projectId, phaseId } = createTestProject();

    // Add and answer a question
    const addResult = JSON.parse(
      await tool.execute({
        action: "add",
        projectId,
        phaseId,
        question: "Approach?",
        options: [{ label: "A", rationale: "Simple" }],
      }, toolContext),
    );

    await tool.execute({
      action: "answer",
      projectId,
      questionId: addResult.questionId,
      decision: "A",
    }, toolContext);

    const result = JSON.parse(
      await tool.execute({ action: "complete", projectId, phaseId }, toolContext),
    );

    expect(result.complete).toBe(true);
    expect(result.nextState).toBe("phase-planning");

    // Verify state advanced
    const project = store.getProjectOrThrow(projectId);
    expect(project.state).toBe("phase-planning");
  });

  it("blocks completion when pending questions exist", async () => {
    const { projectId, phaseId } = createTestProject();

    // Add a question but don't answer it
    await tool.execute({
      action: "add",
      projectId,
      phaseId,
      question: "Unanswered?",
      options: [],
    }, toolContext);

    const result = JSON.parse(
      await tool.execute({ action: "complete", projectId, phaseId }, toolContext),
    );

    expect(result.error).toContain("Unresolved questions");
    expect(result.pendingCount).toBe(1);
  });

  it("allows completion with no questions (fast-track)", async () => {
    const { projectId, phaseId } = createTestProject();

    // No questions added — should complete immediately
    const result = JSON.parse(
      await tool.execute({ action: "complete", projectId, phaseId }, toolContext),
    );

    expect(result.complete).toBe(true);
  });

  it("idempotent: completes discussion when project already in phase-planning", async () => {
    // Simulate Phase 1 discussion already completed — project is in phase-planning
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Multi-phase project",
      config: defaultConfig,
    });
    store.updateProjectState(project.id, "phase-planning");

    const phase2 = store.createPhase({
      projectId: project.id,
      name: "Phase 2",
      goal: "Second phase",
      requirements: ["CTX-01"],
      successCriteria: ["Works"],
      phaseOrder: 1,
    });

    // Phase 2 discussion completes while project is already in phase-planning
    const result = JSON.parse(
      await tool.execute({ action: "complete", projectId: project.id, phaseId: phase2.id }, toolContext),
    );

    expect(result.complete).toBe(true);
    // Project should remain in phase-planning (idempotent)
    const updated = store.getProjectOrThrow(project.id);
    expect(updated.state).toBe("phase-planning");
  });
});
