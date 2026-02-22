import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";
import { findTool } from "./find.js";

describe("findTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("finds files matching pattern", async () => {
    writeFileSync(join(dir, "foo.ts"), "");
    writeFileSync(join(dir, "bar.ts"), "");
    mkdirSync(join(dir, "sub"));
    writeFileSync(join(dir, "sub", "baz.ts"), "");
    const result = await findTool.execute({ pattern: "\\.ts$", path: dir }, ctx);
    expect(result).toContain("foo.ts");
    expect(result).toContain("bar.ts");
    expect(result).toContain("baz.ts");
  });

  it("returns no files found for no matches", async () => {
    writeFileSync(join(dir, "readme.md"), "");
    const result = await findTool.execute({ pattern: "\\.xyz$", path: dir }, ctx);
    expect(result).toBe("No files found");
  });
});
