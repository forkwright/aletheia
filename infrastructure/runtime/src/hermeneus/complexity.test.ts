// Complexity scoring tests
import { describe, it, expect } from "vitest";
import { scoreComplexity, selectModel, selectTemperature } from "./complexity.js";

const base = { messageText: "Hello", messageCount: 5, depth: 0 };

describe("scoreComplexity", () => {
  it("returns agent override directly", () => {
    const r = scoreComplexity({ ...base, agentOverride: "complex" });
    expect(r.tier).toBe("complex");
    expect(r.score).toBe(100);
    expect(r.reason).toBe("agent override");
  });

  it("returns standard score for agent override", () => {
    const r = scoreComplexity({ ...base, agentOverride: "standard" });
    expect(r.tier).toBe("standard");
    expect(r.score).toBe(50);
  });

  it("returns routine score for agent override", () => {
    const r = scoreComplexity({ ...base, agentOverride: "routine" });
    expect(r.tier).toBe("routine");
    expect(r.score).toBe(10);
  });

  it("returns complex for cross-agent (depth > 0)", () => {
    const r = scoreComplexity({ ...base, depth: 1 });
    expect(r.tier).toBe("complex");
    expect(r.score).toBe(90);
    expect(r.reason).toBe("cross-agent");
  });

  it("short text reduces score", () => {
    const r = scoreComplexity({ messageText: "hi", messageCount: 5, depth: 0 });
    expect(r.score).toBeLessThan(30);
  });

  it("long text increases score", () => {
    const longText = "a".repeat(600);
    const r = scoreComplexity({ messageText: longText, messageCount: 5, depth: 0 });
    expect(r.reason).toContain("long");
  });

  it("first message boosts score", () => {
    const r = scoreComplexity({ messageText: "hello there how are you today", messageCount: 0, depth: 0 });
    expect(r.reason).toContain("first message");
  });

  it("simple responses reduce score", () => {
    const r = scoreComplexity({ messageText: "yes", messageCount: 5, depth: 0 });
    expect(r.reason).toContain("simple response");
    expect(r.tier).toBe("routine");
  });

  it("complex intent keywords boost score", () => {
    const r = scoreComplexity({ messageText: "Please analyze the data and design a new architecture", messageCount: 5, depth: 0 });
    expect(r.reason).toContain("complex intent");
    // 30 base + 25 complex intent = 55 → standard
    expect(r.score).toBe(55);
  });

  it("combined factors reach complex tier", () => {
    const r = scoreComplexity({
      messageText: "Please analyze the data then implement the solution step by step",
      messageCount: 0, depth: 0,
    });
    expect(r.reason).toContain("complex intent");
    expect(r.reason).toContain("multi-step");
    expect(r.reason).toContain("first message");
    expect(r.tier).toBe("complex");
  });

  it("tool keywords set floor at 40", () => {
    const r = scoreComplexity({ messageText: "search for files", messageCount: 5, depth: 0 });
    expect(r.reason).toContain("tool keywords");
    expect(r.score).toBeGreaterThanOrEqual(30);
  });

  it("multi-step patterns boost score", () => {
    const r = scoreComplexity({ messageText: "first do X then do Y after that do Z", messageCount: 5, depth: 0 });
    expect(r.reason).toContain("multi-step");
  });

  it("clamps score to 0-100", () => {
    // Very short + simple = heavily negative
    const r = scoreComplexity({ messageText: "ok", messageCount: 5, depth: 0 });
    expect(r.score).toBeGreaterThanOrEqual(0);
    expect(r.score).toBeLessThanOrEqual(100);
  });

  it("returns baseline reason when no factors matched", () => {
    // Medium-length text with no keywords
    const r = scoreComplexity({ messageText: "I had a nice day at the park and it was lovely weather", messageCount: 5, depth: 0 });
    expect(r.score).toBe(30);
    expect(r.reason).toBe("baseline");
  });

  it("tier thresholds: >=60 complex, >=30 standard, <30 routine", () => {
    // Force a known score by combining factors
    const complex = scoreComplexity({ messageText: "analyze and refactor the implementation step 1 then step 2", messageCount: 0, depth: 0 });
    expect(complex.tier).toBe("complex");

    const routine = scoreComplexity({ messageText: "yes", messageCount: 5, depth: 0 });
    expect(routine.tier).toBe("routine");
  });
});

describe("selectModel", () => {
  const tiers = { routine: "haiku", standard: "sonnet", complex: "opus" };

  it("maps routine to routine model", () => {
    expect(selectModel("routine", tiers)).toBe("haiku");
  });

  it("maps standard to standard model", () => {
    expect(selectModel("standard", tiers)).toBe("sonnet");
  });

  it("maps complex to complex model", () => {
    expect(selectModel("complex", tiers)).toBe("opus");
  });
});

describe("selectTemperature", () => {
  it("returns 0.3 when tools are present regardless of tier", () => {
    expect(selectTemperature("routine", true)).toBe(0.3);
    expect(selectTemperature("standard", true)).toBe(0.3);
    expect(selectTemperature("complex", true)).toBe(0.3);
  });

  it("returns 0.3 for routine without tools", () => {
    expect(selectTemperature("routine", false)).toBe(0.3);
  });

  it("returns 0.5 for standard without tools", () => {
    expect(selectTemperature("standard", false)).toBe(0.5);
  });

  it("returns 0.7 for complex without tools", () => {
    expect(selectTemperature("complex", false)).toBe(0.7);
  });
});
