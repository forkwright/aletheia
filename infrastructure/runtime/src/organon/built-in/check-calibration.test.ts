// Check calibration tool tests
import { describe, it, expect } from "vitest";
import { createCheckCalibrationTool } from "./check-calibration.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = {
  nousId: "test-agent",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

describe("createCheckCalibrationTool", () => {
  it("returns tool with correct name", () => {
    const tool = createCheckCalibrationTool();
    expect(tool.definition.name).toBe("check_calibration");
  });

  it("handles missing competence and uncertainty gracefully", async () => {
    const tool = createCheckCalibrationTool();
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.nousId).toBe("test-agent");
    expect(result.competence.note).toContain("not available");
    expect(result.calibration.note).toContain("not available");
  });

  it("returns competence data when model provided", async () => {
    const mockCompetence = {
      getAgentCompetence: (id: string) =>
        id === "test-agent"
          ? {
              nousId: "test-agent",
              overallScore: 0.72,
              domains: {
                code: { domain: "code", score: 0.8, corrections: 1, successes: 5, disagreements: 0, lastUpdated: "2026-01-01" },
              },
            }
          : null,
      getScore: () => 0.5,
    };
    const tool = createCheckCalibrationTool(mockCompetence as never);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.overallScore).toBe(0.72);
    expect(result.domains.code.score).toBe(0.8);
  });

  it("filters to specific domain when requested", async () => {
    const mockCompetence = {
      getAgentCompetence: () => ({
        nousId: "test-agent",
        overallScore: 0.6,
        domains: {
          code: { domain: "code", score: 0.8, corrections: 1, successes: 5, disagreements: 0, lastUpdated: "2026-01-01" },
          health: { domain: "health", score: 0.4, corrections: 3, successes: 1, disagreements: 0, lastUpdated: "2026-01-01" },
        },
      }),
    };
    const tool = createCheckCalibrationTool(mockCompetence as never);
    const result = JSON.parse(await tool.execute({ domain: "code" }, ctx));
    expect(result.domainScore.score).toBe(0.8);
    expect(result.domains).toBeUndefined();
  });
});
