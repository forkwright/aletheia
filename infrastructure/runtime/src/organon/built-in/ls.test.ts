import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";
import { lsTool } from "./ls.js";

describe("lsTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("lists files in directory", async () => {
    writeFileSync(join(dir, "a.txt"), "hello");
    mkdirSync(join(dir, "subdir"));
    const result = await lsTool.execute({}, ctx);
    expect(result).toContain("a.txt");
    expect(result).toContain("subdir/");
  });

  it("shows empty directory message", async () => {
    const result = await lsTool.execute({}, ctx);
    expect(result).toBe("(empty directory)");
  });

  it("excludes hidden files by default", async () => {
    writeFileSync(join(dir, ".hidden"), "secret");
    writeFileSync(join(dir, "visible"), "public");
    const result = await lsTool.execute({}, ctx);
    expect(result).toContain("visible");
    expect(result).not.toContain(".hidden");
  });

  it("includes hidden files with all=true", async () => {
    writeFileSync(join(dir, ".hidden"), "secret");
    writeFileSync(join(dir, "visible"), "public");
    const result = await lsTool.execute({ all: true }, ctx);
    expect(result).toContain(".hidden");
    expect(result).toContain("visible");
  });
});
