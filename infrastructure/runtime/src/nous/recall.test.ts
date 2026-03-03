// Unit tests for recall utilities — MMR selection and temporal scoring
import { describe, expect, it } from "vitest";
import { mmrSelect, computeRecencyBoost, temporalDecay } from "./recall.js";

describe("mmrSelect", () => {
  const hit = (memory: string, score: number) => ({
    memory,
    score,
    id: memory.slice(0, 8),
  });

  it("returns empty for empty input", () => {
    expect(mmrSelect([], 5)).toEqual([]);
  });

  it("returns single item unchanged", () => {
    const items = [hit("only one", 0.9)];
    expect(mmrSelect(items, 5)).toEqual(items);
  });

  it("always picks highest-scoring item first", () => {
    const items = [hit("top scorer", 0.95), hit("second", 0.85), hit("third", 0.75)];
    const result = mmrSelect(items, 3);
    expect(result[0]!.memory).toBe("top scorer");
  });

  it("penalizes near-duplicate content", () => {
    // Two near-identical memories and one distinct one
    const items = [
      hit("the truck needs new brake pads installed soon", 0.95),
      hit("the truck needs new brake pads replaced soon", 0.90),
      hit("alice prefers dark roast coffee in the morning", 0.85),
    ];
    const result = mmrSelect(items, 2);
    // Should pick top scorer + the diverse one, not top scorer + near-duplicate
    expect(result[0]!.memory).toContain("brake pads installed");
    expect(result[1]!.memory).toContain("coffee");
  });

  it("respects limit", () => {
    const items = Array.from({ length: 10 }, (_, i) => hit(`memory ${i}`, 0.9 - i * 0.01));
    const result = mmrSelect(items, 3);
    expect(result).toHaveLength(3);
  });

  it("returns all items when fewer than limit", () => {
    const items = [hit("a", 0.9), hit("b", 0.8)];
    const result = mmrSelect(items, 10);
    expect(result).toHaveLength(2);
  });

  it("lambda=1.0 degrades to pure relevance ranking", () => {
    const items = [
      hit("the truck brake system needs repair work done", 0.95),
      hit("the truck brake system needs maintenance work done", 0.90),
      hit("coffee preferences for the morning routine", 0.85),
    ];
    const result = mmrSelect(items, 3, 1.0);
    // Pure relevance = just sorted by score descending
    expect(result[0]!.score).toBe(0.95);
    expect(result[1]!.score).toBe(0.90);
    expect(result[2]!.score).toBe(0.85);
  });

  it("lambda=0.0 maximizes diversity", () => {
    const items = [
      hit("the truck brake system needs repair work done", 0.95),
      hit("the truck brake system needs maintenance work done", 0.90),
      hit("completely different topic about gardening and plants", 0.85),
    ];
    const result = mmrSelect(items, 2, 0.0);
    // First pick is always highest score, second should maximize diversity
    expect(result[0]!.memory).toContain("repair");
    expect(result[1]!.memory).toContain("gardening");
  });

  it("does not mutate input array", () => {
    const items = [hit("a", 0.9), hit("b", 0.8), hit("c", 0.7)];
    const original = [...items];
    mmrSelect(items, 2);
    expect(items).toEqual(original);
  });
});

describe("temporalDecay", () => {
  it("returns 1.0 for null/undefined (no penalty)", () => {
    expect(temporalDecay(null, Date.now())).toBe(1);
    expect(temporalDecay(undefined, Date.now())).toBe(1);
  });

  it("returns 1.0 for just-created memory", () => {
    const now = Date.now();
    const justNow = new Date(now - 1000).toISOString();
    const decay = temporalDecay(justNow, now);
    expect(decay).toBeGreaterThan(0.999);
    expect(decay).toBeLessThanOrEqual(1);
  });

  it("returns ~0.5 at half-life", () => {
    const now = Date.now();
    const thirtyDaysAgo = new Date(now - 30 * 24 * 3600 * 1000).toISOString();
    const decay = temporalDecay(thirtyDaysAgo, now, 30);
    expect(decay).toBeCloseTo(0.5, 2);
  });

  it("returns ~0.25 at 2x half-life", () => {
    const now = Date.now();
    const sixtyDaysAgo = new Date(now - 60 * 24 * 3600 * 1000).toISOString();
    const decay = temporalDecay(sixtyDaysAgo, now, 30);
    expect(decay).toBeCloseTo(0.25, 2);
  });

  it("returns ~0.125 at 3x half-life", () => {
    const now = Date.now();
    const ninetyDaysAgo = new Date(now - 90 * 24 * 3600 * 1000).toISOString();
    const decay = temporalDecay(ninetyDaysAgo, now, 30);
    expect(decay).toBeCloseTo(0.125, 2);
  });

  it("supports custom half-life", () => {
    const now = Date.now();
    const sevenDaysAgo = new Date(now - 7 * 24 * 3600 * 1000).toISOString();
    const decay = temporalDecay(sevenDaysAgo, now, 7);
    expect(decay).toBeCloseTo(0.5, 2);
  });

  it("returns 1.0 for future timestamps", () => {
    const now = Date.now();
    const future = new Date(now + 3600 * 1000).toISOString();
    expect(temporalDecay(future, now)).toBe(1);
  });

  it("never returns zero (asymptotic)", () => {
    const now = Date.now();
    const yearAgo = new Date(now - 365 * 24 * 3600 * 1000).toISOString();
    const decay = temporalDecay(yearAgo, now, 30);
    expect(decay).toBeGreaterThan(0);
  });
});

describe("computeRecencyBoost (deprecated)", () => {
  it("returns 0 for null/undefined", () => {
    expect(computeRecencyBoost(null, Date.now())).toBe(0);
    expect(computeRecencyBoost(undefined, Date.now())).toBe(0);
  });

  it("returns max boost for just-created memory", () => {
    const now = Date.now();
    const justNow = new Date(now - 1000).toISOString(); // 1 second ago
    const boost = computeRecencyBoost(justNow, now);
    expect(boost).toBeGreaterThan(0.14);
    expect(boost).toBeLessThanOrEqual(0.15);
  });

  it("returns ~half boost at 12 hours", () => {
    const now = Date.now();
    const twelveHoursAgo = new Date(now - 12 * 3600 * 1000).toISOString();
    const boost = computeRecencyBoost(twelveHoursAgo, now);
    expect(boost).toBeCloseTo(0.075, 1);
  });

  it("returns 0 for memories older than 24 hours", () => {
    const now = Date.now();
    const twoDaysAgo = new Date(now - 48 * 3600 * 1000).toISOString();
    expect(computeRecencyBoost(twoDaysAgo, now)).toBe(0);
  });

  it("returns 0 for future timestamps", () => {
    const now = Date.now();
    const future = new Date(now + 3600 * 1000).toISOString();
    expect(computeRecencyBoost(future, now)).toBe(0);
  });
});
