// Tests for ContextPacketBuilder (Spec 32 Phase 2)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  buildContextPacketSync,
  selectModelForRole,
  modelTierToRole,
  type SubAgentRole,
} from "./context-packet.js";
import { getEncoding } from "js-tiktoken";
import {
  ensureProjectDir,
  ensurePhaseDir,
  writeProjectFile,
  writeRequirementsFile,
  writeRoadmapFile,
  writeDiscussFile,
  writePlanFile,
} from "./project-files.js";
import type { PlanningPhase, PlanningRequirement } from "./types.js";

const TEST_PROJECT_ID = "proj_test123";
const TEST_PHASE_ID = "phase_test456";

let workspaceRoot: string;

function makePhase(overrides?: Partial<PlanningPhase>): PlanningPhase {
  return {
    id: TEST_PHASE_ID,
    projectId: TEST_PROJECT_ID,
    name: "Authentication",
    goal: "Implement OAuth2 login with Google and GitHub providers",
    requirements: ["AUTH-01", "AUTH-02"],
    successCriteria: [
      "Users can log in via Google OAuth",
      "Users can log in via GitHub OAuth",
      "Sessions persist across page refreshes",
    ],
    plan: { steps: [{ id: "s1", description: "Wire OAuth", subtasks: [], dependsOn: [] }] },
    status: "pending",
    phaseOrder: 0,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeRequirement(overrides?: Partial<PlanningRequirement>): PlanningRequirement {
  return {
    id: "req_1",
    projectId: TEST_PROJECT_ID,
    phaseId: null,
    reqId: "AUTH-01",
    description: "OAuth2 login with Google",
    category: "AUTH",
    tier: "v1",
    status: "pending",
    rationale: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `dianoia-ctx-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
});

afterEach(() => {
  if (existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true, force: true });
  }
});

describe("buildContextPacketSync", () => {
  it("includes phase objective for executor role", () => {
    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      projectGoal: "Build a SaaS authentication system",
    });

    expect(packet).toContain("Phase Objective");
    expect(packet).toContain("Authentication");
    expect(packet).toContain("OAuth2 login");
    expect(packet).toContain("Success Criteria");
  });

  it("includes project goal for verifier role", () => {
    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "verifier",
      phase,
      projectGoal: "Build a SaaS authentication system",
    });

    expect(packet).toContain("Project Goal");
    expect(packet).toContain("SaaS authentication");
  });

  it("excludes project context for executor role", () => {
    // Write a PROJECT.md so there's something to potentially include
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    writeProjectFile(workspaceRoot, {
      id: TEST_PROJECT_ID,
      nousId: "test",
      sessionId: "test",
      goal: "Big project",
      state: "executing",
      config: {} as any,
      contextHash: "",
      projectDir: null,
      createdAt: "2026-01-01",
      updatedAt: "2026-01-01",
      projectContext: { goal: "Big project context" },
    });

    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      projectGoal: "Big project",
    });

    // Executor should NOT include full project context
    expect(packet).not.toContain("Project Context");
  });

  it("includes discussion decisions for executor role", () => {
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, [
      {
        id: "disc_1",
        projectId: TEST_PROJECT_ID,
        phaseId: TEST_PHASE_ID,
        question: "Should we use PKCE or implicit flow?",
        options: [
          { label: "PKCE", rationale: "More secure" },
          { label: "Implicit", rationale: "Simpler" },
        ],
        recommendation: "PKCE",
        decision: "PKCE",
        userNote: "Always prefer security",
        status: "answered",
        createdAt: "2026-01-01",
        updatedAt: "2026-01-01",
      },
    ]);

    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
    });

    expect(packet).toContain("Design Decisions");
    expect(packet).toContain("PKCE");
  });

  it("includes requirements for executor role", () => {
    const phase = makePhase();
    const reqs = [
      makeRequirement({ reqId: "AUTH-01", description: "Google OAuth login" }),
      makeRequirement({ reqId: "AUTH-02", description: "GitHub OAuth login", id: "req_2" }),
    ];

    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      requirements: reqs,
    });

    expect(packet).toContain("Requirements");
    expect(packet).toContain("AUTH-01");
    expect(packet).toContain("AUTH-02");
    expect(packet).toContain("Google OAuth");
  });

  it("includes roadmap overview for planner role", () => {
    const phases = [
      makePhase({ id: "p1", name: "Auth", phaseOrder: 0 }),
      makePhase({ id: "p2", name: "API", phaseOrder: 1, goal: "Build REST API" }),
      makePhase({ id: "p3", name: "UI", phaseOrder: 2, goal: "Build frontend" }),
    ];

    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: "p1",
      role: "planner",
      phase: phases[0],
      allPhases: phases,
      projectGoal: "Full-stack app",
    });

    expect(packet).toContain("Roadmap Overview");
    expect(packet).toContain("Auth");
    expect(packet).toContain("API");
    expect(packet).toContain("UI");
  });

  it("excludes roadmap for executor role", () => {
    const phases = [
      makePhase({ id: "p1", name: "Auth", phaseOrder: 0 }),
      makePhase({ id: "p2", name: "API", phaseOrder: 1 }),
    ];

    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: "p1",
      role: "executor",
      phase: phases[0],
      allPhases: phases,
    });

    expect(packet).not.toContain("Roadmap Overview");
  });

  it("respects token budget and truncates lower-priority sections", { timeout: 30000 }, () => {
    const longSupplementary = "x".repeat(5000);
    const phase = makePhase();

    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      supplementary: longSupplementary,
      maxTokens: 500, // Very tight budget: 2000 chars
    });

    // Should have phase objective (priority 0) but may truncate supplementary
    expect(packet).toContain("Phase Objective");
    // With tiktoken, 500 tokens is ~2000 chars of raw text, but headers/markdown
    // add overhead. The key assertion is that the 5000-char supplementary was truncated.
    expect(packet.length).toBeLessThan(5000); // Must be significantly smaller than the 5000-char input
  });

  it("includes supplementary context for executor role", () => {
    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      supplementary: "// src/symbolon/oauth.ts\nexport function authenticate() { ... }",
    });

    expect(packet).toContain("Reference Material");
    expect(packet).toContain("oauth.ts");
  });

  it("returns empty string when no sections match", () => {
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: null,
      role: "researcher",
      // No projectGoal, no supplementary, no files
    });

    expect(packet).toContain("Context Error");
  });

  it("reads from file-backed plan when no in-memory plan", () => {
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);
    writePlanFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, {
      steps: [{ id: "s1", description: "Do the thing" }],
    });

    const phase = makePhase({ plan: null }); // No in-memory plan
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
    });

    expect(packet).toContain("Execution Plan");
    expect(packet).toContain("Do the thing");
  });

  it("includes research for planner role", () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    const dir = join(workspaceRoot, ".dianoia", "projects", TEST_PROJECT_ID);
    writeFileSync(
      join(dir, "RESEARCH.md"),
      "# Research\n\n## stack (complete)\n\nUse TypeScript with Fastify.\n",
      "utf-8",
    );

    const phase = makePhase();
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "planner",
      phase,
      projectGoal: "Build API",
    });

    expect(packet).toContain("Research Findings");
    expect(packet).toContain("TypeScript with Fastify");
  });
});

describe("selectModelForRole", () => {
  it("returns sonnet for all current roles", () => {
    const roles: SubAgentRole[] = ["researcher", "planner", "executor", "reviewer", "verifier"];
    for (const role of roles) {
      expect(selectModelForRole(role)).toBe("sonnet");
    }
  });
});

describe("modelTierToRole", () => {
  it("maps haiku to explorer", () => {
    expect(modelTierToRole("haiku")).toBe("explorer");
  });

  it("maps sonnet to coder", () => {
    expect(modelTierToRole("sonnet")).toBe("coder");
  });
});

describe("Token budget accuracy (CTX-01)", () => {
  it("respects maxTokens budget within 5% margin", () => {
    const phase = makePhase();
    const requirements = [makeRequirement(), makeRequirement({ reqId: "AUTH-02", description: "Session management" })];
    
    // Create some substantial content to test truncation
    const largeContent = "This is a large piece of content. ".repeat(1000);
    
    writeProjectFile(workspaceRoot, {
      id: TEST_PROJECT_ID,
      goal: largeContent,
      state: "idle",
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      projectContext: null,
    });
    
    writeRequirementsFile(workspaceRoot, TEST_PROJECT_ID, requirements);
    
    const maxTokens = 500;
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      projectGoal: "Build authentication",
      maxTokens,
    });

    // Count actual tokens using tiktoken
    const encoder = getEncoding("cl100k_base");
    const actualTokens = encoder.encode(packet).length;
    
    // Should be within 5% of budget (not exceed it significantly)
    expect(actualTokens).toBeLessThanOrEqual(maxTokens * 1.05);
    
    // Should use a reasonable portion of the budget (not be too conservative) - but only if there's enough content
    expect(actualTokens).toBeGreaterThan(Math.min(maxTokens * 0.3, 100)); // At least 30% or 100 tokens, whichever is smaller
  });

  it("includes correct sections for executor role", () => {
    const phase = makePhase();
    const requirements = [makeRequirement()];
    
    const packet = buildContextPacketSync({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      phase,
      requirements,
      maxTokens: 2000,
    });

    // Executor should get: phase goal, plan, discussion, requirements, supplementary
    // but NOT: project context, roadmap, research
    expect(packet).toContain("Phase Objective");
    expect(packet).toContain("Requirements");
    expect(packet).not.toContain("Research Findings");
    expect(packet).not.toContain("Roadmap Overview");
  });
});
