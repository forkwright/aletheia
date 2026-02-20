// Self-observation tool — calibration and competence snapshot
import type { ToolContext, ToolHandler } from "../registry.js";
import type { CompetenceModel } from "../../nous/competence.js";
import type { UncertaintyTracker } from "../../nous/uncertainty.js";

export function createCheckCalibrationTool(
  competence?: CompetenceModel,
  uncertainty?: UncertaintyTracker,
): ToolHandler {
  return {
    definition: {
      name: "check_calibration",
      description:
        "Get your competence scores, calibration metrics, and confidence accuracy.\n\n" +
        "USE WHEN:\n" +
        "- Reflecting on your reliability before giving high-stakes advice\n" +
        "- Checking if you're well-calibrated in a specific domain\n" +
        "- Deciding whether to defer to another agent\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Simple factual questions with low stakes\n\n" +
        "TIPS:\n" +
        "- Brier score: 0 = perfect, 0.25 = coin flip, 1 = worst\n" +
        "- ECE: lower is better-calibrated\n" +
        "- Domain scores: 0.5 = baseline, <0.3 = significant correction history",
      input_schema: {
        type: "object",
        properties: {
          domain: {
            type: "string",
            description: "Optional domain to focus on (e.g., 'health', 'code', 'scheduling')",
          },
        },
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const domain = input["domain"] as string | undefined;
      const result: Record<string, unknown> = { nousId: context.nousId };

      if (competence) {
        const agent = competence.getAgentCompetence(context.nousId);
        if (agent) {
          result["overallScore"] = agent.overallScore;
          if (domain) {
            const d = agent.domains[domain];
            result["domainScore"] = d
              ? { score: d.score, corrections: d.corrections, successes: d.successes, lastUpdated: d.lastUpdated }
              : { note: `No data for domain "${domain}"` };
          } else {
            result["domains"] = Object.fromEntries(
              Object.entries(agent.domains).map(([k, v]) => [
                k,
                { score: v.score, corrections: v.corrections, successes: v.successes },
              ]),
            );
          }
        } else {
          result["competence"] = { note: "No competence data recorded yet" };
        }
      } else {
        result["competence"] = { note: "Competence model not available" };
      }

      if (uncertainty) {
        result["calibration"] = uncertainty.getSummary(context.nousId);
      } else {
        result["calibration"] = { note: "Uncertainty tracker not available" };
      }

      return JSON.stringify(result);
    },
  };
}
