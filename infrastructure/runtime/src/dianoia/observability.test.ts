/**
 * Observability tests — OBS-03 (decision audit trail), OBS-05 (turn tracking),
 * INTERJ-04/OBS-02 (spawn record visibility).
 * Tests store methods + HTTP route endpoints.
 */
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Hono } from "hono";
import { mkdirSync, rmSync } from "fs";
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
  PLANNING_V29_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { planningRoutes } from "./routes.js";
import { DianoiaOrchestrator } from "./orchestrator.js";

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
  d.exec(PLANNING_V29_MIGRATION);
  return d;
}

function seedProject(): string {
  const project = store.createProject({
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Observability test project",
    config: defaultConfig,
  });
  return project.id;
}

beforeEach(() => {
  db = initDb();
  store = new PlanningStore(db);
  tmpDir = join(tmpdir(), `dianoia-obs-test-${Date.now()}`);
  mkdirSync(tmpDir, { recursive: true });

  const orch = new DianoiaOrchestrator(db, {
    depth: "standard",
    parallelization: false,
    research: true,
    plan_check: true,
    verifier: true,
    mode: "interactive",
  });
  orch.setWorkspaceRoot(tmpDir);

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
  try { rmSync(tmpDir, { recursive: true, force: true }); } catch {}
});

// ================================================================
// OBS-03: Decision Audit Trail — Store
// ================================================================

describe("Decision audit trail (store)", () => {
  it("logs a decision and retrieves it by id", () => {
    const decision = store.logDecision({
      projectId,
      source: "user",
      type: "scope",
      summary: "Include auth module in v1",
      rationale: "Security is table stakes",
    });

    expect(decision.id).toBeTruthy();
    expect(decision.source).toBe("user");
    expect(decision.type).toBe("scope");
    expect(decision.summary).toBe("Include auth module in v1");
    expect(decision.rationale).toBe("Security is table stakes");
    expect(decision.createdAt).toBeTruthy();

    const retrieved = store.getDecision(decision.id);
    expect(retrieved).toEqual(decision);
  });

  it("logs decisions with different sources", () => {
    const sources = ["user", "agent", "checkpoint", "system"] as const;
    for (const source of sources) {
      store.logDecision({
        projectId,
        source,
        type: "config",
        summary: `Decision from ${source}`,
      });
    }

    const all = store.listDecisions(projectId);
    expect(all).toHaveLength(4);
    expect(new Set(all.map((d) => d.source))).toEqual(new Set(sources));
  });

  it("filters decisions by phase", () => {
    store.logDecision({
      projectId,
      phaseId: "phase-1",
      source: "agent",
      type: "architecture",
      summary: "Use repository pattern",
    });
    store.logDecision({
      projectId,
      phaseId: "phase-2",
      source: "agent",
      type: "architecture",
      summary: "Use event sourcing",
    });
    store.logDecision({
      projectId,
      source: "system",
      type: "transition",
      summary: "Project state changed",
    });

    const phase1 = store.listDecisions(projectId, "phase-1");
    expect(phase1).toHaveLength(1);
    expect(phase1[0].summary).toBe("Use repository pattern");

    const all = store.listDecisions(projectId);
    expect(all).toHaveLength(3);
  });

  it("stores and retrieves context JSON", () => {
    const context = { requirement: "AUTH-01", tier: "v1", previousTier: "v2" };
    const decision = store.logDecision({
      projectId,
      source: "user",
      type: "scope-change",
      summary: "Promoted AUTH-01 to v1",
      context,
    });

    const retrieved = store.getDecision(decision.id);
    expect(retrieved?.context).toEqual(context);
  });

  it("cascades on project delete", () => {
    store.logDecision({
      projectId,
      source: "user",
      type: "test",
      summary: "Will be deleted",
    });
    expect(store.listDecisions(projectId)).toHaveLength(1);

    store.deleteProject(projectId);
    expect(store.listDecisions(projectId)).toHaveLength(0);
  });
});

// ================================================================
// OBS-05: Turn Tracking — Store
// ================================================================

