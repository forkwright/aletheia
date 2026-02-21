// Meta-tool — status report: agent metrics + blackboard + recent activity
import type { ToolContext, ToolHandler } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";
import type { CompetenceModel } from "../../nous/competence.js";

export function createStatusReportTool(
  store: SessionStore,
  competence?: CompetenceModel,
): ToolHandler {
  return {
    definition: {
      name: "status_report",
      description:
        "Generate a structured status report: agent metrics, blackboard state, and recent signals.\n\n" +
        "USE WHEN:\n" +
        "- Asked to give a system status update\n" +
        "- Running a heartbeat or check-in routine\n" +
        "- Preparing context for the operator\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You need detailed session-level data (use session_status)\n\n" +
        "TIPS:\n" +
        "- Includes blackboard entries, competence snapshot, and interaction signals\n" +
        "- Designed for quick situational reports",
      input_schema: {
        type: "object",
        properties: {},
      },
    },
    async execute(
      _input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const report: Record<string, unknown> = {
        agent: context.nousId,
        session: context.sessionId,
        timestamp: new Date().toISOString(),
      };

      // Blackboard state
      try {
        const bbKeys = store.blackboardList();
        report["blackboard"] = {
          activeKeys: bbKeys.length,
          keys: bbKeys.slice(0, 10),
        };
      } catch { /* section gathering failed */
        report["blackboard"] = { error: "unavailable" };
      }

      // Recent interaction signals
      try {
        const signals = store.getSignalHistory(context.nousId, 10);
        const grouped: Record<string, number> = {};
        for (const s of signals) {
          grouped[s.signal] = (grouped[s.signal] ?? 0) + 1;
        }
        report["recentSignals"] = grouped;
      } catch { /* section gathering failed */
        report["recentSignals"] = { error: "unavailable" };
      }

      // Competence overview
      if (competence) {
        const agent = competence.getAgentCompetence(context.nousId);
        if (agent) {
          report["competence"] = {
            overall: agent.overallScore,
            domains: Object.keys(agent.domains).length,
          };
        }
      }

      // Active sessions for this agent
      try {
        const sessions = store.listSessions(context.nousId);
        const active = sessions.filter((s) => s.status === "active");
        report["sessions"] = {
          active: active.length,
          total: sessions.length,
        };
      } catch { /* section gathering failed */
        report["sessions"] = { error: "unavailable" };
      }

      return JSON.stringify(report);
    },
  };
}
