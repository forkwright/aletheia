// Tests for structured extraction with instructor-js and Zod validation
import { describe, expect, it, vi } from "vitest";
import { 
  classifyTask,
  extractStructured,
  parseDispatchResponse,
  parseSubAgentResponse,
  schemas,
  selectRoleForTask,
  taskTypeToRole 
} from "./structured-extraction.js";

describe("Structured Extraction", () => {
  describe("extractStructured", () => {
    it("should extract valid JSON from response", async () => {
      const response = "Here's the result:\n\n```json\n{\"status\": \"success\", \"confidence\": 0.9}\n```";
      const schema = schemas.SubAgentResult.partial();
      
      const result = await extractStructured(response, schema);
      
      expect(result).toEqual({ status: "success", confidence: 0.9 });
    });

    it("should return null for missing JSON block", async () => {
      const response = "No JSON here";
      const schema = schemas.SubAgentResult;
      
      const result = await extractStructured(response, schema);
      
      expect(result).toBeNull();
    });

    it("should call retry callback on validation failure", async () => {
      const response = "```json\n{\"invalid\": \"data\"}\n```";
      const schema = schemas.SubAgentResult;
      const retryResponse = "```json\n{\"role\": \"coder\", \"task\": \"test\", \"status\": \"success\", \"summary\": \"done\", \"details\": {}, \"confidence\": 0.8}\n```";
      const retryCallback = vi.fn().mockResolvedValue(retryResponse);
      
      const result = await extractStructured(response, schema, retryCallback);
      
      expect(retryCallback).toHaveBeenCalledWith(expect.stringContaining("Schema validation failed"));
      expect(result).toMatchObject({
        role: "coder",
        task: "test", 
        status: "success",
        summary: "done",
        confidence: 0.8
      });
    });

    it("should call retry callback on JSON parse failure", async () => {
      const response = "```json\n{invalid json}\n```";
      const schema = schemas.SubAgentResult;
      const retryResponse = "```json\n{\"role\": \"coder\", \"task\": \"test\", \"status\": \"success\", \"summary\": \"done\", \"details\": {}, \"confidence\": 0.8}\n```";
      const retryCallback = vi.fn().mockResolvedValue(retryResponse);
      
      const result = await extractStructured(response, schema, retryCallback);
      
      expect(retryCallback).toHaveBeenCalledWith(expect.stringContaining("JSON parsing failed"));
      expect(result).toMatchObject({
        role: "coder",
        status: "success",
        confidence: 0.8
      });
    });

    it("should not retry twice", async () => {
      const response = "```json\n{\"invalid\": \"data\"}\n```";
      const schema = schemas.SubAgentResult;
      const retryResponse = "```json\n{\"still\": \"invalid\"}\n```";
      const retryCallback = vi.fn().mockResolvedValue(retryResponse);
      
      const result = await extractStructured(response, schema, retryCallback);
      
      expect(retryCallback).toHaveBeenCalledTimes(1);
      expect(result).toBeNull();
    });
  });

  describe("parseSubAgentResponse", () => {
    it("should parse valid sub-agent response", async () => {
      const response = [
        "Task completed successfully.",
        "",
        "```json",
        "{",
        "  \"role\": \"coder\",", 
        "  \"task\": \"implement feature\",",
        "  \"status\": \"success\",",
        "  \"summary\": \"Feature implemented successfully\",",
        "  \"details\": {\"filesCreated\": [\"feature.ts\"]},",
        "  \"filesChanged\": [\"src/feature.ts\"],",
        "  \"confidence\": 0.95",
        "}",
        "```"
      ].join("\n");

      const result = await parseSubAgentResponse(response);
      
      expect(result).toMatchObject({
        role: "coder",
        task: "implement feature",
        status: "success",
        summary: "Feature implemented successfully",
        confidence: 0.95,
        filesChanged: ["src/feature.ts"]
      });
    });

    it("should handle optional fields correctly", async () => {
      const response = [
        "```json",
        "{",
        "  \"role\": \"explorer\",",
        "  \"task\": \"find files\",",
        "  \"status\": \"success\",",
        "  \"summary\": \"Found 5 files\",",
        "  \"details\": {},",
        "  \"confidence\": 0.8",
        "}",
        "```"
      ].join("\n");

      const result = await parseSubAgentResponse(response);
      
      expect(result).toMatchObject({
        role: "explorer",
        task: "find files",
        status: "success",
        summary: "Found 5 files",
        confidence: 0.8
      });
      expect(result?.filesChanged).toBeUndefined();
      expect(result?.issues).toBeUndefined();
    });
  });

  describe("parseDispatchResponse", () => {
    it("should parse valid dispatch response", async () => {
      const response = [
        "```json",
        "{",
        "  \"taskCount\": 2,",
        "  \"succeeded\": 1,",
        "  \"failed\": 1,",
        "  \"results\": [",
        "    {",
        "      \"index\": 0,",
        "      \"task\": \"task 1\",",
        "      \"status\": \"success\",",
        "      \"result\": \"done\",",
        "      \"durationMs\": 100",
        "    },",
        "    {",
        "      \"index\": 1,",
        "      \"task\": \"task 2\",",
        "      \"status\": \"error\",",
        "      \"error\": \"failed\",",
        "      \"durationMs\": 50",
        "    }",
        "  ],",
        "  \"timing\": {",
        "    \"wallClockMs\": 150,",
        "    \"sequentialMs\": 150,",
        "    \"savedMs\": 0",
        "  },",
        "  \"totalTokens\": 1000",
        "}",
        "```"
      ].join("\n");

      const result = await parseDispatchResponse(response);
      
      expect(result).toMatchObject({
        taskCount: 2,
        succeeded: 1,
        failed: 1,
        results: expect.arrayContaining([
          expect.objectContaining({ status: "success", task: "task 1" }),
          expect.objectContaining({ status: "error", task: "task 2" })
        ]),
        totalTokens: 1000
      });
    });
  });
});

