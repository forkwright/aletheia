// Research meta-tool tests
import { describe, expect, it, vi } from "vitest";
import { createResearchTool } from "./research.js";
import { ToolRegistry } from "../registry.js";

function makeRegistry() {
  const registry = new ToolRegistry();
  registry.register({
    definition: { name: "mem0_search", description: "", input_schema: { type: "object", properties: {} } },
    async execute() {
      return JSON.stringify({ results: [{ memory: "user prefers dark mode", score: 0.9 }], count: 1 });
    },
  });
  registry.register({
    definition: { name: "web_search", description: "", input_schema: { type: "object", properties: {} } },
    async execute() { return "1. Result Title\n   https://example.com\n   Snippet text"; },
  });
  return registry;
}

const ctx = { nousId: "test", sessionId: "s1" };

describe("research", () => {
  it("searches memory and web when memory insufficient", async () => {
    const registry = makeRegistry();
    const tool = createResearchTool(registry);
    const raw = await tool.execute({ query: "dark mode preference" }, ctx);
    const result = JSON.parse(raw);
    expect(result.query).toBe("dark mode preference");
    expect(result.memory.count).toBe(1);
    // 1 result < 2 threshold → web search triggered
    expect(result.web.results).toBeDefined();
    expect(result.sources.memory).toBe(true);
    expect(result.sources.web).toBe(true);
  });

  it("skips web when memory has enough results", async () => {
    const registry = new ToolRegistry();
    registry.register({
      definition: { name: "mem0_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() {
        return JSON.stringify({
          results: [
            { memory: "fact 1", score: 0.9 },
            { memory: "fact 2", score: 0.8 },
            { memory: "fact 3", score: 0.7 },
          ],
          count: 3,
        });
      },
    });
    registry.register({
      definition: { name: "web_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return "should not be called"; },
    });

    const tool = createResearchTool(registry);
    const raw = await tool.execute({ query: "test" }, ctx);
    const result = JSON.parse(raw);
    expect(result.memory.count).toBe(3);
    expect(result.web.skipped).toBe(true);
    expect(result.sources.web).toBe(false);
  });

  it("forces web search when web=true", async () => {
    const registry = new ToolRegistry();
    registry.register({
      definition: { name: "mem0_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() {
        return JSON.stringify({
          results: [{ memory: "a", score: 0.9 }, { memory: "b", score: 0.8 }, { memory: "c", score: 0.7 }],
          count: 3,
        });
      },
    });
    registry.register({
      definition: { name: "web_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return "web result"; },
    });

    const tool = createResearchTool(registry);
    const raw = await tool.execute({ query: "test", web: true }, ctx);
    const result = JSON.parse(raw);
    expect(result.web.results).toBe("web result");
    expect(result.sources.web).toBe(true);
  });

  it("handles memory search failure gracefully", async () => {
    const registry = new ToolRegistry();
    // mem0_search returns unparseable result → caught as error
    registry.register({
      definition: { name: "mem0_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { throw new Error("sidecar down"); },
    });
    registry.register({
      definition: { name: "web_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return "web fallback"; },
    });

    const tool = createResearchTool(registry);
    const raw = await tool.execute({ query: "test" }, ctx);
    const result = JSON.parse(raw);
    expect(result.memory.error).toBeDefined();
    expect(result.web.results).toBe("web fallback");
  });

  it("prefers brave_search over web_search when available", async () => {
    const registry = new ToolRegistry();
    registry.register({
      definition: { name: "mem0_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return JSON.stringify({ results: [], count: 0 }); },
    });
    registry.register({
      definition: { name: "brave_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return "brave result"; },
    });
    registry.register({
      definition: { name: "web_search", description: "", input_schema: { type: "object", properties: {} } },
      async execute() { return "ddg result"; },
    });

    const tool = createResearchTool(registry);
    const raw = await tool.execute({ query: "test" }, ctx);
    const result = JSON.parse(raw);
    expect(result.web.source).toBe("brave_search");
    expect(result.web.results).toBe("brave result");
  });
});
