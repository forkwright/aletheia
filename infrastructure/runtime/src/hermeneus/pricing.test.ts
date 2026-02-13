// Pricing module tests
import { describe, it, expect } from "vitest";
import { calculateTurnCost, calculateCostBreakdown } from "./pricing.js";

const baseUsage = {
  inputTokens: 1_000_000,
  outputTokens: 1_000_000,
  cacheReadTokens: 1_000_000,
  cacheWriteTokens: 1_000_000,
};

describe("calculateTurnCost", () => {
  it("calculates Opus pricing (exact model ID)", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "claude-opus-4-6" });
    expect(cost).toBeCloseTo(15 + 75 + 1.5 + 18.75);
  });

  it("calculates Sonnet pricing (exact model ID)", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "claude-sonnet-4-5-20250929" });
    expect(cost).toBeCloseTo(3 + 15 + 0.3 + 3.75);
  });

  it("calculates Haiku pricing (exact model ID)", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "claude-haiku-4-5-20251001" });
    expect(cost).toBeCloseTo(0.8 + 4 + 0.08 + 1);
  });

  it("fuzzy-matches opus in model name", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "anthropic/claude-opus-4-6-preview" });
    expect(cost).toBeCloseTo(15 + 75 + 1.5 + 18.75);
  });

  it("fuzzy-matches sonnet in model name", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "claude-sonnet-latest" });
    expect(cost).toBeCloseTo(3 + 15 + 0.3 + 3.75);
  });

  it("fuzzy-matches haiku in model name", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "haiku-fast" });
    expect(cost).toBeCloseTo(0.8 + 4 + 0.08 + 1);
  });

  it("defaults to Sonnet for unknown models", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: "gpt-4o" });
    expect(cost).toBeCloseTo(3 + 15 + 0.3 + 3.75);
  });

  it("defaults to Sonnet for null model", () => {
    const cost = calculateTurnCost({ ...baseUsage, model: null });
    expect(cost).toBeCloseTo(3 + 15 + 0.3 + 3.75);
  });

  it("returns 0 for zero tokens", () => {
    const cost = calculateTurnCost({
      inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, model: null,
    });
    expect(cost).toBe(0);
  });

  it("handles realistic usage numbers", () => {
    const cost = calculateTurnCost({
      inputTokens: 5000, outputTokens: 1500, cacheReadTokens: 3000, cacheWriteTokens: 0,
      model: "claude-sonnet-4-5-20250929",
    });
    expect(cost).toBeCloseTo(5000 / 1e6 * 3 + 1500 / 1e6 * 15 + 3000 / 1e6 * 0.3);
  });
});

describe("calculateCostBreakdown", () => {
  it("returns all cost components", () => {
    const bd = calculateCostBreakdown({ ...baseUsage, model: "claude-sonnet-4-5-20250929" });
    expect(bd.inputCost).toBeCloseTo(3);
    expect(bd.outputCost).toBeCloseTo(15);
    expect(bd.cacheReadCost).toBeCloseTo(0.3);
    expect(bd.cacheWriteCost).toBeCloseTo(3.75);
    expect(bd.totalCost).toBeCloseTo(bd.inputCost + bd.outputCost + bd.cacheReadCost + bd.cacheWriteCost);
  });

  it("totalCost matches calculateTurnCost", () => {
    const usage = { ...baseUsage, model: "claude-opus-4-6" };
    const bd = calculateCostBreakdown(usage);
    const total = calculateTurnCost(usage);
    expect(bd.totalCost).toBeCloseTo(total);
  });

  it("handles null model", () => {
    const bd = calculateCostBreakdown({ ...baseUsage, model: null });
    expect(bd.totalCost).toBeGreaterThan(0);
  });
});
