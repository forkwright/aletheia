// Integration test for Context & State Foundation (Spec 32 CTX-S5)
// Tests the full pipeline using the real DianoiaOrchestrator API

import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, rmSync, existsSync } from "node:fs";
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
import { DianoiaOrchestrator } from "./orchestrator.js";
import { RequirementsOrchestrator } from "./requirements.js";
import { PlanningStore } from "./store.js";
import { buildContextPacketWithPriompt } from "./priompt-context.js";
import {
  readProjectFile,
  readResearchFile,
  readRequirementsFile,
} from "./project-files.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
};

const NOUS_ID = "test-nous";
const SESSION_ID = "test-session";

let workspaceRoot: string;
let db: Database.Database;
let orchestrator: DianoiaOrchestrator;
let store: PlanningStore;
let requirementsOrch: RequirementsOrchestrator;

function makeDb(): Database.Database {
  const d = new Database(":memory:");
  d.exec(PLANNING_V20_DDL);
  d.exec(PLANNING_V21_MIGRATION);
  d.exec(PLANNING_V22_MIGRATION);
  d.exec(PLANNING_V23_MIGRATION);
  d.exec(PLANNING_V24_MIGRATION);
  d.exec(PLANNING_V25_MIGRATION);
  d.exec(PLANNING_V26_MIGRATION);
  d.exec(PLANNING_V27_MIGRATION);
  return d;
}

beforeEach(() => {
  workspaceRoot = join(tmpdir(), `ctx-foundation-test-${Date.now()}`);
  mkdirSync(workspaceRoot, { recursive: true });

  db = makeDb();
  store = new PlanningStore(db);
  orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
  orchestrator.setWorkspaceRoot(workspaceRoot);
  requirementsOrch = new RequirementsOrchestrator(db, workspaceRoot);
});

afterEach(() => {
  if (workspaceRoot && existsSync(workspaceRoot)) {
    rmSync(workspaceRoot, { recursive: true, force: true });
  }
  db?.close();
});

