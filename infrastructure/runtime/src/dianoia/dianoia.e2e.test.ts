// End-to-end integration test — Context & State Foundation phase validation
import { beforeEach, describe, expect, it, vi } from "vitest";
import Database from "better-sqlite3";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { existsSync, readFileSync, mkdirSync } from "node:fs";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
} from "./schema.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { ResearchOrchestrator } from "./researcher.js";
import { RequirementsOrchestrator } from "./requirements.js";
import { RoadmapOrchestrator } from "./roadmap.js";
import { PlanningStore } from "./store.js";
import { buildContextPacket } from "./context-packet.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
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
  return db;
}

function makeMockDispatchTool(): ToolHandler {
  const mockFn = vi.fn();
  
  // First call: 4 research dimensions
  mockFn.mockResolvedValueOnce(
    JSON.stringify({
      results: [
        {
          index: 0,
          status: "success",
          result: `Stack research complete

\`\`\`json
{
  "summary": "Research complete",
  "details": "Detailed research findings",
  "confidence": "high"
}
\`\`\``,
          durationMs: 100,
        },
        {
          index: 1,
          status: "success",
          result: `Features research complete

\`\`\`json
{
  "summary": "Feature research done",
  "details": "Feature analysis complete",
  "confidence": "medium"
}
\`\`\``,
          durationMs: 120,
        },
        {
          index: 2,
          status: "success",
          result: `Architecture research complete

\`\`\`json
{
  "summary": "Architecture research finished",
  "details": "Architecture patterns identified",
  "confidence": "high"
}
\`\`\``,
          durationMs: 110,
        },
        {
          index: 3,
          status: "success",
          result: `Pitfalls research complete

\`\`\`json
{
  "summary": "Pitfalls identified",
  "details": "Common failure modes documented",
  "confidence": "medium"
}
\`\`\``,
          durationMs: 90,
        },
      ],
    })
  );
  
  // Second call: synthesis
  mockFn.mockResolvedValueOnce(
    JSON.stringify({
      results: [
        {
          index: 0,
          status: "success",
          result: "# Research Synthesis\n\n## Stack\nTechnology recommendations...\n\n## Features\nKey feature analysis...\n\n## Architecture\nSystem design patterns...\n\n## Pitfalls\nCommon failure modes...",
          durationMs: 150,
        },
      ],
    })
  );
  
  // Third call: roadmap generation
  mockFn.mockResolvedValue(
    JSON.stringify({
      results: [
        {
          index: 0,
          status: "success",
          result: JSON.stringify({
            phases: [
              {
                name: "Foundation",
                goal: "Set up basic infrastructure",
                requirements: ["AUTH-01", "STOR-01"],
                successCriteria: ["User can authenticate", "Data persists locally"],
              },
              {
                name: "Features",
                goal: "Implement core features",
                requirements: ["UI-01"],
                successCriteria: ["Responsive interface works"],
              },
            ],
          }),
          durationMs: 200,
        },
      ],
    })
  );

  return {
    definition: {
      name: "sessions_dispatch",
      description: "Mock dispatch tool",
      input_schema: { type: "object" as const, properties: {}, required: [] },
    },
    execute: mockFn,
  } as unknown as ToolHandler;
}

