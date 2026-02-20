// Token counter tests
import { describe, expect, it } from "vitest";
import {
  estimateMessageTokens,
  estimateTokens,
  estimateTokensSafe,
  estimateToolDefTokens,
  SAFETY_MARGIN,
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
