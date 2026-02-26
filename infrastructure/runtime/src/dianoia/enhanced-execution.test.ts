// Tests for enhanced execution orchestrator with wave concurrency and intelligent dispatch
import { beforeEach, describe, expect, it, type MockedFunction, vi } from "vitest";
import Database from "better-sqlite3";
import { 
  computeWaves,
  DEFAULT_EXECUTION_OPTIONS,
  directDependents,
  EnhancedExecutionOrchestrator,
  findResumeWave
} from "./enhanced-execution.js";
import { PlanningStore } from "./store.js";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { PlanningPhase } from "./types.js";

const defaultConfig = {
  depth: "standard" as const,
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
  pause_between_phases: false,
};

function makeDb(): Database.Database {
  const d = new Database(":memory:");
  d.pragma("journal_mode = WAL");
  d.pragma("foreign_keys = ON");
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

/** Build a valid DispatchResult JSON string that passes Zod schema validation */
function mockDispatchResult(taskCount = 1): string {
  return JSON.stringify({
    taskCount,
    succeeded: taskCount,
    failed: 0,
    results: Array.from({ length: taskCount }, (_, i) => ({
      index: i,
      task: `task-${i}`,
      status: "success",
      result: "done",
      durationMs: 100,
    })),
    timing: { wallClockMs: 100, sequentialMs: 100, savedMs: 0 },
    totalTokens: 0,
  });
}

describe("EnhancedExecutionOrchestrator", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let mockDispatchTool: ToolHandler;
  let orchestrator: EnhancedExecutionOrchestrator;
  let mockToolContext: ToolContext;

  beforeEach(() => {
    db = makeDb();
    store = new PlanningStore(db);

    mockDispatchTool = {
      definition: {
        name: "mock_dispatch",
        description: "Mock dispatch tool",
        input_schema: { type: "object", properties: {}, required: [] }
      },
      execute: vi.fn()
    } as any;

    mockToolContext = {
      nousId: "test-nous",
      sessionId: "test-session",
      depth: 0
    };

    orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool);
  });

  describe("initialization", () => {
    it("should initialize with default options", () => {
      const defaultOrchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool);
      expect(defaultOrchestrator).toBeDefined();
    });

    it("should accept custom options", () => {
      const customOptions = {
        enableWaveConcurrency: false,
        useIntelligentDispatch: false,
        maxConcurrentTasks: 5
      };
      
      const customOrchestrator = new EnhancedExecutionOrchestrator(
        db, 
        mockDispatchTool, 
        customOptions
      );
      
      expect(customOrchestrator).toBeDefined();
    });
  });

  describe("wave computation", () => {
    it("should compute waves with no dependencies", () => {
      const phases: Partial<PlanningPhase>[] = [
        { id: "phase1", name: "Phase 1", plan: null, dependencies: [], phaseOrder: 0 },
        { id: "phase2", name: "Phase 2", plan: null, dependencies: [], phaseOrder: 0 },
        { id: "phase3", name: "Phase 3", plan: null, dependencies: [], phaseOrder: 0 }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(1);
      expect(waves[0]).toHaveLength(3);
    });

    it("should compute waves with dependencies", () => {
      const phases: Partial<PlanningPhase>[] = [
        { id: "phase1", name: "Phase 1", dependencies: [], phaseOrder: 1 },
        { id: "phase2", name: "Phase 2", dependencies: ["phase1"], phaseOrder: 2 },
        { id: "phase3", name: "Phase 3", dependencies: ["phase1", "phase2"], phaseOrder: 3 }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(3);
      expect(waves[0]!.map(p => p.id)).toContain("phase1");
      expect(waves[1]!.map(p => p.id)).toContain("phase2");
      expect(waves[2]!.map(p => p.id)).toContain("phase3");
    });

    it("should handle dependency cycles gracefully", () => {
      const phases: Partial<PlanningPhase>[] = [
        { id: "phase1", name: "Phase 1", dependencies: ["phase2"], phaseOrder: 1 },
        { id: "phase2", name: "Phase 2", dependencies: ["phase1"], phaseOrder: 2 }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(1);
      expect(waves[0]).toHaveLength(2); // Both phases in same wave
    });
  });

  describe("task-to-role mapping integration", () => {
    let testProjectId: string;

    beforeEach(() => {
      const project = store.createProject({
        nousId: "test-nous",
        sessionId: "test-session",
        goal: "Test project goal",
        config: defaultConfig,
      });
      testProjectId = project.id;
      store.updateProjectState(project.id, "executing");
      store.createPhase({
        projectId: project.id,
        name: "Implementation Phase",
        goal: "implement user authentication",
        requirements: ["AUTH-01"],
        successCriteria: ["Users can log in securely"],
        phaseOrder: 1,
      });
    });

    it("should use intelligent dispatch when enabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(mockDispatchResult());

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useIntelligentDispatch: true,
        enableWaveConcurrency: false
      });

      const result = await orchestrator.executePhase(testProjectId, mockToolContext);

      expect(mockExecute).toHaveBeenCalled();
      const dispatchCall = mockExecute.mock.calls[0][0];
      
      // Should have mapped "implement user authentication" to coder role
      expect(dispatchCall.tasks[0].role).toBeDefined();
    });
  });

  describe("concurrent execution", () => {
    let concurrentProjectId: string;

    beforeEach(() => {
      const project = store.createProject({
        nousId: "test-nous",
        sessionId: "test-session",
        goal: "Test concurrent execution",
        config: defaultConfig,
      });
      concurrentProjectId = project.id;
      store.updateProjectState(project.id, "executing");

      const phases = [
        { name: "Phase 1", goal: "implement feature A" },
        { name: "Phase 2", goal: "implement feature B" },
        { name: "Phase 3", goal: "implement feature C" },
      ];

      for (let i = 0; i < phases.length; i++) {
        store.createPhase({
          projectId: project.id,
          name: phases[i]!.name,
          goal: phases[i]!.goal,
          requirements: [],
          successCriteria: [`Complete ${phases[i]!.name}`],
          phaseOrder: i + 1,
        });
      }
    });

    it("should execute tasks concurrently when enabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(mockDispatchResult(3));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        enableWaveConcurrency: true,
        maxConcurrentTasks: 3
      });

      const result = await orchestrator.executePhase(concurrentProjectId, mockToolContext);

      expect(result.concurrent).toBe(true);
      expect(result.failed).toBe(0);
      expect(mockExecute).toHaveBeenCalledWith(
        expect.objectContaining({
          tasks: expect.arrayContaining([
            expect.objectContaining({ task: expect.any(String), role: expect.any(String) })
          ])
        }),
        mockToolContext
      );
    });

    it("should fall back to sequential execution when concurrency disabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(mockDispatchResult());

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        enableWaveConcurrency: false
      });

      const result = await orchestrator.executePhase(concurrentProjectId, mockToolContext);

      expect(result.concurrent).toBe(false);
      expect(mockExecute).toHaveBeenCalledTimes(3); // Sequential calls
    });
  });

  describe("structured extraction", () => {
    let extractionProjectId: string;

    beforeEach(() => {
      const project = store.createProject({
        nousId: "test-nous",
        sessionId: "test-session",
        goal: "Test structured extraction",
        config: defaultConfig,
      });
      extractionProjectId = project.id;
      store.updateProjectState(project.id, "executing");
      store.createPhase({
        projectId: project.id,
        name: "Extraction Phase",
        goal: "test extraction",
        requirements: [],
        successCriteria: ["Extract data successfully"],
        phaseOrder: 1,
      });
    });

    it("should use structured extraction when enabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(mockDispatchResult());

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useStructuredExtraction: true
      });

      const result = await orchestrator.executePhase(extractionProjectId, mockToolContext);

      expect(result.failed).toBe(0);
    });

    it("should handle dispatch parse failures gracefully", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      // Return unparseable garbage — should trigger parse failure path
      mockExecute.mockResolvedValue("this is not json at all");

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useStructuredExtraction: true,
        enableAutoRetry: false,
        enableWaveConcurrency: false,
      });

      const result = await orchestrator.executePhase(extractionProjectId, mockToolContext);

      expect(result.failed).toBe(1);
    });
  });
});

