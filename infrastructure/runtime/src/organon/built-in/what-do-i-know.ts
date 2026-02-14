// Self-observation tool — knowledge domain inventory
import type { ToolHandler, ToolContext } from "../registry.js";
import type { CompetenceModel } from "../../nous/competence.js";
import type { SessionStore } from "../../mneme/store.js";

export function createWhatDoIKnowTool(
  competence?: CompetenceModel,
  store?: SessionStore,
): ToolHandler {
  return {
    definition: {
      name: "what_do_i_know",
      description:
        "Inventory your knowledge domains, strengths, and recent activity patterns.\n\n" +
        "USE WHEN:\n" +
        "- Assessing whether you're the right agent for a task\n" +
        "- Preparing a self-introduction or capability summary\n" +
        "- Checking what domains you've been active in recently\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You already know your strengths from SOUL.md / IDENTITY.md\n\n" +
        "TIPS:\n" +
        "- Combines competence model data with recent interaction signals\n" +
        "- Domains with high success count and score > 0.6 are strengths\n" +
        "- Domains with correction count > 3 are areas for caution",
      input_schema: {
        type: "object",
        properties: {},
      },
    },
    async execute(
      _input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const result: Record<string, unknown> = { nousId: context.nousId };

      if (competence) {
        const agent = competence.getAgentCompetence(context.nousId);
        if (agent) {
          const domains = Object.values(agent.domains);
          const strengths = domains
            .filter((d) => d.score >= 0.6 && d.successes >= 2)
            .sort((a, b) => b.score - a.score)
            .map((d) => ({ domain: d.domain, score: d.score, successes: d.successes }));

          const weaknesses = domains
            .filter((d) => d.score < 0.4 || d.corrections >= 3)
            .sort((a, b) => a.score - b.score)
            .map((d) => ({ domain: d.domain, score: d.score, corrections: d.corrections }));

          const active = domains
            .sort((a, b) => b.lastUpdated.localeCompare(a.lastUpdated))
            .slice(0, 5)
            .map((d) => ({ domain: d.domain, lastUpdated: d.lastUpdated }));

          result["overallScore"] = agent.overallScore;
          result["totalDomains"] = domains.length;
          result["strengths"] = strengths;
          result["weaknesses"] = weaknesses;
          result["recentlyActive"] = active;
        } else {
          result["note"] = "No competence data recorded yet — you're starting fresh";
        }
      } else {
        result["note"] = "Competence model not available";
      }

      if (store) {
        const signals = store.getSignalHistory(context.nousId, 20);
        const corrections = signals.filter((s) => s.signal === "correction");
        const successes = signals.filter((s) => s.signal === "success");
        result["recentSignals"] = {
          total: signals.length,
          corrections: corrections.length,
          successes: successes.length,
        };
      }

      return JSON.stringify(result);
    },
  };
}
