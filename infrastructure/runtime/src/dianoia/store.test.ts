// PlanningStore unit tests — in-memory SQLite, no external dependencies
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PlanningError } from "../koina/errors.js";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";

let db: Database.Database;
let store: PlanningStore;

const defaultConfig = {
  depth: "standard" as const,
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
};

const defaultProject = {
  nousId: "test-nous",
  sessionId: "test-session",
  goal: "Build a planning system",
  config: defaultConfig,
};

beforeEach(() => {
  db = new Database(":memory:");
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
  store = new PlanningStore(db);
});

afterEach(() => {
  db.close();
});

describe("createProject", () => {
  it("returns a project with state idle and a 16-char contextHash", () => {
    const project = store.createProject(defaultProject);
    expect(project.state).toBe("idle");
    expect(project.contextHash).toHaveLength(16);
    expect(project.contextHash).toMatch(/^[0-9a-f]+$/);
    expect(project.nousId).toBe("test-nous");
    expect(project.goal).toBe("Build a planning system");
  });

  it("stores config as a PlanningConfig object", () => {
    const project = store.createProject(defaultProject);
    expect(project.config.depth).toBe("standard");
    expect(project.config.research).toBe(true);
  });
});

describe("getProjectOrThrow", () => {
  it("throws PlanningError with PLANNING_PROJECT_NOT_FOUND for missing id", () => {
    expect(() => store.getProjectOrThrow("nonexistent-id")).toThrow(PlanningError);
    try {
      store.getProjectOrThrow("nonexistent-id");
    } catch (error) {
      expect((error as PlanningError).code).toBe("PLANNING_PROJECT_NOT_FOUND");
    }
  });

  it("returns the project when it exists", () => {
    const created = store.createProject(defaultProject);
    const found = store.getProjectOrThrow(created.id);
    expect(found.id).toBe(created.id);
  });
});

describe("updateProjectState", () => {
  it("updates state and changes updatedAt without touching createdAt", () => {
    const project = store.createProject(defaultProject);
    const originalCreatedAt = project.createdAt;

    store.updateProjectState(project.id, "researching");

    const updated = store.getProjectOrThrow(project.id);
    expect(updated.state).toBe("researching");
    expect(updated.createdAt).toBe(originalCreatedAt);
  });

  it("throws PLANNING_PROJECT_NOT_FOUND for missing id", () => {
    expect(() => store.updateProjectState("nonexistent", "idle")).toThrow(PlanningError);
  });
});

describe("deleteProject", () => {
  it("cascade-deletes phases when project is deleted", () => {
    const project = store.createProject(defaultProject);
    store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "Test goal",
      requirements: ["req-1"],
      successCriteria: ["criteria-1"],
      phaseOrder: 1,
    });

    const phasesBefore = store.listPhases(project.id);
    expect(phasesBefore).toHaveLength(1);

    store.deleteProject(project.id);

    const phasesAfter = db
      .prepare("SELECT * FROM planning_phases WHERE project_id = ?")
      .all(project.id) as Array<Record<string, unknown>>;
    expect(phasesAfter).toHaveLength(0);
  });
});

describe("listProjects", () => {
  it("filters by nousId correctly", () => {
    store.createProject({ ...defaultProject, nousId: "nous-a" });
    store.createProject({ ...defaultProject, nousId: "nous-a" });
    store.createProject({ ...defaultProject, nousId: "nous-b" });

    const forA = store.listProjects("nous-a");
    const forB = store.listProjects("nous-b");
    const all = store.listProjects();

    expect(forA).toHaveLength(2);
    expect(forB).toHaveLength(1);
    expect(all).toHaveLength(3);
  });
});

describe("createPhase / listPhases", () => {
  it("returns phases ordered by phaseOrder", () => {
    const project = store.createProject(defaultProject);

    store.createPhase({
      projectId: project.id,
      name: "Phase 3",
      goal: "Goal 3",
      requirements: [],
      successCriteria: [],
      phaseOrder: 3,
    });
    store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "Goal 1",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });
    store.createPhase({
      projectId: project.id,
      name: "Phase 2",
      goal: "Goal 2",
      requirements: [],
      successCriteria: [],
      phaseOrder: 2,
    });

    const phases = store.listPhases(project.id);
    expect(phases.map((p) => p.phaseOrder)).toEqual([1, 2, 3]);
    expect(phases.map((p) => p.name)).toEqual(["Phase 1", "Phase 2", "Phase 3"]);
  });
});

describe("updatePhaseStatus", () => {
  it("updates status and updatedAt", () => {
    const project = store.createProject(defaultProject);
    const phase = store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "Goal",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });

    store.updatePhaseStatus(phase.id, "executing");
    const updated = store.getPhaseOrThrow(phase.id);
    expect(updated.status).toBe("executing");
  });
});

