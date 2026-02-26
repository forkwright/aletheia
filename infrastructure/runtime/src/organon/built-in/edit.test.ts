import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";

vi.mock("../../taxis/workspace-git.js", () => ({
  commitWorkspaceChange: vi.fn(),
}));

const { editTool } = await import("./edit.js");

describe("editTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("replaces text when old_text found once", async () => {
    writeFileSync(join(dir, "f.txt"), "hello world");
    const result = await editTool.execute(
      { path: "f.txt", old_text: "world", new_text: "earth" },
      ctx,
    );
    expect(result).toContain("Edited");
    expect(readFileSync(join(dir, "f.txt"), "utf-8")).toBe("hello earth");
  });

  it("rejects empty old_text", async () => {
    writeFileSync(join(dir, "f.txt"), "content");
    const result = await editTool.execute(
      { path: "f.txt", old_text: "", new_text: "x" },
      ctx,
    );
    expect(result).toContain("old_text cannot be empty");
  });

  it("rejects when old_text not found", async () => {
    writeFileSync(join(dir, "f.txt"), "hello world");
    const result = await editTool.execute(
      { path: "f.txt", old_text: "missing", new_text: "x" },
      ctx,
    );
    expect(result).toContain("old_text not found");
  });

  it("rejects when old_text matches multiple locations", async () => {
    writeFileSync(join(dir, "f.txt"), "aaa bbb aaa");
    const result = await editTool.execute(
      { path: "f.txt", old_text: "aaa", new_text: "zzz" },
      ctx,
    );
    expect(result).toContain("multiple locations");
  });
});
