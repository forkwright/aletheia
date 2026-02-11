// Sub-nous spawning — run a scoped task on a temporary agent
import type { ToolHandler, ToolContext } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

export function createSessionsSpawnTool(
  dispatcher?: AgentDispatcher,
): ToolHandler {
  return {
    definition: {
      name: "sessions_spawn",
      description:
        "Spawn a sub-agent to handle a scoped task. Runs the task to completion and returns the result. Good for parallel work, research, or isolating complex operations.",
      input_schema: {
        type: "object",
        properties: {
          task: {
            type: "string",
            description: "Task description for the sub-agent",
          },
          agentId: {
            type: "string",
            description:
              "Which nous to run as (default: same as caller). Determines workspace, tools, and identity.",
          },
          sessionKey: {
            type: "string",
            description:
              "Session key for the spawn (default: auto-generated). Use a specific key for continuity across spawns.",
          },
          timeoutSeconds: {
            type: "number",
            description: "Max execution time in seconds (default: 180)",
          },
        },
        required: ["task"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const task = input.task as string;
      const agentId = (input.agentId as string) ?? context.nousId;
      const timeoutSeconds = (input.timeoutSeconds as number) ?? 180;
      const sessionKey =
        (input.sessionKey as string) ??
        `spawn:${context.nousId}:${Date.now().toString(36)}`;

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      // Audit trail
      const auditId = dispatcher.store?.recordCrossAgentCall({
        sourceSessionId: context.sessionId,
        sourceNousId: context.nousId,
        targetNousId: agentId,
        kind: "spawn",
        content: task.slice(0, 2000),
      });

      let timer: ReturnType<typeof setTimeout>;
      const timeoutPromise = new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error(`Spawn timeout after ${timeoutSeconds}s`)),
          timeoutSeconds * 1000,
        );
      });

      try {
        const outcome = await Promise.race([
          dispatcher.handleMessage({
            text: task,
            nousId: agentId,
            sessionKey,
            parentSessionId: context.sessionId,
            channel: "spawn",
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

        return JSON.stringify({
          agentId,
          sessionKey,
          result: outcome.text,
          toolCalls: outcome.toolCalls,
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
          sessionKey,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    },
  };
}

export const sessionsSpawnTool = createSessionsSpawnTool();