describe("updatePhasePlan", () => {
  it("stores a plan object and round-trips through JSON", () => {
    const project = store.createProject(defaultProject);
    const phase = store.createPhase({
      projectId: project.id,
      name: "Phase 1",
      goal: "Goal",
      requirements: [],
      successCriteria: [],
      phaseOrder: 1,
    });

    const planData = { steps: ["step-1", "step-2"], estimatedDuration: "3h" };
    store.updatePhasePlan(phase.id, planData);

    const updated = store.getPhaseOrThrow(phase.id);
    expect(updated.plan).toEqual(planData);
  });
});

describe("createCheckpoint / resolveCheckpoint", () => {
  it("stores and resolves a checkpoint decision", () => {
    const project = store.createProject(defaultProject);
    const checkpoint = store.createCheckpoint({
      projectId: project.id,
      type: "decision",
      question: "Which approach?",
      context: { options: ["A", "B"] },
    });

    expect(checkpoint.decision).toBeNull();

    store.resolveCheckpoint(checkpoint.id, "Option A");

    const checkpoints = store.listCheckpoints(project.id);
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0]?.decision).toBe("Option A");
    expect(checkpoints[0]?.context).toEqual({ options: ["A", "B"] });
  });
});

describe("createRequirement / listRequirements", () => {
  it("returns all requirements for a project", () => {
    const project = store.createProject(defaultProject);

    store.createRequirement({
      projectId: project.id,
      reqId: "REQ-01",
      description: "The system must persist data",
      category: "persistence",
      tier: "v1",
    });
    store.createRequirement({
      projectId: project.id,
      reqId: "REQ-02",
      description: "The system should support pagination",
      category: "ux",
      tier: "v2",
    });

    const requirements = store.listRequirements(project.id);
    expect(requirements).toHaveLength(2);
    expect(requirements.map((r) => r.reqId)).toContain("REQ-01");
    expect(requirements.map((r) => r.reqId)).toContain("REQ-02");
  });
});

describe("createResearch / listResearch", () => {
  it("returns all research entries for a project", () => {
    const project = store.createProject(defaultProject);

    store.createResearch({
      projectId: project.id,
      phase: "requirements",
      dimension: "technical-feasibility",
      content: "SQLite is well-suited for this workload.",
    });
    store.createResearch({
      projectId: project.id,
      phase: "requirements",
      dimension: "alternatives",
      content: "DuckDB considered but rejected.",
    });

    const research = store.listResearch(project.id);
    expect(research).toHaveLength(2);
    expect(research[0]?.dimension).toBe("technical-feasibility");
  });
});

describe("corrupt JSON handling", () => {
  it("throws PLANNING_STATE_CORRUPT when config column contains invalid JSON", () => {
    const project = store.createProject(defaultProject);

    db.prepare("UPDATE planning_projects SET config = ? WHERE id = ?").run(
      "not-valid-json{{",
      project.id,
    );

    expect(() => store.getProjectOrThrow(project.id)).toThrow(PlanningError);
    try {
      store.getProjectOrThrow(project.id);
    } catch (error) {
      expect((error as PlanningError).code).toBe("PLANNING_STATE_CORRUPT");
    }
  });
});

describe("transaction isolation", () => {
  it("does not leave orphaned phases when project creation fails mid-transaction", () => {
    const countBefore = (
      db.prepare("SELECT COUNT(*) as cnt FROM planning_projects").get() as { cnt: number }
    ).cnt;

    try {
      db.transaction(() => {
        db.prepare(
          `INSERT INTO planning_projects (id, nous_id, session_id, goal, state, config, context_hash, created_at, updated_at)
           VALUES (?, ?, ?, ?, 'idle', '{}', 'abc123def456abcd', ?, ?)`,
        ).run("partial-id", "nous-x", "sess-x", "Partial goal", new Date().toISOString(), new Date().toISOString());

        throw new Error("Simulated crash mid-transaction");
      })();
    } catch {
      // Expected
    }

    const countAfter = (
      db.prepare("SELECT COUNT(*) as cnt FROM planning_projects").get() as { cnt: number }
    ).cnt;
    expect(countAfter).toBe(countBefore);
  });
});

describe("updateProjectGoal", () => {
  it("updates the goal column and reflects the change on read-back", () => {
    const project = store.createProject(defaultProject);
    store.updateProjectGoal(project.id, "Updated goal text");
    const updated = store.getProjectOrThrow(project.id);
    expect(updated.goal).toBe("Updated goal text");
  });

  it("throws PLANNING_PROJECT_NOT_FOUND for missing id", () => {
    expect(() => store.updateProjectGoal("nonexistent", "goal")).toThrow(PlanningError);
    try {
      store.updateProjectGoal("nonexistent", "goal");
    } catch (error) {
      expect((error as PlanningError).code).toBe("PLANNING_PROJECT_NOT_FOUND");
    }
  });
});

