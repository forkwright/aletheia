// Synchronous cross-agent messaging — waits for response
import type { ToolHandler } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
}

export function createSessionsAskTool(dispatcher?: AgentDispatcher): ToolHandler {
  return {
    definition: {
      name: "sessions_ask",
      description:
        "Ask another nous (agent) a question and wait for their response. Synchronous — blocks until the target responds or times out.",
      input_schema: {
        type: "object",
        properties: {
          agentId: {
            type: "string",
            description: "Target nous ID (e.g., 'syn', 'eiron', 'arbor')",
          },
          message: {
            type: "string",
            description: "Question or request to send",
          },
          sessionKey: {
            type: "string",
            description: "Target session key (default: 'ask:<caller>')",
          },
          timeoutSeconds: {
            type: "number",
            description: "Max wait time in seconds (default: 120)",
          },
        },
        required: ["agentId", "message"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: { nousId: string; sessionId: string },
    ): Promise<string> {
      const agentId = input.agentId as string;
      const message = input.message as string;
      const sessionKey =
        (input.sessionKey as string) ?? `ask:${context.nousId}`;
      const timeoutSeconds = (input.timeoutSeconds as number) ?? 120;

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      if (agentId === context.nousId) {
        return JSON.stringify({ error: "Cannot ask yourself" });
      }

      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(
          () => reject(new Error(`Timeout after ${timeoutSeconds}s`)),
          timeoutSeconds * 1000,
        ),
      );

      try {
        const outcome = await Promise.race([
          dispatcher.handleMessage({
            text: message,
            nousId: agentId,
            sessionKey,
            channel: "internal",
            peerKind: "agent",
            peerId: context.nousId,
          }),
          timeoutPromise,
        ]);

        return JSON.stringify({
          agentId,
          response: outcome.text,
          toolCalls: outcome.toolCalls,
          tokens: {
            input: outcome.inputTokens,
            output: outcome.outputTokens,
          },
        });
      } catch (err) {
        return JSON.stringify({
          agentId,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    },
  };
}

export const sessionsAskTool = createSessionsAskTool();
