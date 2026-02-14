// Cross-agent fire-and-forget messaging
import type { ToolHandler } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
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
    async execute(input: Record<string, unknown>): Promise<string> {
      const agentId = input.agentId as string;
      const message = input.message as string;
      const sessionKey = (input.sessionKey as string) ?? "main";

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      dispatcher
        .handleMessage({
          text: message,
          nousId: agentId,
          sessionKey,
          channel: "internal",
          peerKind: "agent",
        })
        .catch(() => {});

      return JSON.stringify({ sent: true, agentId, sessionKey });
    },
  };
}

export const sessionsSendTool = createSessionsSendTool();
