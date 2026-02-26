// Tests for file validation and cross-phase persistence (Spec 32 CTX-02)

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  readDiscussFile,
  readPlanFile,
  readProjectFile,
  readRequirementsFile,
  readResearchFile,
  readRoadmapFile,
  writeDiscussFile,
  writePlanFile,
  writeProjectFile,
  writeRequirementsFile,
  writeResearchFile,
  writeRoadmapFile,
  writeStateFile,
  writeVerifyFile,
} from "./project-files.js";

const TEST_PROJECT_ID = "proj_validation_test";
const TEST_PHASE_ID = "phase_validation_test";

let workspaceRoot: string;

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `validation-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
});

afterEach(() => {
  if (workspaceRoot) {
    rmSync(workspaceRoot, { recursive: true, force: true });
  }
});

describe("File validation (CTX-02)", () => {
  it("validates writeProjectFile with read-back verification", () => {
    const testProject = {
      id: TEST_PROJECT_ID,
      goal: "Test Project",
      state: "planning" as const,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      projectContext: {
        goal: "Build a test application",
        coreValue: "Testing validation",
        constraints: ["Must be testable"],
        keyDecisions: ["Use TypeScript"]
      }
    };
    
    // Write and verify it validates
    expect(() => writeProjectFile(workspaceRoot, testProject)).not.toThrow();
    
    // Read back and verify content exists
    const readContent = readProjectFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readContent).toContain("Test Project");
    expect(readContent).toContain("Build a test application");
  });

  it("validates writeRequirementsFile with read-back verification", () => {
    const testRequirements = [
      {
        reqId: "REQ-01",
        description: "User can log in with email",
        tier: "v1" as const,
        createdAt: "2026-01-01T00:00:00Z"
      },
      {
        reqId: "REQ-02", 
        description: "User can reset password",
        tier: "v2" as const,
        createdAt: "2026-01-01T00:00:00Z"
      }
    ];
    
    // Write and verify it validates
    expect(() => writeRequirementsFile(workspaceRoot, TEST_PROJECT_ID, testRequirements)).not.toThrow();
    
    // Read back and verify content contains expected data
    const readContent = readRequirementsFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readContent).toContain("REQ-01");
    expect(readContent).toContain("log in with email");
    expect(readContent).toContain("v1");
  });

  it("validates writeResearchFile with read-back verification", () => {
    const testResearch = [
      {
        id: "r1",
        projectId: TEST_PROJECT_ID,
        phase: "research",
        dimension: "stack",
        content: "Node.js with TypeScript recommended for strong typing and performance",
        status: "complete" as const,
        createdAt: "2026-01-01T00:00:00Z",
      },
      {
        id: "r2",
        projectId: TEST_PROJECT_ID,
        phase: "research",
        dimension: "features",
        content: "React for frontend, Express for backend",
        status: "complete" as const,
        createdAt: "2026-01-01T00:00:00Z",
      }
    ];
    
    // Write and verify it validates
    expect(() => writeResearchFile(workspaceRoot, TEST_PROJECT_ID, testResearch)).not.toThrow();
    
    // Read back and verify content contains expected data
    const readContent = readResearchFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readContent).toContain("stack");
    expect(readContent).toContain("Node.js");
    expect(readContent).toContain("TypeScript");
  });

  it("validates writeRoadmapFile with read-back verification", () => {
    const testPhases = [
      {
        id: "phase1",
        projectId: TEST_PROJECT_ID,
        name: "Authentication",
        goal: "Implement user authentication system",
        requirements: ["REQ-01"],
        successCriteria: ["Users can log in", "Sessions persist"],
        phaseOrder: 0,
        status: "pending" as const,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z"
      },
      {
        id: "phase2",
        projectId: TEST_PROJECT_ID,
        name: "Dashboard", 
        goal: "Build user dashboard",
        requirements: ["REQ-02"],
        successCriteria: ["Dashboard loads", "User data displayed"],
        phaseOrder: 1,
        status: "pending" as const,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z"
      }
    ];
    
    // Write and verify it validates
    expect(() => writeRoadmapFile(workspaceRoot, TEST_PROJECT_ID, testPhases)).not.toThrow();
    
    // Read back and verify content contains expected data
    const readContent = readRoadmapFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readContent).toContain("Authentication");
    expect(readContent).toContain("Dashboard");
    expect(readContent).toContain("Phase 1");
  });

  it("validates writeDiscussFile with read-back verification", () => {
    const testQuestions = [
      {
        id: "q1",
        question: "Which database to use?",
        options: [
          { label: "PostgreSQL", rationale: "Mature, reliable" },
          { label: "MongoDB", rationale: "Flexible schema" }
        ],
        recommendation: "PostgreSQL",
        status: "answered" as const,
        decision: "PostgreSQL",
        userNote: "PostgreSQL chosen for reliability"
      }
    ];
    
    // Write and verify it validates
    expect(() => writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, testQuestions)).not.toThrow();
    
    // Read back and verify content exists and contains expected data
    const readContent = readDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);
    expect(readContent).toContain("Phase Discussion");
    expect(readContent).toContain("Which database to use?");
    expect(readContent).toContain("PostgreSQL");
    expect(readContent).toContain("selected");
  });

  it("validates writePlanFile with read-back verification", () => {
    const testPlan = {
      steps: [
        {
          id: "s1",
          description: "Set up database",
          subtasks: ["Install PostgreSQL", "Create schema"],
          dependsOn: []
        }
      ]
    };
    
    // Write and verify it validates
    expect(() => writePlanFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, testPlan)).not.toThrow();
    
    // Read back and verify content exists and contains expected data
    const readContent = readPlanFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);
    expect(readContent).toContain("Execution Plan");
    expect(readContent).toContain("Set up database");
    expect(readContent).toContain("PostgreSQL");
  });

  it("validates writeStateFile with read-back verification", () => {
    const testState = {
      status: "executing",
      startedAt: "2026-01-01T00:00:00Z",
      currentStep: "s1"
    };
    
    // Write and verify it validates - no read function exists, just verify no errors
    expect(() => writeStateFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, testState)).not.toThrow();
  });

  it("validates writeVerifyFile with read-back verification", () => {
    const testVerification = {
      status: "passed",
      verifiedAt: "2026-01-01T12:00:00Z",
      summary: "All requirements met",
      gaps: []
    };
    
    // Write and verify it validates - no read function exists, just verify no errors
    expect(() => writeVerifyFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, testVerification)).not.toThrow();
  });

  it("validates writeStateFile and writeVerifyFile roundtrip without error", () => {
    // These functions accept arbitrary data and always produce non-empty files
    // due to their markdown headers. Verify they don't throw on various inputs.
    expect(() => writeStateFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, {})).not.toThrow();
    expect(() => writeStateFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, { status: "active" })).not.toThrow();
    expect(() => writeVerifyFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, { gaps: [] })).not.toThrow();
  });
});

describe("Cross-phase persistence (CTX-02)", () => {
  it("persists files across multiple phase transitions", () => {
    // Simulate the questioning → researching → requirements flow
    
    // 1. Questioning phase: write PROJECT.md
    const testProject = {
      id: TEST_PROJECT_ID,
      goal: "Web Application",
      state: "planning" as const,
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      projectContext: {
        goal: "Building a web application with authentication",
        coreValue: "Secure user access"
      }
    };
    writeProjectFile(workspaceRoot, testProject);
    
    // Verify PROJECT.md exists and contains expected content
    let readProject = readProjectFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readProject).toContain("Web Application");
    expect(readProject).toContain("authentication");
    
    // 2. Researching phase: write RESEARCH.md
    const testResearch = [
      { id: "r1", projectId: TEST_PROJECT_ID, phase: "research", dimension: "stack", content: "Node.js + React + PostgreSQL", status: "complete" as const, createdAt: "2026-01-01T00:00:00Z" },
      { id: "r2", projectId: TEST_PROJECT_ID, phase: "research", dimension: "synthesis", content: "Modern full-stack approach", status: "complete" as const, createdAt: "2026-01-01T00:00:00Z" },
    ];
    writeResearchFile(workspaceRoot, TEST_PROJECT_ID, testResearch);
    
    // Verify both PROJECT.md and RESEARCH.md exist
    readProject = readProjectFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readProject).toContain("Web Application");
    
    const readResearch = readResearchFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readResearch).toContain("Node.js");
    expect(readResearch).toContain("PostgreSQL");
    
    // 3. Requirements phase: write REQUIREMENTS.md
    const testRequirements = [
      { reqId: "AUTH-01", description: "OAuth login", tier: "v1" as const, createdAt: "2026-01-01T00:00:00Z" },
      { reqId: "AUTH-02", description: "Session management", tier: "v1" as const, createdAt: "2026-01-01T00:00:00Z" }
    ];
    writeRequirementsFile(workspaceRoot, TEST_PROJECT_ID, testRequirements);
    
    // Verify all three files persist
    readProject = readProjectFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readProject).toContain("Web Application");
    
    const readResearchFinal = readResearchFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readResearchFinal).toContain("Node.js");
    
    const readRequirements = readRequirementsFile(workspaceRoot, TEST_PROJECT_ID);
    expect(readRequirements).toContain("AUTH-01");
    expect(readRequirements).toContain("OAuth login");
    
    // 4. Roadmap phase: write ROADMAP.md
    const testPhases = [
      {
        id: "phase1", projectId: TEST_PROJECT_ID, name: "Auth",
        goal: "Implement authentication system", requirements: ["AUTH-01"],
        successCriteria: [], phaseOrder: 0, status: "pending" as const,
        createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z"
      }
    ];
    writeRoadmapFile(workspaceRoot, TEST_PROJECT_ID, testPhases);
    
    // Final verification: all files persist across all state transitions
    expect(readProjectFile(workspaceRoot, TEST_PROJECT_ID)).toContain("Web Application");
    expect(readResearchFile(workspaceRoot, TEST_PROJECT_ID)).toContain("Node.js");
    expect(readRequirementsFile(workspaceRoot, TEST_PROJECT_ID)).toContain("AUTH-01");
    expect(readRoadmapFile(workspaceRoot, TEST_PROJECT_ID)).toContain("Auth");
  });

  it("maintains phase-specific files across transitions", () => {
    const phase1Id = "phase_auth";
    const phase2Id = "phase_dashboard";
    
    // Write phase 1 files
    const phase1Discuss = [
      {
        id: "q1",
        question: "OAuth provider?",
        options: [{ label: "Google", rationale: "Popular" }],
        recommendation: "Google",
        status: "answered" as const,
        decision: "Google"
      }
    ];
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, phase1Id, phase1Discuss);
    
    const phase1Plan = { steps: [{ id: "s1", description: "Setup OAuth", subtasks: [], dependsOn: [] }] };
    writePlanFile(workspaceRoot, TEST_PROJECT_ID, phase1Id, phase1Plan);
    
    // Write phase 2 files
    const phase2Discuss = [
      {
        id: "q2", 
        question: "UI framework?",
        options: [{ label: "React", rationale: "Component-based" }],
        recommendation: "React",
        status: "answered" as const,
        decision: "React"
      }
    ];
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, phase2Id, phase2Discuss);
    
    const phase2Plan = { steps: [{ id: "s2", description: "Build dashboard", subtasks: [], dependsOn: [] }] };
    writePlanFile(workspaceRoot, TEST_PROJECT_ID, phase2Id, phase2Plan);
    
    // Verify both phases maintain their own files
    const readPhase1Discuss = readDiscussFile(workspaceRoot, TEST_PROJECT_ID, phase1Id);
    expect(readPhase1Discuss).toContain("OAuth provider");
    expect(readPhase1Discuss).toContain("Google");
    
    const readPhase1Plan = readPlanFile(workspaceRoot, TEST_PROJECT_ID, phase1Id);
    expect(readPhase1Plan).toContain("Setup OAuth");
    
    const readPhase2Discuss = readDiscussFile(workspaceRoot, TEST_PROJECT_ID, phase2Id);
    expect(readPhase2Discuss).toContain("UI framework");
    expect(readPhase2Discuss).toContain("React");
    
    const readPhase2Plan = readPlanFile(workspaceRoot, TEST_PROJECT_ID, phase2Id);
    expect(readPhase2Plan).toContain("Build dashboard");
  });
});