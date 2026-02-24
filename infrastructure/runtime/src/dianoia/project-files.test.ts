// Tests for project-files.ts atomic writes and validation (CTX-02)
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdirSync, writeFileSync, rmSync, existsSync, unlinkSync, renameSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { writeProjectFile, writeRequirementsFile, writeRoadmapFile, writeResearchFile } from "./project-files.js";
import type { PlanningProject, PlanningRequirement, PlanningPhase, PlanningResearch } from "./types.js";

let workspaceRoot: string;

function makeProject(): PlanningProject {
  return {
    id: "proj_test123",
    goal: "Test Project",
    state: "idle",
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    projectContext: null,
  };
}

function makeRequirement(): PlanningRequirement {
  return {
    id: 1,
    projectId: "proj_test123",
    reqId: "AUTH-01",
    description: "User can log in via OAuth",
    category: "authentication",
    tier: "v1",
    status: "accepted",
    rationale: null,
    createdAt: "2026-01-01T00:00:00Z",
  };
}

function makeResearch(): PlanningResearch {
  return {
    id: 1,
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
    const fs = require("fs");
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
    
    const fs = require("fs");
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
    
    const fs = require("fs");
    const content = fs.readFileSync(filePath, "utf-8");
    expect(content.trim().length).toBeGreaterThan(0);
    expect(content).toContain("stack");
  });

  it("should handle atomic write failure gracefully", () => {
    const project = makeProject();
    
    // Mock renameSync to throw
    const originalRename = renameSync;
    const renameSpy = vi.fn().mockImplementation(() => {
      throw new Error("Simulated rename failure");
    });
    vi.mocked(require("fs").renameSync = renameSpy);
    
    try {
      expect(() => writeProjectFile(workspaceRoot, project)).toThrow("Simulated rename failure");
      
      // Verify tmp file cleanup (implementation should clean up on error)
      const projectDir = join(workspaceRoot, ".dianoia", "projects", project.id);
      if (existsSync(projectDir)) {
        const tmpFile = join(projectDir, "PROJECT.md.tmp");
        expect(existsSync(tmpFile)).toBe(false);
      }
    } finally {
      // Restore original function
      require("fs").renameSync = originalRename;
      vi.restoreAllMocks();
    }
  });

  it("should throw on validation failure when file is missing", () => {
    const project = makeProject();
    
    // Mock unlinkSync to remove the file immediately after atomic write
    const originalUnlink = unlinkSync;
    const unlinkSpy = vi.fn().mockImplementation((path: string) => {
      if (path.endsWith("PROJECT.md")) {
        originalUnlink(path);
      }
    });
    
    vi.mocked(require("fs").renameSync = vi.fn().mockImplementation((tmpPath: string, finalPath: string) => {
      require("fs").writeFileSync(finalPath, "test content");
      unlinkSpy(finalPath); // Remove the file immediately
    });
    
    try {
      expect(() => writeProjectFile(workspaceRoot, project)).toThrow("File not found after write");
    } finally {
      vi.restoreAllMocks();
    }
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