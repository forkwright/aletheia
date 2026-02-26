// End-to-end integration test for S5 Context & State Foundation
// Tests the full pipeline: project creation → questioning → research → requirements → roadmap
// Validates all .md files are written with expected content, context packet assembly, and token counting
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import Database from "better-sqlite3";
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
import { DianoiaOrchestrator } from "./orchestrator.js";
import { ResearchOrchestrator } from "./researcher.js";
import { RequirementsOrchestrator } from "./requirements.js";
import { RoadmapOrchestrator } from "./roadmap.js";
import { buildContextPacketSync, type SubAgentRole } from "./context-packet.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { CategoryProposal, ScopingDecision } from "./requirements.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";
import { getEncoding } from "js-tiktoken";

// Use tiktoken for accurate token counting validation
const encoder = getEncoding("cl100k_base");

function countTokens(text: string): number {
  try {
    return encoder.encode(text).length;
  } catch (error) {
    // Fallback to character estimation
    return Math.ceil(text.length / 4);
  }
}

let workspaceRoot: string;
let db: Database.Database;

const TOOL_CONTEXT: ToolContext = {
  nousId: "test-nous",
  sessionId: "test-session",
  workspace: "/tmp",
};

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
};

// Mock research results with structured JSON blocks
const MOCK_RESEARCH_RESULTS = {
  results: [
    {
      index: 0,
      status: "success",
      result: `Technology Stack Research: \`\`\`json\n{"summary":"Node.js with TypeScript recommended","details":"Use Node.js 20+ with TypeScript 5.x, Express.js for API, React for frontend. Strong ecosystem support.","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 1,
      status: "success",
      result: `Feature Analysis: \`\`\`json\n{"summary":"Authentication and CRUD operations are table-stakes","details":"OAuth2 integration, user management, data persistence with relational DB required for MVP.","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 2,
      status: "success",
      result: `Architecture Patterns: \`\`\`json\n{"summary":"RESTful API with microservices readiness","details":"Start with modular monolith, prepare for microservices extraction. Use event-driven patterns.","confidence":"medium"}\n\`\`\``,
      durationMs: 1000,
    },
    {
      index: 3,
      status: "success",
      result: `Common Pitfalls: \`\`\`json\n{"summary":"Security vulnerabilities and performance bottlenecks","details":"Watch for SQL injection, implement proper JWT validation, avoid N+1 queries, use connection pooling.","confidence":"high"}\n\`\`\``,
      durationMs: 1000,
    },
  ],
};

// Mock synthesis results
const MOCK_SYNTHESIS_RESULT = {
  results: [
    {
      index: 0,
      status: "success",
      result: `# Research Summary

## Technology Stack
Node.js 20+ with TypeScript 5.x provides the optimal balance of performance, type safety, and ecosystem maturity.

## Core Features  
Authentication (OAuth2), user management, and CRUD operations form the foundation that all other features depend on.

## Architecture Approach
Start with a well-structured monolith using domain modules, then extract microservices as needed based on traffic patterns.

## Key Risks
Security vulnerabilities (SQL injection, JWT handling) and performance issues (N+1 queries) must be addressed from the start.

## Implementation Recommendations
1. Use TypeScript for type safety
2. Implement OAuth2 with proper session management
3. Design database schema with proper indexing
4. Plan API versioning strategy from day one`,
      durationMs: 2000,
    },
  ],
};

// Mock roadmap generation results  
const MOCK_ROADMAP_RESULT = {
  results: [
    {
      index: 0,
      status: "success",
      result: `\`\`\`json
[
  {
    "name": "Authentication Foundation",
    "goal": "Establish secure user authentication and session management",
    "requirements": ["AUTH-01", "AUTH-02"],
    "successCriteria": [
      "Users can register with email/password",
      "JWT tokens are issued and validated properly",
      "Session management works across page refreshes"
    ],
    "phaseOrder": 1
  },
  {
    "name": "Data Management Core",
    "goal": "Implement core CRUD operations and data persistence",
    "requirements": ["DATA-01", "DATA-02"],
    "successCriteria": [
      "Database schema is implemented and migrated",
      "Basic CRUD operations work for all entities",
      "Data validation and error handling is comprehensive"
    ],
    "phaseOrder": 2
  },
  {
    "name": "API Integration",
    "goal": "Build RESTful API with proper error handling and documentation",
    "requirements": ["API-01"],
    "successCriteria": [
      "API endpoints follow REST conventions",
      "Proper HTTP status codes and error messages",
      "API documentation is generated and accessible"
    ],
    "phaseOrder": 3
  }
]
\`\`\``,
      durationMs: 3000,
    },
  ],
};