describe("Turn tracking (store)", () => {
  it("records and increments turn counts", () => {
    store.recordTurn(projectId, "phase-1", "syn", 1500);
    store.recordTurn(projectId, "phase-1", "syn", 2000);

    const counts = store.getTurnCounts(projectId, "phase-1");
    expect(counts).toHaveLength(1);
    expect(counts[0].turnCount).toBe(2);
    expect(counts[0].tokenCount).toBe(3500);
    expect(counts[0].nousId).toBe("syn");
  });

  it("tracks multiple agents independently", () => {
    store.recordTurn(projectId, "phase-1", "syn", 1000);
    store.recordTurn(projectId, "phase-1", "syn", 1000);
    store.recordTurn(projectId, "phase-1", "coder-1", 500);

    const counts = store.getTurnCounts(projectId, "phase-1");
    expect(counts).toHaveLength(2);

    const syn = counts.find((c) => c.nousId === "syn");
    const coder = counts.find((c) => c.nousId === "coder-1");
    expect(syn?.turnCount).toBe(2);
    expect(syn?.tokenCount).toBe(2000);
    expect(coder?.turnCount).toBe(1);
    expect(coder?.tokenCount).toBe(500);
  });

  it("tracks multiple phases independently", () => {
    store.recordTurn(projectId, "phase-1", "syn", 1000);
    store.recordTurn(projectId, "phase-2", "syn", 2000);

    const p1 = store.getTurnCounts(projectId, "phase-1");
    const p2 = store.getTurnCounts(projectId, "phase-2");
    expect(p1).toHaveLength(1);
    expect(p2).toHaveLength(1);
    expect(p1[0].tokenCount).toBe(1000);
    expect(p2[0].tokenCount).toBe(2000);

    // All phases
    const all = store.getTurnCounts(projectId);
    expect(all).toHaveLength(2);
  });

  it("returns project totals", () => {
    store.recordTurn(projectId, "phase-1", "syn", 1000);
    store.recordTurn(projectId, "phase-1", "coder-1", 500);
    store.recordTurn(projectId, "phase-2", "syn", 2000);

    const totals = store.getProjectTurnTotal(projectId);
    expect(totals.turns).toBe(3);
    expect(totals.tokens).toBe(3500);
  });

  it("returns zeros for empty project", () => {
    const totals = store.getProjectTurnTotal(projectId);
    expect(totals.turns).toBe(0);
    expect(totals.tokens).toBe(0);
  });

  it("defaults token count to 0", () => {
    store.recordTurn(projectId, "phase-1", "syn");

    const counts = store.getTurnCounts(projectId, "phase-1");
    expect(counts[0].turnCount).toBe(1);
    expect(counts[0].tokenCount).toBe(0);
  });

  it("cascades on project delete", () => {
    store.recordTurn(projectId, "phase-1", "syn", 1000);
    expect(store.getTurnCounts(projectId)).toHaveLength(1);

    store.deleteProject(projectId);
    expect(store.getTurnCounts(projectId)).toHaveLength(0);
  });
});

// ================================================================
// OBS-03: Decision Audit Trail — Routes
// ================================================================