describe("utility functions", () => {
  describe("findResumeWave", () => {
    it("should find correct resume wave with mixed status", () => {
      const records = [
        { id: "1", phaseId: "p1", waveNumber: 0, status: "done" },
        { id: "2", phaseId: "p2", waveNumber: 0, status: "done" },
        { id: "3", phaseId: "p3", waveNumber: 1, status: "running" },
        { id: "4", phaseId: "p4", waveNumber: 1, status: "pending" }
      ] as any;

      const resumeWave = findResumeWave(records);

      expect(resumeWave).toBe(1);
    });

    it("should return -1 when all waves complete", () => {
      const records = [
        { id: "1", phaseId: "p1", waveNumber: 0, status: "done" },
        { id: "2", phaseId: "p2", waveNumber: 1, status: "done" }
      ] as any;

      const resumeWave = findResumeWave(records);

      expect(resumeWave).toBe(-1);
    });

    it("should return 0 for empty records", () => {
      const resumeWave = findResumeWave([]);
      expect(resumeWave).toBe(0);
    });
  });

  describe("directDependents", () => {
    it("should find phases that directly depend on failed phase", () => {
      const phases = [
        { id: "p1", plan: { dependencies: [] } },
        { id: "p2", plan: { dependencies: ["p1"] } },
        { id: "p3", plan: { dependencies: ["p1", "p2"] } },
        { id: "p4", plan: { dependencies: ["p2"] } }
      ] as any;

      const dependents = directDependents("p1", phases);

      expect(dependents).toHaveLength(2);
      expect(dependents.map(p => p.id)).toContain("p2");
      expect(dependents.map(p => p.id)).toContain("p3");
    });

    it("should return empty array when no dependents", () => {
      const phases = [
        { id: "p1", plan: { dependencies: [] } },
        { id: "p2", plan: { dependencies: [] } }
      ] as any;

      const dependents = directDependents("p1", phases);

      expect(dependents).toHaveLength(0);
    });
  });
});

// Helper function to create mock sub-agent results
function createMockSubAgentResult(role: string, status: string, confidence: number = 0.8): string {
  return `
Response text here.

\`\`\`json
{
  "role": "${role}",
  "task": "mock task",
  "status": "${status}",
  "summary": "Mock summary that is long enough to pass validation",
  "details": {},
  "confidence": ${confidence}
}
\`\`\`
`;
}