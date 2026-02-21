// Tool registry tests
import { describe, expect, it } from "vitest";
import { type ToolContext, type ToolHandler, ToolRegistry, matchesToolFilter } from "./registry.js";

function makeHandler(name: string, result = "ok"): ToolHandler {
  return {
    definition: { name, description: `${name} tool`, input_schema: {} },
    execute: async () => result,
  };
}

const ctx: ToolContext = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("ToolRegistry", () => {
  it("registers and retrieves tools", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("grep"));
    expect(reg.get("grep")).toBeDefined();
    expect(reg.size).toBe(1);
  });

  it("overwrites on name collision", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("grep", "old"));
    reg.register(makeHandler("grep", "new"));
    expect(reg.size).toBe(1);
  });

  it("get returns undefined for missing tool", () => {
    const reg = new ToolRegistry();
    expect(reg.get("nope")).toBeUndefined();
  });

  it("getDefinitions returns all tool definitions", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("a"));
    reg.register(makeHandler("b"));
    const defs = reg.getDefinitions();
    expect(defs).toHaveLength(2);
    expect(defs.map((d) => d.name)).toEqual(["a", "b"]);
  });

  it("getDefinitions filters by allow list", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("a"));
    reg.register(makeHandler("b"));
    reg.register(makeHandler("c"));
    const defs = reg.getDefinitions({ allow: ["a", "c"] });
    expect(defs.map((d) => d.name)).toEqual(["a", "c"]);
  });

  it("getDefinitions filters by deny list", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("a"));
    reg.register(makeHandler("b"));
    reg.register(makeHandler("c"));
    const defs = reg.getDefinitions({ deny: ["b"] });
    expect(defs.map((d) => d.name)).toEqual(["a", "c"]);
  });

  it("execute returns handler result", async () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("test", '{"data":1}'));
    const result = await reg.execute("test", {}, ctx);
    expect(result).toBe('{"data":1}');
  });

  it("execute returns error JSON for unknown tool", async () => {
    const reg = new ToolRegistry();
    const result = await reg.execute("nope", {}, ctx);
    expect(JSON.parse(result)).toEqual({ error: "Unknown tool: nope" });
  });

  it("execute truncates large results", async () => {
    const reg = new ToolRegistry();
    const bigResult = "x".repeat(100_000);
    reg.register(makeHandler("big", bigResult));
    const result = await reg.execute("big", {}, ctx);
    expect(result.length).toBeLessThan(bigResult.length);
  });

  it("getDefinitions filters by toolFilter glob patterns", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("read"));
    reg.register(makeHandler("write"));
    reg.register(makeHandler("grep"));
    reg.register(makeHandler("mem0_search"));
    reg.register(makeHandler("mem0_add"));
    const defs = reg.getDefinitions({ toolFilter: ["read", "mem0_*"] });
    expect(defs.map((d) => d.name)).toEqual(["read", "mem0_search", "mem0_add"]);
  });

  it("toolFilter with wildcard-only matches all", () => {
    const reg = new ToolRegistry();
    reg.register(makeHandler("a"));
    reg.register(makeHandler("b"));
    const defs = reg.getDefinitions({ toolFilter: ["*"] });
    expect(defs).toHaveLength(2);
  });
});

describe("matchesToolFilter", () => {
  it("matches exact names", () => {
    expect(matchesToolFilter("read", ["read", "write"])).toBe(true);
    expect(matchesToolFilter("exec", ["read", "write"])).toBe(false);
  });

  it("matches wildcard at end", () => {
    expect(matchesToolFilter("mem0_search", ["mem0_*"])).toBe(true);
    expect(matchesToolFilter("mem0_add", ["mem0_*"])).toBe(true);
    expect(matchesToolFilter("grep", ["mem0_*"])).toBe(false);
  });

  it("matches wildcard at start", () => {
    expect(matchesToolFilter("graph_search", ["*_search"])).toBe(true);
    expect(matchesToolFilter("mem0_search", ["*_search"])).toBe(true);
    expect(matchesToolFilter("read", ["*_search"])).toBe(false);
  });

  it("matches star-only pattern", () => {
    expect(matchesToolFilter("anything", ["*"])).toBe(true);
  });

  it("returns false for empty patterns", () => {
    expect(matchesToolFilter("read", [])).toBe(false);
  });
});
