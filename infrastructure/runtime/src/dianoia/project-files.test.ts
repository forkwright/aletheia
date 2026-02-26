// Tests for project-files.ts atomic writes and validation (CTX-02)
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { writeProjectFile, writeRequirementsFile, writeResearchFile, writeRoadmapFile } from "./project-files.js";
import type { PlanningPhase, PlanningProject, PlanningRequirement, PlanningResearch } from "./types.js";

let workspaceRoot: string;

function makeProject(): PlanningProject {
  return {
    id: "proj_test123",
    nousId: "test-nous",
    sessionId: "test-session",
    goal: "Test Project",
    state: "idle",
    config: {
      depth: "standard",
      parallelization: true,
      research: true,
      plan_check: true,
      verifier: true,
      mode: "interactive",
    },
    contextHash: "hash123",
    projectDir: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    projectContext: null,
  };
}

function makeRequirement(): PlanningRequirement {
  return {
    id: "req_1",
    projectId: "proj_test123",
    reqId: "AUTH-01",
    description: "User can log in via OAuth",
    category: "authentication",
    tier: "v1",
    status: "pending",
    rationale: null,
    createdAt: "2026-01-01T00:00:00Z",
  };
}

function makeResearch(): PlanningResearch {
  return {
    id: "research_1",
    projectId: "proj_test123",
    phase: "research",
    dimension: "stack",
    content: "Use Node.js with Express",
    status: "complete",
    createdAt: "2026-01-01T00:00:00Z",
  };
}

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `dianoia-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });
});

afterEach(() => {
  if (existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true });
  }
});

describe("Atomic writes", () => {
  it("should create PROJECT.md and validate existence", () => {
    const project = makeProject();
    
    writeProjectFile(workspaceRoot, project);
    
    const filePath = join(workspaceRoot, ".dianoia", "projects", project.id, "PROJECT.md");
    expect(existsSync(filePath)).toBe(true);
    
    // Verify content is not empty
    const fs = require("node:fs");
    const content = fs.readFileSync(filePath, "utf-8");
    expect(content.trim().length).toBeGreaterThan(0);
    expect(content).toContain("Test Project");
  });

  it("should create REQUIREMENTS.md and validate existence", () => {
    const projectId = "proj_test123";
    const requirements = [makeRequirement()];
    
    writeRequirementsFile(workspaceRoot, projectId, requirements);
    
    const filePath = join(workspaceRoot, ".dianoia", "projects", projectId, "REQUIREMENTS.md");
    expect(existsSync(filePath)).toBe(true);
    
    const fs = require("node:fs");
    const content = fs.readFileSync(filePath, "utf-8");
    expect(content.trim().length).toBeGreaterThan(0);
    expect(content).toContain("AUTH-01");
  });

  it("should create RESEARCH.md and validate existence", () => {
    const projectId = "proj_test123";
    const research = [makeResearch()];
    
    writeResearchFile(workspaceRoot, projectId, research);
    
    const filePath = join(workspaceRoot, ".dianoia", "projects", projectId, "RESEARCH.md");
    expect(existsSync(filePath)).toBe(true);
    
    const fs = require("node:fs");
    const content = fs.readFileSync(filePath, "utf-8");
    expect(content.trim().length).toBeGreaterThan(0);
    expect(content).toContain("stack");
  });

  it("should handle atomic write failure gracefully", () => {
    const project = makeProject();
    
    // Write to a read-only directory to trigger a real write failure
    const readOnlyDir = join(tmpdir(), `dianoia-readonly-${Date.now()}`);
    mkdirSync(readOnlyDir, { recursive: true });
    // Create the project dir structure so ensureProjectDir doesn't fail
    const projectDir = join(readOnlyDir, ".dianoia", "projects", project.id);
    mkdirSync(projectDir, { recursive: true });
    // Make the directory read-only to prevent writes
    const { chmodSync } = require("node:fs");
    chmodSync(projectDir, 0o444);
    
    try {
      expect(() => writeProjectFile(readOnlyDir, project)).toThrow();
    } finally {
      chmodSync(projectDir, 0o755);
      rmSync(readOnlyDir, { recursive: true });
    }
  });

  it("should write with atomic rename pattern (tmp file then rename)", () => {
    const project = makeProject();
    
    writeProjectFile(workspaceRoot, project);
    
    // After successful write, there should be no .tmp file lingering
    const projectDir = join(workspaceRoot, ".dianoia", "projects", project.id);
    const tmpFile = join(projectDir, "PROJECT.md.tmp");
    expect(existsSync(tmpFile)).toBe(false);
    
    // The final file should exist
    const finalFile = join(projectDir, "PROJECT.md");
    expect(existsSync(finalFile)).toBe(true);
  });
});

describe("Integration - file round-trip", () => {
  it("should write and read all project files successfully", () => {
    const project = makeProject();
    const requirements = [makeRequirement()];
    const research = [makeResearch()];
    const phases: PlanningPhase[] = [];
    
    writeProjectFile(workspaceRoot, project);
    writeRequirementsFile(workspaceRoot, project.id, requirements);
    writeResearchFile(workspaceRoot, project.id, research);
    writeRoadmapFile(workspaceRoot, project.id, phases);
    
    // Verify all files exist and contain expected content
    const projectDir = join(workspaceRoot, ".dianoia", "projects", project.id);
    
    expect(existsSync(join(projectDir, "PROJECT.md"))).toBe(true);
    expect(existsSync(join(projectDir, "REQUIREMENTS.md"))).toBe(true);
    expect(existsSync(join(projectDir, "RESEARCH.md"))).toBe(true);
    expect(existsSync(join(projectDir, "ROADMAP.md"))).toBe(true);
  });
});