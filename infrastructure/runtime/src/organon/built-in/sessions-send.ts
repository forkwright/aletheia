// Cross-agent fire-and-forget messaging
import { createLogger } from "../../koina/logger.js";
import type { ToolHandler, ToolContext } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";

const log = createLogger("organon:sessions-send");

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

export function createSessionsSendTool(dispatcher?: AgentDispatcher): ToolHandler {
  return {
    definition: {
      name: "sessions_send",
      description:
        "Send a message to another nous (agent). Fire-and-forget — does not wait for a response.",
      input_schema: {
        type: "object",
        properties: {
          agentId: {
            type: "string",
            description: "Target nous ID (e.g., 'syn', 'eiron', 'arbor')",
          },
          message: {
            type: "string",
            description: "Message to send",
          },
          sessionKey: {
            type: "string",
            description: "Target session key (default: 'main')",
          },
        },
        required: ["agentId", "message"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const agentId = input.agentId as string;
      const message = input.message as string;
      const sessionKey = (input.sessionKey as string) ?? "main";

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      // Audit trail
      const auditId = dispatcher.store?.recordCrossAgentCall({
        sourceSessionId: context.sessionId,
        sourceNousId: context.nousId,
        targetNousId: agentId,
        kind: "send",
        content: message.slice(0, 2000),
      });

      dispatcher
        .handleMessage({
          text: message,
          nousId: agentId,
          sessionKey,
          channel: "internal",
          peerKind: "agent",
          peerId: context.nousId,
        })
        .then((outcome) => {
          if (auditId && dispatcher.store) {
            dispatcher.store.updateCrossAgentCall(auditId, {
              targetSessionId: outcome.sessionId,
              status: "delivered",
              response: outcome.text,
            });
          }
        })
        .catch((err) => {
          log.warn(`sessions_send to ${agentId} failed: ${err instanceof Error ? err.message : err}`);
          if (auditId && dispatcher.store) {
            dispatcher.store.updateCrossAgentCall(auditId, {
              status: "error",
              response: err instanceof Error ? err.message : String(err),
            });
          }
        });

      return JSON.stringify({ sent: true, agentId, sessionKey });
    },
  };
}

export const sessionsSendTool = createSessionsSendTool();