describe("GET /api/planning/projects/:id/decisions", () => {
  it("returns empty list for project with no decisions", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/decisions`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.decisions).toEqual([]);
    expect(body.count).toBe(0);
    expect(body.projectId).toBe(projectId);
  });

  it("returns all decisions for project", async () => {
    store.logDecision({ projectId, source: "user", type: "scope", summary: "Include auth" });
    store.logDecision({ projectId, source: "agent", type: "design", summary: "Use REST" });

    const res = await app.request(`/api/planning/projects/${projectId}/decisions`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.decisions).toHaveLength(2);
    expect(body.count).toBe(2);
  });

  it("filters by phaseId query param", async () => {
    store.logDecision({ projectId, phaseId: "p1", source: "user", type: "scope", summary: "A" });
    store.logDecision({ projectId, phaseId: "p2", source: "user", type: "scope", summary: "B" });
    store.logDecision({ projectId, source: "system", type: "transition", summary: "C" });

    const res = await app.request(`/api/planning/projects/${projectId}/decisions?phaseId=p1`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.decisions).toHaveLength(1);
    expect(body.decisions[0].summary).toBe("A");
  });
});

// ================================================================
// OBS-05: Turn Tracking — Routes
// ================================================================

describe("GET /api/planning/projects/:id/usage", () => {
  it("returns zeros for project with no turns", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/usage`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.turnCounts).toEqual([]);
    expect(body.totals.turns).toBe(0);
    expect(body.totals.tokens).toBe(0);
    expect(body.projectId).toBe(projectId);
  });

  it("returns turn counts and totals", async () => {
    store.recordTurn(projectId, "p1", "syn", 1000);
    store.recordTurn(projectId, "p1", "coder-1", 500);
    store.recordTurn(projectId, "p2", "syn", 2000);

    const res = await app.request(`/api/planning/projects/${projectId}/usage`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.turnCounts).toHaveLength(3);
    expect(body.totals.turns).toBe(3);
    expect(body.totals.tokens).toBe(3500);
  });

  it("filters by phaseId query param", async () => {
    store.recordTurn(projectId, "p1", "syn", 1000);
    store.recordTurn(projectId, "p2", "syn", 2000);

    const res = await app.request(`/api/planning/projects/${projectId}/usage?phaseId=p1`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.turnCounts).toHaveLength(1);
    expect(body.turnCounts[0].phaseId).toBe("p1");
  });
});

// ================================================================
// INTERJ-04 / OBS-02: Spawn Records — Routes
// ================================================================

