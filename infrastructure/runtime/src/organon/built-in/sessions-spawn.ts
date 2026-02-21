// Sub-nous spawning — run a scoped task on a temporary agent (supports ephemeral specialists)
import type { ToolContext, ToolHandler } from "../registry.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";
import {
  harvestOutput,
  recordEphemeralTurn,
  spawnEphemeral,
  teardownEphemeral,
} from "../../nous/ephemeral.js";
import { resolveRole, ROLE_NAMES } from "../config/sub-agent-roles.js";
import { parseStructuredResult } from "../../nous/roles/index.js";
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
          tools: {
            type: "array",
            items: { type: "string" },
            description:
              "Glob patterns restricting which tools the spawned agent can use " +
              "(e.g. ['read', 'write', 'exec', 'grep*']). Omit for all tools. " +
              "Patterns support * as wildcard (e.g. 'mem0_*' matches mem0_search).",
          },
          model: {
            type: "string",
            description: "Model override for the spawned agent (e.g., 'anthropic/claude-sonnet-4-20250514' for cheaper tasks). Default: agent's configured model.",
          },
          budgetTokens: {
            type: "number",
            description: "Maximum total tokens (input + output) for the spawn. The turn is aborted if the budget is exceeded. Default: no limit.",
          },
          role: {
            type: "string",
            enum: ROLE_NAMES,
            description: "Pre-configured role preset (coder/reviewer/researcher/explorer/runner). Sets model, system prompt, tools, maxTurns, and token budget. Explicit params override role defaults.",
          },
          tasks: {
            type: "array",
            items: {
              type: "object",
              properties: {
                task: { type: "string", description: "Task description" },
                role: { type: "string", enum: ROLE_NAMES, description: "Role preset" },
                agentId: { type: "string", description: "Agent to run as" },
                model: { type: "string", description: "Model override" },
              },
              required: ["task"],
            },
            description: "Array of tasks for parallel dispatch (max 3 concurrent). When provided, 'task' field is ignored.",
          },
        },
        required: [],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      // Parallel dispatch path
      const tasksArray = input["tasks"] as Array<Record<string, unknown>> | undefined;
      if (tasksArray && tasksArray.length > 0) {
        return executeParallel(tasksArray, input, context, dispatcher, sharedRoot);
      }

      const task = input["task"] as string;
      if (!task) return JSON.stringify({ error: "Either 'task' or 'tasks' is required" });
      const agentId = (input["agentId"] as string) ?? context.nousId;
      const isEphemeral = input["ephemeral"] === true;
      const roleName = input["role"] as string | undefined;
      const role = roleName ? resolveRole(roleName) : null;

      const modelOverride = (input["model"] as string | undefined) ?? role?.model;
      const budgetTokens = (input["budgetTokens"] as number | undefined) ?? role?.maxTokenBudget;
      const timeoutSeconds = (input["timeoutSeconds"] as number) ?? 180;
      const toolFilter = input["tools"] as string[] | undefined;
      const sessionKey =
        (input["sessionKey"] as string) ??
        `spawn:${context.nousId}:${Date.now().toString(36)}`;
      const startTime = Date.now();

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
              ...(toolFilter ? { toolFilter } : {}),
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
            ...(toolFilter ? { toolFilter } : {}),
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

        const durationMs = Date.now() - startTime;
        const structured = parseStructuredResult(outcome.text);

        if (dispatcher.store) {
          dispatcher.store.logSubAgentCall({
            sessionId: outcome.sessionId,
            parentSessionId: context.sessionId,
            parentNousId: context.nousId,
            ...(roleName ? { role: roleName } : {}),
            agentId,
            task,
            ...(modelOverride ? { model: modelOverride } : {}),
            inputTokens: outcome.inputTokens ?? 0,
            outputTokens: outcome.outputTokens ?? 0,
            toolCalls: outcome.toolCalls ?? 0,
            status: "completed",
            durationMs,
          });
        }

        return JSON.stringify({
          agentId,
          sessionKey,
          ...(roleName ? { role: roleName } : {}),
          result: outcome.text,
          ...(structured ? { structuredResult: structured } : {}),
          toolCalls: outcome.toolCalls,
          tokens: {
            input: outcome.inputTokens,
            output: outcome.outputTokens,
            total: totalTokens,
            budget: budgetTokens ?? null,
            overBudget: overBudget ?? false,
          },
          durationMs,
        });
      } catch (err) {
        clearTimeout(timer!);
        const durationMs = Date.now() - startTime;
        const errMsg = err instanceof Error ? err.message : String(err);

        if (auditId && dispatcher.store) {
          const isTimeout = err instanceof Error && err.message.includes("Timeout");
          dispatcher.store.updateCrossAgentCall(auditId, {
            status: isTimeout ? "timeout" : "error",
            response: errMsg,
          });
        }

        if (dispatcher.store) {
          dispatcher.store.logSubAgentCall({
            sessionId: sessionKey,
            parentSessionId: context.sessionId,
            parentNousId: context.nousId,
            ...(roleName ? { role: roleName } : {}),
            agentId,
            task,
            ...(modelOverride ? { model: modelOverride } : {}),
            inputTokens: 0,
            outputTokens: 0,
            toolCalls: 0,
            status: "error",
            error: errMsg,
            durationMs,
          });
        }

        return JSON.stringify({
          agentId,
          sessionKey,
          error: errMsg,
        });
      }
    },
  };
}

