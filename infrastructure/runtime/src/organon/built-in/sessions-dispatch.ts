// Parallel sub-agent dispatch — spawn multiple sub-agents concurrently and collect results
import type { ToolContext, ToolHandler } from "../registry.js";
import type { AgentDispatcher } from "./sessions-spawn.js";
import { resolveRole, ROLE_NAMES } from "../config/sub-agent-roles.js";
import { parseStructuredResult } from "../../nous/roles/index.js";
import { createLogger } from "../../koina/logger.js";

const log = createLogger("organon.dispatch");

interface DispatchTask {
  role?: string;
  task: string;
  context?: string;
  agentId?: string;
  model?: string;
  timeoutSeconds?: number;
}

interface DispatchResult {
  index: number;
  role?: string | undefined;
  task: string;
  status: "success" | "error" | "timeout";
  result?: string;
  structuredResult?: ReturnType<typeof parseStructuredResult>;
  error?: string;
  tokens?: {
    input: number;
    output: number;
    total: number;
  };
  durationMs: number;
}

export function createSessionsDispatchTool(
  dispatcher?: AgentDispatcher,
  _sharedRoot?: string,
): ToolHandler {
  return {
    definition: {
      name: "sessions_dispatch",
      description:
        "Spawn multiple sub-agents in parallel and wait for all results.\n\n" +
        "USE WHEN:\n" +
        "- Decomposing a task into independent sub-tasks that can run concurrently\n" +
        "- Reviewing multiple PRs/files simultaneously\n" +
        "- Running parallel investigations or research queries\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Tasks depend on each other's output (use sequential sessions_spawn)\n" +
        "- Single task (just use sessions_spawn directly)\n\n" +
        "TIPS:\n" +
        "- All tasks run concurrently — results returned when ALL complete\n" +
        "- Each task gets its own isolated context window\n" +
        "- Failed tasks don't block others — partial results are returned\n" +
        "- Use roles (coder/reviewer/researcher/explorer/runner) for preset configs",
      input_schema: {
        type: "object",
        properties: {
          tasks: {
            type: "array",
            items: {
              type: "object",
              properties: {
                role: {
                  type: "string",
                  enum: ROLE_NAMES,
                  description: "Role preset (coder/reviewer/researcher/explorer/runner)",
                },
                task: {
                  type: "string",
                  description: "Task description for the sub-agent",
                },
                context: {
                  type: "string",
                  description: "Additional context to prepend to the task",
                },
                agentId: {
                  type: "string",
                  description: "Which nous to run as (default: caller's nous)",
                },
                model: {
                  type: "string",
                  description: "Model override (default: role default or agent default)",
                },
                timeoutSeconds: {
                  type: "number",
                  description: "Per-task timeout in seconds (default: 180)",
                },
              },
              required: ["task"],
            },
            description: "Array of tasks to dispatch in parallel",
            minItems: 1,
            maxItems: 10,
          },
          reducer: {
            type: "string",
            enum: ["concat", "dedup", "merge"],
            description:
              "How to combine results: concat (default, all results), " +
              "dedup (remove duplicate results by content hash), " +
              "merge (deep-merge JSON object results)",
          },
        },
        required: ["tasks"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      if (!dispatcher) {
        return JSON.stringify({ error: "Agent dispatch not available" });
      }

      const tasks = input["tasks"] as DispatchTask[];
      if (!tasks || !Array.isArray(tasks) || tasks.length === 0) {
        return JSON.stringify({ error: "tasks array is required and must not be empty" });
      }

      if (tasks.length > 10) {
        return JSON.stringify({ error: "Maximum 10 parallel tasks allowed" });
      }

      const startTime = Date.now();
      log.info(`Dispatching ${tasks.length} parallel tasks from ${context.nousId}`);

      // Spawn all tasks concurrently
      const promises = tasks.map(async (taskDef, index): Promise<DispatchResult> => {
        const taskStart = Date.now();
        const roleName = taskDef.role;
        const role = roleName ? resolveRole(roleName) : null;
        const modelOverride = taskDef.model ?? role?.model;
        const agentId = taskDef.agentId ?? context.nousId;
        const timeoutSeconds = taskDef.timeoutSeconds ?? 180;
        const sessionKey = `dispatch:${context.nousId}:${Date.now().toString(36)}:${index}`;

        // Build the full message with optional context
        const fullMessage = taskDef.context
          ? `${taskDef.context}\n\n---\n\n${taskDef.task}`
          : taskDef.task;

        // Audit trail
        const auditId = dispatcher.store?.recordCrossAgentCall({
          sourceSessionId: context.sessionId,
          sourceNousId: context.nousId,
          targetNousId: agentId,
          kind: "spawn",
          content: `[dispatch ${index + 1}/${tasks.length}] ${taskDef.task.slice(0, 1500)}`,
        });

        let timer: ReturnType<typeof setTimeout>;
        const timeoutPromise = new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error(`Dispatch task ${index} timeout after ${timeoutSeconds}s`)),
            timeoutSeconds * 1000,
          );
        });

        try {
          const outcome = await Promise.race([
            dispatcher.handleMessage({
              text: fullMessage,
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

          const durationMs = Date.now() - taskStart;
          const structured = parseStructuredResult(outcome.text);
          const totalTokens = (outcome.inputTokens ?? 0) + (outcome.outputTokens ?? 0);

          if (auditId && dispatcher.store) {
            dispatcher.store.updateCrossAgentCall(auditId, {
              targetSessionId: outcome.sessionId,
              status: "responded",
              response: outcome.text?.slice(0, 2000),
            });
          }

          // Log sub-agent call for cost tracking
          if (dispatcher.store) {
            dispatcher.store.logSubAgentCall({
              sessionId: outcome.sessionId,
              parentSessionId: context.sessionId,
              parentNousId: context.nousId,
              ...(roleName ? { role: roleName } : {}),
              agentId,
              task: taskDef.task,
              ...(modelOverride ? { model: modelOverride } : {}),
              inputTokens: outcome.inputTokens ?? 0,
              outputTokens: outcome.outputTokens ?? 0,
              toolCalls: outcome.toolCalls ?? 0,
              status: "completed",
              durationMs,
            });
          }

          return {
            index,
            role: roleName,
            task: taskDef.task,
            status: "success",
            result: outcome.text,
            ...(structured ? { structuredResult: structured } : {}),
            tokens: {
              input: outcome.inputTokens ?? 0,
              output: outcome.outputTokens ?? 0,
              total: totalTokens,
            },
            durationMs,
          };
        } catch (err) {
          clearTimeout(timer!);
          const durationMs = Date.now() - taskStart;
          const errMsg = err instanceof Error ? err.message : String(err);
          const isTimeout = errMsg.includes("timeout") || errMsg.includes("Timeout");

          if (auditId && dispatcher.store) {
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
              task: taskDef.task,
              ...(modelOverride ? { model: modelOverride } : {}),
              inputTokens: 0,
              outputTokens: 0,
              toolCalls: 0,
              status: "error",
              error: errMsg,
              durationMs,
            });
          }

          return {
            index,
            role: roleName,
            task: taskDef.task,
            status: isTimeout ? "timeout" : "error",
            error: errMsg,
            durationMs,
          };
        }
      });

      // Wait for all tasks
      const results = await Promise.allSettled(promises);
      const totalDurationMs = Date.now() - startTime;

      // Collect results, handling any unexpected rejections
      const dispatchResults: DispatchResult[] = results.map((r, i) => {
        if (r.status === "fulfilled") return r.value;
        return {
          index: i,
          task: tasks[i]?.task ?? "unknown",
          status: "error" as const,
          error: r.reason instanceof Error ? r.reason.message : String(r.reason),
          durationMs: Date.now() - startTime,
        };
      });

      // Apply reducer
      const reducer = (input["reducer"] as string) ?? "concat";
      const { reduced, reducerInfo } = applyReducer(dispatchResults, reducer);

      // Summary stats
      const succeeded = dispatchResults.filter(r => r.status === "success").length;
      const failed = dispatchResults.filter(r => r.status !== "success").length;
      const totalTokens = dispatchResults.reduce((sum, r) => sum + (r.tokens?.total ?? 0), 0);
      const sequentialMs = dispatchResults.reduce((sum, r) => sum + r.durationMs, 0);

      log.info(
        `Dispatch complete: ${succeeded}/${tasks.length} succeeded, ` +
        `${totalDurationMs}ms wall (${sequentialMs}ms sequential), ` +
        `${totalTokens} tokens` +
        (reducer !== "concat" ? `, reducer=${reducer} (${reduced.length}/${dispatchResults.length} kept)` : ""),
      );

      return JSON.stringify({
        taskCount: tasks.length,
        succeeded,
        failed,
        ...(reducer !== "concat" ? { reducer, reducerInfo } : {}),
        results: reduced,
        timing: {
          wallClockMs: totalDurationMs,
          sequentialMs,
          savedMs: sequentialMs - totalDurationMs,
        },
        totalTokens,
      });
    },
  };
}

