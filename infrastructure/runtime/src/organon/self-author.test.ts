// Self-authoring tool tests
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadAuthoredTools, createSelfAuthorTools } from "./self-author.js";
import type { ToolRegistry, ToolHandler } from "./registry.js";

let tmpDir: string;
let workspace: string;
let authoredDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "self-author-"));
  // self-author computes dir as workspace/../../shared/tools/authored
  workspace = join(tmpDir, "nous", "syn");
  mkdirSync(workspace, { recursive: true });
  authoredDir = join(tmpDir, "shared", "tools", "authored");
  mkdirSync(authoredDir, { recursive: true });
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

function makeRegistry(): ToolRegistry & { registered: ToolHandler[] } {
  const registered: ToolHandler[] = [];
  return {
    registered,
    register(handler: ToolHandler) { registered.push(handler); },
    get: vi.fn(),
    execute: vi.fn(),
    getDefinitions: vi.fn().mockReturnValue([]),
  } as never;
}

describe("loadAuthoredTools", () => {
  it("returns 0 with empty directory", () => {
    const registry = makeRegistry();
    const loaded = loadAuthoredTools(workspace, registry);
    expect(loaded).toBe(0);
  });

  it("loads a valid authored tool", () => {
    const code = `
exports.definition = { name: "test_tool", description: "A test", input_schema: { type: "object", properties: {} } };
exports.execute = function(input) { return "ok"; };
`;
    writeFileSync(join(authoredDir, "test_tool.tool.mjs"), code);
    writeFileSync(join(authoredDir, "test_tool.meta.json"), JSON.stringify({
      name: "test_tool", author: "syn", version: 1, failures: 0, quarantined: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }));

    const registry = makeRegistry();
    const loaded = loadAuthoredTools(workspace, registry);
    expect(loaded).toBe(1);
    expect(registry.registered).toHaveLength(1);
  });

  it("skips quarantined tools", () => {
    writeFileSync(join(authoredDir, "bad_tool.tool.mjs"), `exports.definition = {}; exports.execute = () => "";`);
    writeFileSync(join(authoredDir, "bad_tool.meta.json"), JSON.stringify({
      name: "bad_tool", author: "syn", version: 1, failures: 3, quarantined: true,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }));

    const registry = makeRegistry();
    const loaded = loadAuthoredTools(workspace, registry);
    expect(loaded).toBe(0);
  });

  it("skips files without .tool.mjs extension", () => {
    writeFileSync(join(authoredDir, "readme.md"), "# Notes");
    const registry = makeRegistry();
    const loaded = loadAuthoredTools(workspace, registry);
    expect(loaded).toBe(0);
  });
});

describe("createSelfAuthorTools", () => {
  it("returns 3 tool handlers", () => {
    const registry = makeRegistry();
    const tools = createSelfAuthorTools(workspace, registry);
    expect(tools).toHaveLength(3);
    expect(tools[0]!.definition.name).toBe("tool_create");
    expect(tools[1]!.definition.name).toBe("tool_list_authored");
    expect(tools[2]!.definition.name).toBe("tool_record_failure");
  });

  it("tool_create creates a new tool", async () => {
    const registry = makeRegistry();
    const [toolCreate] = createSelfAuthorTools(workspace, registry);
    const code = `
exports.definition = { name: "new_tool", description: "new", input_schema: { type: "object", properties: {} } };
exports.execute = function() { return "created"; };
`;
    const result = await toolCreate!.execute(
      { name: "new_tool", code },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.created).toBe(true);
    expect(parsed.name).toBe("new_tool");
    expect(parsed.version).toBe(1);
  });

  it("tool_create rejects oversized code", async () => {
    const registry = makeRegistry();
    const [toolCreate] = createSelfAuthorTools(workspace, registry);
    const bigCode = "x".repeat(10000);
    const result = await toolCreate!.execute(
      { name: "big", code: bigCode },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    expect(JSON.parse(result).error).toContain("byte limit");
  });

  it("tool_create rejects invalid name", async () => {
    const registry = makeRegistry();
    const [toolCreate] = createSelfAuthorTools(workspace, registry);
    const result = await toolCreate!.execute(
      { name: "!!!!", code: "exports.definition = {}; exports.execute = () => '';" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    expect(JSON.parse(result).error).toContain("Invalid tool name");
  });

  it("tool_list_authored lists tools with metadata", async () => {
    const registry = makeRegistry();
    const tools = createSelfAuthorTools(workspace, registry);
    const toolList = tools[1]!;

    writeFileSync(join(authoredDir, "t1.meta.json"), JSON.stringify({
      name: "t1", author: "syn", version: 2, failures: 0, quarantined: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }));

    const result = await toolList.execute({}, { nousId: "syn", sessionId: "ses_1", workspace });
    const parsed = JSON.parse(result);
    expect(parsed.tools).toHaveLength(1);
    expect(parsed.tools[0].name).toBe("t1");
    expect(parsed.tools[0].version).toBe(2);
  });

  it("tool_record_failure increments failures", async () => {
    const registry = makeRegistry();
    const tools = createSelfAuthorTools(workspace, registry);
    const toolRecordFailure = tools[2]!;

    writeFileSync(join(authoredDir, "flaky.meta.json"), JSON.stringify({
      name: "flaky", author: "syn", version: 1, failures: 0, quarantined: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }));

    const result = await toolRecordFailure.execute(
      { name: "flaky", error: "timeout" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.failures).toBe(1);
    expect(parsed.quarantined).toBe(false);
  });

  it("tool_record_failure quarantines after 3 failures", async () => {
    const registry = makeRegistry();
    const tools = createSelfAuthorTools(workspace, registry);
    const toolRecordFailure = tools[2]!;

    writeFileSync(join(authoredDir, "bad.meta.json"), JSON.stringify({
      name: "bad", author: "syn", version: 1, failures: 2, quarantined: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }));

    const result = await toolRecordFailure.execute(
      { name: "bad" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    const parsed = JSON.parse(result);
    expect(parsed.failures).toBe(3);
    expect(parsed.quarantined).toBe(true);
  });

  it("tool_record_failure returns error for unknown tool", async () => {
    const registry = makeRegistry();
    const tools = createSelfAuthorTools(workspace, registry);
    const toolRecordFailure = tools[2]!;

    const result = await toolRecordFailure.execute(
      { name: "nonexistent" },
      { nousId: "syn", sessionId: "ses_1", workspace },
    );
    expect(JSON.parse(result).error).toContain("not found");
  });
});
