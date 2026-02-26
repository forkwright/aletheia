import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";
import { readTool } from "./read.js";

describe("readTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("reads text file correctly", async () => {
    writeFileSync(join(dir, "hello.txt"), "line1\nline2\nline3");
    const result = await readTool.execute({ path: "hello.txt" }, ctx);
    expect(result).toBe("line1\nline2\nline3");
  });

  it("respects maxLines", async () => {
    writeFileSync(join(dir, "long.txt"), "a\nb\nc\nd\ne");
    const result = await readTool.execute({ path: "long.txt", maxLines: 2 }, ctx);
    expect(result).toBe("a\nb");
  });

  it("rejects binary files", async () => {
    const buf = Buffer.alloc(100);
    buf[50] = 0x00;
    writeFileSync(join(dir, "bin.dat"), buf);
    const result = await readTool.execute({ path: "bin.dat" }, ctx);
    expect(result).toContain("Binary file detected");
  });

  it("rejects files over 5MB", async () => {
    writeFileSync(join(dir, "big.txt"), "x".repeat(6 * 1024 * 1024));
    const result = await readTool.execute({ path: "big.txt" }, ctx);
    expect(result).toContain("File too large");
  });

  it("returns error for missing files", async () => {
    const result = await readTool.execute({ path: "nope.txt" }, ctx);
    expect(result).toContain("Error:");
  });
});