const MAX_PARALLEL = 3;

async function executeParallel(
  tasks: Array<Record<string, unknown>>,
  parentInput: Record<string, unknown>,
  context: ToolContext,
  dispatcher?: AgentDispatcher,
  _sharedRoot?: string,
): Promise<string> {
  if (!dispatcher) return JSON.stringify({ error: "Agent dispatch not available" });

  const capped = tasks.slice(0, MAX_PARALLEL);
  if (tasks.length > MAX_PARALLEL) {
    log.warn(`Parallel dispatch capped at ${MAX_PARALLEL} (${tasks.length} requested)`);
  }

  const timeoutSeconds = (parentInput["timeoutSeconds"] as number) ?? 180;
  const startTime = Date.now();

  const promises = capped.map(async (taskDef, idx) => {
    const task = taskDef["task"] as string;
    const roleName = (taskDef["role"] as string | undefined) ?? (parentInput["role"] as string | undefined);
    const role = roleName ? resolveRole(roleName) : null;
    const agentId = (taskDef["agentId"] as string | undefined) ?? (parentInput["agentId"] as string | undefined) ?? context.nousId;
    const modelOverride = (taskDef["model"] as string | undefined) ?? (parentInput["model"] as string | undefined) ?? role?.model;
    const budgetTokens = role?.maxTokenBudget;
    const toolFilter = parentInput["tools"] as string[] | undefined;
    const sessionKey = `spawn:${context.nousId}:${Date.now().toString(36)}:${idx}`;

    let timer: ReturnType<typeof setTimeout>;
    const timeoutPromise = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Spawn timeout after ${timeoutSeconds}s`)), timeoutSeconds * 1000);
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
          ...(toolFilter ? { toolFilter } : {}),
          depth: (context.depth ?? 0) + 1,
        }),
        timeoutPromise,
      ]);
      clearTimeout(timer!);

      const totalTokens = (outcome.inputTokens ?? 0) + (outcome.outputTokens ?? 0);
      const structured = parseStructuredResult(outcome.text);
      const durationMs = Date.now() - startTime;

      if (dispatcher.store) {
        dispatcher.store.logSubAgentCall({
          sessionId: outcome.sessionId,
          parentSessionId: context.sessionId,
          parentNousId: context.nousId,
          ...(roleName ? { role: roleName } : {}),
          agentId,
          task,
          ...(modelOverride ? { model: modelOverride } : {}),
          inputTokens: outcome.inputTokens ?? 0,
          outputTokens: outcome.outputTokens ?? 0,
          toolCalls: outcome.toolCalls ?? 0,
          status: "completed",
          durationMs,
        });
      }

      return {
        index: idx,
        task: task.slice(0, 200),
        ...(roleName ? { role: roleName } : {}),
        result: outcome.text,
        ...(structured ? { structuredResult: structured } : {}),
        tokens: { input: outcome.inputTokens, output: outcome.outputTokens, total: totalTokens, budget: budgetTokens ?? null },
        durationMs,
      };
    } catch (err) {
      clearTimeout(timer!);
      return {
        index: idx,
        task: task.slice(0, 200),
        error: err instanceof Error ? err.message : String(err),
      };
    }
  });

  const results = await Promise.allSettled(promises);
  const output = results.map((r) =>
    r.status === "fulfilled" ? r.value : { error: String(r.reason) },
  );

  return JSON.stringify({ parallel: true, count: output.length, results: output, totalDurationMs: Date.now() - startTime });
}

export const sessionsSpawnTool = createSessionsSpawnTool();
