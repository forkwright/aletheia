// Synchronous cross-agent messaging — waits for response
import { createLogger } from "../../koina/logger.js";
import type { ToolHandler, ToolContext } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";

const log = createLogger("organon.sessions-ask");

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

export function createSessionsAskTool(dispatcher?: AgentDispatcher): ToolHandler {
  return {
    definition: {
      name: "sessions_ask",
      description:
        "Ask another agent a question and wait for their response (synchronous).\n\n" +
        "USE WHEN:\n" +
        "- You need expertise from another agent's domain\n" +
        "- Cross-checking your understanding with a specialist\n" +
        "- Getting a second opinion before acting\n\n" +
        "DO NOT USE WHEN:\n" +
        "- You don't need the response — use sessions_send instead\n" +
        "- You need a long-running sub-task — use sessions_spawn instead\n" +
        "- The other agent is the same as you\n\n" +
        "TIPS:\n" +
        "- Blocks until response or timeout (default 120s)\n" +
        "- Detects disagreement in responses automatically\n" +
        "- Returns both the response text and token usage\n" +
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
      context: ToolContext,
    ): Promise<string> {
      const agentId = input["agentId"] as string;
      const message = input["message"] as string;
      const sessionKey =
        (input["sessionKey"] as string) ?? `ask:${context.nousId}`;
      const timeoutSeconds = (input["timeoutSeconds"] as number) ?? 120;

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      if (agentId === context.nousId) {
        return JSON.stringify({ error: "Cannot ask yourself" });
      }

      // Audit trail
      const auditId = dispatcher.store?.recordCrossAgentCall({
        sourceSessionId: context.sessionId,
        sourceNousId: context.nousId,
        targetNousId: agentId,
        kind: "ask",
        content: message.slice(0, 2000),
      });

      let timer: ReturnType<typeof setTimeout>;
      const timeoutPromise = new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error(`Timeout after ${timeoutSeconds}s`)),
          timeoutSeconds * 1000,
        );
      });

      try {
        const outcome = await Promise.race([
          dispatcher.handleMessage({
            text: message,
            nousId: agentId,
            sessionKey,
            parentSessionId: context.sessionId,
            channel: "internal",
            peerKind: "agent",
            peerId: context.nousId,
            depth: (context.depth ?? 0) + 1,
          }),
          timeoutPromise,
        ]);
        clearTimeout(timer!);

        if (auditId && dispatcher.store) {
          dispatcher.store.updateCrossAgentCall(auditId, {
            targetSessionId: outcome.sessionId,
            status: "responded",
            response: outcome.text,
          });
        }

        // Lightweight disagreement detection — heuristic, no extra API call
        const disagreement = detectDisagreement(outcome.text);
        if (disagreement) {
          log.info(
            `Disagreement detected: ${context.nousId} → ${agentId}: ${disagreement}`,
          );
        }

        return JSON.stringify({
          agentId,
          response: outcome.text,
          toolCalls: outcome.toolCalls,
          disagreement: disagreement ?? undefined,
          tokens: {
            input: outcome.inputTokens,
            output: outcome.outputTokens,
          },
        });
      } catch (err) {
        clearTimeout(timer!);

        if (auditId && dispatcher.store) {
          const isTimeout = err instanceof Error && err.message.includes("Timeout");
          dispatcher.store.updateCrossAgentCall(auditId, {
            status: isTimeout ? "timeout" : "error",
            response: err instanceof Error ? err.message : String(err),
          });
        }

        return JSON.stringify({
          agentId,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    },
  };
}

export const sessionsAskTool = createSessionsAskTool();

const DISAGREEMENT_PATTERNS = [
  { pattern: /\bI disagree\b/i, signal: "explicit disagreement" },
  { pattern: /\bthat's not (?:correct|right|accurate)\b/i, signal: "factual correction" },
  { pattern: /\bactually,?\s/i, signal: "correction" },
  { pattern: /\bhowever,?\s.*\binstead\b/i, signal: "alternative proposal" },
  { pattern: /\bI'd (?:suggest|recommend) (?:instead|rather|a different)\b/i, signal: "counter-suggestion" },
  { pattern: /\bthat (?:won't|wouldn't|doesn't|can't) work\b/i, signal: "rejection" },
  { pattern: /\bI (?:don't think|wouldn't say|wouldn't agree)\b/i, signal: "pushback" },
];

function detectDisagreement(responseText: string): string | null {
  for (const { pattern, signal } of DISAGREEMENT_PATTERNS) {
    if (pattern.test(responseText)) return signal;
  }
  return null;
}
