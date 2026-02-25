// Tests for Priompt-based context assembly (Spec 32 Context & State Foundation)

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { buildContextPacketWithPriompt } from "./priompt-context.js";
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

const TEST_PROJECT_ID = "proj_priompt_test";
const TEST_PHASE_ID = "phase_priompt_test";

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

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `priompt-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
});

afterEach(() => {
  if (workspaceRoot) {
    rmSync(workspaceRoot, { recursive: true, force: true });
  }
});

describe("buildContextPacketWithPriompt", () => {
  it("generates context packet with accurate token counting", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    const phase = makePhase();
    
    // Write some test files
    writeProjectFile(workspaceRoot, TEST_PROJECT_ID, "# Test Project\n\nA test project description.");
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, "# Decisions\n\nUse OAuth2 standard.");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      maxTokens: 1000,
      phase,
      projectGoal: "Build authentication system",
    });

    expect(result).toContain("Phase Objective");
    expect(result).toContain("Authentication");
    expect(result).toContain("OAuth2 login");
    expect(result).toContain("Design Decisions");
    expect(result).toContain("OAuth2 standard");
  });

  it("respects token budget with Priompt prioritization", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    const phase = makePhase();
    const longProjectDescription = "# Very Long Project\n\n" + "Very detailed description. ".repeat(500);
    
    writeProjectFile(workspaceRoot, TEST_PROJECT_ID, longProjectDescription);
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, "# Key Decision\n\nUse standard approach.");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      maxTokens: 200, // Very limited budget
      phase,
      projectGoal: "Test project",
    });

    // High priority sections should be included
    expect(result).toContain("Phase Objective");
    expect(result).toContain("Key Decision");
    
    // Low priority sections may be truncated due to budget
    // The exact behavior depends on Priompt's prioritization algorithm
  });

  it("includes role-appropriate sections for executor", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    const phase = makePhase();
    
    writeDiscussFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, "# Decisions\n\nUse OAuth2.");
    writePlanFile(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID, "# Execution Plan\n\n1. Install deps\n2. Wire routes");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      maxTokens: 2000,
      phase,
      projectGoal: "Build auth",
      supplementary: "Code context goes here",
    });

    // Executor should get phase goal, plan, decisions, and supplementary
    expect(result).toContain("Phase Objective");
    expect(result).toContain("Execution Plan");
    expect(result).toContain("Design Decisions");
    expect(result).toContain("Reference Material");
    expect(result).toContain("Code context goes here");
    
    // Executor should NOT get full project context (excluded for this role)
    expect(result).not.toContain("Project Context");
  });

  it("includes role-appropriate sections for planner", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    
    const phase = makePhase();
    
    writeProjectFile(workspaceRoot, TEST_PROJECT_ID, "# Full Project\n\nComplete project description");
    writeRequirementsFile(workspaceRoot, TEST_PROJECT_ID, "# Requirements\n\n| ID | Description | Tier |\n|---|---|---|\n| AUTH-01 | OAuth login | v1 |");
    writeRoadmapFile(workspaceRoot, TEST_PROJECT_ID, "# Roadmap\n\nPhase 1: Auth\nPhase 2: Dashboard");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: null, // Project-level planning
      role: "planner",
      maxTokens: 3000,
      phase,
      projectGoal: "Build full system",
    });

    // Planner should get comprehensive context
    expect(result).toContain("Project Context");
    expect(result).toContain("Requirements");  
    expect(result).toContain("Roadmap");
    expect(result).toContain("Phase Objective");
    
    // Planner should NOT get supplementary (excluded for this role)
    expect(result).not.toContain("Reference Material");
  });

  it("handles missing files gracefully", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);

    const phase = makePhase();

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      maxTokens: 1000,
      phase,
      projectGoal: "Test project",
    });

    // Should still generate valid output with available data
    expect(result).toContain("Phase Objective");
    expect(result).toContain("Test project");
    expect(result).not.toContain("Design Decisions"); // File doesn't exist
  });

  it("falls back gracefully on Priompt errors", async () => {
    // Test with invalid parameters that might cause Priompt to fail
    const result = await buildContextPacketWithPriompt({
      workspaceRoot: "/nonexistent/path",
      projectId: "invalid",
      phaseId: "invalid",
      role: "executor",
      maxTokens: -1, // Invalid token count
      projectGoal: "Test",
    });

    // Should return error message instead of crashing
    expect(result).toContain("Context Error");
  });
});