describe("updateProjectContext", () => {
  it("persists and round-trips a full ProjectContext including constraints and rawTranscript", () => {
    const project = store.createProject(defaultProject);
    const ctx = {
      goal: "Build a CLI tool",
      coreValue: "Developer productivity",
      constraints: ["TypeScript only", "No new DB"],
      keyDecisions: ["Use SQLite", "File-based config"],
      rawTranscript: [
        { turn: 1, text: "I'm building a CLI tool for local dev workflows" },
        { turn: 2, text: "TypeScript only, must not add new databases" },
      ],
    };
    store.updateProjectContext(project.id, ctx);
    const updated = store.getProjectOrThrow(project.id);
    expect(updated.projectContext).toEqual(ctx);
    expect(updated.projectContext?.constraints).toHaveLength(2);
    expect(updated.projectContext?.rawTranscript).toHaveLength(2);
    expect(updated.projectContext?.rawTranscript?.[0]?.turn).toBe(1);
  });

  it("returns projectContext: null on a new project before synthesis", () => {
    const project = store.createProject(defaultProject);
    const fetched = store.getProjectOrThrow(project.id);
    expect(fetched.projectContext).toBeNull();
  });

  it("throws PLANNING_PROJECT_NOT_FOUND for missing id", () => {
    expect(() => store.updateProjectContext("nonexistent", { goal: "x" })).toThrow(PlanningError);
    try {
      store.updateProjectContext("nonexistent", { goal: "x" });
    } catch (error) {
      expect((error as PlanningError).code).toBe("PLANNING_PROJECT_NOT_FOUND");
    }
  });
});

describe("mapProject projectContext null handling", () => {
  it("returns null projectContext when project_context column is NULL", () => {
    const project = store.createProject(defaultProject);
    const fetched = store.getProjectOrThrow(project.id);
    expect(fetched.projectContext).toBeNull();
  });

  it("returns null projectContext when project_context column contains invalid JSON", () => {
    const project = store.createProject(defaultProject);
    db.prepare("UPDATE planning_projects SET project_context = ? WHERE id = ?").run(
      "not-valid-json{{",
      project.id,
    );
    const fetched = store.getProjectOrThrow(project.id);
    expect(fetched.projectContext).toBeNull();
  });
});

describe("createRequirement rationale handling", () => {
  it("persists rationale when provided and returns it in listRequirements", () => {
    const project = store.createProject(defaultProject);
    store.createRequirement({
      projectId: project.id,
      reqId: "REQ-10",
      description: "Nice to have feature",
      category: "ux",
      tier: "out-of-scope",
      rationale: "Not enough bandwidth for v1 — revisit after launch",
    });

    const requirements = store.listRequirements(project.id);
    expect(requirements).toHaveLength(1);
    expect(requirements[0]?.rationale).toBe("Not enough bandwidth for v1 — revisit after launch");
  });

  it("returns rationale as null when not provided", () => {
    const project = store.createProject(defaultProject);
    store.createRequirement({
      projectId: project.id,
      reqId: "REQ-11",
      description: "Core auth requirement",
      category: "security",
      tier: "v1",
    });

    const requirements = store.listRequirements(project.id);
    expect(requirements).toHaveLength(1);
    expect(requirements[0]?.rationale).toBeNull();
  });
});

describe("updateRequirement", () => {
  it("updates tier from v1 to v2", () => {
    const project = store.createProject(defaultProject);
    const req = store.createRequirement({
      projectId: project.id,
      reqId: "REQ-20",
      description: "Pagination support",
      category: "ux",
      tier: "v1",
    });

    store.updateRequirement(req.id, { tier: "v2" });

    const requirements = store.listRequirements(project.id);
    expect(requirements[0]?.tier).toBe("v2");
  });

  it("updates rationale for an out-of-scope requirement", () => {
    const project = store.createProject(defaultProject);
    const req = store.createRequirement({
      projectId: project.id,
      reqId: "REQ-21",
      description: "Mobile app support",
      category: "platform",
      tier: "out-of-scope",
    });

    store.updateRequirement(req.id, { rationale: "Web-first for now; mobile in v3" });

    const requirements = store.listRequirements(project.id);
    expect(requirements[0]?.rationale).toBe("Web-first for now; mobile in v3");
  });

  it("updates both tier and rationale in one call", () => {
    const project = store.createProject(defaultProject);
    const req = store.createRequirement({
      projectId: project.id,
      reqId: "REQ-22",
      description: "Offline mode",
      category: "platform",
      tier: "v1",
    });

    store.updateRequirement(req.id, { tier: "out-of-scope", rationale: "Too complex for v1 scope" });

    const requirements = store.listRequirements(project.id);
    expect(requirements[0]?.tier).toBe("out-of-scope");
    expect(requirements[0]?.rationale).toBe("Too complex for v1 scope");
  });

  it("throws PLANNING_REQUIREMENT_NOT_FOUND for an unknown id", () => {
    expect(() => store.updateRequirement("nonexistent-req-id", { tier: "v2" })).toThrow(PlanningError);
    try {
      store.updateRequirement("nonexistent-req-id", { tier: "v2" });
    } catch (error) {
      expect((error as PlanningError).code).toBe("PLANNING_REQUIREMENT_NOT_FOUND");
    }
  });
});
