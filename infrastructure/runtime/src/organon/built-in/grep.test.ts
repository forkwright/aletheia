import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";
import { grepTool } from "./grep.js";

describe("grepTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("finds matching lines", async () => {
    writeFileSync(join(dir, "code.ts"), "const x = 1;\nconst y = 2;\nlet z = 3;");
    const result = await grepTool.execute({ pattern: "const", path: dir }, ctx);
    expect(result).toContain("const x = 1");
    expect(result).toContain("const y = 2");
    expect(result).not.toContain("let z");
  });

  it("returns no matches found for no matches", async () => {
    writeFileSync(join(dir, "empty.txt"), "nothing here");
    const result = await grepTool.execute({ pattern: "ZZZZZ", path: dir }, ctx);
    expect(result).toBe("No matches found");
  });
});
