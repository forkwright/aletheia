// End-to-end integration test for Context & State Foundation phase (CTX-01 through CTX-04)
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdirSync, rmSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { ResearchOrchestrator } from "./researcher.js";
import { RequirementsOrchestrator } from "./requirements.js";
import { buildContextPacketSync } from "./context-packet.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { CategoryProposal, ScopingDecision } from "./requirements.js";

let workspaceRoot: string;
let db: Database.Database;

const TOOL_CONTEXT: ToolContext = {
  nousId: "test-nous",
  sessionId: "test-session",
  workspace: "/tmp",
};

const DEFAULT_CONFIG = {
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
};

const MOCK_RESEARCH_RESULTS = {
  results: [
    {
      index: 0,
      status: "success",
      result: `Technology Stack Research: \`\`\`json\n{"summary":"Node.js with TypeScript recommended","details":"Use Node.js 20+ with TypeScript 5.x, Express.js for API, React for frontend","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 1,
      status: "success", 
      result: `Feature Analysis: \`\`\`json\n{"summary":"Authentication and data management are table-stakes","details":"OAuth2, CRUD operations, real-time updates are must-haves","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 2,
      status: "success",
      result: `Architecture Patterns: \`\`\`json\n{"summary":"REST API with microservices","details":"Use REST for external APIs, GraphQL for internal, event-driven architecture","confidence":"medium"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 3,
      status: "success",
      result: `Common Pitfalls: \`\`\`json\n{"summary":"Authentication security and performance bottlenecks","details":"Implement proper JWT validation, watch for N+1 queries, use connection pooling","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
  ],
};

const MOCK_SYNTHESIS_RESULT = {
  results: [
    {
      index: 0,
      status: "success",
      result: `# Research Summary

## Stack
Node.js 20+ with TypeScript 5.x ecosystem provides the best balance of performance and developer experience.

## Features  
Authentication (OAuth2), CRUD operations, and real-time updates are table-stakes for modern applications.

## Architecture
REST API with selective microservices and event-driven patterns for scalability.

## Pitfalls
Focus on JWT security, query optimization, and connection management from the start.

## Recommendations
Start with monolith using Node.js/TypeScript, plan for microservices extraction points.`,
      durationMs: 2000,
    },
  ],
};

function setupMockDispatch(): ToolHandler {
  let callCount = 0;
  return {
    definition: { name: "mock_dispatch", description: "", input_schema: {} },
    execute: vi.fn().mockImplementation(() => {
      callCount++;
      if (callCount === 1) {
        // First call: research dimensions
        return Promise.resolve(JSON.stringify(MOCK_RESEARCH_RESULTS));
      } else {
        // Second call: synthesis
        return Promise.resolve(JSON.stringify(MOCK_SYNTHESIS_RESULT));
      }
    }),
  };
}

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `dianoia-e2e-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
  
  db = new Database(":memory:");
  db.exec(PLANNING_V20_DDL);
  db.exec(PLANNING_V21_MIGRATION);
  db.exec(PLANNING_V22_MIGRATION);
  db.exec(PLANNING_V23_MIGRATION);
});

afterEach(() => {
  if (existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true });
  }
  db.close();
  vi.restoreAllMocks();
});

