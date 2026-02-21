// Skill learner tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

import { extractSkillCandidate, saveLearnedSkill, type ToolCallRecord, type LearnedSkillCandidate } from "./skill-learner.js";

function makeToolCalls(count: number): ToolCallRecord[] {
  return Array.from({ length: count }, (_, i) => ({
    name: `tool_${i}`,
    input: { arg: `value_${i}` },
    output: `result_${i}`,
  }));
}

function makeRouter(responseText: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: responseText }],
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "haiku",
    }),
  };
}

describe("extractSkillCandidate", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("returns null when tool calls < 3", async () => {
    const router = makeRouter("anything");
    const result = await extractSkillCandidate(router as never, makeToolCalls(2), "haiku", "ses_1", 1, "test-agent");
    expect(result).toBeNull();
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("returns null when any tool call has error output", async () => {
    const tools: ToolCallRecord[] = [
      { name: "read", input: {}, output: "content" },
      { name: "write", input: {}, output: "Error: permission denied" },
      { name: "exec", input: {}, output: "ok" },
    ];
    const router = makeRouter("anything");
    const result = await extractSkillCandidate(router as never, tools, "haiku", "ses_1", 1, "error-agent");
    expect(result).toBeNull();
  });

  it("returns null when LLM says NOT_GENERALIZABLE", async () => {
    const router = makeRouter("NOT_GENERALIZABLE");
    const result = await extractSkillCandidate(router as never, makeToolCalls(3), "haiku", "ses_1", 1, "nogen-agent");
    expect(result).toBeNull();
  });

  it("returns skill candidate from valid LLM response", async () => {
    const skillMd = `---
# Deploy Script
Automates deployment to staging.

## When to Use
When deploying the app to staging environment.

## Steps
1. Run tests
2. Build
3. Deploy

## Tools Used
- exec: runs commands
---`;
    const router = makeRouter(skillMd);
    const result = await extractSkillCandidate(router as never, makeToolCalls(4), "haiku", "ses_1", 5, "skill-agent");
    expect(result).not.toBeNull();
    expect(result!.name).toBe("Deploy Script");
    expect(result!.id).toBe("deploy-script");
    expect(result!.toolSequence).toEqual(["tool_0", "tool_1", "tool_2", "tool_3"]);
    expect(result!.sourceSession).toBe("ses_1");
    expect(result!.sourceTurn).toBe(5);
  });

  it("returns null when LLM call fails", async () => {
    const router = { complete: vi.fn().mockRejectedValue(new Error("API down")) };
    const result = await extractSkillCandidate(router as never, makeToolCalls(3), "haiku", "ses_1", 1, "fail-agent");
    expect(result).toBeNull();
  });

  it("is rate-limited per agent", async () => {
    const router = makeRouter("NOT_GENERALIZABLE");
    // First call goes through (rate limit is per-agent)
    await extractSkillCandidate(router as never, makeToolCalls(3), "haiku", "ses_1", 1, "rl-agent");
    expect(router.complete).toHaveBeenCalledTimes(1);

    // Second call within the hour is blocked
    const result = await extractSkillCandidate(router as never, makeToolCalls(3), "haiku", "ses_1", 2, "rl-agent");
    expect(result).toBeNull();
    expect(router.complete).toHaveBeenCalledTimes(1); // Still 1 — second was rate-limited
  });
});

describe("saveLearnedSkill", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = mkdtempSync(join(tmpdir(), "skill-test-"));
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("creates skill directory and writes SKILL.md", () => {
    const candidate: LearnedSkillCandidate = {
      id: "test-skill",
      toolSequence: ["read", "write"],
      name: "Test Skill",
      description: "A test skill",
      instructions: "# Test Skill\nDo the thing.",
      sourceSession: "ses_1",
      sourceTurn: 3,
    };

    saveLearnedSkill(candidate, tmpDir);

    const skillPath = join(tmpDir, "test-skill", "SKILL.md");
    expect(existsSync(skillPath)).toBe(true);
    expect(readFileSync(skillPath, "utf-8")).toBe("# Test Skill\nDo the thing.");
  });

  it("does not overwrite existing skill", () => {
    const dir = join(tmpDir, "existing-skill");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "SKILL.md"), "original content", "utf-8");

    const candidate: LearnedSkillCandidate = {
      id: "existing-skill",
      toolSequence: [],
      name: "Existing",
      description: "",
      instructions: "new content",
      sourceSession: "ses_1",
      sourceTurn: 1,
    };

    saveLearnedSkill(candidate, tmpDir);
    expect(readFileSync(join(dir, "SKILL.md"), "utf-8")).toBe("original content");
  });
});