describe("GET /api/planning/projects/:id/spawns", () => {
  let phaseId: string;

  beforeEach(() => {
    const phase = store.createPhase({
      projectId,
      name: "Test Phase",
      goal: "Test goal",
      requirements: ["REQ-01"],
      successCriteria: ["Tests pass"],
      phaseOrder: 0,
    });
    phaseId = phase.id;
  });

  it("returns empty when no spawns exist", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/spawns`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.spawns).toEqual([]);
    expect(body.summary.total).toBe(0);
    expect(body.summary.running).toBe(0);
  });

  it("returns spawn records with summary counts", async () => {
    const s1 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:coder:abc", waveNumber: 1 });
    const s2 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:reviewer:def", waveNumber: 1 });

    // Update one to running
    store.updateSpawnRecord(s1.id, { status: "running" });

    const res = await app.request(`/api/planning/projects/${projectId}/spawns`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.spawns).toHaveLength(2);
    expect(body.summary.total).toBe(2);
    expect(body.summary.running).toBe(1);
    expect(body.summary.pending).toBe(1);
  });

  it("filters by phaseId query param", async () => {
    const phase2 = store.createPhase({
      projectId,
      name: "Phase 2",
      goal: "Goal 2",
      requirements: ["REQ-02"],
      successCriteria: ["Tests pass"],
      phaseOrder: 1,
    });

    store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:coder:p1", waveNumber: 1 });
    store.createSpawnRecord({ projectId, phaseId: phase2.id, agentSessionId: "spawn:coder:p2", waveNumber: 1 });

    const res = await app.request(`/api/planning/projects/${projectId}/spawns?phaseId=${phaseId}`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.spawns).toHaveLength(1);
    expect(body.spawns[0].phaseId).toBe(phaseId);
  });

  it("filters by status query param", async () => {
    const s1 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:coder:a", waveNumber: 1 });
    store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:coder:b", waveNumber: 1 });
    store.updateSpawnRecord(s1.id, { status: "complete" });

    const res = await app.request(`/api/planning/projects/${projectId}/spawns?status=complete`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.spawns).toHaveLength(1);
    expect(body.summary.total).toBe(1);
    expect(body.summary.complete).toBe(1);
  });

  it("counts all status types correctly", async () => {
    const s1 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:a:1", waveNumber: 1 });
    const s2 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:b:2", waveNumber: 1 });
    const s3 = store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:c:3", waveNumber: 2 });
    store.createSpawnRecord({ projectId, phaseId, agentSessionId: "spawn:d:4", waveNumber: 2 });

    store.updateSpawnRecord(s1.id, { status: "running" });
    store.updateSpawnRecord(s2.id, { status: "complete" });
    store.updateSpawnRecord(s3.id, { status: "failed" });

    const res = await app.request(`/api/planning/projects/${projectId}/spawns`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.summary).toEqual({
      total: 4,
      running: 1,
      complete: 1,
      failed: 1,
      pending: 1,
    });
  });
});

// ================================================================
// INTERJ-01: Message Injection Queue — Store
// ================================================================

describe("Message queue (store)", () => {
  it("enqueues and retrieves a message", () => {
    const msg = store.enqueueMessage({
      projectId,
      source: "user",
      content: "Please review the auth implementation",
    });

    expect(msg.id).toBeTruthy();
    expect(msg.status).toBe("pending");
    expect(msg.source).toBe("user");
    expect(msg.priority).toBe("normal");
    expect(msg.content).toBe("Please review the auth implementation");
    expect(msg.createdAt).toBeTruthy();

    const retrieved = store.getMessage(msg.id);
    expect(retrieved).toEqual(msg);
  });

  it("drains pending messages in priority order", () => {
    store.enqueueMessage({ projectId, source: "user", content: "Low priority", priority: "low" });
    store.enqueueMessage({ projectId, source: "agent", content: "Critical alert", priority: "critical" });
    store.enqueueMessage({ projectId, source: "user", content: "Normal message" });
    store.enqueueMessage({ projectId, source: "sub-agent", content: "High priority", priority: "high" });

    const drained = store.drainMessages(projectId);
    expect(drained).toHaveLength(4);
    // Priority order: critical, high, normal, low
    expect(drained[0].priority).toBe("critical");
    expect(drained[1].priority).toBe("high");
    expect(drained[2].priority).toBe("normal");
    expect(drained[3].priority).toBe("low");

    // All should now be delivered
    for (const msg of drained) {
      const updated = store.getMessage(msg.id);
      expect(updated?.status).toBe("delivered");
      expect(updated?.deliveredAt).toBeTruthy();
    }

    // Draining again returns empty
    const secondDrain = store.drainMessages(projectId);
    expect(secondDrain).toHaveLength(0);
  });

  it("filters drain by phaseId (includes null-phase messages)", () => {
    store.enqueueMessage({ projectId, source: "user", content: "General msg" });
    store.enqueueMessage({ projectId, source: "user", content: "Phase 1 msg", phaseId: "phase-1" });
    store.enqueueMessage({ projectId, source: "user", content: "Phase 2 msg", phaseId: "phase-2" });

    const drained = store.drainMessages(projectId, "phase-1");
    expect(drained).toHaveLength(2); // General (null phase) + phase-1
    expect(drained.map(m => m.content).sort()).toEqual(["General msg", "Phase 1 msg"]);
  });

  it("expires messages past their expiresAt", () => {
    // Create a message that expired 1 second ago
    const pastTime = new Date(Date.now() - 1000).toISOString();
    store.enqueueMessage({
      projectId,
      source: "user",
      content: "This is expired",
      expiresAt: pastTime,
    });
    store.enqueueMessage({
      projectId,
      source: "user",
      content: "This is not expired",
    });

    const drained = store.drainMessages(projectId);
    expect(drained).toHaveLength(1);
    expect(drained[0].content).toBe("This is not expired");

    // Expired message should be marked as expired
    const all = store.listMessages(projectId);
    const expired = all.find(m => m.content === "This is expired");
    expect(expired?.status).toBe("expired");
  });

  it("counts pending messages", () => {
    expect(store.countPendingMessages(projectId)).toBe(0);

    store.enqueueMessage({ projectId, source: "user", content: "Msg 1" });
    store.enqueueMessage({ projectId, source: "user", content: "Msg 2" });
    expect(store.countPendingMessages(projectId)).toBe(2);

    store.drainMessages(projectId);
    expect(store.countPendingMessages(projectId)).toBe(0);
  });

  it("lists messages with status filter", () => {
    store.enqueueMessage({ projectId, source: "user", content: "Pending" });
    store.enqueueMessage({ projectId, source: "user", content: "Will be delivered" });
    store.drainMessages(projectId); // Delivers both

    store.enqueueMessage({ projectId, source: "user", content: "New pending" });

    const pending = store.listMessages(projectId, { status: "pending" });
    expect(pending).toHaveLength(1);
    expect(pending[0].content).toBe("New pending");

    const delivered = store.listMessages(projectId, { status: "delivered" });
    expect(delivered).toHaveLength(2);
  });

  it("stores sub-agent source with session ID", () => {
    const msg = store.enqueueMessage({
      projectId,
      source: "sub-agent",
      sourceSessionId: "spawn:coder:abc123",
      content: "Found a dependency conflict",
      priority: "high",
    });

    expect(msg.source).toBe("sub-agent");
    expect(msg.sourceSessionId).toBe("spawn:coder:abc123");
  });

  it("cascades on project delete", () => {
    store.enqueueMessage({ projectId, source: "user", content: "Will be deleted" });
    expect(store.listMessages(projectId)).toHaveLength(1);

    store.deleteProject(projectId);
    expect(store.listMessages(projectId)).toHaveLength(0);
  });
});

// ================================================================
// INTERJ-01: Message Injection — Routes
// ================================================================

describe("POST /api/planning/projects/:id/messages", () => {
  it("enqueues a message via API", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "Please check the tests" }),
    });

    expect(res.status).toBe(201);
    const body = await res.json();
    expect(body.message.content).toBe("Please check the tests");
    expect(body.message.source).toBe("user");
    expect(body.message.priority).toBe("normal");
    expect(body.message.status).toBe("pending");
  });

  it("accepts priority and source params", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        content: "Critical issue found",
        priority: "critical",
        source: "sub-agent",
        sourceSessionId: "spawn:coder:xyz",
      }),
    });

    expect(res.status).toBe(201);
    const body = await res.json();
    expect(body.message.priority).toBe("critical");
    expect(body.message.source).toBe("sub-agent");
    expect(body.message.sourceSessionId).toBe("spawn:coder:xyz");
  });

  it("rejects empty content", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "" }),
    });

    expect(res.status).toBe(400);
  });

  it("rejects invalid source", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "Test", source: "alien" }),
    });

    expect(res.status).toBe(400);
  });

  it("rejects invalid priority", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "Test", priority: "ultra" }),
    });

    expect(res.status).toBe(400);
  });

  it("supports expiresInSeconds", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "Temporary message", expiresInSeconds: 60 }),
    });

    expect(res.status).toBe(201);
    const body = await res.json();
    expect(body.message.expiresAt).toBeTruthy();
    // expiresAt should be roughly 60 seconds from now
    const expiresAt = new Date(body.message.expiresAt).getTime();
    const now = Date.now();
    expect(expiresAt - now).toBeGreaterThan(50000);
    expect(expiresAt - now).toBeLessThan(70000);
  });
});

describe("GET /api/planning/projects/:id/messages", () => {
  it("returns empty list for no messages", async () => {
    const res = await app.request(`/api/planning/projects/${projectId}/messages`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.messages).toEqual([]);
    expect(body.pendingCount).toBe(0);
  });

  it("returns all messages with pending count", async () => {
    store.enqueueMessage({ projectId, source: "user", content: "Msg 1" });
    store.enqueueMessage({ projectId, source: "user", content: "Msg 2" });

    const res = await app.request(`/api/planning/projects/${projectId}/messages`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.messages).toHaveLength(2);
    expect(body.pendingCount).toBe(2);
  });

  it("filters by status query param", async () => {
    store.enqueueMessage({ projectId, source: "user", content: "Pending" });
    store.enqueueMessage({ projectId, source: "user", content: "Will deliver" });
    store.drainMessages(projectId);
    store.enqueueMessage({ projectId, source: "user", content: "New pending" });

    const res = await app.request(`/api/planning/projects/${projectId}/messages?status=pending`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.messages).toHaveLength(1);
    expect(body.messages[0].content).toBe("New pending");
  });
});
