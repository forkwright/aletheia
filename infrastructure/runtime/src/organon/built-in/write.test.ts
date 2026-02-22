import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";

vi.mock("../../taxis/workspace-git.js", () => ({
  commitWorkspaceChange: vi.fn(),
}));

const { writeTool } = await import("./write.js");

describe("writeTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("writes new file", async () => {
    const result = await writeTool.execute({ path: "new.txt", content: "hello" }, ctx);
    expect(result).toContain("Written");
    expect(readFileSync(join(dir, "new.txt"), "utf-8")).toBe("hello");
  });

  it("overwrites existing file", async () => {
    writeFileSync(join(dir, "exist.txt"), "old");
    await writeTool.execute({ path: "exist.txt", content: "new" }, ctx);
    expect(readFileSync(join(dir, "exist.txt"), "utf-8")).toBe("new");
  });

  it("appends with append flag", async () => {
    writeFileSync(join(dir, "log.txt"), "first\n");
    await writeTool.execute({ path: "log.txt", content: "second\n", append: true }, ctx);
    expect(readFileSync(join(dir, "log.txt"), "utf-8")).toBe("first\nsecond\n");
  });

  it("creates parent directories", async () => {
    const result = await writeTool.execute({ path: "a/b/c.txt", content: "deep" }, ctx);
    expect(result).toContain("Written");
    expect(readFileSync(join(dir, "a/b/c.txt"), "utf-8")).toBe("deep");
  });
});
