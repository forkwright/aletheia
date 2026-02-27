// Tests for Handoff — .continue-here.md session survival files (ENG-12)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import Database from "better-sqlite3";
import {
  writeHandoffFile,
  readHandoffFile,
  clearHandoffFile,
  discoverHandoffs,
  buildHandoffState,
  type HandoffState,
} from "./handoff.js";
import { PlanningStore } from "./store.js";
import { getProjectDir, ensureProjectDir } from "./project-files.js";
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
  const dir = join(tmpdir(), `dianoia-handoff-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function createTestHandoffState(overrides?: Partial<HandoffState>): HandoffState {
  return {
    projectId: "proj_test123",
    projectGoal: "Build a test system",
    phaseId: "phase_abc",
    phaseName: "Authentication",
    phaseGoal: "Implement user auth",
    currentWave: 1,
    totalWaves: 4,
    currentTaskId: "task-3",
    currentTaskLabel: "Task 3/7: Implement auth routes",
    completedTaskIds: ["task-1", "task-2"],
    pendingTaskIds: ["task-3", "task-4", "task-5"],
    pauseReason: "manual",
    pauseDetail: "User requested pause",
    lastCommitHash: "abc123def",
    uncommittedChanges: ["src/auth.ts"],
    resumeAction: "Resume execution from current wave",
    resumeContext: "Execution was manually paused.",
    blockers: [],
    createdAt: "2026-02-26T22:00:00.000Z",
    ...overrides,
  };
}

describe("Handoff", () => {
  let workspace: string;

  beforeEach(() => {
    workspace = createTempWorkspace();
  });

  afterEach(() => {
    try { rmSync(workspace, { recursive: true, force: true }); } catch { /* ignore */ }
  });

  describe("writeHandoffFile", () => {
    it("creates .continue-here.md in project directory", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      const filePath = writeHandoffFile(projectDirValue, state);

      expect(existsSync(filePath)).toBe(true);
      expect(filePath).toContain(".continue-here.md");
    });

    it("includes project and phase info in markdown", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      expect(content).toContain("# Continue Here");
      expect(content).toContain("Build a test system");
      expect(content).toContain("Authentication");
      expect(content).toContain("Implement user auth");
    });

    it("includes task progress", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      expect(content).toContain("task-1");
      expect(content).toContain("task-2");
      expect(content).toContain("Completed (2)");
      expect(content).toContain("Pending (3)");
    });

    it("includes pause reason and detail", () => {
      const state = createTestHandoffState({
        pauseReason: "checkpoint",
        pauseDetail: "Security review required",
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      expect(content).toContain("checkpoint");
      expect(content).toContain("Security review required");
    });

    it("includes git state", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      expect(content).toContain("abc123def");
      expect(content).toContain("src/auth.ts");
    });

    it("includes blockers section when blockers exist", () => {
      const state = createTestHandoffState({
        blockers: ["Checkpoint requires human resolution", "API key expired"],
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      expect(content).toContain("Blockers");
      expect(content).toContain("Checkpoint requires human resolution");
      expect(content).toContain("API key expired");
    });

    it("embeds full JSON state at end of file", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);

      const projectDir = getProjectDir(projectDirValue);
      const content = readFileSync(join(projectDir, ".continue-here.md"), "utf-8");

      // Extract JSON block
      const jsonMatch = content.match(/```json\n([\s\S]+?)\n```/);
      expect(jsonMatch).not.toBeNull();

      const parsed = JSON.parse(jsonMatch![1]!);
      expect(parsed.projectId).toBe(state.projectId);
      expect(parsed.phaseId).toBe(state.phaseId);
      expect(parsed.pauseReason).toBe(state.pauseReason);
    });
  });

  describe("readHandoffFile", () => {
    it("reads and parses a written handoff file", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);
      writeHandoffFile(projectDirValue, state);

      const read = readHandoffFile(projectDirValue);

      expect(read).not.toBeNull();
      expect(read!.projectId).toBe(state.projectId);
      expect(read!.phaseId).toBe(state.phaseId);
      expect(read!.phaseName).toBe("Authentication");
      expect(read!.pauseReason).toBe("manual");
      expect(read!.completedTaskIds).toEqual(["task-1", "task-2"]);
      expect(read!.pendingTaskIds).toEqual(["task-3", "task-4", "task-5"]);
    });

    it("returns null when no handoff file exists", () => {
      const projectDirValue = join(workspace, ".dianoia", "projects", "proj_nonexistent");
      const result = readHandoffFile(projectDirValue);
      expect(result).toBeNull();
    });

    it("returns null for corrupt handoff file", () => {
      const projectId = "proj_corrupt";
      const projectDirValue = join(workspace, ".dianoia", "projects", projectId);
      const projectDir = getProjectDir(projectDirValue);
      mkdirSync(projectDir, { recursive: true });

      const { writeFileSync: writeFs } = require("node:fs");
      writeFs(join(projectDir, ".continue-here.md"), "# Continue Here\n\nNo JSON block here.");

      const result = readHandoffFile(projectDirValue);
      expect(result).toBeNull();
    });

    it("returns null for handoff file with invalid JSON", () => {
      const projectId = "proj_badjson";
      const projectDirValue = join(workspace, ".dianoia", "projects", projectId);
      const projectDir = getProjectDir(projectDirValue);
      mkdirSync(projectDir, { recursive: true });

      const { writeFileSync: writeFs } = require("node:fs");
      writeFs(join(projectDir, ".continue-here.md"), "# Continue Here\n\n```json\n{invalid json}\n```");

      const result = readHandoffFile(projectDirValue);
      expect(result).toBeNull();
    });
  });

  describe("clearHandoffFile", () => {
    it("removes handoff file and returns true", () => {
      const state = createTestHandoffState();
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);
      writeHandoffFile(projectDirValue, state);

      const cleared = clearHandoffFile(projectDirValue);

      expect(cleared).toBe(true);
      expect(readHandoffFile(projectDirValue)).toBeNull();
    });

    it("returns false when no handoff file exists", () => {
      const projectDirValue = join(workspace, ".dianoia", "projects", "proj_nothing");
      const cleared = clearHandoffFile(projectDirValue);
      expect(cleared).toBe(false);
    });
  });

  describe("discoverHandoffs", () => {
    it("finds all projects with handoff files", () => {
      const state1 = createTestHandoffState({ projectId: "proj_one" });
      const state2 = createTestHandoffState({ projectId: "proj_two", pauseReason: "checkpoint" });

      const dir1 = join(workspace, ".dianoia", "projects", state1.projectId);
      const dir2 = join(workspace, ".dianoia", "projects", state2.projectId);
      const dir3 = join(workspace, ".dianoia", "projects", "proj_three");

      ensureProjectDir(dir1);
      ensureProjectDir(dir2);

      writeHandoffFile(dir1, state1);
      writeHandoffFile(dir2, state2);

      // Also create a project WITHOUT a handoff file
      ensureProjectDir(dir3);

      const handoffs = discoverHandoffs(workspace);

      expect(handoffs).toHaveLength(2);
      const ids = handoffs.map((h) => h.projectId);
      expect(ids).toContain("proj_one");
      expect(ids).toContain("proj_two");
    });

    it("returns empty array when no handoffs exist", () => {
      const handoffs = discoverHandoffs(workspace);
      expect(handoffs).toHaveLength(0);
    });

    it("skips non-project directories", () => {
      const projectsDir = join(workspace, ".dianoia", "projects");
      mkdirSync(projectsDir, { recursive: true });
      mkdirSync(join(projectsDir, "not-a-project"), { recursive: true });

      const handoffs = discoverHandoffs(workspace);
      expect(handoffs).toHaveLength(0);
    });
  });

  describe("buildHandoffState", () => {
    let db: Database.Database;
    let store: PlanningStore;

    beforeEach(() => {
      db = createTestDb();
      store = new PlanningStore(db);
    });

    afterEach(() => {
      db.close();
    });

    it("builds correct handoff state for manual pause", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Build auth",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Auth Phase",
        goal: "Implement auth",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      const state = buildHandoffState({
        store,
        project: store.getProjectOrThrow(project.id),
        phase: store.getPhaseOrThrow(phase.id),
        currentWave: 0,
        totalWaves: 3,
        pauseReason: "manual",
        pauseDetail: "User paused",
      });

      expect(state.projectId).toBe(project.id);
      expect(state.phaseName).toBe("Auth Phase");
      expect(state.pauseReason).toBe("manual");
      expect(state.resumeAction).toContain("Resume");
      expect(state.blockers).toHaveLength(0);
    });

    it("adds checkpoint blocker for checkpoint pause", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Checkpoint test",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Deploy Phase",
        goal: "Deploy",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      const state = buildHandoffState({
        store,
        project: store.getProjectOrThrow(project.id),
        phase: store.getPhaseOrThrow(phase.id),
        currentWave: 2,
        totalWaves: 4,
        pauseReason: "checkpoint",
        pauseDetail: "Security review needed",
      });

      expect(state.blockers.length).toBeGreaterThan(0);
      expect(state.blockers[0]).toContain("Checkpoint");
    });

    it("adds uncommitted changes blocker", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Uncommitted test",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Code Phase",
        goal: "Write code",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      const state = buildHandoffState({
        store,
        project: store.getProjectOrThrow(project.id),
        phase: store.getPhaseOrThrow(phase.id),
        currentWave: 1,
        totalWaves: 3,
        pauseReason: "distillation",
        pauseDetail: "Context compacted",
        uncommittedChanges: ["src/foo.ts", "src/bar.ts"],
      });

      expect(state.blockers.length).toBeGreaterThan(0);
      expect(state.blockers.some((b) => b.includes("uncommitted"))).toBe(true);
    });

    it("sets correct resume context per pause reason", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Resume test",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Test Phase",
        goal: "Test",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      const reasons: Array<HandoffState["pauseReason"]> = ["manual", "checkpoint", "crash", "distillation", "timeout", "error"];

      for (const reason of reasons) {
        const state = buildHandoffState({
          store,
          project: store.getProjectOrThrow(project.id),
          phase: store.getPhaseOrThrow(phase.id),
          currentWave: 0,
          totalWaves: 1,
          pauseReason: reason,
          pauseDetail: `Testing ${reason}`,
        });
        expect(state.resumeAction.length).toBeGreaterThan(0);
        expect(state.resumeContext.length).toBeGreaterThan(0);
      }
    });

    it("includes task IDs when provided", () => {
      const project = store.createProject({
        nousId: "test",
        sessionId: "test",
        goal: "Task tracking",
        config: {},
      });
      store.updateProjectState(project.id, "roadmap");
      const phase = store.createPhase({
        projectId: project.id,
        name: "Task Phase",
        goal: "Do tasks",
        requirements: [],
        successCriteria: [],
        phaseOrder: 0,
      });

      const state = buildHandoffState({
        store,
        project: store.getProjectOrThrow(project.id),
        phase: store.getPhaseOrThrow(phase.id),
        currentWave: 0,
        totalWaves: 1,
        currentTaskId: "task-42",
        currentTaskLabel: "Implement the widget",
        completedTaskIds: ["task-1", "task-2"],
        pendingTaskIds: ["task-42", "task-43"],
        pauseReason: "manual",
        pauseDetail: "paused",
      });

      expect(state.currentTaskId).toBe("task-42");
      expect(state.currentTaskLabel).toBe("Implement the widget");
      expect(state.completedTaskIds).toEqual(["task-1", "task-2"]);
      expect(state.pendingTaskIds).toEqual(["task-42", "task-43"]);
    });
  });

  describe("roundtrip: write → read → clear", () => {
    it("preserves all fields through write/read cycle", () => {
      const state = createTestHandoffState({
        blockers: ["Need API key"],
        uncommittedChanges: ["a.ts", "b.ts", "c.ts"],
      });
      const projectDirValue = join(workspace, ".dianoia", "projects", state.projectId);
      ensureProjectDir(projectDirValue);

      writeHandoffFile(projectDirValue, state);
      const read = readHandoffFile(projectDirValue);

      expect(read).not.toBeNull();
      expect(read!.projectId).toBe(state.projectId);
      expect(read!.projectGoal).toBe(state.projectGoal);
      expect(read!.phaseId).toBe(state.phaseId);
      expect(read!.phaseName).toBe(state.phaseName);
      expect(read!.phaseGoal).toBe(state.phaseGoal);
      expect(read!.currentWave).toBe(state.currentWave);
      expect(read!.totalWaves).toBe(state.totalWaves);
      expect(read!.currentTaskId).toBe(state.currentTaskId);
      expect(read!.currentTaskLabel).toBe(state.currentTaskLabel);
      expect(read!.completedTaskIds).toEqual(state.completedTaskIds);
      expect(read!.pendingTaskIds).toEqual(state.pendingTaskIds);
      expect(read!.pauseReason).toBe(state.pauseReason);
      expect(read!.pauseDetail).toBe(state.pauseDetail);
      expect(read!.lastCommitHash).toBe(state.lastCommitHash);
      expect(read!.uncommittedChanges).toEqual(state.uncommittedChanges);
      expect(read!.resumeAction).toBe(state.resumeAction);
      expect(read!.resumeContext).toBe(state.resumeContext);
      expect(read!.blockers).toEqual(state.blockers);

      // Clear and verify gone
      const cleared = clearHandoffFile(projectDirValue);
      expect(cleared).toBe(true);
      expect(readHandoffFile(projectDirValue)).toBeNull();
    });
  });
});
