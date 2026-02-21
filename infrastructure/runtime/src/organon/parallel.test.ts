import { describe, it, expect } from "vitest";
import { groupForParallelExecution } from "./parallel.js";
import type { ToolUseBlock } from "../hermeneus/anthropic.js";

function tool(name: string, input: Record<string, unknown> = {}): ToolUseBlock {
  return { type: "tool_use", id: `tool-${name}-${Math.random().toString(36).slice(2, 6)}`, name, input };
}

describe("groupForParallelExecution", () => {
  it("returns empty array for no tools", () => {
    expect(groupForParallelExecution([])).toEqual([]);
  });

  it("returns single batch for one tool", () => {
    const tools = [tool("read", { path: "/foo" })];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toHaveLength(1);
  });

  it("groups all read-only tools into one batch", () => {
    const tools = [
      tool("grep", { pattern: "foo", path: "/a" }),
      tool("read", { path: "/b" }),
      tool("find", { pattern: "*.ts", path: "/c" }),
      tool("ls", { path: "/d" }),
      tool("mem0_search", { query: "test" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toHaveLength(5);
  });

  it("splits at exec (never-parallel) tool", () => {
    const tools = [
      tool("grep", { pattern: "foo" }),
      tool("read", { path: "/a" }),
      tool("exec", { command: "npm test" }),
      tool("read", { path: "/b" }),
      tool("grep", { pattern: "bar" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(3);
    expect(groups[0]).toHaveLength(2); // grep + read
    expect(groups[1]).toHaveLength(1); // exec alone
    expect(groups[2]).toHaveLength(2); // read + grep
  });

  it("splits edits to the same file into separate batches", () => {
    const tools = [
      tool("edit", { path: "/foo.ts" }),
      tool("edit", { path: "/foo.ts" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(2);
    expect(groups[0]).toHaveLength(1);
    expect(groups[1]).toHaveLength(1);
  });

  it("keeps edits to different files in same batch", () => {
    const tools = [
      tool("edit", { path: "/foo.ts" }),
      tool("edit", { path: "/bar.ts" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toHaveLength(2);
  });

  it("handles mixed always, conditional, and never tools", () => {
    const tools = [
      tool("grep", { pattern: "a" }),
      tool("grep", { pattern: "b" }),
      tool("grep", { pattern: "c" }),
      tool("exec", { command: "build" }),
      tool("read", { path: "/x" }),
      tool("edit", { path: "/x" }),
      tool("message", { to: "user", text: "done" }),
      tool("read", { path: "/y" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(5);
    expect(groups[0]).toHaveLength(3); // 3x grep
    expect(groups[1]).toHaveLength(1); // exec (never)
    expect(groups[2]).toHaveLength(2); // read + edit (no conflict — first touch of /x)
    expect(groups[3]).toHaveLength(1); // message (never)
    expect(groups[4]).toHaveLength(1); // read /y
  });

  it("treats unknown tools as never-parallel", () => {
    const tools = [
      tool("read", { path: "/a" }),
      tool("some_custom_tool", {}),
      tool("read", { path: "/b" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(3);
    expect(groups[1]).toHaveLength(1);
    expect(groups[1]![0]!.name).toBe("some_custom_tool");
  });

  it("handles consecutive never-parallel tools", () => {
    const tools = [
      tool("exec", { command: "a" }),
      tool("exec", { command: "b" }),
      tool("message", { to: "user" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(3);
    expect(groups[0]).toHaveLength(1);
    expect(groups[1]).toHaveLength(1);
    expect(groups[2]).toHaveLength(1);
  });

  it("handles blackboard conditional on same key", () => {
    const tools = [
      tool("blackboard", { action: "write", key: "status" }),
      tool("blackboard", { action: "write", key: "status" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(2);
  });

  it("handles blackboard conditional on different keys", () => {
    const tools = [
      tool("blackboard", { action: "write", key: "status" }),
      tool("blackboard", { action: "read", key: "other" }),
    ];
    const groups = groupForParallelExecution(tools);
    expect(groups).toHaveLength(1);
    expect(groups[0]).toHaveLength(2);
  });
});
