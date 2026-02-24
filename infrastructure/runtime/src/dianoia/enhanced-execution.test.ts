// Tests for enhanced execution orchestrator with wave concurrency and intelligent dispatch
import { describe, expect, it, beforeEach, vi, type MockedFunction } from "vitest";
import Database from "better-sqlite3";
import { 
  EnhancedExecutionOrchestrator,
  DEFAULT_EXECUTION_OPTIONS,
  computeWaves,
  findResumeWave,
  directDependents
} from "./enhanced-execution.js";
import type { ToolHandler, ToolContext } from "../organon/registry.js";
import type { PlanningPhase } from "./types.js";

describe("EnhancedExecutionOrchestrator", () => {
  let db: Database.Database;
  let mockDispatchTool: ToolHandler;
  let orchestrator: EnhancedExecutionOrchestrator;
  let mockToolContext: ToolContext;

  beforeEach(() => {
    db = new Database(":memory:");
    
    // Initialize database schema (simplified for testing)
    db.exec(`
      CREATE TABLE projects (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        goal TEXT NOT NULL,
        state TEXT NOT NULL DEFAULT 'planning',
        config TEXT NOT NULL DEFAULT '{}'
      );
      
      CREATE TABLE phases (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        name TEXT NOT NULL,
        goal TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        phase_order INTEGER NOT NULL,
        requirements TEXT NOT NULL DEFAULT '[]',
        success_criteria TEXT NOT NULL DEFAULT '[]',
        plan TEXT,
        FOREIGN KEY (project_id) REFERENCES projects(id)
      );
      
      CREATE TABLE spawn_records (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        phase_id TEXT NOT NULL,
        wave_number INTEGER NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        started_at TEXT,
        completed_at TEXT,
        error_message TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
      );
      
      CREATE TABLE requirements (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        req_id TEXT NOT NULL,
        description TEXT NOT NULL,
        tier TEXT NOT NULL DEFAULT 'v1'
      );
    `);

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
        { id: "phase1", name: "Phase 1", plan: null },
        { id: "phase2", name: "Phase 2", plan: null },
        { id: "phase3", name: "Phase 3", plan: null }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(1);
      expect(waves[0]).toHaveLength(3);
    });

    it("should compute waves with dependencies", () => {
      const phases: Partial<PlanningPhase>[] = [
        { id: "phase1", name: "Phase 1", plan: { dependencies: [] } },
        { id: "phase2", name: "Phase 2", plan: { dependencies: ["phase1"] } },
        { id: "phase3", name: "Phase 3", plan: { dependencies: ["phase1", "phase2"] } }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(3);
      expect(waves[0]).toContain(expect.objectContaining({ id: "phase1" }));
      expect(waves[1]).toContain(expect.objectContaining({ id: "phase2" }));
      expect(waves[2]).toContain(expect.objectContaining({ id: "phase3" }));
    });

    it("should handle dependency cycles gracefully", () => {
      const phases: Partial<PlanningPhase>[] = [
        { id: "phase1", name: "Phase 1", plan: { dependencies: ["phase2"] } },
        { id: "phase2", name: "Phase 2", plan: { dependencies: ["phase1"] } }
      ];

      const waves = computeWaves(phases as PlanningPhase[]);

      expect(waves).toHaveLength(1);
      expect(waves[0]).toHaveLength(2); // Both phases in same wave
    });
  });

  describe("task-to-role mapping integration", () => {
    beforeEach(() => {
      // Setup test project and phases
      db.prepare(`
        INSERT INTO projects (id, name, goal, state) 
        VALUES ('test-project', 'Test Project', 'Test project goal', 'executing')
      `).run();

      db.prepare(`
        INSERT INTO phases (id, project_id, name, goal, status, phase_order, requirements, success_criteria)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      `).run(
        "phase1",
        "test-project",
        "Implementation Phase",
        "implement user authentication",
        "pending",
        0,
        JSON.stringify(["AUTH-01"]),
        JSON.stringify(["Users can log in securely"])
      );

      db.prepare(`
        INSERT INTO requirements (id, project_id, req_id, description, tier)
        VALUES (?, ?, ?, ?, ?)
      `).run(
        "req1",
        "test-project", 
        "AUTH-01",
        "User can authenticate with email/password",
        "v1"
      );
    });

    it("should use intelligent dispatch when enabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(JSON.stringify({
        results: [{
          status: "success",
          result: JSON.stringify({
            role: "coder",
            task: "implement user authentication",
            status: "success", 
            summary: "Successfully implemented authentication",
            confidence: 0.9
          }),
          durationMs: 1000
        }]
      }));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useIntelligentDispatch: true,
        enableWaveConcurrency: false
      });

      const result = await orchestrator.executePhase("test-project", mockToolContext);

      expect(mockExecute).toHaveBeenCalled();
      const dispatchCall = mockExecute.mock.calls[0][0];
      
      // Should have mapped "implement user authentication" to coder role
      expect(dispatchCall.role).toBeDefined();
    });
  });

  describe("concurrent execution", () => {
    beforeEach(() => {
      // Setup project with multiple independent phases
      db.prepare(`
        INSERT INTO projects (id, name, goal, state) 
        VALUES ('concurrent-project', 'Concurrent Test', 'Test concurrent execution', 'executing')
      `).run();

      const phases = [
        { id: "phase1", name: "Phase 1", goal: "implement feature A" },
        { id: "phase2", name: "Phase 2", goal: "implement feature B" },
        { id: "phase3", name: "Phase 3", goal: "implement feature C" }
      ];

      for (let i = 0; i < phases.length; i++) {
        const phase = phases[i]!;
        db.prepare(`
          INSERT INTO phases (id, project_id, name, goal, status, phase_order, requirements, success_criteria)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        `).run(
          phase.id,
          "concurrent-project",
          phase.name,
          phase.goal,
          "pending",
          i,
          JSON.stringify([]),
          JSON.stringify([`Complete ${phase.name}`])
        );
      }
    });

    it("should execute tasks concurrently when enabled", async () => {
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(JSON.stringify({
        parallel: true,
        count: 3,
        results: [
          { status: "success", result: createMockSubAgentResult("coder", "success"), durationMs: 1000, index: 0 },
          { status: "success", result: createMockSubAgentResult("coder", "success"), durationMs: 1200, index: 1 },
          { status: "success", result: createMockSubAgentResult("coder", "success"), durationMs: 800, index: 2 }
        ]
      }));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        enableWaveConcurrency: true,
        maxConcurrentTasks: 3
      });

      const result = await orchestrator.executePhase("concurrent-project", mockToolContext);

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
      mockExecute
        .mockResolvedValueOnce(JSON.stringify({ result: createMockSubAgentResult("coder", "success") }))
        .mockResolvedValueOnce(JSON.stringify({ result: createMockSubAgentResult("coder", "success") }))
        .mockResolvedValueOnce(JSON.stringify({ result: createMockSubAgentResult("coder", "success") }));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        enableWaveConcurrency: false
      });

      const result = await orchestrator.executePhase("concurrent-project", mockToolContext);

      expect(result.concurrent).toBe(false);
      expect(mockExecute).toHaveBeenCalledTimes(3); // Sequential calls
    });
  });

  describe("structured extraction", () => {
    beforeEach(() => {
      db.prepare(`
        INSERT INTO projects (id, name, goal, state) 
        VALUES ('extraction-project', 'Extraction Test', 'Test structured extraction', 'executing')
      `).run();

      db.prepare(`
        INSERT INTO phases (id, project_id, name, goal, status, phase_order, requirements, success_criteria)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      `).run(
        "extract-phase",
        "extraction-project",
        "Extraction Phase",
        "test extraction",
        "pending", 
        0,
        JSON.stringify([]),
        JSON.stringify(["Extract data successfully"])
      );
    });

    it("should use structured extraction when enabled", async () => {
      const validResult = createMockSubAgentResult("coder", "success", 0.9);
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(JSON.stringify({
        results: [{ status: "success", result: validResult, durationMs: 1000 }]
      }));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useStructuredExtraction: true
      });

      const result = await orchestrator.executePhase("extraction-project", mockToolContext);

      expect(result.failed).toBe(0);
    });

    it("should handle validation failures gracefully", async () => {
      const invalidResult = `
        Some response text
        \`\`\`json
        {
          "role": "",
          "task": "test",
          "status": "invalid_status",
          "summary": "bad",
          "confidence": 2.0
        }
        \`\`\`
      `;
      
      const mockExecute = mockDispatchTool.execute as MockedFunction<any>;
      mockExecute.mockResolvedValue(JSON.stringify({
        results: [{ status: "success", result: invalidResult, durationMs: 1000 }]
      }));

      orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool, {
        useStructuredExtraction: true,
        enableAutoRetry: false
      });

      const result = await orchestrator.executePhase("extraction-project", mockToolContext);

      expect(result.failed).toBe(1); // Should fail due to validation
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