describe("Achievement and Blocker Schemas", () => {
  it("should parse sub-agent response with achievements", async () => {
    const response = JSON.stringify({
      role: "coder",
      task: "implement auth",
      status: "success",
      summary: "Auth implemented",
      details: {},
      confidence: 0.9,
      achievements: [
        { claim: "Created auth middleware", evidence: "src/auth.ts:15", verifiable: true },
        { claim: "Added JWT validation", evidence: "src/jwt.ts:1" },
      ],
      blockers: ["Need to configure OAuth provider"],
    });

    const result = await parseSubAgentResponse(response);

    expect(result).toBeDefined();
    expect(result?.achievements).toHaveLength(2);
    expect(result?.achievements?.[0]).toMatchObject({
      claim: "Created auth middleware",
      evidence: "src/auth.ts:15",
      verifiable: true,
    });
    expect(result?.achievements?.[1]?.verifiable).toBeUndefined();
    expect(result?.blockers).toEqual(["Need to configure OAuth provider"]);
  });

  it("should parse sub-agent response without achievements (backward compat)", async () => {
    const response = JSON.stringify({
      role: "coder",
      task: "implement feature",
      status: "success",
      summary: "Done",
      details: {},
      confidence: 0.8,
    });

    const result = await parseSubAgentResponse(response);

    expect(result).toBeDefined();
    expect(result?.achievements).toBeUndefined();
    expect(result?.blockers).toBeUndefined();
  });

  it("should parse blockers array on failed response", async () => {
    const response = JSON.stringify({
      role: "coder",
      task: "migrate database",
      status: "failed",
      summary: "Migration blocked",
      details: {},
      confidence: 0.2,
      blockers: ["Database is read-only", "Missing migration scripts"],
    });

    const result = await parseSubAgentResponse(response);

    expect(result).toBeDefined();
    expect(result?.blockers).toHaveLength(2);
    expect(result?.blockers).toContain("Database is read-only");
  });
});

