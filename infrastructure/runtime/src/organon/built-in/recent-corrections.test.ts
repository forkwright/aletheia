// Recent corrections tool tests
import { describe, expect, it } from "vitest";
import { createRecentCorrectionsTool } from "./recent-corrections.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = {
  nousId: "test-agent",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

describe("createRecentCorrectionsTool", () => {
  it("returns tool with correct name", () => {
    const tool = createRecentCorrectionsTool();
    expect(tool.definition.name).toBe("recent_corrections");
  });

  it("returns error when store not available", async () => {
    const tool = createRecentCorrectionsTool();
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.error).toContain("not available");
  });

  it("returns empty state when no signals exist", async () => {
    const mockStore = {
      getSignalHistory: () => [],
    };
    const tool = createRecentCorrectionsTool(mockStore as never);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.signals).toHaveLength(0);
    expect(result.note).toContain("No interaction signals");
  });

  it("groups signals by type", async () => {
    const mockStore = {
      getSignalHistory: () => [
        { signal: "correction", confidence: 0.9, turnSeq: 1, createdAt: "2026-02-17T10:00:00Z" },
        { signal: "correction", confidence: 0.8, turnSeq: 2, createdAt: "2026-02-17T10:01:00Z" },
        { signal: "success", confidence: 0.7, turnSeq: 3, createdAt: "2026-02-17T10:02:00Z" },
      ],
    };
    const tool = createRecentCorrectionsTool(mockStore as never);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.summary.correction).toBe(2);
    expect(result.summary.success).toBe(1);
    expect(result.signals).toHaveLength(3);
  });

  it("respects custom limit", async () => {
    const mockStore = {
      getSignalHistory: (_id: string, limit: number) =>
        Array.from({ length: Math.min(limit, 5) }, (_, i) => ({
          signal: "correction",
          confidence: 0.9,
          turnSeq: i,
          createdAt: `2026-02-17T10:0${i}:00Z`,
        })),
    };
    const tool = createRecentCorrectionsTool(mockStore as never);
    const result = JSON.parse(await tool.execute({ limit: 3 }, ctx));
    expect(result.signals).toHaveLength(3);
  });
});