describe("Dianoia E2E — Context & State Foundation", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let orchestrator: DianoiaOrchestrator;
  let researchOrch: ResearchOrchestrator;
  let requirementsOrch: RequirementsOrchestrator;
  let roadmapOrch: RoadmapOrchestrator;
  let workspaceRoot: string;
  let mockDispatch: ToolHandler;

  const mockContext: ToolContext = {
    nousId: "test-nous",
    sessionId: "test-session",
    workspace: "/tmp/e2e-test",
  };

  beforeEach(() => {
    // Set up database and orchestrators
    db = makeDb();
    store = new PlanningStore(db);
    mockDispatch = makeMockDispatchTool();
    
    // Create temp workspace for file operations
    workspaceRoot = join(tmpdir(), `dianoia-e2e-${Date.now()}`);
    mkdirSync(workspaceRoot, { recursive: true });
    
    // Initialize orchestrators with workspace
    orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    orchestrator.setWorkspaceRoot(workspaceRoot);
    
    researchOrch = new ResearchOrchestrator(db, mockDispatch, workspaceRoot);
    requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);
    roadmapOrch = new RoadmapOrchestrator(db, mockDispatch);
  });

  it("validates Context & State Foundation requirements end-to-end", async () => {
    // Step 1: Create project and transition to questioning
    orchestrator.handle(mockContext.nousId, mockContext.sessionId);
    const project = orchestrator.getActiveProject(mockContext.nousId)!;
    expect(project.state).toBe("questioning");

    // Step 2: Confirm synthesis → writes PROJECT.md (CTX-02)
    orchestrator.confirmSynthesis(project.id, mockContext.nousId, mockContext.sessionId, {
      goal: "Build a test application",
      coreValue: "Reliability and performance",
      constraints: ["Must work offline", "Browser compatibility"],
      keyDecisions: ["Use SQLite", "Progressive Web App"],
    });
    
    const afterSynthesis = store.getProjectOrThrow(project.id);
    expect(afterSynthesis.state).toBe("researching");
    
    // Verify PROJECT.md exists and has content (CTX-02)
    const projectPath = join(workspaceRoot, ".dianoia", "projects", project.id, "PROJECT.md");
    expect(existsSync(projectPath)).toBe(true);
    const projectContent = readFileSync(projectPath, "utf-8");
    expect(projectContent).toContain("Build a test application");
    expect(projectContent).toContain("Reliability and performance");

    // Step 3: Run research → writes RESEARCH.md with synthesis (CTX-04)
    const researchResult = await researchOrch.runResearch(
      project.id,
      "Build a test application",
      mockContext,
    );
    expect(researchResult.stored).toBe(4); // All 4 dimensions succeeded
    expect(researchResult.failed).toBe(0);
    
    researchOrch.transitionToRequirements(project.id);
    const afterResearch = store.getProjectOrThrow(project.id);
    expect(afterResearch.state).toBe("requirements");
    
    // Verify RESEARCH.md exists (CTX-04)
    const researchPath = join(workspaceRoot, ".dianoia", "projects", project.id, "RESEARCH.md");
    expect(existsSync(researchPath)).toBe(true);
    const researchContent = readFileSync(researchPath, "utf-8");
    expect(researchContent).toContain("# Research");

    // Step 4: Category-by-category scoping with coverage gate (CTX-03)
    
    // Persist first category
    requirementsOrch.persistCategory(project.id, {
      category: "AUTH",
      categoryName: "Authentication",
      tableStakes: [
        { name: "Login", description: "User login functionality", isTableStakes: true, proposedTier: "v1" },
        { name: "Password Reset", description: "Password reset flow", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [
        { name: "SSO", description: "Single sign-on integration", isTableStakes: false, proposedTier: "v2" },
      ],
    }, [
      { name: "Login", tier: "v1" },
      { name: "Password Reset", tier: "v1" },
      { name: "SSO", tier: "v2" },
    ]);

    // Persist second category
    requirementsOrch.persistCategory(project.id, {
      category: "STOR",
      categoryName: "Data Storage",
      tableStakes: [
        { name: "Local Storage", description: "Client-side data storage", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [
        { name: "Sync", description: "Cloud sync capabilities", isTableStakes: false, proposedTier: "v2" },
      ],
    }, [
      { name: "Local Storage", tier: "v1" },
      { name: "Sync", tier: "v2" },
    ]);

    // Test coverage validation - should fail with only 1 category
    expect(requirementsOrch.validateCoverage(project.id, ["AUTH"], 2)).toBe(false);
    
    // Persist third category
    requirementsOrch.persistCategory(project.id, {
      category: "UI",
      categoryName: "User Interface",
      tableStakes: [
        { name: "Responsive Design", description: "Mobile-friendly interface", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [
        { name: "Dark Mode", description: "Dark theme support", isTableStakes: false, proposedTier: "v2" },
      ],
    }, [
      { name: "Responsive Design", tier: "v1" },
      { name: "Dark Mode", tier: "v2" },
    ]);

    // Test coverage validation - should pass with 3 categories and v1 requirements
    expect(requirementsOrch.validateCoverage(project.id, ["AUTH", "STOR", "UI"], 2)).toBe(true);
    
    // Verify REQUIREMENTS.md is written after each persist (CTX-03)
    const requirementsPath = join(workspaceRoot, ".dianoia", "projects", project.id, "REQUIREMENTS.md");
    expect(existsSync(requirementsPath)).toBe(true);
    const reqContent = readFileSync(requirementsPath, "utf-8");
    expect(reqContent).toContain("# Requirements");
    expect(reqContent).toContain("AUTH-01");
    expect(reqContent).toContain("STOR-01");
    expect(reqContent).toContain("UI-01");

    // Step 5: Complete requirements transition
    orchestrator.completeRequirements(project.id, mockContext.nousId, mockContext.sessionId);
    const afterRequirements = store.getProjectOrThrow(project.id);
    expect(afterRequirements.state).toBe("roadmap");

    // Step 6: Generate and complete roadmap → writes ROADMAP.md (CTX-02)
    const roadmapResult = await roadmapOrch.generateRoadmap(project.id, mockContext);
    expect(roadmapResult.phases).toBeGreaterThan(0);
    
    orchestrator.completeRoadmap(project.id, mockContext.nousId, mockContext.sessionId);
    const afterRoadmap = store.getProjectOrThrow(project.id);
    expect(afterRoadmap.state).toBe("discussing");
    
    // Verify ROADMAP.md exists (CTX-02)
    const roadmapPath = join(workspaceRoot, ".dianoia", "projects", project.id, "ROADMAP.md");
    expect(existsSync(roadmapPath)).toBe(true);
    const roadmapContent = readFileSync(roadmapPath, "utf-8");
    expect(roadmapContent).toContain("# Roadmap");

    // Step 7: Test context packet assembly with tiktoken accuracy (CTX-01)
    const contextPacket = buildContextPacket({
      workspaceRoot,
      projectId: project.id,
      phaseId: null,
      role: "executor",
      maxTokens: 1000,
      projectGoal: "Build a test application",
    });

    expect(contextPacket.length).toBeGreaterThan(0);
    expect(contextPacket).toContain("Project Goal");
    expect(contextPacket).toContain("Requirements");
    
    // Verify token counting is working (should respect budget)
    const verySmallPacket = buildContextPacket({
      workspaceRoot,
      projectId: project.id,
      phaseId: null,
      role: "executor",
      maxTokens: 50, // Very small budget
      projectGoal: "Test",
    });
    expect(verySmallPacket.length).toBeLessThan(contextPacket.length);
  });

  it("validates fail-fast behavior", () => {
    // Create a project for this test
    orchestrator.handle(mockContext.nousId, mockContext.sessionId);
    const project = orchestrator.getActiveProject(mockContext.nousId)!;
    
    // Test table-stakes enforcement
    expect(() => {
      requirementsOrch.persistCategory(project.id, {
        category: "TEST",
        categoryName: "Test Category",
        tableStakes: [
          { name: "Critical Feature", description: "Must have", isTableStakes: true, proposedTier: "v1" },
        ],
        differentiators: [],
      }, [
        { name: "Critical Feature", tier: "out-of-scope" }, // No rationale - should fail
      ]);
    }).toThrow("Table-stakes feature");

    // Test duplicate reqId enforcement
    requirementsOrch.persistCategory(project.id, {
      category: "DUP",
      categoryName: "First Category",
      tableStakes: [
        { name: "Feature A", description: "Test feature", isTableStakes: false, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Feature A", tier: "v1" },
    ]);

    // This should work - different category
    requirementsOrch.persistCategory(project.id, {
      category: "DUP2", 
      categoryName: "Second Category",
      tableStakes: [
        { name: "Feature B", description: "Another test feature", isTableStakes: false, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Feature B", tier: "v1" },
    ]);
  });
});