function setupMockDispatch(): ToolHandler {
  let callCount = 0;
  return {
    definition: { name: "sessions_dispatch", description: "mock", input_schema: {} },
    execute: vi.fn().mockImplementation(() => {
      callCount++;
      if (callCount === 1) {
        // First call: research dimensions
        return Promise.resolve(JSON.stringify(MOCK_RESEARCH_RESULTS));
      } else if (callCount === 2) {
        // Second call: research synthesis
        return Promise.resolve(JSON.stringify(MOCK_SYNTHESIS_RESULT));
      } else if (callCount === 3) {
        // Third call: roadmap generation
        return Promise.resolve(JSON.stringify(MOCK_ROADMAP_RESULT));
      }
      throw new Error(`Unexpected dispatch call ${callCount}`);
    }),
  } as unknown as ToolHandler;
}

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
  db.exec(PLANNING_V27_MIGRATION);
  return db;
}

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `dianoia-e2e-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
  
  db = makeDb();
});

afterEach(() => {
  if (existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true });
  }
  db.close();
  vi.restoreAllMocks();
});

describe("S5 End-to-End Integration Test", () => {
  it("completes full pipeline: creation → research → requirements → roadmap with file validation", async () => {
    const store = new PlanningStore(db);
    const orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    orchestrator.setWorkspaceRoot(workspaceRoot);
    
    const mockDispatch = setupMockDispatch();
    const researchOrch = new ResearchOrchestrator(db, mockDispatch, workspaceRoot);
    const requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);
    const roadmapOrch = new RoadmapOrchestrator(db, mockDispatch);
    roadmapOrch.setWorkspaceRoot(workspaceRoot);

    // Step 1: Create project via handle() 
    const handleResult = orchestrator.handle("test-nous", "test-session");
    expect(handleResult).toContain("what are you building");
    
    const project = orchestrator.getActiveProject("test-nous");
    expect(project).toBeDefined();
    expect(project!.state).toBe("questioning");

    // Step 2: Confirm synthesis to advance questioning → researching
    const synthesisResult = orchestrator.confirmSynthesis(project!.id, "test-nous", "test-session", {
      goal: "Build a SaaS task management application with team collaboration",
      coreValue: "Simplicity and reliability",
      constraints: ["Must support 1000+ concurrent users", "GDPR compliant"],
      keyDecisions: ["Use PostgreSQL for data persistence", "Single-page React application"],
    });
    
    expect(synthesisResult).toContain("research");
    
    // Verify PROJECT.md was written
    const projectDir = join(workspaceRoot, ".dianoia", "projects", project!.id);
    const projectFile = join(projectDir, "PROJECT.md");
    expect(existsSync(projectFile)).toBe(true);
    
    const projectContent = readFileSync(projectFile, "utf-8");
    expect(projectContent).toContain("# Build a SaaS task management application");
    expect(projectContent).toContain("Simplicity and reliability");
    expect(projectContent).toContain("PostgreSQL for data persistence");

    // Step 3: Run research phase
    const researchResult = await researchOrch.runResearch(
      project!.id,
      project!.goal!,
      TOOL_CONTEXT,
    );

    expect(researchResult.stored).toBe(4); // All dimensions successful
    expect(researchResult.partial).toBe(0);
    expect(researchResult.failed).toBe(0);

    // Transition to requirements
    researchOrch.transitionToRequirements(project!.id);
    
    // Verify RESEARCH.md was written with expected structure
    const researchFile = join(projectDir, "RESEARCH.md");
    expect(existsSync(researchFile)).toBe(true);
    const researchContent = readFileSync(researchFile, "utf-8");
    expect(researchContent).toContain("# Research");
    expect(researchContent).toContain("## stack");
    expect(researchContent).toContain("Node.js");
    expect(researchContent).toContain("TypeScript");
    expect(researchContent).toContain("## synthesis");

    // Step 4: Requirements scoping - persist 3 categories
    
    // Category 1: Authentication
    const authCategory: CategoryProposal = {
      category: "AUTH",
      categoryName: "Authentication",
      tableStakes: [
        {
          name: "Email/password login",
          description: "Basic email and password authentication",
          isTableStakes: true,
          proposedTier: "v1",
        },
        {
          name: "Password reset flow",
          description: "Secure password reset via email",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [
        {
          name: "OAuth2 SSO",
          description: "Single sign-on with Google/GitHub",
          isTableStakes: false,
          proposedTier: "v2",
        },
      ],
    };

    const authDecisions: ScopingDecision[] = [
      { name: "Email/password login", tier: "v1" },
      { name: "Password reset flow", tier: "v1" },
      { name: "OAuth2 SSO", tier: "v2" },
    ];

    requirementsOrch.persistCategory(project!.id, authCategory, authDecisions);

    // Category 2: Data Management
    const dataCategory: CategoryProposal = {
      category: "DATA",
      categoryName: "Data Management",
      tableStakes: [
        {
          name: "CRUD operations",
          description: "Create, read, update, delete operations for all entities",
          isTableStakes: true,
          proposedTier: "v1",
        },
        {
          name: "Data validation",
          description: "Input validation and sanitization",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [
        {
          name: "Real-time sync",
          description: "Live updates across connected clients",
          isTableStakes: false,
          proposedTier: "v2",
        },
      ],
    };

    const dataDecisions: ScopingDecision[] = [
      { name: "CRUD operations", tier: "v1" },
      { name: "Data validation", tier: "v1" },
      { name: "Real-time sync", tier: "out-of-scope", rationale: "Not needed for MVP" },
    ];

    requirementsOrch.persistCategory(project!.id, dataCategory, dataDecisions);

    // Category 3: API Layer  
    const apiCategory: CategoryProposal = {
      category: "API",
      categoryName: "API Layer",
      tableStakes: [
        {
          name: "RESTful endpoints",
          description: "Standard REST API with proper HTTP methods",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [
        {
          name: "GraphQL support",
          description: "GraphQL API for flexible queries",
          isTableStakes: false,
          proposedTier: "v2",
        },
      ],
    };

    const apiDecisions: ScopingDecision[] = [
      { name: "RESTful endpoints", tier: "v1" },
      { name: "GraphQL support", tier: "v2" },
    ];

    requirementsOrch.persistCategory(project!.id, apiCategory, apiDecisions);

    // Verify REQUIREMENTS.md written with all categories
    const requirementsFile = join(projectDir, "REQUIREMENTS.md");
    expect(existsSync(requirementsFile)).toBe(true);
    const requirementsContent = readFileSync(requirementsFile, "utf-8");
    expect(requirementsContent).toContain("AUTH-01");
    expect(requirementsContent).toContain("AUTH-02"); 
    expect(requirementsContent).toContain("DATA-01");
    expect(requirementsContent).toContain("DATA-02");
    expect(requirementsContent).toContain("API-01");
    expect(requirementsContent).toContain("v1"); // Tier annotations
    expect(requirementsContent).toContain("v2");

    // Verify coverage gate validation
    const coverageValid = requirementsOrch.validateCoverage(project!.id, ["AUTH", "DATA", "API"]);
    expect(coverageValid).toBe(true);

    // Complete requirements phase
    const reqCompleteResult = orchestrator.completeRequirements(project!.id, "test-nous", "test-session");
    expect(reqCompleteResult).toContain("roadmap");
    
    const projectAfterReq = store.getProjectOrThrow(project!.id);
    expect(projectAfterReq.state).toBe("roadmap");

    // Step 5: Generate roadmap
    const roadmapPhases = await roadmapOrch.generateRoadmap(
      project!.id,
      project!.goal!,
      TOOL_CONTEXT,
    );

    expect(roadmapPhases).toHaveLength(3);
    expect(roadmapPhases[0]!.name).toBe("Authentication Foundation");
    expect(roadmapPhases[1]!.name).toBe("Data Management Core");
    expect(roadmapPhases[2]!.name).toBe("API Integration");
    
    // Validate coverage
    const coverage = roadmapOrch.validateCoverage(project!.id, roadmapPhases);
    expect(coverage.covered).toBe(true);
    expect(coverage.missing).toHaveLength(0);
    
    // Commit roadmap
    roadmapOrch.commitRoadmap(project!.id, roadmapPhases);
    
    // Complete roadmap phase
    const roadmapCompleteResult = orchestrator.completeRoadmap(project!.id, "test-nous", "test-session");
    expect(roadmapCompleteResult).toContain("Roadmap complete");
    
    const projectAfterRoadmap = store.getProjectOrThrow(project!.id);
    expect(projectAfterRoadmap.state).toBe("discussing");
    
    // Verify ROADMAP.md was written
    const roadmapFile = join(projectDir, "ROADMAP.md");
    expect(existsSync(roadmapFile)).toBe(true);
    const roadmapContent = readFileSync(roadmapFile, "utf-8");
    expect(roadmapContent).toContain("# Roadmap");
    expect(roadmapContent).toContain("Authentication Foundation");
    expect(roadmapContent).toContain("Data Management Core");
    expect(roadmapContent).toContain("API Integration");

    // Step 6: Test context packet assembly with different roles
    const phases = store.listPhases(project!.id);
    expect(phases).toHaveLength(3);
    
    const testCases: Array<{
      role: SubAgentRole;
      maxTokens: number;
      shouldContain: string[];
      shouldNotContain: string[];
    }> = [
      {
        role: "executor",
        maxTokens: 2000,
        shouldContain: ["Requirements", "Authentication Foundation"],
        shouldNotContain: ["Research Findings", "Project Context"],
      },
      {
        role: "planner", 
        maxTokens: 4000,
        shouldContain: ["Phase Objective", "Requirements", "Research Findings", "Roadmap Overview"],
        shouldNotContain: ["Execution Plan"],
      },
      {
        role: "verifier",
        maxTokens: 3000,
        shouldContain: ["Phase Objective", "Requirements", "Roadmap Overview"],
        shouldNotContain: [],
      },
    ];

    for (const testCase of testCases) {
      const contextPacket = buildContextPacketSync({
        workspaceRoot,
        projectId: project!.id,
        phaseId: phases[0]!.id,
        role: testCase.role,
        maxTokens: testCase.maxTokens,
        projectGoal: project!.goal!,
        phase: phases[0]!,
        allPhases: phases,
      });

      // Verify content inclusions
      for (const shouldContain of testCase.shouldContain) {
        expect(contextPacket).toContain(shouldContain);
      }
      
      for (const shouldNotContain of testCase.shouldNotContain) {
        expect(contextPacket).not.toContain(shouldNotContain);
      }

      // Test tiktoken-accurate token counting
      const actualTokens = countTokens(contextPacket);
      expect(actualTokens).toBeLessThanOrEqual(testCase.maxTokens);
      expect(actualTokens).toBeGreaterThan(0);
    }

    // Step 7: Verify all expected files exist with proper headers
    const expectedFiles = [
      { name: "PROJECT.md", headerPattern: /^# Build a SaaS task management/ },
      { name: "RESEARCH.md", headerPattern: /^# Research/ },
      { name: "REQUIREMENTS.md", headerPattern: /^# Requirements/ },
      { name: "ROADMAP.md", headerPattern: /^# Roadmap/ },
    ];

    for (const { name, headerPattern } of expectedFiles) {
      const filePath = join(projectDir, name);
      expect(existsSync(filePath)).toBe(true);
      
      const content = readFileSync(filePath, "utf-8");
      expect(content).toMatch(headerPattern);
      expect(content.trim().length).toBeGreaterThan(0);
    }
  });

  it("validates token budget enforcement in context packet assembly", () => {
    // Test that context packets respect strict token limits
    const mockPhase = {
      id: "phase_test",
      projectId: "proj_test",
      name: "Test Phase",
      goal: "Test the token counting",
      requirements: [],
      successCriteria: ["Token counting works"],
      status: "pending" as const,
      phaseOrder: 1,
      createdAt: "2023-01-01T00:00:00Z",
      updatedAt: "2023-01-01T00:00:00Z",
      plan: null,
    };

    // Very small token budget to force truncation
    const contextPacket = buildContextPacketSync({
      workspaceRoot,
      projectId: "proj_test",
      phaseId: "phase_test", 
      role: "executor",
      maxTokens: 100, // Very small budget
      projectGoal: "Test project with a very long goal that should be truncated when the token budget is exceeded",
      phase: mockPhase,
      supplementary: "This is supplementary context that might be truncated due to token limits. ".repeat(20),
    });

    const actualTokens = countTokens(contextPacket);
    expect(actualTokens).toBeLessThanOrEqual(100);
    expect(contextPacket.length).toBeGreaterThan(0);
  });

  it("validates coverage gate enforcement", () => {
    // Test that coverage gate properly validates requirement coverage
    const store = new PlanningStore(db);
    const requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);

    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session", 
      goal: "Test coverage validation",
      config: DEFAULT_CONFIG,
    });

    // Add single requirement
    const category: CategoryProposal = {
      category: "TEST",
      categoryName: "Test Category",
      tableStakes: [
        {
          name: "Test requirement",
          description: "Single test requirement",
          isTableStakes: true,
          proposedTier: "v1",
        },
      ],
      differentiators: [],
    };

    requirementsOrch.persistCategory(project.id, category, [
      { name: "Test requirement", tier: "v1" },
    ]);

    // Single category should pass with default minimum (1)
    expect(requirementsOrch.validateCoverage(project.id, ["TEST"])).toBe(true);

    // Should fail with minimum set to 2
    expect(requirementsOrch.validateCoverage(project.id, ["TEST"], 2)).toBe(false);
  });
});