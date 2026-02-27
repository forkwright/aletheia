// Tests for ContextBudget — orchestrator token ceiling enforcement (ENG-08)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import Database from "better-sqlite3";
import {
  calculateBudgetAllocation,
  buildOrchestratorContext,
  checkBudget,
  DEFAULT_ORCHESTRATOR_CEILING,
} from "./context-budget.js";
import { writeHandoffFile } from "./handoff.js";
import { PlanningStore } from "./store.js";
import {
  writeProjectFile,
  writeRoadmapFile,
  ensureProjectDir,
} from "./project-files.js";
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
import type { PlanningProject, PlanningPhase } from "./types.js";

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
  const dir = join(tmpdir(), `dianoia-budget-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

describe("ContextBudget", () => {
  let db: Database.Database;
  let workspace: string;
  let store: PlanningStore;

  beforeEach(() => {
    db = createTestDb();
    workspace = createTempWorkspace();
    store = new PlanningStore(db);
  });

  afterEach(() => {
    db.close();
    try { rmSync(workspace, { recursive: true, force: true }); } catch { /* ignore */ }
  });

  function createProjectWithFiles(): { project: PlanningProject; phases: PlanningPhase[]; projectDirValue: string } {
    const project = store.createProject({
      nousId: "test",
      sessionId: "test",
      goal: "Build a collaborative planning system",
      config: {},
    });
    store.updateProjectState(project.id, "roadmap");

    const phase1 = store.createPhase({
      projectId: project.id,
      name: "Foundation",
      goal: "Set up state persistence",
      requirements: ["ENG-01"],
      successCriteria: ["DB and files co-primary"],
      phaseOrder: 0,
    });
    const phase2 = store.createPhase({
      projectId: project.id,
      name: "Execution",
      goal: "Fix sub-agent pipeline",
      requirements: ["ENG-03"],
      successCriteria: ["Sub-agents write code"],
      phaseOrder: 1,
    });

    // Set projectDir so file functions know where to write
    const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
    store.updateProjectDir(project.id, projectDirValue);

    const projectObj = store.getProjectOrThrow(project.id);
    const phases = store.listPhases(project.id);

    // Write files using the new single-arg projectDirValue API
    writeProjectFile(projectObj, null);
    writeRoadmapFile(projectDirValue, phases);

    return { project: projectObj, phases, projectDirValue };
  }

  describe("calculateBudgetAllocation", () => {
    it("returns budget breakdown for a project", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const budget = calculateBudgetAllocation({
        projectDirValue,
        project,
        phases,
      });

      expect(budget.totalBudget).toBe(DEFAULT_ORCHESTRATOR_CEILING);
      expect(budget.projectTokens).toBeGreaterThan(0);
      expect(budget.roadmapTokens).toBeGreaterThan(0);
      expect(budget.phaseStatusTokens).toBeGreaterThan(0);
      expect(budget.totalConsumed).toBe(
        budget.projectTokens + budget.roadmapTokens + budget.phaseStatusTokens + budget.handoffTokens,
      );
      expect(budget.remaining).toBe(budget.totalBudget - budget.totalConsumed);
      expect(budget.withinBudget).toBe(true);
    });

    it("reports zero tokens when no files exist", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Empty project",
        config: {},
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      const projectObj = store.getProjectOrThrow(project.id);

      const budget = calculateBudgetAllocation({
        projectDirValue,
        project: projectObj,
        phases: [],
      });

      expect(budget.projectTokens).toBe(0);
      expect(budget.roadmapTokens).toBe(0);
      expect(budget.phaseStatusTokens).toBeGreaterThan(0); // "(no phases)" still has tokens
    });

    it("respects custom ceiling", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const budget = calculateBudgetAllocation({
        projectDirValue,
        project,
        phases,
        ceiling: 10_000,
      });

      expect(budget.totalBudget).toBe(10_000);
    });

    it("warns when budget is over ceiling", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      // Use a ceiling that's too small for the content (under 8000 reserve)
      const budget = calculateBudgetAllocation({
        projectDirValue,
        project,
        phases,
        ceiling: 100, // Way too small — exceeds safe ceiling
      });

      expect(budget.withinBudget).toBe(false);
      expect(budget.warning).not.toBeNull();
      expect(budget.warning).toContain("exceeds");
    });

    it("detects over-budget when ceiling is too small", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const budget = calculateBudgetAllocation({
        projectDirValue,
        project,
        phases,
        ceiling: 50, // Way too small
      });

      expect(budget.withinBudget).toBe(false);
      expect(budget.warning).not.toBeNull();
    });

    it("includes handoff tokens when handoff file exists", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      // Write a handoff file
      ensureProjectDir(projectDirValue);
      writeHandoffFile(projectDirValue, {
        projectId: project.id,
        projectGoal: project.goal,
        phaseId: phases[0]!.id,
        phaseName: phases[0]!.name,
        phaseGoal: phases[0]!.goal,
        currentWave: 0,
        totalWaves: 2,
        currentTaskId: null,
        currentTaskLabel: null,
        completedTaskIds: [],
        pendingTaskIds: [],
        pauseReason: "manual",
        pauseDetail: "test",
        lastCommitHash: null,
        uncommittedChanges: [],
        resumeAction: "Resume",
        resumeContext: "Test resume",
        blockers: [],
        createdAt: new Date().toISOString(),
      });

      const budget = calculateBudgetAllocation({
        projectDirValue,
        project,
        phases,
      });

      expect(budget.handoffTokens).toBeGreaterThan(0);
    });
  });

  describe("buildOrchestratorContext", () => {
    it("builds context string with project and phases", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const { context, budget } = buildOrchestratorContext({
        projectDirValue,
        project,
        phases,
      });

      expect(context).toContain("Build a collaborative planning system");
      expect(context).toContain("Foundation");
      expect(context).toContain("Execution");
      expect(budget.withinBudget).toBe(true);
    });

    it("uses compact format when roadmap is large", () => {
      // Create many phases to make roadmap large
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Large project",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");

      for (let i = 0; i < 20; i++) {
        store.createPhase({
          projectId: project.id,
          name: `Phase ${i + 1}: ${"A very long phase name that takes up lots of tokens ".repeat(3)}`,
          goal: `Goal for phase ${i + 1}: ${"This is a detailed goal description ".repeat(5)}`,
          requirements: [`REQ-${String(i).padStart(2, "0")}`],
          successCriteria: ["Criterion 1", "Criterion 2", "Criterion 3"],
          phaseOrder: i,
        });
      }

      const projectDirValue = join(workspace, ".dianoia", "projects", project.id);
      store.updateProjectDir(project.id, projectDirValue);
      const projectObj = store.getProjectOrThrow(project.id);
      const phases = store.listPhases(project.id);

      writeProjectFile(projectObj, null);
      writeRoadmapFile(projectDirValue, phases);

      // Use small ceiling to force compact format
      const { context } = buildOrchestratorContext({
        projectDirValue,
        project: projectObj,
        phases,
        ceiling: 5000,
      });

      // Should still include some phase info even if compact
      expect(context).toContain("Phase 1");
    });

    it("includes active phase section when phases are executing", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      // Mark first phase as executing
      store.updatePhaseStatus(phases[0]!.id, "executing");
      const updatedPhases = store.listPhases(project.id);

      const { context } = buildOrchestratorContext({
        projectDirValue,
        project,
        phases: updatedPhases,
      });

      expect(context).toContain("Active");
      expect(context).toContain("🔄");
    });

    it("includes resume context when handoff exists", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      writeHandoffFile(projectDirValue, {
        projectId: project.id,
        projectGoal: project.goal,
        phaseId: phases[0]!.id,
        phaseName: phases[0]!.name,
        phaseGoal: phases[0]!.goal,
        currentWave: 0,
        totalWaves: 2,
        currentTaskId: null,
        currentTaskLabel: null,
        completedTaskIds: [],
        pendingTaskIds: [],
        pauseReason: "distillation",
        pauseDetail: "Context compacted",
        lastCommitHash: null,
        uncommittedChanges: [],
        resumeAction: "Resume execution",
        resumeContext: "Pick up where we left off",
        blockers: [],
        createdAt: new Date().toISOString(),
      });

      const { context } = buildOrchestratorContext({
        projectDirValue,
        project,
        phases,
      });

      expect(context).toContain("Resume Context");
      expect(context).toContain("distillation");
    });
  });

  describe("checkBudget", () => {
    it("returns true for a normal project", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const result = checkBudget({
        projectDirValue,
        project,
        phases,
      });

      expect(result).toBe(true);
    });

    it("returns false when ceiling is impossibly small", () => {
      const { project, phases, projectDirValue } = createProjectWithFiles();

      const result = checkBudget({
        projectDirValue,
        project,
        phases,
        ceiling: 10,
      });

      expect(result).toBe(false);
    });
  });

  describe("DEFAULT_ORCHESTRATOR_CEILING", () => {
    it("is 40k tokens", () => {
      expect(DEFAULT_ORCHESTRATOR_CEILING).toBe(40_000);
    });
  });
});