describe("Task Classification", () => {
  describe("classifyTask", () => {
    it("should classify code generation tasks", () => {
      const result = classifyTask("implement a new user authentication feature");
      expect(result.type).toBe("code-generation");
      expect(result.complexity).toBe("medium");
      expect(result.requiresTooling).toBe(true);
      expect(result.readOnly).toBe(false);
    });

    it("should classify code editing tasks", () => {
      const result = classifyTask("fix the bug in user login function");
      expect(result.type).toBe("code-editing");
      expect(result.complexity).toBe("medium");
      expect(result.requiresTooling).toBe(true);
      expect(result.readOnly).toBe(false);
    });

    it("should classify code review tasks", () => {
      const result = classifyTask("review the changes in this PR for security issues");
      expect(result.type).toBe("code-review");
      expect(result.complexity).toBe("low");
      expect(result.requiresTooling).toBe(false);
      expect(result.readOnly).toBe(true);
    });

    it("should classify exploration tasks", () => {
      const result = classifyTask("find all references to the UserService class");
      expect(result.type).toBe("exploration");
      expect(result.complexity).toBe("low");
      expect(result.requiresTooling).toBe(false);
      expect(result.readOnly).toBe(true);
    });

    it("should classify testing tasks", () => {
      const result = classifyTask("run the unit tests for the auth module");
      expect(result.type).toBe("testing");
      expect(result.complexity).toBe("low");
      expect(result.requiresTooling).toBe(true);
      expect(result.readOnly).toBe(true);
    });

    it("should classify research tasks", () => {
      const result = classifyTask("research the best practices for JWT token validation");
      expect(result.type).toBe("research");
      expect(result.complexity).toBe("medium");
      expect(result.requiresTooling).toBe(true);
      expect(result.readOnly).toBe(true);
    });

    it("should classify planning tasks", () => {
      const result = classifyTask("design the architecture for the new payment system");
      expect(result.type).toBe("planning");
      expect(result.complexity).toBe("high");
      expect(result.requiresTooling).toBe(false);
      expect(result.readOnly).toBe(false);
    });

    it("should classify verification tasks", () => {
      const result = classifyTask("verify that all requirements have been met");
      expect(result.type).toBe("verification");
      expect(result.complexity).toBe("medium");
      expect(result.requiresTooling).toBe(false);
      expect(result.readOnly).toBe(true);
    });

    it("should default to code generation for ambiguous tasks", () => {
      const result = classifyTask("handle the thing");
      expect(result.type).toBe("code-generation");
      expect(result.complexity).toBe("medium");
    });
  });

  describe("taskTypeToRole", () => {
    it("should map code generation to coder", () => {
      const classification = { type: "code-generation" as const, complexity: "medium" as const, requiresTooling: true, readOnly: false };
      expect(taskTypeToRole(classification)).toBe("coder");
    });

    it("should map code editing to coder", () => {
      const classification = { type: "code-editing" as const, complexity: "medium" as const, requiresTooling: true, readOnly: false };
      expect(taskTypeToRole(classification)).toBe("coder");
    });

    it("should map code review to reviewer", () => {
      const classification = { type: "code-review" as const, complexity: "low" as const, requiresTooling: false, readOnly: true };
      expect(taskTypeToRole(classification)).toBe("reviewer");
    });

    it("should map exploration to explorer", () => {
      const classification = { type: "exploration" as const, complexity: "low" as const, requiresTooling: false, readOnly: true };
      expect(taskTypeToRole(classification)).toBe("explorer");
    });

    it("should map testing to runner", () => {
      const classification = { type: "testing" as const, complexity: "low" as const, requiresTooling: true, readOnly: true };
      expect(taskTypeToRole(classification)).toBe("runner");
    });

    it("should map research to researcher", () => {
      const classification = { type: "research" as const, complexity: "medium" as const, requiresTooling: true, readOnly: true };
      expect(taskTypeToRole(classification)).toBe("researcher");
    });

    it("should map high complexity planning to coder", () => {
      const classification = { type: "planning" as const, complexity: "high" as const, requiresTooling: false, readOnly: false };
      expect(taskTypeToRole(classification)).toBe("coder");
    });

    it("should map low complexity planning to reviewer", () => {
      const classification = { type: "planning" as const, complexity: "low" as const, requiresTooling: false, readOnly: false };
      expect(taskTypeToRole(classification)).toBe("reviewer");
    });
  });

  describe("selectRoleForTask", () => {
    it("should select appropriate role end-to-end", () => {
      expect(selectRoleForTask("implement user login")).toBe("coder");
      expect(selectRoleForTask("fix authentication bug")).toBe("coder");
      expect(selectRoleForTask("review this pull request")).toBe("reviewer");
      expect(selectRoleForTask("find all TODO comments")).toBe("explorer");
      expect(selectRoleForTask("run the test suite")).toBe("runner");
      expect(selectRoleForTask("research OAuth 2.0 best practices")).toBe("researcher");
    });
  });
});