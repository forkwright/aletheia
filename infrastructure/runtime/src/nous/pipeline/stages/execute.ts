// Execute stage — LLM streaming + tool loop
import { createLogger } from "../../../koina/logger.js";
import { PipelineError } from "../../../koina/errors.js";
import { estimateTokens } from "../../../hermeneus/token-counter.js";
import { getReversibility, requiresSimulation } from "../../../organon/reversibility.js";
import { executeWithTimeout, resolveTimeout, ToolTimeoutError } from "../../../organon/timeout.js";
import { requiresApproval as checkApproval } from "../../../organon/approval.js";
import { checkResponseQuality } from "../../circuit-breaker.js";
import { NarrationFilter } from "../../narration-filter.js";
import { groupForParallelExecution } from "../../../organon/parallel.js";
import { eventBus } from "../../../koina/event-bus.js";
import type {
  ContentBlock,
  ContextManagement,
  ToolUseBlock,
  UserContentBlock,
} from "../../../hermeneus/anthropic.js";
import type {
  RuntimeServices,
  TurnOutcome,
  TurnState,
  TurnStreamEvent,
} from "../types.js";
import { truncateToolResult } from "./truncate.js";

/** Dynamic thinking budget based on message complexity. */
function computeThinkingBudget(messages: readonly { role: string; content: unknown }[], toolCount: number, baseBudget: number): number {
  const lastUser = [...messages].reverse().find(m => m.role === "user");
  const userLen = lastUser ? (typeof lastUser.content === "string" ? lastUser.content.length : JSON.stringify(lastUser.content).length) : 0;

  if (userLen < 100 && toolCount === 0) return Math.min(baseBudget, 2000);
  if (userLen < 500 && toolCount <= 2) return Math.min(baseBudget, 6000);
  if (userLen > 1000 || toolCount > 5) return Math.max(baseBudget, 16000);
  return baseBudget;
}

const log = createLogger("pipeline:execute");

/**
 * Build context management config for Anthropic's server-side context editing.
 * Clears old tool results at 60% of context window and thinking blocks (keeping last 2 turns).
 * This delays distillation by freeing context space automatically.
 */
function buildContextManagement(contextTokens: number, thinkingEnabled: boolean): ContextManagement | undefined {
  const edits: ContextManagement["edits"] = [];

  // Thinking clearing must come first in the edits array (API requirement)
  if (thinkingEnabled) {
    edits.push({
      type: "clear_thinking_20251015",
      keep: { type: "thinking_turns", value: 2 },
    });
  }

  // Clear old tool results at 60% context, keep last 8, clear at least 20K tokens
  edits.push({
    type: "clear_tool_uses_20250919",
    trigger: { type: "input_tokens", value: Math.floor(contextTokens * 0.6) },
    keep: { type: "tool_uses", value: 8 },
    clear_at_least: { type: "input_tokens", value: 20000 },
  });

  return edits.length > 0 ? { edits } : undefined;
}

