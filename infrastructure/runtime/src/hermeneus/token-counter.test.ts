// Token counter tests
import { describe, expect, it } from "vitest";
import {
  dynamicThinkingBudget,
  estimateMessageTokens,
  estimateTokens,
  estimateTokensSafe,
  estimateToolDefTokens,
  SAFETY_MARGIN,
  truncateToolResult,
} from "./token-counter.js";

describe("estimateTokens", () => {
  it("estimates based on 3.5 chars per token", () => {
    expect(estimateTokens("1234567")).toBe(2); // 7 / 3.5 = 2
  });

  it("ceils the result", () => {
    expect(estimateTokens("12345")).toBe(2); // 5 / 3.5 = 1.43 → 2
  });

  it("returns 0 for empty string", () => {
    expect(estimateTokens("")).toBe(0);
  });

  it("handles single character", () => {
    expect(estimateTokens("a")).toBe(1); // 1 / 3.5 = 0.29 → 1
  });
});

describe("estimateTokensSafe", () => {
  it("adds safety margin", () => {
    const base = estimateTokens("hello world");
    const safe = estimateTokensSafe("hello world");
    expect(safe).toBe(Math.ceil(base * SAFETY_MARGIN));
  });

  it("SAFETY_MARGIN is 1.15", () => {
    expect(SAFETY_MARGIN).toBe(1.15);
  });
});

describe("estimateMessageTokens", () => {
  it("sums tokens with overhead per message", () => {
    const msgs = [
      { role: "user", content: "hello" },
      { role: "assistant", content: "hi there" },
    ];
    const result = estimateMessageTokens(msgs);
    const expected =
      estimateTokens("hello") + 4 +
      estimateTokens("hi there") + 4;
    expect(result).toBe(expected);
  });

  it("returns 0 for empty array", () => {
    expect(estimateMessageTokens([])).toBe(0);
  });
});

describe("estimateToolDefTokens", () => {
  it("returns 0 for empty array", () => {
    expect(estimateToolDefTokens([])).toBe(0);
  });

  it("includes JSON size + overhead + safety margin", () => {
    const tools = [{ name: "test", description: "a tool" }];
    const jsonTokens = estimateTokens(JSON.stringify(tools));
    const overhead = 1 * 200;
    const expected = Math.ceil((jsonTokens + overhead) * SAFETY_MARGIN);
    expect(estimateToolDefTokens(tools)).toBe(expected);
  });

  it("scales overhead with number of tools", () => {
    const oneResult = estimateToolDefTokens([{ name: "a" }]);
    const twoResult = estimateToolDefTokens([{ name: "a" }, { name: "b" }]);
    expect(twoResult).toBeGreaterThan(oneResult);
  });
});

describe("truncateToolResult", () => {
  it("returns short results unchanged", () => {
    const result = truncateToolResult("grep", "line1\nline2\nline3");
    expect(result).toBe("line1\nline2\nline3");
  });

  it("truncates results exceeding tool-specific limit", () => {
    // grep limit is 5000 chars
    const longResult = "x".repeat(8000);
    const truncated = truncateToolResult("grep", longResult);
    expect(truncated.length).toBeLessThan(longResult.length);
    expect(truncated).toContain("chars truncated for storage");
  });

  it("preserves head and tail", () => {
    const head = "HEAD_MARKER_" + "a".repeat(2000);
    const middle = "m".repeat(5000);
    const tail = "b".repeat(2000) + "_TAIL_MARKER";
    const longResult = head + middle + tail;
    const truncated = truncateToolResult("exec", longResult); // exec limit: 8000
    expect(truncated).toContain("HEAD_MARKER_");
    expect(truncated).toContain("_TAIL_MARKER");
  });

  it("uses default limit for unknown tools", () => {
    // Default is 5000 chars
    const longResult = "x".repeat(6000);
    const truncated = truncateToolResult("unknown_tool", longResult);
    expect(truncated.length).toBeLessThan(longResult.length);
    expect(truncated).toContain("chars truncated for storage");
  });

  it("respects tool-specific limits", () => {
    // read limit is 10000, grep limit is 5000
    const result = "x".repeat(7000);
    const readTruncated = truncateToolResult("read", result);
    const grepTruncated = truncateToolResult("grep", result);
    // read should NOT truncate (7000 < 10000)
    expect(readTruncated).toBe(result);
    // grep SHOULD truncate (7000 > 5000)
    expect(grepTruncated).toContain("chars truncated for storage");
  });
});

describe("dynamicThinkingBudget", () => {
  it("returns minimal budget for very short messages", () => {
    expect(dynamicThinkingBudget("hi")).toBe(1024);
    expect(dynamicThinkingBudget("yes")).toBe(1024);
  });

  it("returns reduced budget for short messages", () => {
    const budget = dynamicThinkingBudget("What is the capital of France?");
    expect(budget).toBeLessThan(10_000);
    expect(budget).toBeGreaterThanOrEqual(1024);
  });

  it("returns full budget for long/complex messages", () => {
    const longMsg = "x".repeat(1000);
    expect(dynamicThinkingBudget(longMsg)).toBe(10_000);
  });

  it("reduces budget for tool loop iterations", () => {
    const msg = "Review the codebase architecture and identify patterns";
    const firstLoop = dynamicThinkingBudget(msg, { toolLoopIteration: 0 });
    const secondLoop = dynamicThinkingBudget(msg, { toolLoopIteration: 1 });
    expect(secondLoop).toBeLessThan(firstLoop);
  });

  it("respects custom base budget", () => {
    const longMsg = "x".repeat(1000);
    expect(dynamicThinkingBudget(longMsg, { baseBudget: 20_000 })).toBe(20_000);
  });

  it("never returns below minimum", () => {
    expect(dynamicThinkingBudget("", { baseBudget: 500 })).toBe(1024);
  });
});
