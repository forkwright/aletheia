import { describe, it, expect } from "vitest";
import Database from "better-sqlite3";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
} from "./schema.js";
import { PlanningStore } from "./store.js";
import { RequirementsOrchestrator } from "./requirements.js";
import type { CategoryProposal, ScopingDecision } from "./requirements.js";
import { transition } from "./machine.js";

function makeDb(): Database.Database {
  const db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
  db.exec(PLANNING_V24_MIGRATION);
  db.exec(PLANNING_V25_MIGRATION);
  return db;
}

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
  store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));
  store.updateProjectState(project.id, transition("questioning", "START_RESEARCH"));
  store.updateProjectState(project.id, transition("researching", "RESEARCH_COMPLETE"));
  return project.id;
}

const AUTH_CATEGORY: CategoryProposal = {
  category: "AUTH",
  categoryName: "Authentication",
  tableStakes: [
    {
      name: "Login with email/password",
      description: "authenticate with email and password",
      isTableStakes: true,
      proposedTier: "v1",
    },
    {
      name: "Password reset",
      description: "reset a forgotten password via email",
      isTableStakes: true,
      proposedTier: "v1",
    },
  ],
  differentiators: [
    {
      name: "SSO",
      description: "sign in via third-party identity providers",
      isTableStakes: false,
      proposedTier: "v2",
    },
  ],
};

describe("RequirementsOrchestrator.getSynthesis()", () => {
  it("returns synthesis content when dimension=synthesis row exists", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const store = new PlanningStore(db);

    store.createResearch({
      projectId,
      phase: "research",
      dimension: "synthesis",
      content: "This is the synthesis content",
      status: "complete",
    });

    const orch = new RequirementsOrchestrator(db);
    expect(orch.getSynthesis(projectId)).toBe("This is the synthesis content");
  });

  it("returns null when no research rows exist (skip path)", () => {
    const db = makeDb();
    const projectId = makeProject(db);

    const orch = new RequirementsOrchestrator(db);
    expect(orch.getSynthesis(projectId)).toBeNull();
  });
});

describe("RequirementsOrchestrator.persistCategory()", () => {
  it("creates requirements with correct REQ-IDs starting at 01", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const decisions: ScopingDecision[] = [
      { name: "Login with email/password", tier: "v1" },
      { name: "Password reset", tier: "v1" },
      { name: "SSO", tier: "v2" },
    ];

    orch.persistCategory(projectId, AUTH_CATEGORY, decisions);

    const store = new PlanningStore(db);
    const reqs = store.listRequirements(projectId);

    expect(reqs).toHaveLength(3);
    expect(reqs[0]!.reqId).toBe("AUTH-01");
    expect(reqs[1]!.reqId).toBe("AUTH-02");
    expect(reqs[2]!.reqId).toBe("AUTH-03");
  });

  it("continues numbering from MAX when category already has rows (re-presentation safe)", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const firstBatch: ScopingDecision[] = [
      { name: "Login with email/password", tier: "v1" },
      { name: "Password reset", tier: "v1" },
    ];
    orch.persistCategory(projectId, AUTH_CATEGORY, firstBatch);

    const secondBatch: ScopingDecision[] = [
      { name: "SSO", tier: "v2" },
    ];
    orch.persistCategory(projectId, AUTH_CATEGORY, secondBatch);

    const store = new PlanningStore(db);
    const reqs = store.listRequirements(projectId);

    expect(reqs).toHaveLength(3);
    expect(reqs[2]!.reqId).toBe("AUTH-03");
  });

  it("sets rationale to null for v1/v2 requirements, non-null for out-of-scope", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const decisions: ScopingDecision[] = [
      { name: "Login with email/password", tier: "v1" },
      { name: "Password reset", tier: "v2" },
      { name: "SSO", tier: "out-of-scope", rationale: "Not needed for MVP" },
    ];

    orch.persistCategory(projectId, AUTH_CATEGORY, decisions);

    const store = new PlanningStore(db);
    const reqs = store.listRequirements(projectId);

    expect(reqs[0]!.rationale).toBeNull();
    expect(reqs[1]!.rationale).toBeNull();
    expect(reqs[2]!.rationale).toBe("Not needed for MVP");
  });
});

describe("RequirementsOrchestrator.validateCoverage()", () => {
  it("returns false when no v1 requirement exists", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const decisions: ScopingDecision[] = [
      { name: "SSO", tier: "v2" },
    ];
    orch.persistCategory(projectId, AUTH_CATEGORY, decisions);

    expect(orch.validateCoverage(projectId, ["AUTH"])).toBe(false);
  });

  it("returns false when a presented category has no requirements", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const decisions: ScopingDecision[] = [
      { name: "Login with email/password", tier: "v1" },
    ];
    orch.persistCategory(projectId, AUTH_CATEGORY, decisions);

    expect(orch.validateCoverage(projectId, ["AUTH", "STOR"])).toBe(false);
  });

  it("returns true when 1+ v1 exists and all presented categories are covered", () => {
    const db = makeDb();
    const projectId = makeProject(db);
    const orch = new RequirementsOrchestrator(db);

    const decisions: ScopingDecision[] = [
      { name: "Login with email/password", tier: "v1" },
      { name: "SSO", tier: "v2" },
    ];
    orch.persistCategory(projectId, AUTH_CATEGORY, decisions);

    expect(orch.validateCoverage(projectId, ["AUTH"])).toBe(true);
  });
});
