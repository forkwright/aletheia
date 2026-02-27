/**
 * Planning routes test — ORCH-01 through ORCH-06, SYNC-01 through SYNC-04.
 * Tests CRUD endpoints, file sync, category workflow, batch ops, and SSE events.
 *
 * Uses in-memory SQLite + real Hono app (no HTTP, direct handler calls).
 */
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Hono } from "hono";
import { existsSync, mkdirSync, readFileSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
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
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { planningRoutes } from "./routes.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { eventBus } from "../koina/event-bus.js";

let db: Database.Database;
let store: PlanningStore;
let app: Hono;
let tmpDir: string;
let projectId: string;

const defaultConfig = {
  depth: "standard" as const,
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
};

function initDb(): Database.Database {
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
  return d;
}

function seedProject(): string {
  const project = store.createProject({
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Test project",
    config: defaultConfig,
  });
  return project.id;
}

beforeEach(() => {
  db = initDb();
  store = new PlanningStore(db);
  tmpDir = join(tmpdir(), `dianoia-routes-test-${Date.now()}`);
  mkdirSync(tmpDir, { recursive: true });

  // Create orchestrator with workspace root for file sync
  const orch = new DianoiaOrchestrator(db, {
    depth: "standard",
    parallelization: false,
    research: true,
    plan_check: true,
    verifier: true,
    mode: "interactive",
  });
  orch.setWorkspaceRoot(tmpDir);

  // Build Hono app from routes
  const routeApp = planningRoutes(
    {
      store: { getDb: () => db } as any,
      manager: { getActiveTurnDetails: () => [] } as any,
      planningOrchestrator: orch,
    } as any,
    {} as any,
  );
  app = new Hono();
  app.route("/", routeApp);

  projectId = seedProject();
});

afterEach(() => {
  db.close();
  try { rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ok */ }
});

// ─── Helpers ───────────────────────────────────────────────

async function req(method: string, path: string, body?: unknown) {
  const init: RequestInit = { method, headers: { "Content-Type": "application/json" } };
  if (body) init.body = JSON.stringify(body);
  return app.request(path, init);
}

async function json(res: Response) {
  return res.json();
}

// ─── ORCH-01: Requirement CRUD ─────────────────────────────

describe("ORCH-01: Requirement CRUD", () => {
  it("creates a requirement and syncs to file", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "User can log in",
      category: "AUTH",
      tier: "v1",
    });
    expect(res.status).toBe(201);
    const data = await json(res);
    expect(data.reqId).toMatch(/^AUTH-/);
    expect(data.tier).toBe("v1");

    // Verify file sync
    const reqFile = join(tmpDir, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
    expect(existsSync(reqFile)).toBe(true);
    const content = readFileSync(reqFile, "utf-8");
    expect(content).toContain("AUTH");
    expect(content).toContain("User can log in");
  });

  it("updates a requirement by reqId", async () => {
    // Create first
    await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "User can log in",
      category: "AUTH",
      tier: "v1",
    });

    const res = await req("PATCH", `/api/planning/projects/${projectId}/requirements/AUTH-01`, {
      tier: "v2",
      rationale: "Defer to v2",
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.tier).toBe("v2");
    expect(data.rationale).toBe("Defer to v2");
  });

  it("deletes a requirement and syncs file", async () => {
    const createRes = await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "User can log in",
      category: "AUTH",
      tier: "v1",
    });
    const created = await json(createRes);

    const res = await req("DELETE", `/api/planning/projects/${projectId}/requirements/${created.reqId}`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.success).toBe(true);

    // File should be updated (empty requirements)
    const reqFile = join(tmpDir, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
    if (existsSync(reqFile)) {
      const content = readFileSync(reqFile, "utf-8");
      expect(content).not.toContain("User can log in");
    }
  });

  it("returns 404 for non-existent requirement", async () => {
    const res = await req("PATCH", `/api/planning/projects/${projectId}/requirements/NOPE-01`, {
      tier: "v2",
    });
    expect(res.status).toBe(404);
  });

  it("auto-generates reqId with incrementing sequence", async () => {
    await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "First", category: "UI",
    });
    const res = await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "Second", category: "UI",
    });
    const data = await json(res);
    expect(data.reqId).toBe("UI-02");
  });
});

