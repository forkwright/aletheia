// Context check meta-tool tests
import { describe, it, expect } from "vitest";
import { createContextCheckTool } from "./context-check.js";
import { ToolRegistry } from "../registry.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = {
  nousId: "test-agent",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

describe("createContextCheckTool", () => {
  it("returns tool with correct name", () => {
    const registry = new ToolRegistry();
    const tool = createContextCheckTool(registry);
    expect(tool.definition.name).toBe("context_check");
  });

  it("returns error info when sub-tools missing", async () => {
    const registry = new ToolRegistry();
    const tool = createContextCheckTool(registry);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.session.error).toBeTruthy();
    expect(result.calibration.error).toBeTruthy();
  });

  it("combines results from sub-tools", async () => {
    const registry = new ToolRegistry();
    registry.register({
      definition: { name: "session_status", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return JSON.stringify({ model: "test", messageCount: 5 }); },
    });
    registry.register({
      definition: { name: "check_calibration", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return JSON.stringify({ overallScore: 0.7 }); },
    });

    const tool = createContextCheckTool(registry);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.session.model).toBe("test");
    expect(result.calibration.overallScore).toBe(0.7);
  });
});
