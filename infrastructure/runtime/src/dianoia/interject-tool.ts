// plan_interject — sub-agent interjection tool (INTERJ-02)
//
// Allows a sub-agent running within dianoia execution to send a message
// back to the orchestrator. The message is queued and consumed at the next
// turn boundary (between waves or between tasks).
//
// This provides the bidirectional communication channel:
// - Orchestrator → sub-agent: via task prompt (existing)
// - Sub-agent → orchestrator: via plan_interject (this tool)

import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type Database from "better-sqlite3";

const log = createLogger("dianoia:interject");

export function createPlanInterjectTool(db: Database.Database): ToolHandler {
  const store = new PlanningStore(db);

  return {
    definition: {
      name: "plan_interject",
      description:
        "Send a message from a sub-agent back to the orchestrator during plan execution.\n\n" +
        "USE WHEN:\n" +
        "- You encounter an issue that the orchestrator needs to know about\n" +
        "- You need to escalate a decision that's beyond your scope\n" +
        "- You want to share a discovery that affects other phases\n" +
        "- You need to request a pause or human review\n\n" +
        "The message is queued and delivered at the next turn boundary. " +
        "Critical-priority messages will pause execution.",
      input_schema: {
        type: "object",
        properties: {
          projectId: {
            type: "string",
            description: "Planning project ID (from your task context)",
          },
          content: {
            type: "string",
            description: "Message content — be specific about what you found or need",
          },
          priority: {
            type: "string",
            enum: ["low", "normal", "high", "critical"],
            description:
              "Message priority. critical = pause execution. high = address before next wave. " +
              "normal = informational. low = can be reviewed later. Default: normal.",
          },
          phaseId: {
            type: "string",
            description: "Phase ID this message relates to (optional, for routing)",
          },
        },
        required: ["projectId", "content"],
      },
    },

    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const projectId = input["projectId"] as string;
      const content = input["content"] as string;
      const priority = (input["priority"] as string | undefined) ?? "normal";
      const phaseId = input["phaseId"] as string | undefined;

      if (!projectId || !content) {
        return JSON.stringify({ error: "projectId and content are required" });
      }

      try {
        const enqueueOpts: Parameters<typeof store.enqueueMessage>[0] = {
          projectId,
          source: "sub-agent",
          sourceSessionId: context.sessionId,
          content,
          priority: priority as "low" | "normal" | "high" | "critical",
        };
        if (phaseId) enqueueOpts.phaseId = phaseId;

        const message = store.enqueueMessage(enqueueOpts);

        log.info(
          `Sub-agent interjection from ${context.sessionId}: [${priority}] ${content.slice(0, 80)} → project ${projectId}`,
        );

        return JSON.stringify({
          queued: true,
          messageId: message.id,
          priority,
          note:
            priority === "critical"
              ? "Message queued — execution will pause at next turn boundary"
              : "Message queued — will be delivered at next turn boundary",
        });
      } catch (error) {
        const errMsg = error instanceof Error ? error.message : String(error);
        log.error(`Interjection failed: ${errMsg}`, { projectId, sessionId: context.sessionId });
        return JSON.stringify({ error: errMsg });
      }
    },
  };
}
