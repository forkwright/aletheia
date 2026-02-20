// Sub-nous spawning — run a scoped task on a temporary agent (supports ephemeral specialists)
import type { ToolHandler, ToolContext } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";
import {
  spawnEphemeral,
  recordEphemeralTurn,
  teardownEphemeral,
  harvestOutput,
} from "../../nous/ephemeral.js";
import { createLogger } from "../../koina/logger.js";

const log = createLogger("organon.spawn");

export interface AgentDispatcher {
  handleMessage(msg: InboundMessage): Promise<TurnOutcome>;
  store?: SessionStore;
}

export function createSessionsSpawnTool(
  dispatcher?: AgentDispatcher,
  sharedRoot?: string,
): ToolHandler {
  return {
    definition: {
      name: "sessions_spawn",
      description:
        "Spawn a sub-agent to handle an isolated task, returning the result.\n\n" +
        "USE WHEN:\n" +
        "- Running research or analysis in parallel with your main work\n" +
        "- Isolating complex operations that might fail\n" +
        "- Creating temporary specialists with custom identities (ephemeral=true)\n" +
        "- Delegating a task to a different agent's workspace and toolset\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Simple question for another agent — use sessions_ask instead\n" +
        "- Fire-and-forget notification — use sessions_send instead\n" +
        "- The task is trivial and doesn't need isolation\n\n" +
        "TIPS:\n" +
        "- Ephemeral mode creates a temporary specialist with a custom SOUL.md\n" +
        "- Ephemeral agents auto-teardown after maxTurns or maxDurationSeconds\n" +
        "- Use a specific sessionKey for continuity across repeated spawns\n" +
        "- Default timeout 180s — increase for complex tasks",
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
          ephemeral: {
            type: "boolean",
            description: "If true, creates a temporary specialist agent with a custom SOUL (default: false)",
          },
          ephemeralName: {
            type: "string",
            description: "Name for the ephemeral specialist (required when ephemeral=true)",
          },
          ephemeralSoul: {
            type: "string",
            description: "SOUL.md content defining the specialist's identity and capabilities (required when ephemeral=true)",
          },
          maxTurns: {
            type: "number",
            description: "Maximum turns for ephemeral agent before teardown (default: 5)",
          },
          maxDurationSeconds: {
            type: "number",
            description: "Maximum lifetime for ephemeral agent in seconds (default: 600)",
          },
          model: {
            type: "string",
            description: "Model override for the spawned agent (e.g., 'anthropic/claude-sonnet-4-20250514' for cheaper tasks). Default: agent's configured model.",
          },
          budgetTokens: {
            type: "number",
            description: "Maximum total tokens (input + output) for the spawn. The turn is aborted if the budget is exceeded. Default: no limit.",
          },
        },
        required: ["task"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const task = input["task"] as string;
      const agentId = (input["agentId"] as string) ?? context.nousId;
      const timeoutSeconds = (input["timeoutSeconds"] as number) ?? 180;
      const isEphemeral = input["ephemeral"] === true;
      const modelOverride = input["model"] as string | undefined;
      const budgetTokens = input["budgetTokens"] as number | undefined;
      const sessionKey =
        (input["sessionKey"] as string) ??
        `spawn:${context.nousId}:${Date.now().toString(36)}`;

      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      // Ephemeral specialist path — spawn, execute, harvest, teardown
      if (isEphemeral) {
        const name = (input["ephemeralName"] as string) ?? "specialist";
        const soul = input["ephemeralSoul"] as string;
        if (!soul) {
          return JSON.stringify({ error: "ephemeralSoul is required when ephemeral=true" });
        }
        if (!sharedRoot) {
          return JSON.stringify({ error: "Ephemeral agents not available (no shared root)" });
        }

        const maxTurns = (input["maxTurns"] as number) ?? 5;
        const maxDurationMs = ((input["maxDurationSeconds"] as number) ?? 600) * 1000;

        let agent;
        try {
          agent = spawnEphemeral({ name, soul, maxTurns, maxDurationMs }, sharedRoot);
        } catch (err) {
          return JSON.stringify({ error: err instanceof Error ? err.message : String(err) });
        }

        log.info(`Ephemeral ${agent.id} (${name}) spawned by ${context.nousId}`);

        // Audit trail
        const auditId = dispatcher.store?.recordCrossAgentCall({
          sourceSessionId: context.sessionId,
          sourceNousId: context.nousId,
          targetNousId: `ephemeral:${agent.id}`,
          kind: "spawn",
          content: task.slice(0, 2000),
        });

        let timer: ReturnType<typeof setTimeout>;
        const timeoutPromise = new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error(`Ephemeral timeout after ${timeoutSeconds}s`)),
            timeoutSeconds * 1000,
          );
        });

        try {
          const outcome = await Promise.race([
            dispatcher.handleMessage({
              text: task,
              nousId: agentId,
              sessionKey: `ephemeral:${agent.id}`,
              parentSessionId: context.sessionId,
              channel: "spawn",
              peerKind: "agent",
              peerId: context.nousId,
              ...(modelOverride ? { model: modelOverride } : {}),
              depth: (context.depth ?? 0) + 1,
            }),
            timeoutPromise,
          ]);
          clearTimeout(timer!);

          recordEphemeralTurn(agent.id, outcome.text);
          const output = harvestOutput(agent.id);
          const torn = teardownEphemeral(agent.id);
          const totalTokens = (outcome.inputTokens ?? 0) + (outcome.outputTokens ?? 0);
          const overBudget = budgetTokens && totalTokens > budgetTokens;

          if (auditId && dispatcher.store) {
            dispatcher.store.updateCrossAgentCall(auditId, {
              targetSessionId: outcome.sessionId,
              status: "responded",
              response: outcome.text,
            });
          }

          return JSON.stringify({
            ephemeral: true,
            agentId: agent.id,
            name,
            result: outcome.text,
            turnsUsed: torn?.turnCount ?? 1,
            output: output.slice(0, 2000),
            tokens: {
              input: outcome.inputTokens,
              output: outcome.outputTokens,
              total: totalTokens,
              budget: budgetTokens ?? null,
              overBudget: overBudget ?? false,
            },
          });
        } catch (err) {
          clearTimeout(timer!);
          teardownEphemeral(agent.id);

          if (auditId && dispatcher.store) {
            dispatcher.store.updateCrossAgentCall(auditId, {
              status: "error",
              response: err instanceof Error ? err.message : String(err),
            });
          }

          return JSON.stringify({
            ephemeral: true,
            agentId: agent.id,
            name,
            error: err instanceof Error ? err.message : String(err),
          });
        }
      }

      // Standard spawn path
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
            ...(modelOverride ? { model: modelOverride } : {}),
            depth: (context.depth ?? 0) + 1,
          }),
          timeoutPromise,
        ]);
        clearTimeout(timer!);

        const totalTokens = (outcome.inputTokens ?? 0) + (outcome.outputTokens ?? 0);
        const overBudget = budgetTokens && totalTokens > budgetTokens;

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
            total: totalTokens,
            budget: budgetTokens ?? null,
            overBudget: overBudget ?? false,
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
