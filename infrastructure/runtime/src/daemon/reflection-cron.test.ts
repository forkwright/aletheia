// Reflection cron tests
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

vi.mock("../distillation/reflect.js", () => ({
  reflectOnAgent: vi.fn(),
  weeklyReflection: vi.fn(),
}));

import { runNightlyReflection, runWeeklyReflection } from "./reflection-cron.js";
import { reflectOnAgent, weeklyReflection } from "../distillation/reflect.js";
import type { AletheiaConfig } from "../taxis/schema.js";

function makeConfig(agentIds: string[]): AletheiaConfig {
  const list: Record<string, unknown> = {};
  for (const id of agentIds) list[id] = { id };
  return {
    agents: {
      defaults: { compaction: { distillationModel: "haiku" } },
      list,
    },
  } as unknown as AletheiaConfig;
}

describe("runNightlyReflection", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("skips when no agents configured", async () => {
    const result = await runNightlyReflection({} as never, {} as never, makeConfig([]));
    expect(result.agentsReflected).toBe(0);
    expect(result.errors).toEqual([]);
  });

  it("reflects on each configured agent", async () => {
    vi.mocked(reflectOnAgent).mockResolvedValue({
      sessionsReviewed: 3,
      findings: { patterns: ["p1"], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
      memoriesStored: 2,
      tokensUsed: 1000,
      durationMs: 500,
    } as never);

    const result = await runNightlyReflection({} as never, {} as never, makeConfig(["syl", "chiron"]));
    expect(result.agentsReflected).toBe(2);
    expect(result.totalFindings).toBe(2);
    expect(result.totalMemoriesStored).toBe(4);
    expect(reflectOnAgent).toHaveBeenCalledTimes(2);
  });

  it("counts only agents with reviewed sessions", async () => {
    vi.mocked(reflectOnAgent)
      .mockResolvedValueOnce({
        sessionsReviewed: 5, findings: { patterns: [], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
        memoriesStored: 1, tokensUsed: 500, durationMs: 200,
      } as never)
      .mockResolvedValueOnce({
        sessionsReviewed: 0, findings: { patterns: [], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
        memoriesStored: 0, tokensUsed: 0, durationMs: 0,
      } as never);

    const result = await runNightlyReflection({} as never, {} as never, makeConfig(["syl", "idle"]));
    expect(result.agentsReflected).toBe(1);
  });

  it("captures errors without stopping other agents", async () => {
    vi.mocked(reflectOnAgent)
      .mockRejectedValueOnce(new Error("API timeout"))
      .mockResolvedValueOnce({
        sessionsReviewed: 2, findings: { patterns: ["p"], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
        memoriesStored: 1, tokensUsed: 500, durationMs: 200,
      } as never);

    const result = await runNightlyReflection({} as never, {} as never, makeConfig(["broken", "working"]));
    expect(result.agentsReflected).toBe(1);
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0]).toContain("API timeout");
  });
});

describe("runWeeklyReflection", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("skips when no agents configured", async () => {
    const result = await runWeeklyReflection({} as never, {} as never, makeConfig([]));
    expect(result.agentsReflected).toBe(0);
  });

  it("reflects on all agents and aggregates findings", async () => {
    vi.mocked(weeklyReflection).mockResolvedValue({
      summariesReviewed: 5,
      trajectory: ["trend1"],
      topicDrift: [],
      weeklyPatterns: ["wp1"],
      unresolvedArcs: [],
      tokensUsed: 2000,
      durationMs: 1000,
    } as never);

    const result = await runWeeklyReflection({} as never, {} as never, makeConfig(["syl", "chiron"]));
    expect(result.agentsReflected).toBe(2);
    expect(result.totalFindings).toBe(4); // 2 agents * (1 trajectory + 1 weeklyPattern)
    expect(weeklyReflection).toHaveBeenCalledTimes(2);
  });

  it("captures errors without stopping others", async () => {
    vi.mocked(weeklyReflection)
      .mockRejectedValueOnce(new Error("timeout"))
      .mockResolvedValueOnce({
        summariesReviewed: 3, trajectory: [], topicDrift: [], weeklyPatterns: [], unresolvedArcs: [],
        tokensUsed: 500, durationMs: 200,
      } as never);

    const result = await runWeeklyReflection({} as never, {} as never, makeConfig(["fail", "ok"]));
    expect(result.errors).toHaveLength(1);
    expect(result.agentsReflected).toBe(1);
  });
});
