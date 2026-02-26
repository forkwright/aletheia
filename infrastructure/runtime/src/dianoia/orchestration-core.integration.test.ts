// Integration test demonstrating ORCH-04: Verification failure auto-skip and rollback plan
import { describe, expect, it } from "vitest";
import Database from "better-sqlite3";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { DianoiaOrchestrator } from "./orchestrator.js";
import { GoalBackwardVerifier } from "./verifier.js";
import { createPlanVerifyTool } from "./verifier-tool.js";
import { PlanningStore } from "./store.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { PlanningConfigSchema } from "../taxis/schema.js";

const DEFAULT_CONFIG: PlanningConfigSchema = {
  depth: "standard",
  parallelization: true,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive",
};

function makeDb(): Database.Database {
  const db = new Database(":memory:");
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

// Mock dispatch tool that simulates verification failure
const mockDispatchTool: ToolHandler = {
  definition: {
    name: "sessions_dispatch",
    description: "Mock dispatch",
    input_schema: { type: "object", properties: {}, required: [] }
  },
  execute: async () => {
    // Simulate verification failure with gaps
    return JSON.stringify({
      results: [{
        status: "success",
        result: JSON.stringify({
          status: "not-met",
          summary: "Critical phase verification failed",
          gaps: [
            {
              criterion: "API endpoints must respond",
              status: "not-met",
              detail: "500 errors on all endpoints",
              proposedFix: "Fix database connection"
            },
            {
              criterion: "Performance tests pass",
              status: "partially-met", 
              detail: "Some endpoints slow",
              proposedFix: "Optimize queries"
            }
          ]
        })
      }]
    });
  }
};

describe("ORCH-04 Integration Test: Verification Failure Auto-Skip", () => {
  it("demonstrates complete verification failure workflow with downstream skipping", async () => {
    // Setup
    const db = makeDb();
    const store = new PlanningStore(db);
    const orchestrator = new DianoiaOrchestrator(db, DEFAULT_CONFIG);
    const verifier = new GoalBackwardVerifier(db, mockDispatchTool);
    const verifyTool = createPlanVerifyTool(orchestrator, verifier, {} as any, store);

    // Create a project with dependent phases and advance to verifying state
    const project = store.createProject({
      nousId: "test-nous",
      sessionId: "test-session", 
      goal: "Build microservice API",
      config: DEFAULT_CONFIG
    });
    // Advance through FSM to verifying state so blockOnVerificationFailure works
    store.updateProjectState(project.id, "verifying");

    // Phase A: Core API (foundation)
    const phaseA = store.createPhase({
      projectId: project.id,
      name: "Core API",
      goal: "Build REST API endpoints", 
      requirements: ["API-01"],
      successCriteria: ["API endpoints respond", "Performance tests pass"],
      phaseOrder: 1
    });

    // Phase B: Authentication (depends on Core API)
    const phaseB = store.createPhase({
      projectId: project.id,
      name: "Authentication", 
      goal: "Add user authentication",
      requirements: ["AUTH-01"],
      successCriteria: ["JWT auth works", "Rate limiting active"],
      phaseOrder: 2
    });
    store.updatePhasePlan(phaseB.id, { dependencies: [phaseA.id] });

    // Phase C: Monitoring (depends on Authentication) 
    const phaseC = store.createPhase({
      projectId: project.id,
      name: "Monitoring",
      goal: "Add metrics and logging",
      requirements: ["MON-01"], 
      successCriteria: ["Metrics collected", "Alerts configured"],
      phaseOrder: 3
    });
    store.updatePhasePlan(phaseC.id, { dependencies: [phaseB.id] });

    // Phase D: Independent feature
    const phaseD = store.createPhase({
      projectId: project.id,
      name: "Documentation",
      goal: "Write API documentation", 
      requirements: ["DOC-01"],
      successCriteria: ["OpenAPI spec complete", "Usage examples added"],
      phaseOrder: 4
    });
    store.updatePhasePlan(phaseD.id, { dependencies: [] }); // Independent

    const mockContext: ToolContext = {
      nousId: "test-nous",
      sessionId: "test-session",
      workspace: "/tmp"
    };

    // Execute verification on Phase A - this will fail
    const verifyResult = await verifyTool.execute({
      action: "run",
      projectId: project.id,
      phaseId: phaseA.id
    }, mockContext);

    const parsed = JSON.parse(verifyResult as string);
    
    // Verify the results
    expect(parsed.status).toBe("not-met");
    expect(parsed.gaps).toHaveLength(2);
    expect(parsed.skippedPhases).toEqual([phaseB.id]);
    expect(parsed.rollbackPlan).toBeDefined();

    // Check rollback plan structure  
    expect(parsed.rollbackPlan.failedPhaseId).toBe(phaseA.id);
    expect(parsed.rollbackPlan.gapCount).toBe(2);
    expect(parsed.rollbackPlan.actions).toHaveLength(3); // 2 gaps + 1 verification
    expect(parsed.rollbackPlan.estimatedEffort).toBe("medium"); // 1 critical gap

    // Verify database state changes
    const updatedPhaseB = store.getPhaseOrThrow(phaseB.id);
    const updatedPhaseC = store.getPhaseOrThrow(phaseC.id);
    const updatedPhaseD = store.getPhaseOrThrow(phaseD.id);

    expect(updatedPhaseB.status).toBe("skipped"); // Direct dependent
    expect(updatedPhaseC.status).toBe("pending"); // NOT skipped - depends on B, not A directly
    expect(updatedPhaseD.status).toBe("pending"); // Independent, unaffected

    // Check project is blocked
    const updatedProject = store.getProjectOrThrow(project.id);
    expect(updatedProject.state).toBe("blocked");

    // Verify rollback actions are actionable
    const actions = parsed.rollbackPlan.actions;
    const gapActions = actions.filter((a: any) => a.type === "fix-verification-gap");
    const verifyAction = actions.find((a: any) => a.type === "verify-phase");

    expect(gapActions[0].priority).toBe("high"); // not-met
    expect(gapActions[1].priority).toBe("medium"); // partially-met  
    expect(verifyAction.priority).toBe("high");
    expect(verifyAction.proposedFix).toContain("plan_verify");
  });
});