function simpleHash(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  }
  return h.toString(36);
}

function applyReducer(
  results: DispatchResult[],
  reducer: string,
): { reduced: DispatchResult[]; reducerInfo?: Record<string, unknown> } {
  if (reducer === "concat" || !reducer) {
    return { reduced: results };
  }

  if (reducer === "dedup") {
    const seen = new Set<string>();
    const kept: DispatchResult[] = [];
    let removed = 0;
    for (const r of results) {
      const hash = simpleHash((r.result ?? r.error ?? "").replace(/\s+/g, " ").trim());
      if (seen.has(hash)) {
        removed++;
        continue;
      }
      seen.add(hash);
      kept.push(r);
    }
    return { reduced: kept, reducerInfo: { duplicatesRemoved: removed } };
  }

  if (reducer === "merge") {
    const jsonResults: Record<string, unknown>[] = [];
    const nonJson: DispatchResult[] = [];
    for (const r of results) {
      if (!r.result) { nonJson.push(r); continue; }
      try {
        const parsed = JSON.parse(r.result);
        if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) {
          jsonResults.push(parsed);
        } else {
          nonJson.push(r);
        }
      } catch {
        nonJson.push(r);
      }
    }

    if (jsonResults.length < 2) return { reduced: results };

    const merged: Record<string, unknown> = {};
    for (const obj of jsonResults) {
      for (const [k, v] of Object.entries(obj)) {
        if (!(k in merged)) {
          merged[k] = v;
        } else if (Array.isArray(merged[k]) && Array.isArray(v)) {
          merged[k] = [...(merged[k] as unknown[]), ...v];
        }
        // First-wins for non-array conflicts
      }
    }

    const mergedResult: DispatchResult = {
      index: 0,
      task: "merged",
      status: "success",
      result: JSON.stringify(merged),
      durationMs: Math.max(...results.map(r => r.durationMs)),
    };

    return {
      reduced: [mergedResult, ...nonJson],
      reducerInfo: { mergedCount: jsonResults.length, nonJsonCount: nonJson.length },
    };
  }

  return { reduced: results };
}

export const sessionsDispatchTool = createSessionsDispatchTool();
