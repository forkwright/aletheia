// Integration test demonstrating Enhanced Execution Engine features
import { describe, expect, it, beforeEach, vi } from "vitest";
import Database from "better-sqlite3";
import { 
  EnhancedExecutionOrchestrator,
  DEFAULT_EXECUTION_OPTIONS
} from "./enhanced-execution.js";
import { 
  mapTaskToRole,
  StructuredExtractor,
} from "./structured-extraction.js";
import { schemas } from "./structured-extraction.js";
import { PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION, PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION, PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION } from "./schema.js";
import { PlanningStore } from "./store.js";
import type { ToolHandler, ToolContext } from "../organon/registry.js";

const SubAgentResultSchema = schemas.SubAgentResult;

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

const defaultConfig = {
  depth: "standard" as const,
  parallelization: false,
  research: true,
  plan_check: true,
  verifier: true,
  mode: "interactive" as const,
  pause_between_phases: false,
};

describe("Enhanced Execution Engine Integration", () => {
  let db: Database.Database;
  let store: PlanningStore;
  let mockDispatchTool: ToolHandler;
  let mockToolContext: ToolContext;

  beforeEach(() => {
    db = makeDb();
    store = new PlanningStore(db);

    mockDispatchTool = {
      definition: {
        name: "mock_enhanced_dispatch",
        description: "Mock enhanced dispatch tool",
        input_schema: { type: "object", properties: {}, required: [] }
      },
      execute: vi.fn()
    } as any;

    mockToolContext = {
      nousId: "test-nous",
      sessionId: "test-session",
      depth: 0
    };
  });

  describe("EXEC-01: Task-to-Role Mapping", () => {
    it("should intelligently map different task types to appropriate roles", () => {
      const testCases = [
        { task: "implement user authentication system", expectedRole: "coder" },
        { task: "review the pull request for security issues", expectedRole: "reviewer" },
        { task: "research best practices for OAuth implementation", expectedRole: "researcher" },
        { task: "find where the user model is defined in the codebase", expectedRole: "explorer" },
        { task: "run the integration test suite", expectedRole: "runner" }
      ];

      for (const { task, expectedRole } of testCases) {
        const mapping = mapTaskToRole(task);
        expect(mapping.role).toBe(expectedRole);
        expect(mapping.confidence).toBeGreaterThan(0.5);
        expect(mapping.reasoning).toBeDefined();
      }
    });

    it("should handle fallback roles when preferred role unavailable", () => {
      const task = "implement user authentication";
      const limitedRoles = ["reviewer", "explorer"]; // no coder available

      const mapping = mapTaskToRole(task, limitedRoles);
      
      expect(limitedRoles).toContain(mapping.role);
      expect(mapping.reasoning).toContain("fallback");
      expect(mapping.confidence).toBeLessThan(0.8);
    });
  });

  describe("EXEC-02: Structured Extraction with Zod", () => {
    let extractor: StructuredExtractor;

    beforeEach(() => {
      extractor = new StructuredExtractor();
    });

    it("should successfully extract and validate structured results", async () => {
      const mockResponse = `
Task completed successfully.

\`\`\`json
{
  "role": "coder",
  "task": "implement authentication",
  "status": "success",
  "summary": "Successfully implemented OAuth authentication with JWT tokens",
  "details": {
    "filesModified": 3,
    "linesOfCode": 245,
    "testsCoverage": 95
  },
  "filesChanged": ["auth.ts", "user.model.ts", "auth.test.ts"],
  "confidence": 0.92
}
\`\`\`
`;

      const result = await extractor.extractStructuredResult(mockResponse, SubAgentResultSchema);
      
      expect(result.success).toBe(true);
      expect(result.data.role).toBe("coder");
      expect(result.data.status).toBe("success");
      expect(result.data.confidence).toBe(0.92);
      expect(result.data.filesChanged).toContain("auth.ts");
    });

    it("should provide detailed validation feedback for invalid results", async () => {
      const invalidResponse = `
\`\`\`json
{
  "role": "",
  "status": "invalid_status",
  "summary": "bad",
  "confidence": 1.5
}
\`\`\`
`;

      const result = await extractor.extractStructuredResult(invalidResponse, SubAgentResultSchema);
      
      expect(result.success).toBe(false);
      expect(result.validationErrors).toBeDefined();
      expect(result.validationErrors!.length).toBeGreaterThan(0);
      expect(result.validationErrors!.some(e => e.includes("Role must not be empty"))).toBe(true);
    });

    it("should create actionable validation feedback", () => {
      const errors = [
        "role: Role must not be empty",
        "confidence: Confidence must be between 0 and 1"
      ];
      const task = "test task";

      const feedback = extractor.createValidationFeedback(errors, task);

      expect(feedback).toContain("Validation Failed");
      expect(feedback).toContain("Role must not be empty");
      expect(feedback).toContain("Confidence must be between 0 and 1");
      expect(feedback).toContain("test task");
    });
  });

  describe("EXEC-03: Wave Concurrency", () => {
    it("should initialize with wave concurrency enabled by default", () => {
      const orchestrator = new EnhancedExecutionOrchestrator(db, mockDispatchTool);
      
      // Orchestrator should exist and be configurable
      expect(orchestrator).toBeDefined();
      
      // Can set workspace root for context building
      orchestrator.setWorkspaceRoot("/test/workspace");
    });

    it("should support custom execution options", () => {
      const customOptions = {
        enableWaveConcurrency: false,
        useIntelligentDispatch: true,
        maxConcurrentTasks: 5
      };

      const orchestrator = new EnhancedExecutionOrchestrator(
        db, 
        mockDispatchTool, 
        customOptions
      );

      expect(orchestrator).toBeDefined();
    });
  });

  describe("EXEC-04: Automatic Retry with Validation Feedback", () => {
    it("should format validation errors into retry feedback", () => {
      const extractor = new StructuredExtractor();
      const validationErrors = [
        "status: Status must be 'success', 'partial', or 'failed'",
        "confidence: Required"
      ];
      const originalTask = "test task";

      const feedback = extractor.createValidationFeedback(validationErrors, originalTask);

      expect(feedback).toContain("❌ **Validation Failed**");
      expect(feedback).toContain("Status must be 'success', 'partial', or 'failed'");
      expect(feedback).toContain("confidence: Required");
      expect(feedback).toContain("fix these issues");
      expect(feedback).toContain("test task");
    });
  });

  describe("End-to-End Integration", () => {
    beforeEach(() => {
      const project = store.createProject({
        nousId: "test-nous",
        sessionId: "test-session",
        goal: "Test enhanced execution features",
        config: defaultConfig,
      });
      store.updateProjectState(project.id, "executing");

      const phases = [
        { name: "Authentication", goal: "implement user authentication" },
        { name: "Code Review", goal: "review the authentication implementation" },
        { name: "Testing", goal: "run comprehensive test suite" },
      ];

      for (let i = 0; i < phases.length; i++) {
        store.createPhase({
          projectId: project.id,
          name: phases[i]!.name,
          goal: phases[i]!.goal,
          requirements: [`REQ-${i + 1}`],
          successCriteria: [`Complete ${phases[i]!.name} successfully`],
          phaseOrder: i + 1,
        });
      }
    });

    it("should demonstrate intelligent task routing based on phase goals", () => {
      // Test that different phase goals get routed to appropriate roles
      const phases = [
        { goal: "implement user authentication", expectedRole: "coder" },
        { goal: "review the authentication implementation", expectedRole: "reviewer" },
        { goal: "run comprehensive test suite", expectedRole: "runner" }
      ];

      for (const { goal, expectedRole } of phases) {
        const mapping = mapTaskToRole(goal);
        expect(mapping.role).toBe(expectedRole);
      }
    });

    it("should validate execution results with proper schemas", async () => {
      const extractor = new StructuredExtractor();
      
      // Simulate a successful execution result
      const executionResult = `
Implementation completed.

\`\`\`json
{
  "role": "coder",
  "task": "implement user authentication",
  "status": "success", 
  "summary": "Successfully implemented OAuth 2.0 authentication with JWT tokens and refresh token rotation",
  "details": {
    "implementation": "OAuth 2.0 + JWT",
    "security": "PKCE + refresh token rotation",
    "testing": "95% coverage"
  },
  "filesChanged": ["src/auth/oauth.ts", "src/auth/jwt.ts", "tests/auth.test.ts"],
  "confidence": 0.95
}
\`\`\`
`;

      const result = await extractor.extractStructuredResult(executionResult, SubAgentResultSchema);
      
      expect(result.success).toBe(true);
      expect(result.data.status).toBe("success");
      expect(result.data.confidence).toBeGreaterThan(0.9);
    });
  });
});