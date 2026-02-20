// Cross-agent fire-and-forget messaging
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";

const log = createLogger("organon:sessions-send");
const MAX_PENDING_SENDS = 5;
let pendingSends = 0;

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

export function createSessionsSendTool(dispatcher?: AgentDispatcher): ToolHandler {
  return {
    definition: {
      name: "sessions_send",
      description:
        "Send a message to another agent without waiting for a response (fire-and-forget).\n\n" +
        "USE WHEN:\n" +
        "- Notifying another agent of information they should know\n" +
        "- Delegating a task where you don't need the result\n" +
        "- Broadcasting updates or status changes\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You need the other agent's response — use sessions_ask instead\n" +
        "- You need a dedicated sub-task with isolation — use sessions_spawn instead\n" +
        "- You want to message a human — use message instead\n\n" +
        "TIPS:\n" +
        "- Cannot send to yourself\n" +
        "- Max 5 concurrent pending sends\n" +
        "- Cross-agent calls are audited and tracked",
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
      const agentId = input["agentId"] as string;
      const message = input["message"] as string;
      const sessionKey = (input["sessionKey"] as string) ?? "main";

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      if (agentId === context.nousId) {
        return JSON.stringify({ error: "Cannot send to yourself" });
      }

      if (pendingSends >= MAX_PENDING_SENDS) {
        return JSON.stringify({ error: "Too many pending sends — try again later" });
      }

      // Audit trail
      const auditId = dispatcher.store?.recordCrossAgentCall({
        sourceSessionId: context.sessionId,
        sourceNousId: context.nousId,
        targetNousId: agentId,
        kind: "send",
        content: message.slice(0, 2000),
      });

      pendingSends++;
      dispatcher
        .handleMessage({
          text: message,
          nousId: agentId,
          sessionKey,
          channel: "internal",
          peerKind: "agent",
          peerId: context.nousId,
          depth: (context.depth ?? 0) + 1,
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
        })
        .finally(() => { pendingSends--; });

      return JSON.stringify({ sent: true, agentId, sessionKey });
    },
  };
}

export const sessionsSendTool = createSessionsSendTool();
