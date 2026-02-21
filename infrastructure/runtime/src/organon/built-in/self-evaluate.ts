// Self-evaluation tool — structured performance assessment
import type { ToolContext, ToolHandler } from "../registry.js";
import type { CompetenceModel } from "../../nous/competence.js";
import type { UncertaintyTracker } from "../../nous/uncertainty.js";
import type { SessionStore } from "../../mneme/store.js";

export function createSelfEvaluateTool(
  store: SessionStore,
  competence?: CompetenceModel,
  uncertainty?: UncertaintyTracker,
): ToolHandler {
  return {
    definition: {
      name: "self_evaluate",
      description:
        "Run a structured self-evaluation of your recent performance.\n\n" +
        "Returns competence scores, calibration metrics, recent activity stats, and actionable recommendations.\n\n" +
        "USE WHEN:\n" +
        "- During periodic self-reflection (daily/weekly)\n" +
        "- Before writing or updating STRATEGY.md\n" +
        "- After receiving multiple corrections in a session\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You just need a quick calibration check (use check_calibration instead)\n\n" +
        "TIPS:\n" +
        "- Use the output to update your STRATEGY.md with adjusted approaches\n" +
        "- Low domain scores (<0.35) suggest delegating that domain\n" +
        "- High correction-to-success ratio suggests reviewing your approach",
      input_schema: {
        type: "object",
        properties: {
          days: {
            type: "number",
            description: "Number of days to look back for activity (default 7)",
          },
        },
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const days = (input["days"] as number) ?? 7;
      const cutoff = new Date(Date.now() - days * 86400000).toISOString();

      const result: Record<string, unknown> = { nousId: context.nousId, evaluationPeriodDays: days };

      // Competence snapshot
      if (competence) {
        const agent = competence.getAgentCompetence(context.nousId);
        if (agent) {
          result["overallScore"] = agent.overallScore;
          result["domains"] = Object.fromEntries(
            Object.entries(agent.domains).map(([k, v]) => [
              k, { score: v.score, corrections: v.corrections, successes: v.successes },
            ]),
          );
        } else {
          result["competence"] = { note: "No competence data recorded yet" };
        }
      } else {
        result["competence"] = { note: "Competence model not available" };
      }

      // Calibration data
      if (uncertainty) {
        result["calibration"] = uncertainty.getSummary(context.nousId);
      } else {
        result["calibration"] = { note: "Uncertainty tracker not available" };
      }

      // Recent activity
      const sessions = store.listSessions(context.nousId);
      const recentSessions = sessions.filter((s) => s.updatedAt >= cutoff);
      result["recentActivity"] = {
        totalSessions: sessions.length,
        recentSessions: recentSessions.length,
        periodStart: cutoff,
      };

      // Recommendations
      const recommendations: string[] = [];

      if (competence) {
        const agent = competence.getAgentCompetence(context.nousId);
        if (agent) {
          for (const [domain, data] of Object.entries(agent.domains)) {
            if (data.score < 0.35) {
              recommendations.push(
                `Domain "${domain}" score is ${data.score.toFixed(2)} — consider delegating tasks in this area via sessions_ask`,
              );
            }
            if (data.corrections > data.successes && data.corrections > 2) {
              recommendations.push(
                `Domain "${domain}" has more corrections (${data.corrections}) than successes (${data.successes}) — review your approach`,
              );
            }
          }
        }
      }

      if (recentSessions.length === 0) {
        recommendations.push("No recent activity — consider checking if there are pending tasks or messages");
      }

      result["recommendations"] = recommendations.length > 0
        ? recommendations
        : ["No issues detected — performance is within normal parameters"];

      return JSON.stringify(result);
    },
  };
}