describe("Context & State Foundation - Full Pipeline (CTX-S5)", () => {
  it("completes full pipeline: project creation → questioning → research → requirements → all artifacts on disk", () => {
    // Step 1: handle() creates project in questioning state
    orchestrator.handle(NOUS_ID, SESSION_ID);
    const project = orchestrator.getActiveProject(NOUS_ID)!;
    expect(project).toBeDefined();
    expect(project.state).toBe("questioning");

    // Step 2: confirmSynthesis transitions questioning → researching
    orchestrator.confirmSynthesis(project.id, NOUS_ID, SESSION_ID, {
      goal: "Build a task management application",
      coreValue: "Help users organize their work",
      constraints: ["Must work offline", "Must sync across devices"],
      keyDecisions: ["Use React Native", "Use SQLite"],
    });
    expect(store.getProjectOrThrow(project.id).state).toBe("researching");

    // Verify PROJECT.md was written
    const projectFile = readProjectFile(workspaceRoot, project.id);
    expect(projectFile).toBeTruthy();
    expect(projectFile).toContain("task management");

    // Step 3: skipResearch transitions researching → requirements
    orchestrator.skipResearch(project.id, NOUS_ID, SESSION_ID);
    expect(store.getProjectOrThrow(project.id).state).toBe("requirements");

    // Step 4: Requirements scoping
    requirementsOrch.persistCategory(project.id, {
      category: "AUTH",
      categoryName: "Authentication",
      tableStakes: [
        { name: "Email login", description: "User logs in with email", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [
        { name: "Social login", description: "Google/Facebook login", isTableStakes: false, proposedTier: "v2" },
      ],
    }, [
      { name: "Email login", tier: "v1" },
      { name: "Social login", tier: "v2" },
    ]);

    // Coverage gate passes with default minimum (1 category)
    expect(requirementsOrch.validateCoverage(project.id, ["AUTH"])).toBe(true);

    // Step 5: completeRequirements transitions requirements → roadmap
    orchestrator.completeRequirements(project.id, NOUS_ID, SESSION_ID);
    expect(store.getProjectOrThrow(project.id).state).toBe("roadmap");

    // Verify REQUIREMENTS.md was written
    const requirementsFile = readRequirementsFile(workspaceRoot, project.id);
    expect(requirementsFile).toBeTruthy();
    expect(requirementsFile).toContain("AUTH-01");
    expect(requirementsFile).toContain("AUTH");
  });

  it("enforces coverage gate: single category passes default, fails min=2", () => {
    // Create project and advance to requirements
    orchestrator.handle(NOUS_ID, SESSION_ID);
    const project = orchestrator.getActiveProject(NOUS_ID)!;
    orchestrator.confirmSynthesis(project.id, NOUS_ID, SESSION_ID, {
      goal: "Testing coverage",
    });
    orchestrator.skipResearch(project.id, NOUS_ID, SESSION_ID);

    // Add one category
    requirementsOrch.persistCategory(project.id, {
      category: "SINGLE",
      categoryName: "Single Category",
      tableStakes: [
        { name: "Basic feature", description: "One feature", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Basic feature", tier: "v1" },
    ]);

    // Passes with default minimum (1)
    expect(requirementsOrch.validateCoverage(project.id, ["SINGLE"])).toBe(true);

    // Fails when minimum set to 2
    expect(requirementsOrch.validateCoverage(project.id, ["SINGLE"], 2)).toBe(false);

    // Add second category, now passes min=2
    requirementsOrch.persistCategory(project.id, {
      category: "SECOND",
      categoryName: "Second Category",
      tableStakes: [
        { name: "Another feature", description: "Second feature", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Another feature", tier: "v1" },
    ]);

    expect(requirementsOrch.validateCoverage(project.id, ["SINGLE", "SECOND"], 2)).toBe(true);
  });

  it("builds executor context packet from completed state", async () => {
    // Set up complete project through to roadmap
    orchestrator.handle(NOUS_ID, SESSION_ID);
    const project = orchestrator.getActiveProject(NOUS_ID)!;
    orchestrator.confirmSynthesis(project.id, NOUS_ID, SESSION_ID, {
      goal: "Test context assembly",
    });
    orchestrator.skipResearch(project.id, NOUS_ID, SESSION_ID);

    requirementsOrch.persistCategory(project.id, {
      category: "CTX",
      categoryName: "Context Features",
      tableStakes: [
        { name: "Context assembly", description: "Build context packets", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Context assembly", tier: "v1" },
    ]);

    orchestrator.completeRequirements(project.id, NOUS_ID, SESSION_ID);

    // Build executor context packet
    const contextPacket = await buildContextPacketWithPriompt({
      workspaceRoot,
      projectId: project.id,
      phaseId: null,
      role: "executor",
      maxTokens: 2000,
      projectGoal: "Test context assembly",
    });

    expect(contextPacket).toBeTruthy();
    expect(contextPacket.length).toBeGreaterThan(0);
    expect(contextPacket).toContain("Project Goal");
    expect(contextPacket).toContain("Test context assembly");
  }, 30_000);

  it("verifies state machine integrity: all intermediate states visited in order", () => {
    orchestrator.handle(NOUS_ID, SESSION_ID);
    const project = orchestrator.getActiveProject(NOUS_ID)!;

    // questioning
    expect(project.state).toBe("questioning");

    // questioning → researching
    orchestrator.confirmSynthesis(project.id, NOUS_ID, SESSION_ID, { goal: "Test transitions" });
    expect(store.getProjectOrThrow(project.id).state).toBe("researching");

    // researching → requirements
    orchestrator.skipResearch(project.id, NOUS_ID, SESSION_ID);
    expect(store.getProjectOrThrow(project.id).state).toBe("requirements");

    // requirements → roadmap
    requirementsOrch.persistCategory(project.id, {
      category: "A",
      categoryName: "Category A",
      tableStakes: [
        { name: "Feature A", description: "First feature", isTableStakes: true, proposedTier: "v1" },
      ],
      differentiators: [],
    }, [
      { name: "Feature A", tier: "v1" },
    ]);

    orchestrator.completeRequirements(project.id, NOUS_ID, SESSION_ID);
    expect(store.getProjectOrThrow(project.id).state).toBe("roadmap");
  });
});