// ─── ORCH-02: Phase CRUD ───────────────────────────────────

describe("ORCH-02: Phase CRUD", () => {
  let phaseId: string;

  beforeEach(() => {
    const phase = store.createPhase({
      projectId,
      name: "Foundation",
      goal: "Build the base",
      requirements: ["AUTH-01"],
      successCriteria: ["Tests pass"],
      dependencies: [],
      phaseOrder: 0,
    });
    phaseId = phase.id;
  });

  it("updates phase name and goal with file sync", async () => {
    const res = await req("PATCH", `/api/planning/projects/${projectId}/phases/${phaseId}`, {
      name: "Core Foundation",
      goal: "Build the core base layer",
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.name).toBe("Core Foundation");
    expect(data.goal).toBe("Build the core base layer");

    // Verify ROADMAP.md updated
    const roadmapFile = join(tmpDir, ".dianoia", "projects", projectId, "ROADMAP.md");
    expect(existsSync(roadmapFile)).toBe(true);
    const content = readFileSync(roadmapFile, "utf-8");
    expect(content).toContain("Core Foundation");
  });

  it("deletes a phase with file sync", async () => {
    const res = await req("DELETE", `/api/planning/projects/${projectId}/phases/${phaseId}`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.success).toBe(true);

    // Roadmap should not contain the phase
    const roadmapFile = join(tmpDir, ".dianoia", "projects", projectId, "ROADMAP.md");
    if (existsSync(roadmapFile)) {
      const content = readFileSync(roadmapFile, "utf-8");
      expect(content).not.toContain("Foundation");
    }
  });

  it("reorders phases with file sync", async () => {
    // Create a second phase
    store.createPhase({
      projectId,
      name: "UI Layer",
      goal: "Build the UI",
      requirements: ["UI-01"],
      successCriteria: ["Components render"],
      dependencies: [],
      phaseOrder: 1,
    });

    const res = await req("POST", `/api/planning/projects/${projectId}/phases/${phaseId}/reorder`, {
      newOrder: 1,
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.phases.length).toBe(2);
  });

  it("returns 404 for non-existent phase", async () => {
    const res = await req("PATCH", `/api/planning/projects/${projectId}/phases/fake-phase-id`, {
      name: "Nope",
    });
    expect(res.status).toBe(404);
  });
});

// ─── ORCH-03: Category Workflow ────────────────────────────

describe("ORCH-03: Category Workflow", () => {
  const authCategory = {
    category: "AUTH",
    categoryName: "Authentication",
    tableStakes: [
      { name: "Login", description: "User can log in", isTableStakes: true, proposedTier: "v1" as const },
      { name: "Logout", description: "User can log out", isTableStakes: true, proposedTier: "v1" as const },
    ],
    differentiators: [
      { name: "SSO", description: "Single sign-on via OIDC", isTableStakes: false, proposedTier: "v2" as const },
    ],
  };

  it("presents a category and returns formatted text", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/categories/present`, authCategory);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.formatted).toContain("Authentication");
    expect(data.formatted).toContain("Login");
    expect(data.formatted).toContain("SSO");
    expect(data.tableStakesCount).toBe(2);
    expect(data.differentiatorCount).toBe(1);
  });

  it("persists category decisions as requirements", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/categories/persist`, {
      category: authCategory,
      decisions: [
        { name: "Login", tier: "v1" },
        { name: "Logout", tier: "v1" },
        { name: "SSO", tier: "v2", rationale: "Nice to have later" },
      ],
    });
    expect(res.status).toBe(201);
    const data = await json(res);
    expect(data.persisted).toBe(3);
    expect(data.totalRequirements).toBe(3);

    // Verify requirements exist in DB
    const reqs = store.listRequirements(projectId);
    expect(reqs.length).toBe(3);
    expect(reqs.find(r => r.reqId === "AUTH-01")?.tier).toBe("v1");
    expect(reqs.find(r => r.reqId === "AUTH-03")?.tier).toBe("v2");

    // Verify REQUIREMENTS.md
    const reqFile = join(tmpDir, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
    expect(existsSync(reqFile)).toBe(true);
    const content = readFileSync(reqFile, "utf-8");
    expect(content).toContain("AUTH-01");
    expect(content).toContain("AUTH-03");
  });

  it("adjusts category requirements by reqId", async () => {
    // First persist some requirements
    await req("POST", `/api/planning/projects/${projectId}/categories/persist`, {
      category: authCategory,
      decisions: [
        { name: "Login", tier: "v1" },
        { name: "Logout", tier: "v1" },
        { name: "SSO", tier: "v2" },
      ],
    });

    // Now adjust
    const res = await req("PATCH", `/api/planning/projects/${projectId}/categories/AUTH`, {
      adjustments: [
        { reqId: "AUTH-03", tier: "v1", rationale: "Actually need SSO for launch" },
      ],
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.updatedCount).toBe(1);
    expect(data.results[0].updated).toBe(true);

    // Verify in DB
    const reqs = store.listRequirements(projectId);
    expect(reqs.find(r => r.reqId === "AUTH-03")?.tier).toBe("v1");
  });

  it("rejects table-stakes out-of-scope without rationale", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/categories/persist`, {
      category: authCategory,
      decisions: [
        { name: "Login", tier: "out-of-scope" }, // table-stakes without rationale
      ],
    });
    expect(res.status).toBe(400);
    const data = await json(res);
    expect(data.error.toLowerCase()).toContain("table-stakes");
  });

  it("rejects adjustment for wrong category", async () => {
    await req("POST", `/api/planning/projects/${projectId}/categories/persist`, {
      category: authCategory,
      decisions: [{ name: "Login", tier: "v1" }],
    });

    const res = await req("PATCH", `/api/planning/projects/${projectId}/categories/UI`, {
      adjustments: [{ reqId: "AUTH-01", tier: "v2" }],
    });
    const data = await json(res);
    expect(data.results[0].updated).toBe(false);
    expect(data.results[0].error).toContain("belongs to AUTH");
  });
});

// ─── ORCH-04: Discussion Answer/Skip ───────────────────────

describe("ORCH-04: Discussion Answer/Skip", () => {
  let phaseId: string;
  let questionId: string;

  beforeEach(() => {
    const phase = store.createPhase({
      projectId,
      name: "Test Phase",
      goal: "Test",
      requirements: [],
      successCriteria: [],
      dependencies: [],
      phaseOrder: 0,
    });
    phaseId = phase.id;
    const q = store.createDiscussionQuestion({
      projectId,
      phaseId,
      question: "Should we use REST or GraphQL?",
      options: JSON.stringify([
        { label: "REST", rationale: "Simpler" },
        { label: "GraphQL", rationale: "Flexible" },
      ]),
      recommendation: "REST",
    });
    questionId = q.id;
  });

  it("answers a discussion question via dedicated endpoint", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/discuss/answer`, {
      questionId,
      decision: "REST",
      userNote: "Keep it simple",
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.success).toBe(true);

    // Verify in DB
    const q = store.getDiscussionQuestion(questionId);
    expect(q?.decision).toBe("REST");
  });

  it("skips a discussion question", async () => {
    const res = await req("POST", `/api/planning/projects/${projectId}/discuss/skip`, {
      questionId,
    });
    expect(res.status).toBe(200);

    const q = store.getDiscussionQuestion(questionId);
    expect(q?.decision).toContain("skipped");
  });

  it("lists discussion questions for a phase", async () => {
    const res = await req("GET", `/api/planning/projects/${projectId}/discuss?phaseId=${phaseId}`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.questions.length).toBe(1);
    expect(data.questions[0].question).toBe("Should we use REST or GraphQL?");
  });
});

// ─── ORCH-05: SSE Events ───────────────────────────────────

describe("ORCH-05: SSE Events on mutations", () => {
  it("emits requirement-changed on create", async () => {
    const events: unknown[] = [];
    const handler = (data: unknown) => events.push(data);
    eventBus.on("planning:requirement-changed", handler);

    await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "Test req", category: "TST",
    });

    eventBus.off("planning:requirement-changed", handler);
    expect(events.length).toBe(1);
    expect((events[0] as any).action).toBe("created");
  });

  it("emits phase-changed on update", async () => {
    const phase = store.createPhase({
      projectId, name: "P1", goal: "G1",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });

    const events: unknown[] = [];
    const handler = (data: unknown) => events.push(data);
    eventBus.on("planning:phase-changed", handler);

    await req("PATCH", `/api/planning/projects/${projectId}/phases/${phase.id}`, {
      name: "P1 Updated",
    });

    eventBus.off("planning:phase-changed", handler);
    expect(events.length).toBe(1);
    expect((events[0] as any).action).toBe("updated");
  });

  it("emits discussion-answered on answer", async () => {
    const phase = store.createPhase({
      projectId, name: "P1", goal: "G1",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });
    const q = store.createDiscussionQuestion({
      projectId, phaseId: phase.id,
      question: "Test?", options: "[]", recommendation: "Yes",
    });

    const events: unknown[] = [];
    const handler = (data: unknown) => events.push(data);
    eventBus.on("planning:discussion-answered", handler);

    await req("POST", `/api/planning/projects/${projectId}/discuss/answer`, {
      questionId: q.id, decision: "Yes",
    });

    eventBus.off("planning:discussion-answered", handler);
    expect(events.length).toBe(1);
    expect((events[0] as any).decision).toBe("Yes");
  });

  it("emits on category persist", async () => {
    const events: unknown[] = [];
    const handler = (data: unknown) => events.push(data);
    eventBus.on("planning:requirement-changed", handler);

    await req("POST", `/api/planning/projects/${projectId}/categories/persist`, {
      category: {
        category: "UI",
        categoryName: "User Interface",
        tableStakes: [{ name: "Buttons", description: "Clickable", isTableStakes: true, proposedTier: "v1" }],
        differentiators: [],
      },
      decisions: [{ name: "Buttons", tier: "v1" }],
    });

    eventBus.off("planning:requirement-changed", handler);
    expect(events.length).toBe(1);
    expect((events[0] as any).action).toBe("category-persisted");
  });
});

