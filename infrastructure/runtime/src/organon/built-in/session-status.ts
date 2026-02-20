// Session status tool — usage and model info
import type { ToolContext, ToolHandler } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";

export function createSessionStatusTool(store?: SessionStore): ToolHandler {
  return {
    definition: {
      name: "session_status",
      description:
        "Get current session info: model, token count, message count, and status.\n\n" +
        "USE WHEN:\n" +
        "- Checking how much context you've used\n" +
        "- Verifying which model is active for this session\n" +
        "- Monitoring session health before long operations\n\n" +
        "TIPS:\n" +
        "- No parameters needed — reads current session automatically\n" +
        "- tokenCount is an estimate, not exact",
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
