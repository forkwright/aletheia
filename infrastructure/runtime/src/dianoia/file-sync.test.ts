// file-sync.test.ts — tests for FileSyncDaemon event-driven file synchronization
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import Database from "better-sqlite3";
import { mkdtempSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { FileSyncDaemon } from "./file-sync.js";
import { PlanningStore } from "./store.js";
import { eventBus } from "../koina/event-bus.js";
import { getProjectDir, getPhaseDir } from "./project-files.js";

import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
  PLANNING_V27_MIGRATION,
  PLANNING_V28_MIGRATION,
  PLANNING_V29_MIGRATION,
  PLANNING_V31_MIGRATION,
} from "./schema.js";

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
  db.exec(PLANNING_V28_MIGRATION);
  db.exec(PLANNING_V29_MIGRATION);
  db.exec(PLANNING_V31_MIGRATION);
  return db;
}

const DEFAULT_CONFIG = {
  mode: "interactive" as const,
  depth: "standard" as const,
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  pause_between_phases: false,
};

/** Compute the absolute projectDirValue for a project in the test workspace */
function projectDirFor(workspaceRoot: string, projectId: string): string {
  return join(workspaceRoot, ".dianoia", "projects", projectId);
}

describe("FileSyncDaemon", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let daemon: FileSyncDaemon;
  let workspaceRoot: string;

  beforeEach(() => {
    db = makeDb();
    store = new PlanningStore(db);
    daemon = new FileSyncDaemon(db);
    workspaceRoot = mkdtempSync(join(tmpdir(), "dianoia-sync-"));
    daemon.start();
  });

  afterEach(() => {
    daemon.stop();
    db.close();
    try {
      rmSync(workspaceRoot, { recursive: true, force: true });
    } catch { /* cleanup best-effort */ }
  });

  it("writes PROJECT.md on project-created event", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Build a widget",
      config: DEFAULT_CONFIG,
    });

    // Set projectDir so the daemon can write files
    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    eventBus.emit("planning:project-created", { projectId: project.id, nousId: "syn", sessionId: "test" });

    const projectFile = join(getProjectDir(projectDir), "PROJECT.md");
    expect(existsSync(projectFile)).toBe(true);

    const content = readFileSync(projectFile, "utf-8");
    expect(content).toContain("Build a widget");
    expect(content).toContain(project.id);
  });

  it("writes REQUIREMENTS.md on requirement-changed event", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Test requirements sync",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    store.createRequirement({
      projectId: project.id,
      reqId: "AUTH-01",
      category: "Authentication",
      description: "Users can log in with email/password",
      tier: "v1",
      rationale: "Core feature",
    });

    eventBus.emit("planning:requirement-changed", {
      projectId: project.id,
      action: "persisted",
    });

    const reqFile = join(getProjectDir(projectDir), "REQUIREMENTS.md");
    expect(existsSync(reqFile)).toBe(true);

    const content = readFileSync(reqFile, "utf-8");
    expect(content).toContain("AUTH-01");
    expect(content).toContain("Users can log in with email/password");
  });

  it("writes ROADMAP.md on phase-changed event", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Test roadmap sync",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    store.createPhase({
      projectId: project.id,
      name: "Foundation",
      goal: "Set up project structure",
      requirements: ["CORE-01"],
      successCriteria: ["Project compiles"],
      phaseOrder: 0,
    });

    eventBus.emit("planning:phase-changed", {
      projectId: project.id,
      action: "created",
    });

    const roadmapFile = join(getProjectDir(projectDir), "ROADMAP.md");
    expect(existsSync(roadmapFile)).toBe(true);

    const content = readFileSync(roadmapFile, "utf-8");
    expect(content).toContain("Foundation");
    expect(content).toContain("Set up project structure");
  });

  it("writes DISCUSS.md on discussion-answered event", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Test discussion sync",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    const phase = store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "First phase",
      requirements: [],
      successCriteria: [],
      phaseOrder: 0,
    });

    const question = store.createDiscussionQuestion({
      projectId: project.id,
      phaseId: phase.id,
      question: "Should we use React or Svelte?",
      options: [
        { label: "React", rationale: "Larger ecosystem" },
        { label: "Svelte", rationale: "Simpler, faster" },
      ],
      recommendation: "Svelte",
    });

    store.answerDiscussionQuestion(question.id, "Svelte", "Aligns with existing UI");

    eventBus.emit("planning:discussion-answered", {
      projectId: project.id,
      phaseId: phase.id,
      questionId: question.id,
    });

    const phaseDir = getPhaseDir(projectDir, phase.id);
    const discussFile = join(phaseDir, "DISCUSS.md");
    expect(existsSync(discussFile)).toBe(true);

    const content = readFileSync(discussFile, "utf-8");
    expect(content).toContain("Should we use React or Svelte?");
    expect(content).toContain("Svelte");
    expect(content).toContain("selected");
  });

  it("does full sync on project completion", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Full sync test",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    store.createRequirement({
      projectId: project.id,
      reqId: "CORE-01",
      category: "Core",
      description: "Render widgets correctly",
      tier: "v1",
      rationale: "Essential",
    });

    store.createPhase({
      projectId: project.id,
      name: "Build",
      goal: "Build the thing",
      requirements: ["CORE-01"],
      successCriteria: ["Tests pass"],
      phaseOrder: 0,
    });

    eventBus.emit("planning:complete", {
      projectId: project.id,
      nousId: "syn",
      sessionId: "test",
    });

    const projDir = getProjectDir(projectDir);
    expect(existsSync(join(projDir, "PROJECT.md"))).toBe(true);
    expect(existsSync(join(projDir, "REQUIREMENTS.md"))).toBe(true);
    expect(existsSync(join(projDir, "ROADMAP.md"))).toBe(true);
  });

  it("syncs PROJECT.md on state transitions", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "State transition test",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    store.updateProjectState(project.id, "researching");

    eventBus.emit("planning:state-transition", {
      projectId: project.id,
      from: "questioning",
      to: "researching",
    });

    const content = readFileSync(join(getProjectDir(projectDir), "PROJECT.md"), "utf-8");
    expect(content).toContain("researching");
  });

  it("reports stats correctly", () => {
    const project = store.createProject({
      nousId: "syn",
      sessionId: "test",
      goal: "Stats test",
      config: DEFAULT_CONFIG,
    });

    const projectDir = projectDirFor(workspaceRoot, project.id);
    store.updateProjectDir(project.id, projectDir);

    eventBus.emit("planning:project-created", { projectId: project.id });

    const stats = daemon.stats();
    expect(stats.active).toBe(true);
    expect(stats.writes).toBeGreaterThanOrEqual(1);
    expect(stats.errors).toBe(0);
  });

  it("handles missing project gracefully (no crash)", () => {
    // Emit event for a project that doesn't exist in the DB
    eventBus.emit("planning:project-created", { projectId: "proj_nonexistent" });

    // Should not throw — daemon catches errors
    const stats = daemon.stats();
    expect(stats.active).toBe(true);
    // No writes (project not found), no errors (graceful return)
    expect(stats.errors).toBe(0);
  });

  it("cleans up listeners on stop", () => {
    daemon.stop();

    const stats = daemon.stats();
    expect(stats.active).toBe(false);
  });
});