// ─── ORCH-06: Batch Operations ─────────────────────────────

describe("ORCH-06: Batch Operations", () => {
  it("applies multiple requirement updates in one call", async () => {
    store.createRequirement({ projectId, reqId: "A-01", description: "One", category: "A", tier: "v1", rationale: null });
    store.createRequirement({ projectId, reqId: "A-02", description: "Two", category: "A", tier: "v1", rationale: null });

    const r1 = store.getRequirementByReqId(projectId, "A-01")!;
    const r2 = store.getRequirementByReqId(projectId, "A-02")!;

    const res = await req("POST", `/api/planning/projects/${projectId}/batch`, {
      operations: [
        { type: "update-requirement", id: r1.id, data: { tier: "v2" } },
        { type: "update-requirement", id: r2.id, data: { tier: "out-of-scope" } },
      ],
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.successCount).toBe(2);
    expect(data.failureCount).toBe(0);

    // Verify DB
    expect(store.getRequirement(r1.id)?.tier).toBe("v2");
    expect(store.getRequirement(r2.id)?.tier).toBe("out-of-scope");

    // Verify file synced
    const reqFile = join(tmpDir, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
    expect(existsSync(reqFile)).toBe(true);
  });

  it("handles mixed success/failure in batch", async () => {
    store.createRequirement({ projectId, reqId: "B-01", description: "One", category: "B", tier: "v1", rationale: null });
    const r1 = store.getRequirementByReqId(projectId, "B-01")!;

    const res = await req("POST", `/api/planning/projects/${projectId}/batch`, {
      operations: [
        { type: "update-requirement", id: r1.id, data: { tier: "v2" } },
        { type: "delete-requirement", id: "nonexistent-id" },
      ],
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.successCount).toBe(1);
    expect(data.failureCount).toBe(1);
    expect(data.results[1].error).toBeDefined();
  });

  it("rejects batches over 50 operations", async () => {
    const ops = Array.from({ length: 51 }, (_, i) => ({
      type: "update-requirement" as const,
      id: `fake-${i}`,
      data: { tier: "v2" },
    }));
    const res = await req("POST", `/api/planning/projects/${projectId}/batch`, { operations: ops });
    expect(res.status).toBe(400);
  });

  it("batch deletes phases and syncs roadmap", async () => {
    const p1 = store.createPhase({
      projectId, name: "P1", goal: "G1",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });
    const p2 = store.createPhase({
      projectId, name: "P2", goal: "G2",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });

    const res = await req("POST", `/api/planning/projects/${projectId}/batch`, {
      operations: [
        { type: "delete-phase", id: p1.id },
        { type: "delete-phase", id: p2.id },
      ],
    });
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.successCount).toBe(2);

    // Verify phases gone
    expect(store.listPhases(projectId).length).toBe(0);
  });
});

// ─── SYNC: File Co-Primary Verification ───────────────────

describe("SYNC: File co-primary (ENG-01 compliance)", () => {
  it("every requirement mutation writes REQUIREMENTS.md", async () => {
    const reqFile = () => {
      const p = join(tmpDir, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
      return existsSync(p) ? readFileSync(p, "utf-8") : "";
    };

    // Create
    await req("POST", `/api/planning/projects/${projectId}/requirements`, {
      description: "Feature A", category: "FEAT",
    });
    expect(reqFile()).toContain("Feature A");

    // Update
    await req("PATCH", `/api/planning/projects/${projectId}/requirements/FEAT-01`, {
      description: "Feature A (updated)",
    });
    expect(reqFile()).toContain("Feature A (updated)");

    // Delete
    await req("DELETE", `/api/planning/projects/${projectId}/requirements/FEAT-01`);
    expect(reqFile()).not.toContain("Feature A");
  });

  it("every phase mutation writes ROADMAP.md", async () => {
    const roadmapFile = () => {
      const p = join(tmpDir, ".dianoia", "projects", projectId, "ROADMAP.md");
      return existsSync(p) ? readFileSync(p, "utf-8") : "";
    };

    const phase = store.createPhase({
      projectId, name: "Alpha", goal: "Build alpha",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });

    // Update
    await req("PATCH", `/api/planning/projects/${projectId}/phases/${phase.id}`, {
      name: "Alpha v2",
    });
    expect(roadmapFile()).toContain("Alpha v2");

    // Delete
    await req("DELETE", `/api/planning/projects/${projectId}/phases/${phase.id}`);
    expect(roadmapFile()).not.toContain("Alpha v2");
  });
});

// ─── Read Endpoints ────────────────────────────────────────

describe("Read endpoints", () => {
  it("GET /projects lists all projects", async () => {
    const res = await req("GET", "/api/planning/projects");
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.projects.length).toBe(1);
  });

  it("GET /projects/:id returns project detail", async () => {
    const res = await req("GET", `/api/planning/projects/${projectId}`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.id).toBe(projectId);
    expect(data.goal).toBe("Test project");
  });

  it("GET /projects/:id/requirements lists requirements", async () => {
    store.createRequirement({ projectId, reqId: "X-01", description: "Test", category: "X", tier: "v1", rationale: null });
    const res = await req("GET", `/api/planning/projects/${projectId}/requirements`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.requirements.length).toBe(1);
  });

  it("GET /projects/:id/phases lists phases", async () => {
    store.createPhase({
      projectId, name: "P1", goal: "G1",
      requirements: [], successCriteria: [], dependencies: [], phaseOrder: 0,
    });
    const res = await req("GET", `/api/planning/projects/${projectId}/phases`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.phases.length).toBe(1);
  });

  it("GET /projects/:id/timeline returns milestones", async () => {
    const res = await req("GET", `/api/planning/projects/${projectId}/timeline`);
    expect(res.status).toBe(200);
    const data = await json(res);
    expect(data.milestones.length).toBeGreaterThanOrEqual(2); // Research + Requirements at minimum
  });

  it("returns 404 for non-existent project", async () => {
    const res = await req("GET", "/api/planning/projects/fake-id");
    expect(res.status).toBe(404);
  });
});
