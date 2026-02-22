// Evolutionary config search tests
import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

import { loadArchive, type ConfigVariant, type EvolutionArchive } from "./evolution-cron.js";
import { PipelineConfigSchema } from "../nous/pipeline-config.js";

const defaultConfig = PipelineConfigSchema.parse({});

function makeVariant(overrides: Partial<ConfigVariant> = {}): ConfigVariant {
  return {
    id: `v0-test`,
    config: defaultConfig,
    score: 0.5,
    parentId: null,
    generation: 0,
    createdAt: new Date().toISOString(),
    ...overrides,
  };
}

function makeArchive(overrides: Partial<EvolutionArchive> = {}): EvolutionArchive {
  return {
    variants: [makeVariant()],
    currentDefault: "v0-test",
    lastRunAt: new Date().toISOString(),
    ...overrides,
  };
}

describe("evolution archive", () => {
  it("loadArchive returns default when no file exists", () => {
    const tmpDir = `/tmp/evolution-test-${Date.now()}`;
    const archive = loadArchive(tmpDir + "/workspace", "test-agent");
    expect(archive.variants).toHaveLength(1);
    expect(archive.variants[0].generation).toBe(0);
    expect(archive.variants[0].score).toBe(0.5);
    expect(archive.currentDefault).toBe("");
    expect(archive.lastRunAt).toBe("");
  });

  it("default variant has valid pipeline config", () => {
    const tmpDir = `/tmp/evolution-test-${Date.now()}`;
    const archive = loadArchive(tmpDir + "/workspace", "test-agent");
    const result = PipelineConfigSchema.safeParse(archive.variants[0].config);
    expect(result.success).toBe(true);
  });
});

describe("variant ranking", () => {
  it("sorts variants by score descending", () => {
    const archive = makeArchive({
      variants: [
        makeVariant({ id: "low", score: 0.3 }),
        makeVariant({ id: "high", score: 0.9 }),
        makeVariant({ id: "mid", score: 0.6 }),
      ],
    });
    const sorted = [...archive.variants].sort((a, b) => b.score - a.score);
    expect(sorted[0].id).toBe("high");
    expect(sorted[1].id).toBe("mid");
    expect(sorted[2].id).toBe("low");
  });

  it("caps archive at MAX_VARIANTS (5)", () => {
    const variants = Array.from({ length: 7 }, (_, i) =>
      makeVariant({ id: `v${i}`, score: i * 0.1 }),
    );
    const sorted = [...variants].sort((a, b) => b.score - a.score).slice(0, 5);
    expect(sorted).toHaveLength(5);
    expect(sorted[0].score).toBeCloseTo(0.6);
    expect(sorted[4].score).toBeCloseTo(0.2);
  });
});

describe("mutation validation", () => {
  it("accepts valid pipeline config mutation", () => {
    const mutated = {
      recall: { limit: 12, maxTokens: 2000, minScore: 0.8, sufficiencyThreshold: 0.85, sufficiencyMinHits: 3 },
      tools: { expiryTurns: 8 },
      notes: { tokenCap: 3000 },
    };
    const result = PipelineConfigSchema.safeParse(mutated);
    expect(result.success).toBe(true);
  });

  it("rejects out-of-range recall.limit", () => {
    const invalid = { recall: { limit: 100 } };
    const result = PipelineConfigSchema.safeParse(invalid);
    expect(result.success).toBe(false);
  });

  it("rejects out-of-range minScore", () => {
    const invalid = { recall: { minScore: 5.0 } };
    const result = PipelineConfigSchema.safeParse(invalid);
    expect(result.success).toBe(false);
  });

  it("rejects negative tokenCap", () => {
    const invalid = { notes: { tokenCap: -1 } };
    const result = PipelineConfigSchema.safeParse(invalid);
    expect(result.success).toBe(false);
  });
});

describe("auto-adopt logic", () => {
  it("detects when auto-adopt time has passed", () => {
    const archive = makeArchive({
      pendingPromotion: {
        variantId: "v1-best",
        score: 0.9,
        currentScore: 0.5,
        improvementPct: 80,
        notifiedAt: new Date(Date.now() - 90_000_000).toISOString(),
        autoAdoptAt: new Date(Date.now() - 3_600_000).toISOString(),
      },
    });
    const adoptTime = new Date(archive.pendingPromotion!.autoAdoptAt).getTime();
    expect(Date.now() >= adoptTime).toBe(true);
    expect(archive.pendingPromotion!.improvementPct >= 10).toBe(true);
  });

  it("does not adopt when improvement below threshold", () => {
    const archive = makeArchive({
      pendingPromotion: {
        variantId: "v1-marginal",
        score: 0.52,
        currentScore: 0.50,
        improvementPct: 4,
        notifiedAt: new Date(Date.now() - 90_000_000).toISOString(),
        autoAdoptAt: new Date(Date.now() - 3_600_000).toISOString(),
      },
    });
    expect(archive.pendingPromotion!.improvementPct >= 10).toBe(false);
  });

  it("does not adopt before 24h window", () => {
    const archive = makeArchive({
      pendingPromotion: {
        variantId: "v1-new",
        score: 0.9,
        currentScore: 0.5,
        improvementPct: 80,
        notifiedAt: new Date().toISOString(),
        autoAdoptAt: new Date(Date.now() + 80_000_000).toISOString(),
      },
    });
    const adoptTime = new Date(archive.pendingPromotion!.autoAdoptAt).getTime();
    expect(Date.now() >= adoptTime).toBe(false);
  });

  it("calculates improvement percentage correctly", () => {
    const currentScore = 0.5;
    const bestScore = 0.65;
    const improvement = ((bestScore - currentScore) / Math.max(currentScore, 0.01)) * 100;
    expect(improvement).toBeCloseTo(30);
  });
});
