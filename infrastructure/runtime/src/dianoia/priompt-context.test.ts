// Tests for Priompt-based context assembly (Spec 32 Context & State Foundation)

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { buildContextPacketWithPriompt } from "./priompt-context.js";
import {
  ensurePhaseDir,
  ensureProjectDir,
  getPhaseDir,
  getProjectDir,
} from "./project-files.js";
import type { PlanningPhase } from "./types.js";

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
  } as PlanningPhase;
}

/** Write a markdown file directly into the project directory */
function writeTestFile(dir: string, filename: string, content: string): void {
  writeFileSync(join(dir, filename), content, "utf-8");
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
    const projDir = getProjectDir(workspaceRoot, TEST_PROJECT_ID);
    const phaseDir = getPhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    writeTestFile(projDir, "PROJECT.md", "# Test Project\n\nA test project description.");
    writeTestFile(phaseDir, "DISCUSS.md", "# Decisions\n\nUse OAuth2 standard.");

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
    const projDir = getProjectDir(workspaceRoot, TEST_PROJECT_ID);
    const phaseDir = getPhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);
    const longContent = "# Very Long Project\n\n" + "Very detailed description. ".repeat(500);

    writeTestFile(projDir, "PROJECT.md", longContent);
    writeTestFile(phaseDir, "DISCUSS.md", "# Key Decision\n\nUse standard approach.");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: TEST_PHASE_ID,
      role: "executor",
      maxTokens: 200,
      phase,
      projectGoal: "Test project",
    });

    // High priority sections should be included
    expect(result).toContain("Phase Objective");
    expect(result).toContain("Key Decision");
  });

  it("includes role-appropriate sections for executor", async () => {
    ensureProjectDir(workspaceRoot, TEST_PROJECT_ID);
    ensurePhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    const phase = makePhase();
    const phaseDir = getPhaseDir(workspaceRoot, TEST_PROJECT_ID, TEST_PHASE_ID);

    writeTestFile(phaseDir, "DISCUSS.md", "# Decisions\n\nUse OAuth2.");
    writeTestFile(phaseDir, "PLAN.md", "# Execution Plan\n\n1. Install deps\n2. Wire routes");

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
    const projDir = getProjectDir(workspaceRoot, TEST_PROJECT_ID);

    writeTestFile(projDir, "PROJECT.md", "# Full Project\n\nComplete project description");
    writeTestFile(projDir, "REQUIREMENTS.md", "# Requirements\n\n| ID | Description | Tier |\n|---|---|---|\n| AUTH-01 | OAuth login | v1 |");
    writeTestFile(projDir, "ROADMAP.md", "# Roadmap\n\nPhase 1: Auth\nPhase 2: Dashboard");

    const result = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: TEST_PROJECT_ID,
      phaseId: null,
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
      maxTokens: -1,
      projectGoal: "Test",
    });

    // Should return error message instead of crashing
    expect(result).toContain("Context Error");
  });
});
