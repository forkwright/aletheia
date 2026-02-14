// Session status tool — usage and model info
import type { ToolHandler, ToolContext } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";

export function createSessionStatusTool(store?: SessionStore): ToolHandler {
  return {
    definition: {
      name: "session_status",
      description:
        "Get current session information including model, token usage, and message count.",
      input_schema: {
        type: "object",
        properties: {},
      },
    },
    async execute(
      _input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      if (!store) {
        return JSON.stringify({
          nousId: context.nousId,
          sessionId: context.sessionId,
          note: "Store not available",
        });
      }

      const session = store.findSessionById(context.sessionId);
      if (!session) {
        return JSON.stringify({
          nousId: context.nousId,
          sessionId: context.sessionId,
          error: "Session not found",
        });
      }

      return JSON.stringify({
        nousId: context.nousId,
        sessionId: session.id,
        model: session.model,
        messageCount: session.messageCount,
        tokenCount: session.tokenCountEstimate,
        status: session.status,
        createdAt: session.createdAt,
        updatedAt: session.updatedAt,
      });
    },
  };
}

export const sessionStatusTool = createSessionStatusTool();
