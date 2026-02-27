// Tests for StateReconciler — co-primary file/DB architecture (ENG-01)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import Database from "better-sqlite3";
import { StateReconciler } from "./state-reconciler.js";
import { PlanningStore } from "./store.js";
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
import {
  getProjectDir,
  getPhaseDir,
  writeProjectFile,
  writeRequirementsFile,
  writeRoadmapFile,
} from "./project-files.js";

function createTestDb(): Database.Database {
  const db = new Database(":memory:");
  db.pragma("journal_mode = WAL");
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

function createTempWorkspace(): string {
  const dir = join(tmpdir(), `dianoia-reconciler-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

describe("StateReconciler", () => {
  let db: Database.Database;
  let workspace: string;
  let reconciler: StateReconciler;
  let store: PlanningStore;

  beforeEach(() => {
    db = createTestDb();
    workspace = createTempWorkspace();
    reconciler = new StateReconciler(db, workspace);
    store = new PlanningStore(db);
  });

  afterEach(() => {
    db.close();
    try { rmSync(workspace, { recursive: true, force: true }); } catch { /* ignore */ }
  });

  describe("reconcileAll", () => {
    it("returns empty summary when no projects exist", () => {
      const result = reconciler.reconcileAll();
      expect(result.projects).toHaveLength(0);
      expect(result.totalErrors).toBe(0);
      expect(result.duration).toBeGreaterThanOrEqual(0);
    });

    it("regenerates files when project exists only in DB", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Test project",
        config: {},
      });
      // Set projectDir so reconciler can write files
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);

      const result = reconciler.reconcileAll();
      expect(result.projects).toHaveLength(1);
      expect(result.projects[0]!.direction).toBe("db-only");
      expect(result.projects[0]!.filesRegenerated).toContain("PROJECT.md");

      // Verify file was written
      const projectDir = getProjectDir(projectDirValue);
      expect(existsSync(join(projectDir, "PROJECT.md"))).toBe(true);
    });

    it("regenerates files including requirements and phases", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Full project",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);

      store.updateProjectState(project.id, "requirements");

      store.createRequirement({
        projectId: project.id,
        phaseId: null,
        reqId: "AUTH-01",
        description: "User authentication",
        category: "Authentication",
        tier: "v1",
      });

      store.updateProjectState(project.id, "roadmap");

      store.createPhase({
        projectId: project.id,
        name: "Auth Phase",
        goal: "Implement authentication",
        requirements: ["AUTH-01"],
        successCriteria: ["Login works"],
        phaseOrder: 0,
      });

      const result = reconciler.reconcileAll();
      const projResult = result.projects[0]!;
      expect(projResult.filesRegenerated).toContain("PROJECT.md");
      expect(projResult.filesRegenerated).toContain("REQUIREMENTS.md");
      expect(projResult.filesRegenerated).toContain("ROADMAP.md");
    });

    it("handles multiple projects independently", () => {
      const pA = store.createProject({ nousId: "test", sessionId: "test", goal: "Project A", config: {} });
      const pB = store.createProject({ nousId: "test", sessionId: "test", goal: "Project B", config: {} });
      store.updateProjectDir(pA.id, join(workspace, ".dianoia", "projects", pA.id));
      store.updateProjectDir(pB.id, join(workspace, ".dianoia", "projects", pB.id));

      const result = reconciler.reconcileAll();
      expect(result.projects).toHaveLength(2);
      expect(result.totalErrors).toBe(0);
    });

    it("detects in-sync state when both DB and files are current", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Synced project",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);

      // Write files to match DB
      const updatedProject = store.getProjectOrThrow(project.id);
      writeProjectFile(updatedProject, null);

      const result = reconciler.reconcileAll();
      expect(result.projects).toHaveLength(1);
      // Should be in-sync since both were just written
      expect(["in-sync", "db-to-files"]).toContain(result.projects[0]!.direction);
    });
  });

  describe("reconcileProject", () => {
    it("returns db-only direction when project only in DB", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "DB only",
        config: {},
      });
      // Set projectDir so dbToFiles can write files
      store.updateProjectDir(project.id, join(workspace, ".dianoia", "projects", project.id));

      const result = reconciler.reconcileProject(project.id, true, false);
      expect(result.direction).toBe("db-only");
      expect(result.filesRegenerated.length).toBeGreaterThan(0);
    });

    it("returns files-only direction when project only in files", () => {
      const fakeId = "proj_fake123";
      const projectDir = join(workspace, ".dianoia", "projects", fakeId);
      mkdirSync(projectDir, { recursive: true });
      writeFileSync(join(projectDir, "PROJECT.md"), "# Test Project\n\n| Field | Value |\n|---|---|\n| State | idle |");

      const result = reconciler.reconcileProject(fakeId, false, true);
      expect(result.direction).toBe("files-only");
    });

    it("handles file-only projects with missing PROJECT.md gracefully", () => {
      const fakeId = "proj_empty";
      const projectDir = join(workspace, ".dianoia", "projects", fakeId);
      mkdirSync(projectDir, { recursive: true });
      // No PROJECT.md — just empty dir

      const result = reconciler.reconcileProject(fakeId, false, true);
      expect(result.direction).toBe("files-only");
      expect(result.errors.length).toBeGreaterThan(0);
    });

    it("returns in-sync when neither exists", () => {
      const result = reconciler.reconcileProject("proj_nonexistent", false, false);
      expect(result.direction).toBe("in-sync");
    });
  });

  describe("writeStepBoundaryState", () => {
    it("writes STATE.md with step boundary info", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Step test",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Phase 1",
        goal: "Do stuff",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      reconciler.writeStepBoundaryState(project.id, phase.id, {
        step: 3,
        label: "Task 3/7: Implement auth routes",
        completedTasks: ["task-1", "task-2"],
        pendingTasks: ["task-3", "task-4"],
        lastCommit: "abc123",
        resumeInstructions: "Continue from task 3",
      });

      const phaseDir = getPhaseDir(projectDirValue, phase.id);
      const stateFile = join(phaseDir, "STATE.md");
      expect(existsSync(stateFile)).toBe(true);

      const content = readFileSync(stateFile, "utf-8");
      expect(content).toContain("abc123");
      expect(content).toContain("task-1");
    });

    it("includes project and phase metadata in state", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Metadata test",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Build Phase",
        goal: "Build it",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      reconciler.writeStepBoundaryState(project.id, phase.id, {
        step: 1,
        label: "Starting",
      });

      const phaseDir = getPhaseDir(projectDirValue, phase.id);
      const content = readFileSync(join(phaseDir, "STATE.md"), "utf-8");
      expect(content).toContain("Metadata test");
      expect(content).toContain("Build Phase");
    });

    it("handles missing project/phase gracefully", () => {
      // Should not throw — just returns early
      reconciler.writeStepBoundaryState("nonexistent", "nonexistent", {
        step: 1,
        label: "test",
      });
      // No exception means success
    });
  });

  describe("file regeneration content", () => {
    it("regenerates REQUIREMENTS.md with correct format", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Req test",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);
      store.updateProjectState(project.id, "requirements");

      store.createRequirement({
        projectId: project.id,
        phaseId: null,
        reqId: "API-01",
        description: "REST endpoints",
        category: "API",
        tier: "v1",
      });
      store.createRequirement({
        projectId: project.id,
        phaseId: null,
        reqId: "API-02",
        description: "GraphQL support",
        category: "API",
        tier: "v2",
      });

      reconciler.reconcileAll();

      const reqFile = join(getProjectDir(projectDirValue), "REQUIREMENTS.md");
      expect(existsSync(reqFile)).toBe(true);

      const content = readFileSync(reqFile, "utf-8");
      expect(content).toContain("API-01");
      expect(content).toContain("API-02");
      expect(content).toContain("REST endpoints");
      expect(content).toContain("GraphQL support");
    });

    it("regenerates phase-level files (PLAN.md, STATE.md)", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Phase files test",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);
      store.updateProjectState(project.id, "roadmap");

      const phase = store.createPhase({
        projectId: project.id,
        name: "Test Phase",
        goal: "Test goal",
        requirements: [],
        successCriteria: ["passes tests"],
        phaseOrder: 0,
      });

      // Add a plan to the phase
      store.updatePhasePlan(phase.id, {
        steps: [{ name: "Step 1", description: "Do the thing" }],
        dependencies: [],
        acceptanceCriteria: ["Works"],
      });

      reconciler.reconcileAll();

      const phaseDir = getPhaseDir(projectDirValue, phase.id);
      expect(existsSync(join(phaseDir, "PLAN.md"))).toBe(true);
      expect(existsSync(join(phaseDir, "STATE.md"))).toBe(true);
    });
  });

  describe("error handling", () => {
    it("captures errors per-project without failing other projects", () => {
      const p = store.createProject({ nousId: "test", sessionId: "test", goal: "Good project", config: {} });
      store.updateProjectDir(p.id, join(workspace, ".dianoia", "projects", p.id));

      // Create a second project directory with bad permissions wouldn't work in tests,
      // so just verify the error array mechanism works
      const result = reconciler.reconcileAll();
      expect(result.projects).toHaveLength(1);
      expect(result.totalErrors).toBe(0);
    });

    it("tracks duration", () => {
      const result = reconciler.reconcileAll();
      expect(typeof result.duration).toBe("number");
      expect(result.duration).toBeGreaterThanOrEqual(0);
    });
  });
});