export async function* executeStreaming(
  state: TurnState,
  services: RuntimeServices,
): AsyncGenerator<TurnStreamEvent, TurnState> {
  const {
    nousId, sessionId, sessionKey, model, toolDefs, toolContext,
    systemPrompt, trace, abortSignal,
  } = state;

  let { currentMessages } = state;
  let totalToolCalls = state.totalToolCalls;
  let totalInputTokens = state.totalInputTokens;
  let totalOutputTokens = state.totalOutputTokens;
  let totalCacheReadTokens = state.totalCacheReadTokens;
  let totalCacheWriteTokens = state.totalCacheWriteTokens;
  const { turnToolCalls, loopDetector } = state;
  const seq = state.seq;

  // Narration filter — suppresses internal monologue at start of response
  const narrationEnabled = services.config.agents.defaults.narrationFilter !== false;
  const narrationFilter = narrationEnabled ? new NarrationFilter() : null;

  for (let loop = 0; ; loop++) {
    let accumulatedText = "";
    let streamResult: import("../../../hermeneus/anthropic.js").TurnResult | null = null;

    // Build thinking config — only for models that support extended thinking (opus, sonnet-4)
    const thinkingConfig = state.sessionId
      ? services.store.getThinkingConfig(state.sessionId)
      : undefined;
    const supportsThinking = /opus|sonnet-4/i.test(model);
    const useThinking = !!(thinkingConfig?.enabled && supportsThinking);

    // Build context management — clears old tool results and thinking blocks server-side
    const contextTokens = services.config.agents.defaults.contextTokens ?? 200000;
    const contextManagement = buildContextManagement(contextTokens, useThinking);

    for await (const streamEvent of services.router.completeStreaming({
      model,
      system: systemPrompt,
      messages: currentMessages,
      ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
      maxTokens: services.config.agents.defaults.maxOutputTokens,
      ...(state.temperature !== undefined ? { temperature: state.temperature } : {}),
      ...(abortSignal ? { signal: abortSignal } : {}),
      ...(useThinking ? { thinking: { type: "enabled" as const, budget_tokens: computeThinkingBudget(currentMessages, totalToolCalls, thinkingConfig.budget) } } : {}),
      ...(contextManagement ? { contextManagement } : {}),
    })) {
      switch (streamEvent.type) {
        case "text_delta":
          accumulatedText += streamEvent.text;
          if (narrationFilter) {
            for (const evt of narrationFilter.feed(streamEvent.text)) {
              yield evt;
            }
          } else {
            yield { type: "text_delta", text: streamEvent.text };
          }
          break;
        case "thinking_delta":
          yield { type: "thinking_delta", text: streamEvent.text };
          break;
        case "tool_use_start":
          // Input not available during streaming — tool_start emitted at execution time with full input
          break;
        case "message_complete":
          streamResult = streamEvent.result;
          break;
      }
    }

    // Flush narration filter at end of LLM response
    if (narrationFilter) {
      for (const evt of narrationFilter.flush()) {
        yield evt;
      }
    }

    if (!streamResult) throw new PipelineError("Stream ended without message_complete", { code: "PIPELINE_STREAM_INCOMPLETE" });

    totalInputTokens += streamResult.usage.inputTokens;
    totalOutputTokens += streamResult.usage.outputTokens;
    totalCacheReadTokens += streamResult.usage.cacheReadTokens;
    totalCacheWriteTokens += streamResult.usage.cacheWriteTokens;

    services.store.recordUsage({
      sessionId,
      turnSeq: seq + loop,
      inputTokens: streamResult.usage.inputTokens,
      outputTokens: streamResult.usage.outputTokens,
      cacheReadTokens: streamResult.usage.cacheReadTokens,
      cacheWriteTokens: streamResult.usage.cacheWriteTokens,
      model: streamResult.model,
    });

    const toolUses = streamResult.content.filter(
      (b): b is ToolUseBlock => b.type === "tool_use",
    );

    // Text-only response — turn is complete
    if (toolUses.length === 0) {
      const text = accumulatedText || streamResult.content
        .filter((b): b is { type: "text"; text: string } => b.type === "text")
        .map((b) => b.text)
        .join("\n");

      const qualityCheck = checkResponseQuality(text);
      if (qualityCheck.triggered) {
        log.warn(`Response quality issue (${qualityCheck.severity}): ${qualityCheck.reason} [${nousId}]`);
        trace.addToolCall({ name: "_circuit_breaker", input: { check: "response_quality" }, output: qualityCheck.reason ?? "quality check triggered", durationMs: 0, isError: true });
      }

      // Store full ContentBlock[] when thinking blocks present, so history preserves reasoning
      const hasThinking = streamResult.content.some((b) => b.type === "thinking");
      const storeContent = hasThinking ? JSON.stringify(streamResult.content) : text;
      services.store.appendMessage(sessionId, "assistant", storeContent, { tokenEstimate: estimateTokens(storeContent) });

      const outcome: TurnOutcome = {
        text, nousId, sessionId, toolCalls: totalToolCalls,
        inputTokens: totalInputTokens, outputTokens: totalOutputTokens,
        cacheReadTokens: totalCacheReadTokens, cacheWriteTokens: totalCacheWriteTokens,
      };

      trace.setUsage(totalInputTokens, totalOutputTokens, totalCacheReadTokens, totalCacheWriteTokens);
      trace.setResponseLength(text.length);
      trace.setToolLoops(loop + 1);

      services.tools.expireUnusedTools(sessionId, seq + loop);

      // Update accumulators on state for finalize
      state.totalToolCalls = totalToolCalls;
      state.totalInputTokens = totalInputTokens;
      state.totalOutputTokens = totalOutputTokens;
      state.totalCacheReadTokens = totalCacheReadTokens;
      state.totalCacheWriteTokens = totalCacheWriteTokens;
      state.currentMessages = currentMessages;
      state.outcome = outcome;

      return state;
    }

    // Store assistant tool_use response
    services.store.appendMessage(sessionId, "assistant", JSON.stringify(streamResult.content), {
      tokenEstimate: estimateTokens(JSON.stringify(streamResult.content)),
    });

    currentMessages = [
      ...currentMessages,
      { role: "assistant" as const, content: streamResult.content as ContentBlock[] },
    ];

    // Execute tools in parallel batches
    const toolResults: UserContentBlock[] = [];
    const batches = groupForParallelExecution(toolUses);

    for (let batchIdx = 0; batchIdx < batches.length; batchIdx++) {
      const batch = batches[batchIdx]!;

      // Abort check before each batch
      if (abortSignal?.aborted) {
        for (let ri = batchIdx; ri < batches.length; ri++) {
          for (const rem of batches[ri]!) {
            toolResults.push({ type: "tool_result", tool_use_id: rem.id, content: "[CANCELLED] Turn aborted by user.", is_error: true });
          }
        }
        currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
        yield { type: "turn_abort", reason: "user" };
        state.totalToolCalls = totalToolCalls;
        state.currentMessages = currentMessages;
        return state;
      }

      // Execute batch — collect results, then process uniformly
      const execResults: Array<{ toolUse: ToolUseBlock; result: string; isError: boolean; durationMs: number }> = [];

      if (batch.length === 1) {
        // Single tool — sequential with approval gate
        const toolUse = batch[0]!;
        totalToolCalls++;
        yield { type: "tool_start", toolName: toolUse.name, toolId: toolUse.id, input: toolUse.input as Record<string, unknown> };

        // Approval gate
        if (services.approvalGate && services.approvalMode && services.approvalMode !== "autonomous") {
          const approvalCheck = checkApproval(
            toolUse.name, toolUse.input as Record<string, unknown>,
            services.approvalMode, services.approvalGate.getSessionAllowList(sessionId),
          );
          if (approvalCheck.required) {
            yield {
              type: "tool_approval_required",
              turnId: state.turnId ?? `${nousId}:${sessionId}`,
              toolName: toolUse.name, toolId: toolUse.id, input: toolUse.input,
              risk: approvalCheck.risk, reason: approvalCheck.reason ?? "Approval required",
            };
            try {
              const decision = await services.approvalGate.waitForApproval(
                state.turnId ?? `${nousId}:${sessionId}`,
                toolUse.id, toolUse.name, toolUse.input, approvalCheck.risk, abortSignal,
              );
              yield { type: "tool_approval_resolved", toolId: toolUse.id, decision: decision.decision };
              if (decision.alwaysAllow) services.approvalGate.addToSessionAllowList(sessionId, toolUse.name);
              if (decision.decision === "deny") {
                toolResults.push({
                  type: "tool_result", tool_use_id: toolUse.id,
                  content: `[DENIED] Tool "${toolUse.name}" was denied by the user.`, is_error: true,
                });
                continue;
              }
            } catch {
              toolResults.push({
                type: "tool_result", tool_use_id: toolUse.id,
                content: `[DENIED] Tool "${toolUse.name}" approval was cancelled.`, is_error: true,
              });
              continue;
            }
          }
        }

        let result: string;
        let isError = false;
        const start = Date.now();
        try {
          const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
          result = await executeWithTimeout(
            () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
            timeoutMs, toolUse.name,
          );
        } catch (err) {
          isError = true;
          if (err instanceof ToolTimeoutError) {
            result = `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round(err.timeoutMs / 1000)}s. The operation may still be running in the background.`;
            log.warn(`Tool timeout: ${toolUse.name} after ${err.timeoutMs}ms [${nousId}]`);
          } else {
            result = err instanceof Error ? err.message : String(err);
          }
        }
        execResults.push({ toolUse, result, isError, durationMs: Date.now() - start });
      } else {
        // Parallel batch — emit all tool_start events, then execute concurrently
        for (const toolUse of batch) {
          totalToolCalls++;
          yield { type: "tool_start", toolName: toolUse.name, toolId: toolUse.id, input: toolUse.input as Record<string, unknown> };
        }

        const batchStart = Date.now();
        const settled = await Promise.allSettled(
          batch.map(async (toolUse) => {
            const start = Date.now();
            try {
              const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
              const result = await executeWithTimeout(
                () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
                timeoutMs, toolUse.name,
              );
              return { result, isError: false, durationMs: Date.now() - start };
            } catch (err) {
              const isTimeout = err instanceof ToolTimeoutError;
              const result = isTimeout
                ? `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round((err as ToolTimeoutError).timeoutMs / 1000)}s. The operation may still be running in the background.`
                : (err instanceof Error ? err.message : String(err));
              if (isTimeout) log.warn(`Tool timeout: ${toolUse.name} after ${(err as ToolTimeoutError).timeoutMs}ms [${nousId}]`);
              return { result, isError: true, durationMs: Date.now() - start };
            }
          }),
        );

        let sequentialMs = 0;
        for (let i = 0; i < batch.length; i++) {
          const toolUse = batch[i]!;
          const s = settled[i]!;
          const { result, isError, durationMs } = s.status === "fulfilled"
            ? s.value
            : { result: String(s.reason), isError: true, durationMs: 0 };
          sequentialMs += durationMs;
          execResults.push({ toolUse, result, isError, durationMs });
        }

        const batchMs = Date.now() - batchStart;
        const savedMs = Math.max(0, sequentialMs - batchMs);
        if (savedMs > 50) log.info(`Parallel batch: ${batch.length} tools in ${batchMs}ms (saved ~${savedMs}ms vs sequential)`);
      }

      // Process execution results — shared between sequential and parallel paths
      for (const { toolUse, result: toolResult, isError, durationMs } of execResults) {
        const reversibility = getReversibility(toolUse.name);
        const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);

        const tokenEstimate = estimateTokens(toolResult);
        yield {
          type: "tool_result", toolName: toolUse.name, toolId: toolUse.id,
          result: toolResult.slice(0, 2000), isError, durationMs, tokenEstimate,
        };

        if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
        services.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
        eventBus.emit(isError ? "tool:failed" : "tool:called", {
          nousId, sessionId, tool: toolUse.name, durationMs,
          ...(isError ? { error: toolResult.slice(0, 200) } : {}),
        });

        trace.addToolCall({
          name: toolUse.name, input: toolUse.input as Record<string, unknown>,
          output: toolResult.slice(0, 500), durationMs, isError,
          ...(reversibility !== "reversible" ? { reversibility } : {}),
          ...(needsSim ? { simulationRequired: true } : {}),
        });

        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id, content: toolResult,
          ...(isError ? { is_error: true } : {}),
        });

        const storedResult = truncateToolResult(toolUse.name, toolResult);
        services.store.appendMessage(sessionId, "tool_result", storedResult, {
          toolCallId: toolUse.id, toolName: toolUse.name, tokenEstimate: estimateTokens(storedResult),
        });

        if (isError && !toolResult.startsWith("[TIMEOUT]") && services.competence) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          services.competence.recordCorrection(nousId, domain);
        }

        // Loop detection
        const loopCheck = loopDetector.record(toolUse.name, toolUse.input, isError);
        const lastResult = toolResults[toolResults.length - 1] as { type: string; tool_use_id?: string; content?: string; is_error?: boolean } | undefined;
        if (loopCheck.verdict === "halt") {
          if (lastResult?.type === "tool_result" && lastResult.tool_use_id === toolUse.id) {
            lastResult.content = (lastResult.content ?? "") + `\n\n[LOOP DETECTED — HALTING] ${loopCheck.reason}`;
            lastResult.is_error = true;
          }
          currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
          yield { type: "error", message: loopCheck.reason ?? "Tool loop detected" };
          state.totalToolCalls = totalToolCalls;
          state.currentMessages = currentMessages;
          return state;
        }
        if (loopCheck.verdict === "warn" && lastResult?.type === "tool_result" && lastResult.tool_use_id === toolUse.id) {
          lastResult.content = (lastResult.content ?? "") + `\n\n[WARNING: Possible loop detected] ${loopCheck.reason}`;
        }
      }
    }

    // Mid-turn message queue — check for human messages sent during tool execution
    const queued = services.store.drainQueue(sessionId);
    if (queued.length > 0) {
      // Append tool results as one user message, then queued messages as a separate user message
      currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
      const queuedContent = queued.map((q) => ({
        type: "text" as const,
        text: `[Mid-turn message from ${q.sender ?? "user"}]: ${q.content}`,
      }));
      currentMessages = [...currentMessages, {
        role: "user" as const,
        content: queuedContent,
      }];
      // Store the queued messages in history
      for (const q of queued) {
        services.store.appendMessage(sessionId, "user", q.content, {
          tokenEstimate: estimateTokens(q.content),

        });
      }
      yield { type: "queue_drained", count: queued.length };
    } else {
      currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
    }
  }
}

