import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";

vi.mock("../../organon/sandbox.js", () => ({
  screenCommand: vi.fn().mockReturnValue({ allowed: true }),
}));
vi.mock("../../organon/docker-exec.js", () => ({
  dockerAvailable: vi.fn().mockReturnValue(false),
  execInDocker: vi.fn(),
}));

const { execTool } = await import("./exec.js");
const { screenCommand } = await import("../../organon/sandbox.js");

describe("execTool", () => {
  let dir: string;
  let ctx: ToolContext;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "aletheia-test-"));
    ctx = { nousId: "test", sessionId: "s1", workspace: dir };
  });

  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("runs simple command", async () => {
    const result = await execTool.execute({ command: "echo hello" }, ctx);
    expect(result).toBe("hello");
  });

  it("blocks denied commands", async () => {
    vi.mocked(screenCommand).mockReturnValueOnce({
      allowed: false,
      matchedPattern: "rm -rf",
    });
    const result = await execTool.execute({ command: "rm -rf /" }, ctx);
    expect(result).toContain("Command blocked");
    expect(result).toContain("rm -rf");
  });
});
