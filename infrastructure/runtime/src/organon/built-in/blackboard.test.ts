// Blackboard tool tests
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { SessionStore } from "../../mneme/store.js";
import { createBlackboardTool } from "./blackboard.js";
import type { ToolContext } from "../registry.js";

let store: SessionStore;
let tmpDir: string;

const ctx: ToolContext = {
  nousId: "agent-a",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

const ctxB: ToolContext = {
  nousId: "agent-b",
  sessionId: "sess-2",
  workspace: "/tmp/test",
};

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "bb-test-"));
  store = new SessionStore(join(tmpDir, "test.db"));
});

afterEach(() => {
  store.close();
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("blackboard tool", () => {
  it("writes and reads an entry", async () => {
    const tool = createBlackboardTool(store);

    const writeResult = JSON.parse(
      await tool.execute({ action: "write", key: "status", value: "working on task X" }, ctx),
    );
    expect(writeResult.written).toBe(true);
    expect(writeResult.key).toBe("status");

    const readResult = JSON.parse(
      await tool.execute({ action: "read", key: "status" }, ctx),
    );
    expect(readResult.entries).toHaveLength(1);
    expect(readResult.entries[0].value).toBe("working on task X");
    expect(readResult.entries[0].author).toBe("agent-a");
  });

  it("lists all keys", async () => {
    const tool = createBlackboardTool(store);

    await tool.execute({ action: "write", key: "k1", value: "v1" }, ctx);
    await tool.execute({ action: "write", key: "k2", value: "v2" }, ctxB);

    const listResult = JSON.parse(
      await tool.execute({ action: "list" }, ctx),
    );
    expect(listResult.keys).toHaveLength(2);
  });

  it("deletes only own entries", async () => {
    const tool = createBlackboardTool(store);

    await tool.execute({ action: "write", key: "shared", value: "from A" }, ctx);
    await tool.execute({ action: "write", key: "shared", value: "from B" }, ctxB);

    // Agent A deletes their entry
    const delResult = JSON.parse(
      await tool.execute({ action: "delete", key: "shared" }, ctx),
    );
    expect(delResult.deleted).toBe(1);

    // Agent B's entry remains
    const readResult = JSON.parse(
      await tool.execute({ action: "read", key: "shared" }, ctx),
    );
    expect(readResult.entries).toHaveLength(1);
    expect(readResult.entries[0].author).toBe("agent-b");
  });

  it("returns error for missing key on write", async () => {
    const tool = createBlackboardTool(store);
    const result = JSON.parse(
      await tool.execute({ action: "write" }, ctx),
    );
    expect(result.error).toBeTruthy();
  });

  it("returns error for unknown action", async () => {
    const tool = createBlackboardTool(store);
    const result = JSON.parse(
      await tool.execute({ action: "unknown" }, ctx),
    );
    expect(result.error).toContain("Unknown action");
  });

  it("overwrites same-key entry from same author", async () => {
    const tool = createBlackboardTool(store);

    await tool.execute({ action: "write", key: "progress", value: "50%" }, ctx);
    await tool.execute({ action: "write", key: "progress", value: "75%" }, ctx);

    const readResult = JSON.parse(
      await tool.execute({ action: "read", key: "progress" }, ctx),
    );
    expect(readResult.entries).toHaveLength(1);
    expect(readResult.entries[0].value).toBe("75%");
  });
});