describe("Context & State Foundation E2E", () => {
  it("completes full workflow: project creation → research → requirements → file validation", async () => {
    const store = new PlanningStore(db);
    const orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    orchestrator.setWorkspaceRoot(workspaceRoot);
    
    const mockDispatch = setupMockDispatch();
    const researchOrch = new ResearchOrchestrator(db, mockDispatch, workspaceRoot);
    const requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);

    // Step 1: Create project and advance to research state
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session",
      goal: "Build a task management SaaS application",
      config: DEFAULT_CONFIG,
    });

    store.updateProjectState(project.id, { from: "idle", to: "questioning", event: "START_QUESTIONING" });
    store.updateProjectState(project.id, { from: "questioning", to: "researching", event: "START_RESEARCH" });

    // Step 2: Run research phase (CTX-04)
    const researchResult = await researchOrch.runResearch(
      project.id,
      project.goal!,
      TOOL_CONTEXT,
    );

    expect(researchResult.stored).toBe(4); // All dimensions successful
    expect(researchResult.partial).toBe(0);
    expect(researchResult.failed).toBe(0);

    // Transition to requirements
    researchOrch.transitionToRequirements(project.id);

    // Verify RESEARCH.md was written (CTX-02)
    const researchFile = join(workspaceRoot, ".dianoia", "projects", project.id, "RESEARCH.md");
    expect(existsSync(researchFile)).toBe(true);
    const researchContent = readFileSync(researchFile, "utf-8");
    expect(researchContent).toContain("stack");
    expect(researchContent).toContain("features");
    expect(researchContent).toContain("synthesis");

    // Step 3: Requirements scoping with coverage gate (CTX-03)
    
    // Present and persist first category
    const authCategory: CategoryProposal = {
      category: "AUTH",
      categoryName: "Authentication",
      tableStakes: [
        {
          name: "Email/password login",
          description: "Basic authentication",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [
        {
          name: "SSO integration",
          description: "Third-party authentication",
          isTableStakes: false,
          proposedTier: "v2",
        },
      ],
    };

    const authDecisions: ScopingDecision[] = [
      { name: "Email/password login", tier: "v1" },
      { name: "SSO integration", tier: "v2" },
    ];

    requirementsOrch.persistCategory(project.id, authCategory, authDecisions);

    // Verify REQUIREMENTS.md written incrementally (CTX-03)
    const requirementsFile = join(workspaceRoot, ".dianoia", "projects", project.id, "REQUIREMENTS.md");
    expect(existsSync(requirementsFile)).toBe(true);
    let requirementsContent = readFileSync(requirementsFile, "utf-8");
    expect(requirementsContent).toContain("AUTH-01");

    // Present and persist second category to meet minimum count
    const dataCategory: CategoryProposal = {
      category: "DATA",
      categoryName: "Data Management",
      tableStakes: [
        {
          name: "CRUD operations",
          description: "Basic data operations",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [
        {
          name: "Real-time sync",
          description: "Live data updates",
          isTableStakes: false,
          proposedTier: "v2",
        },
      ],
    };

    const dataDecisions: ScopingDecision[] = [
      { name: "CRUD operations", tier: "v1" },
      { name: "Real-time sync", tier: "out-of-scope", rationale: "Not needed for MVP" },
    ];

    requirementsOrch.persistCategory(project.id, dataCategory, dataDecisions);

    // Verify coverage gate passes with 2 categories
    const coverageValid = requirementsOrch.validateCoverage(project.id, ["AUTH", "DATA"]);
    expect(coverageValid).toBe(true);

    // Verify REQUIREMENTS.md updated again
    requirementsContent = readFileSync(requirementsFile, "utf-8");
    expect(requirementsContent).toContain("AUTH-01");
    expect(requirementsContent).toContain("DATA-01");

    // Step 4: Context packet assembly with token budgets (CTX-01)
    const contextPacket = buildContextPacketSync({
      workspaceRoot,
      projectId: project.id,
      phaseId: null,
      role: "planner",
      maxTokens: 2000,
      projectGoal: project.goal!,
    });

    // Verify context packet includes expected sections for planner role
    expect(contextPacket).toContain("Project Goal");
    expect(contextPacket).toContain("Requirements");
    expect(contextPacket).toContain("Research Findings");
    expect(contextPacket).not.toContain("Execution Plan"); // Not included for planner

    // Verify token budget respected (rough check)
    expect(contextPacket.length).toBeLessThan(10000); // Should be much less than char equivalent of 2000 tokens

    // Step 5: Verify all critical files exist (CTX-02)
    const projectDir = join(workspaceRoot, ".dianoia", "projects", project.id);
    expect(existsSync(join(projectDir, "PROJECT.md"))).toBe(false); // Not written in this flow
    expect(existsSync(join(projectDir, "REQUIREMENTS.md"))).toBe(true);
    expect(existsSync(join(projectDir, "RESEARCH.md"))).toBe(true);
    // ROADMAP.md would be written in next phase
  });

  it("fails fast when research has zero successful dimensions (CTX-04)", async () => {
    const failDispatch: ToolHandler = {
      definition: { name: "fail_dispatch", description: "", input_schema: {} },
      execute: vi.fn().mockResolvedValue(
        JSON.stringify({
          results: [
            { index: 0, status: "error", error: "Network timeout", durationMs: 5000 },
            { index: 1, status: "error", error: "API limit exceeded", durationMs: 5000 },
            { index: 2, status: "error", error: "Service unavailable", durationMs: 5000 },
            { index: 3, status: "error", error: "Auth failed", durationMs: 5000 },
          ],
        }),
      ),
    };

    const researchOrch = new ResearchOrchestrator(db, failDispatch, workspaceRoot);

    await expect(
      researchOrch.runResearch("proj_123", "Test project", TOOL_CONTEXT),
    ).rejects.toThrow("Research failed: No dimensions completed successfully");
  });

  it("enforces coverage gate with minimum category count (CTX-03)", () => {
    const requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);
    const store = new PlanningStore(db);

    const project = store.createProject({
      nousId: "test-nous", 
      sessionId: "test-session",
      goal: "Test",
      config: DEFAULT_CONFIG,
    });

    // Single category should fail default minimum (2)
    const singleCategory: CategoryProposal = {
      category: "ONLY",
      categoryName: "Only Category",
      tableStakes: [
        {
          name: "Single feature",
          description: "Only feature",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [],
    };

    requirementsOrch.persistCategory(project.id, singleCategory, [
      { name: "Single feature", tier: "v1" },
    ]);

    // Should fail coverage gate due to minimum category count
    expect(requirementsOrch.validateCoverage(project.id, ["ONLY"])).toBe(false);

    // Should pass with minimum set to 1
    expect(requirementsOrch.validateCoverage(project.id, ["ONLY"], 1)).toBe(true);
  });
});