// Integration test — full Dianoia pipeline from idle to complete, with failure path
import { beforeEach, describe, expect, it, vi } from "vitest";
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
import { ExecutionOrchestrator } from "./execution.js";
import { GoalBackwardVerifier } from "./verifier.js";
import { PlanningStore } from "./store.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
};

const MOCK_NOUS_ID = "nous-integration";
const MOCK_SESSION_ID = "session-integration";

const mockContext: ToolContext = {
  nousId: MOCK_NOUS_ID,
  sessionId: MOCK_SESSION_ID,
  workspace: "/tmp/integration-test",
};

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
  return db;
}

function makeSuccessDispatchTool(resultOverride?: object): ToolHandler {
  return {
    definition: {
      name: "sessions_dispatch",
      description: "mock",
      input_schema: { type: "object" as const, properties: {}, required: [] },
    },
    execute: vi.fn().mockResolvedValue(
      JSON.stringify({
        results: [
          {
            index: 0,
            status: "success",
            result: JSON.stringify(
              resultOverride ?? {
                status: "met",
                summary: "All criteria met",
                gaps: [],
              },
            ),
            durationMs: 10,
          },
        ],
      }),
    ),
  } as unknown as ToolHandler;
}

function makeErrorDispatchTool(): ToolHandler {
  return {
    definition: {
      name: "sessions_dispatch",
      description: "mock error",
      input_schema: { type: "object" as const, properties: {}, required: [] },
    },
    execute: vi.fn().mockResolvedValue(
      JSON.stringify({
        results: [
          {
            index: 0,
            status: "error",
            error: "Agent crashed",
            durationMs: 10,
          },
        ],
      }),
    ),
  } as unknown as ToolHandler;
}

describe("Dianoia integration — full pipeline", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let orchestrator: DianoiaOrchestrator;

  beforeEach(() => {
    db = makeDb();
    store = new PlanningStore(db);
    orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    orchestrator.setWorkspaceRoot("/tmp/integration-test");
  });

  it("drives idle → complete via full pipeline", async () => {
    // Step 1: handle() creates project and transitions to questioning
    orchestrator.handle(MOCK_NOUS_ID, MOCK_SESSION_ID);
    const project = orchestrator.getActiveProject(MOCK_NOUS_ID)!;
    expect(project).toBeDefined();
    expect(project.state).toBe("questioning");

    // Step 2: confirmSynthesis transitions questioning → researching
    orchestrator.confirmSynthesis(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID, {
      goal: "Build a thing",
      coreValue: "Fast",
      constraints: [],
      keyDecisions: [],
    });
    const afterSynthesis = store.getProjectOrThrow(project.id);
    expect(afterSynthesis.state).toBe("researching");

    // Step 3: skipResearch transitions researching → requirements
    orchestrator.skipResearch(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const afterSkip = store.getProjectOrThrow(project.id);
    expect(afterSkip.state).toBe("requirements");

    // Step 4: completeRequirements transitions requirements → roadmap
    orchestrator.completeRequirements(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const afterRequirements = store.getProjectOrThrow(project.id);
    expect(afterRequirements.state).toBe("roadmap");

    // Step 5: completeRoadmap transitions roadmap → discussing
    orchestrator.completeRoadmap(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const afterRoadmap = store.getProjectOrThrow(project.id);
    expect(afterRoadmap.state).toBe("discussing");

    // Step 5b: discussing → phase-planning (DISCUSSION_COMPLETE)
    store.updateProjectState(project.id, "phase-planning");

    // Step 6: create a test phase so executePhase has something to run
    const phase = store.createPhase({
      projectId: project.id,
      name: "Integration Phase",
      goal: "Do integration work",
      requirements: [],
      successCriteria: ["Everything works"],
      phaseOrder: 1,
    });

    // Step 7: advanceToExecution transitions phase-planning → executing
    orchestrator.advanceToExecution(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const afterAdvance = store.getProjectOrThrow(project.id);
    expect(afterAdvance.state).toBe("executing");

    // Step 8: executePhase dispatches and marks phase complete
    const dispatchTool = makeSuccessDispatchTool();
    const execOrch = new ExecutionOrchestrator(db, dispatchTool);
    const execResult = await execOrch.executePhase(project.id, mockContext);
    expect(execResult.waveCount).toBeGreaterThan(0);
    expect(dispatchTool.execute).toHaveBeenCalled();

    // Step 9: advanceToVerification transitions executing → verifying
    orchestrator.advanceToVerification(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const afterExec = store.getProjectOrThrow(project.id);
    expect(afterExec.state).toBe("verifying");

    // Step 10: verify() dispatches verifier agent — returns met/partially-met
    const verifyDispatch = makeSuccessDispatchTool({
      status: "met",
      summary: "All criteria met",
      gaps: [],
    });
    const verifier = new GoalBackwardVerifier(db, verifyDispatch);
    const verifyResult = await verifier.verify(project.id, phase.id, mockContext);
    expect(["met", "partially-met", "not-met"]).toContain(verifyResult.status);

    // Step 11: completeAllPhases transitions verifying → complete
    orchestrator.completeAllPhases(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const completed = store.getProjectOrThrow(project.id);
    expect(completed.state).toBe("complete");
  });

  it("blocks FSM when execution fails", async () => {
    // Drive to executing state via same steps as happy path
    orchestrator.handle(MOCK_NOUS_ID, MOCK_SESSION_ID);
    const project = orchestrator.getActiveProject(MOCK_NOUS_ID)!;

    orchestrator.confirmSynthesis(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID, {
      goal: "Build a failing thing",
      coreValue: "Speed",
      constraints: [],
      keyDecisions: [],
    });

    orchestrator.skipResearch(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    orchestrator.completeRequirements(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    orchestrator.completeRoadmap(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    // discussing → phase-planning (skip discussion for test)
    store.updateProjectState(project.id, "phase-planning");

    store.createPhase({
      projectId: project.id,
      name: "Failing Phase",
      goal: "Fail",
      requirements: [],
      successCriteria: ["Should fail"],
      phaseOrder: 1,
    });

    orchestrator.advanceToExecution(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    const inExecution = store.getProjectOrThrow(project.id);
    expect(inExecution.state).toBe("executing");

    // executePhase with error-returning dispatch — phase should be marked failed
    const errorDispatch = makeErrorDispatchTool();
    const execOrch = new ExecutionOrchestrator(db, errorDispatch);
    const execResult = await execOrch.executePhase(project.id, mockContext);
    expect(execResult.failed).toBeGreaterThan(0);

    // Verify spawn record has failed status
    const spawnRecords = store.listSpawnRecords(project.id);
    const failedRecord = spawnRecords.find((r) => r.status === "failed");
    expect(failedRecord).toBeDefined();

    // Drive to verifying and then block
    orchestrator.advanceToVerification(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);
    orchestrator.blockOnVerificationFailure(project.id, MOCK_NOUS_ID, MOCK_SESSION_ID);

    const blocked = store.getProjectOrThrow(project.id);
    expect(blocked.state).toBe("blocked");
  });
});
