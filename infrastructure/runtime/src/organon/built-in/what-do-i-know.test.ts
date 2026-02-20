// What do I know tool tests
import { describe, expect, it } from "vitest";
import { createWhatDoIKnowTool } from "./what-do-i-know.js";
import type { ToolContext } from "../registry.js";

const ctx: ToolContext = {
  nousId: "test-agent",
  sessionId: "sess-1",
  workspace: "/tmp/test",
};

describe("createWhatDoIKnowTool", () => {
  it("returns tool with correct name", () => {
    const tool = createWhatDoIKnowTool();
    expect(tool.definition.name).toBe("what_do_i_know");
  });

  it("handles missing competence gracefully", async () => {
    const tool = createWhatDoIKnowTool();
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.note).toContain("not available");
  });

  it("categorizes strengths and weaknesses", async () => {
    const mockCompetence = {
      getAgentCompetence: () => ({
        nousId: "test-agent",
        overallScore: 0.6,
        domains: {
          code: { domain: "code", score: 0.85, corrections: 0, successes: 8, disagreements: 0, lastUpdated: "2026-02-17" },
          writing: { domain: "writing", score: 0.65, corrections: 1, successes: 3, disagreements: 0, lastUpdated: "2026-02-16" },
          health: { domain: "health", score: 0.3, corrections: 5, successes: 1, disagreements: 0, lastUpdated: "2026-02-15" },
        },
      }),
    };
    const tool = createWhatDoIKnowTool(mockCompetence as never);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.strengths).toHaveLength(2);
    expect(result.strengths[0].domain).toBe("code");
    expect(result.weaknesses).toHaveLength(1);
    expect(result.weaknesses[0].domain).toBe("health");
    expect(result.totalDomains).toBe(3);
  });

  it("returns recently active domains sorted by date", async () => {
    const mockCompetence = {
      getAgentCompetence: () => ({
        nousId: "test-agent",
        overallScore: 0.5,
        domains: {
          a: { domain: "a", score: 0.5, corrections: 0, successes: 0, disagreements: 0, lastUpdated: "2026-02-10" },
          b: { domain: "b", score: 0.5, corrections: 0, successes: 0, disagreements: 0, lastUpdated: "2026-02-17" },
        },
      }),
    };
    const tool = createWhatDoIKnowTool(mockCompetence as never);
    const result = JSON.parse(await tool.execute({}, ctx));
    expect(result.recentlyActive[0].domain).toBe("b");
  });
});