/** Non-streaming execute — buffers all results internally, returns TurnOutcome directly. */
export async function executeBuffered(
  state: TurnState,
  services: RuntimeServices,
): Promise<TurnState> {
  const {
    nousId, sessionId, sessionKey, model, toolDefs, toolContext,
    systemPrompt, trace,
  } = state;

  let { currentMessages } = state;
  let totalToolCalls = state.totalToolCalls;
  let totalInputTokens = state.totalInputTokens;
  let totalOutputTokens = state.totalOutputTokens;
  let totalCacheReadTokens = state.totalCacheReadTokens;
  let totalCacheWriteTokens = state.totalCacheWriteTokens;
  const { turnToolCalls, loopDetector } = state;
  const seq = state.seq;

  // Context management for buffered path
  const contextTokens = services.config.agents.defaults.contextTokens ?? 200000;
  const bufferedContextMgmt = buildContextManagement(contextTokens, false);

  for (let loop = 0; ; loop++) {
    const result = await services.router.complete({
      model,
      system: systemPrompt,
      messages: currentMessages,
      ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
      maxTokens: services.config.agents.defaults.maxOutputTokens,
      ...(state.temperature !== undefined ? { temperature: state.temperature } : {}),
      ...(bufferedContextMgmt ? { contextManagement: bufferedContextMgmt } : {}),
    });

    totalInputTokens += result.usage.inputTokens;
    totalOutputTokens += result.usage.outputTokens;
    totalCacheReadTokens += result.usage.cacheReadTokens;
    totalCacheWriteTokens += result.usage.cacheWriteTokens;

    services.store.recordUsage({
      sessionId,
      turnSeq: seq + loop,
      inputTokens: result.usage.inputTokens,
      outputTokens: result.usage.outputTokens,
      cacheReadTokens: result.usage.cacheReadTokens,
      cacheWriteTokens: result.usage.cacheWriteTokens,
      model: result.model,
    });

    const toolUses = result.content.filter(
      (b): b is ToolUseBlock => b.type === "tool_use",
    );

    if (toolUses.length === 0) {
      const text = result.content
        .filter((b): b is { type: "text"; text: string } => b.type === "text")
        .map((b) => b.text)
        .join("\n");

      const qualityCheck = checkResponseQuality(text);
      if (qualityCheck.triggered) {
        log.warn(`Response quality issue (${qualityCheck.severity}): ${qualityCheck.reason} [${nousId}]`);
        trace.addToolCall({ name: "_circuit_breaker", input: { check: "response_quality" }, output: qualityCheck.reason ?? "quality check triggered", durationMs: 0, isError: true });
      }

      services.store.appendMessage(sessionId, "assistant", text, { tokenEstimate: estimateTokens(text) });

      const cacheHitRate = totalInputTokens > 0
        ? Math.round((totalCacheReadTokens / totalInputTokens) * 100)
        : 0;
      log.info(
        `Turn complete for ${nousId}: ${totalInputTokens}in/${totalOutputTokens}out, ` +
        `cache ${totalCacheReadTokens}r/${totalCacheWriteTokens}w (${cacheHitRate}% hit), ` +
        `${totalToolCalls} tool calls`,
      );

      trace.setUsage(totalInputTokens, totalOutputTokens, totalCacheReadTokens, totalCacheWriteTokens);
      trace.setResponseLength(text.length);
      trace.setToolLoops(loop + 1);

      services.tools.expireUnusedTools(sessionId, seq + loop);

      state.totalToolCalls = totalToolCalls;
      state.totalInputTokens = totalInputTokens;
      state.totalOutputTokens = totalOutputTokens;
      state.totalCacheReadTokens = totalCacheReadTokens;
      state.totalCacheWriteTokens = totalCacheWriteTokens;
      state.currentMessages = currentMessages;
      state.outcome = {
        text, nousId, sessionId, toolCalls: totalToolCalls,
        inputTokens: totalInputTokens, outputTokens: totalOutputTokens,
        cacheReadTokens: totalCacheReadTokens, cacheWriteTokens: totalCacheWriteTokens,
      };

      return state;
    }

    services.store.appendMessage(sessionId, "assistant", JSON.stringify(result.content), {
      tokenEstimate: estimateTokens(JSON.stringify(result.content)),
    });

    currentMessages = [
      ...currentMessages,
      { role: "assistant" as const, content: result.content as ContentBlock[] },
    ];

    const toolResults: UserContentBlock[] = [];
    const batches = groupForParallelExecution(toolUses);

    for (const batch of batches) {
      const execResults: Array<{ toolUse: ToolUseBlock; result: string; isError: boolean; durationMs: number }> = [];

      if (batch.length === 1) {
        const toolUse = batch[0]!;
        totalToolCalls++;
        const reversibility = getReversibility(toolUse.name);
        const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);
        log.debug(`Tool call: ${toolUse.name} (${reversibility}${needsSim ? ", SIMULATED" : ""})`);
        if (needsSim) log.warn(`Simulation required for ${toolUse.name} (${reversibility}) — logging to trace`);

        // Non-streaming approval gate — auto-deny destructive ops (no UI to approve)
        if (services.approvalMode && services.approvalMode !== "autonomous") {
          const nsApprovalCheck = checkApproval(
            toolUse.name, toolUse.input as Record<string, unknown>,
            services.approvalMode, services.approvalGate?.getSessionAllowList(sessionId),
          );
          if (nsApprovalCheck.required) {
            log.warn(`Tool "${toolUse.name}" requires approval but no interactive session — auto-denying`);
            toolResults.push({
              type: "tool_result", tool_use_id: toolUse.id,
              content: `[DENIED] Tool "${toolUse.name}" requires approval but no interactive session is available.`,
              is_error: true,
            });
            continue;
          }
        }

        let result: string;
        let isError = false;
        const start = Date.now();
        try {
          const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
          result = await executeWithTimeout(
            () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
            timeoutMs, toolUse.name,
          );
        } catch (err) {
          isError = true;
          if (err instanceof ToolTimeoutError) {
            result = `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round(err.timeoutMs / 1000)}s. The operation may still be running in the background.`;
            log.warn(`Tool timeout: ${toolUse.name} after ${err.timeoutMs}ms [${nousId}]`);
          } else {
            result = err instanceof Error ? err.message : String(err);
            log.warn(`Tool ${toolUse.name} failed: ${result}`);
          }
        }
        execResults.push({ toolUse, result, isError, durationMs: Date.now() - start });
      } else {
        // Parallel batch
        for (const toolUse of batch) {
          totalToolCalls++;
          log.debug(`Tool call (parallel): ${toolUse.name}`);
        }

        const batchStart = Date.now();
        const settled = await Promise.allSettled(
          batch.map(async (toolUse) => {
            const start = Date.now();
            try {
              const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
              const result = await executeWithTimeout(
                () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
                timeoutMs, toolUse.name,
              );
              return { result, isError: false, durationMs: Date.now() - start };
            } catch (err) {
              const isTimeout = err instanceof ToolTimeoutError;
              const result = isTimeout
                ? `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round((err as ToolTimeoutError).timeoutMs / 1000)}s. The operation may still be running in the background.`
                : (err instanceof Error ? err.message : String(err));
              if (isTimeout) log.warn(`Tool timeout: ${toolUse.name} after ${(err as ToolTimeoutError).timeoutMs}ms [${nousId}]`);
              else log.warn(`Tool ${toolUse.name} failed: ${result}`);
              return { result, isError: true, durationMs: Date.now() - start };
            }
          }),
        );

        let sequentialMs = 0;
        for (let i = 0; i < batch.length; i++) {
          const toolUse = batch[i]!;
          const s = settled[i]!;
          const { result, isError, durationMs } = s.status === "fulfilled"
            ? s.value
            : { result: String(s.reason), isError: true, durationMs: 0 };
          sequentialMs += durationMs;
          execResults.push({ toolUse, result, isError, durationMs });
        }

        const batchMs = Date.now() - batchStart;
        const savedMs = Math.max(0, sequentialMs - batchMs);
        if (savedMs > 50) log.info(`Parallel batch: ${batch.length} tools in ${batchMs}ms (saved ~${savedMs}ms vs sequential)`);
      }

      // Process execution results — shared between sequential and parallel paths
      for (const { toolUse, result: toolResult, isError, durationMs } of execResults) {
        const reversibility = getReversibility(toolUse.name);
        const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);

        if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
        services.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
        eventBus.emit(isError ? "tool:failed" : "tool:called", {
          nousId, sessionId, tool: toolUse.name, durationMs,
          ...(isError ? { error: toolResult.slice(0, 200) } : {}),
        });

        trace.addToolCall({
          name: toolUse.name, input: toolUse.input as Record<string, unknown>,
          output: toolResult.slice(0, 500), durationMs, isError,
          ...(reversibility !== "reversible" ? { reversibility } : {}),
          ...(needsSim ? { simulationRequired: true } : {}),
        });

        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id, content: toolResult,
          ...(isError ? { is_error: true } : {}),
        });

        const storedResult = truncateToolResult(toolUse.name, toolResult);
        services.store.appendMessage(sessionId, "tool_result", storedResult, {
          toolCallId: toolUse.id, toolName: toolUse.name, tokenEstimate: estimateTokens(storedResult),
        });

        if (isError && !toolResult.startsWith("[TIMEOUT]") && services.competence) {
          const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
          services.competence.recordCorrection(nousId, domain);
        }

        // Loop detection
        const loopCheck = loopDetector.record(toolUse.name, toolUse.input, isError);
        const lastResult = toolResults[toolResults.length - 1] as { type: string; tool_use_id?: string; content?: string; is_error?: boolean } | undefined;
        if (loopCheck.verdict === "halt") {
          if (lastResult?.type === "tool_result" && lastResult.tool_use_id === toolUse.id) {
            lastResult.content = (lastResult.content ?? "") + `\n\n[LOOP DETECTED — HALTING] ${loopCheck.reason}`;
            lastResult.is_error = true;
          }
          throw new PipelineError(loopCheck.reason ?? "Tool loop detected", { code: "PIPELINE_TOOL_LOOP" });
        }
        if (loopCheck.verdict === "warn" && lastResult?.type === "tool_result" && lastResult.tool_use_id === toolUse.id) {
          lastResult.content = (lastResult.content ?? "") + `\n\n[WARNING: Possible loop detected] ${loopCheck.reason}`;
        }
      }
    }

    // Mid-turn message queue — check for human messages sent during tool execution
    const queued = services.store.drainQueue(sessionId);
    if (queued.length > 0) {
      currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
      const queuedContent = queued.map((q) => ({
        type: "text" as const,
        text: `[Mid-turn message from ${q.sender ?? "user"}]: ${q.content}`,
      }));
      currentMessages = [...currentMessages, {
        role: "user" as const,
        content: queuedContent,
      }];
      for (const q of queued) {
        services.store.appendMessage(sessionId, "user", q.content, {
          tokenEstimate: estimateTokens(q.content),

        });
      }
    } else {
      currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
    }
  }
}
