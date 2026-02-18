// Execute stage — LLM streaming + tool loop
import { createLogger } from "../../../koina/logger.js";
import { estimateTokens } from "../../../hermeneus/token-counter.js";
import { getReversibility, requiresSimulation } from "../../../organon/reversibility.js";
import { executeWithTimeout, resolveTimeout, ToolTimeoutError } from "../../../organon/timeout.js";
import { requiresApproval as checkApproval } from "../../../organon/approval.js";
import { checkResponseQuality } from "../../circuit-breaker.js";
import { eventBus } from "../../../koina/event-bus.js";
import type {
  ContentBlock,
  ToolUseBlock,
  UserContentBlock,
} from "../../../hermeneus/anthropic.js";
import type {
  TurnState,
  TurnStreamEvent,
  TurnOutcome,
  RuntimeServices,
} from "../types.js";

const log = createLogger("pipeline:execute");

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

  for (let loop = 0; ; loop++) {
    let accumulatedText = "";
    let streamResult: import("../../../hermeneus/anthropic.js").TurnResult | null = null;

    for await (const streamEvent of services.router.completeStreaming({
      model,
      system: systemPrompt,
      messages: currentMessages,
      ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
      maxTokens: services.config.agents.defaults.maxOutputTokens,
      ...(state.temperature !== undefined ? { temperature: state.temperature } : {}),
    })) {
      switch (streamEvent.type) {
        case "text_delta":
          accumulatedText += streamEvent.text;
          yield { type: "text_delta", text: streamEvent.text };
          break;
        case "tool_use_start":
          yield { type: "tool_start", toolName: streamEvent.name, toolId: streamEvent.id };
          break;
        case "message_complete":
          streamResult = streamEvent.result;
          break;
      }
    }

    if (!streamResult) throw new Error("Stream ended without message_complete");

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

      services.store.appendMessage(sessionId, "assistant", text, { tokenEstimate: estimateTokens(text) });

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

    // Execute tools
    const toolResults: UserContentBlock[] = [];
    for (let toolIdx = 0; toolIdx < toolUses.length; toolIdx++) {
      const toolUse = toolUses[toolIdx]!;
      totalToolCalls++;

      // Abort check
      if (abortSignal?.aborted) {
        for (const remaining of toolUses.slice(toolIdx)) {
          toolResults.push({
            type: "tool_result",
            tool_use_id: remaining.id,
            content: "[CANCELLED] Turn aborted by user.",
            is_error: true,
          });
        }
        currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
        yield { type: "turn_abort", reason: "user" };
        state.totalToolCalls = totalToolCalls;
        state.currentMessages = currentMessages;
        return state;
      }

      const reversibility = getReversibility(toolUse.name);
      const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);

      // Tool approval gate — check if this tool needs user confirmation
      if (services.approvalGate && services.approvalMode && services.approvalMode !== "autonomous") {
        const approvalCheck = checkApproval(
          toolUse.name,
          toolUse.input as Record<string, unknown>,
          services.approvalMode,
          services.approvalGate.getSessionAllowList(sessionId),
        );

        if (approvalCheck.required) {
          yield {
            type: "tool_approval_required",
            turnId: state.turnId ?? `${nousId}:${sessionId}`,
            toolName: toolUse.name,
            toolId: toolUse.id,
            input: toolUse.input,
            risk: approvalCheck.risk,
            reason: approvalCheck.reason ?? "Approval required",
          };

          try {
            const decision = await services.approvalGate.waitForApproval(
              state.turnId ?? `${nousId}:${sessionId}`,
              toolUse.id,
              toolUse.name,
              toolUse.input,
              approvalCheck.risk,
              abortSignal,
            );

            yield { type: "tool_approval_resolved", toolId: toolUse.id, decision: decision.decision };

            if (decision.alwaysAllow) {
              services.approvalGate.addToSessionAllowList(sessionId, toolUse.name);
            }

            if (decision.decision === "deny") {
              toolResults.push({
                type: "tool_result", tool_use_id: toolUse.id,
                content: `[DENIED] Tool "${toolUse.name}" was denied by the user.`,
                is_error: true,
              });
              continue;
            }
          } catch {
            // Approval cancelled (abort or timeout) — deny the tool
            toolResults.push({
              type: "tool_result", tool_use_id: toolUse.id,
              content: `[DENIED] Tool "${toolUse.name}" approval was cancelled.`,
              is_error: true,
            });
            continue;
          }
        }
      }

      let toolResult: string;
      let isError = false;
      const toolStart = Date.now();
      try {
        const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
        toolResult = await executeWithTimeout(
          () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
          timeoutMs,
          toolUse.name,
        );
      } catch (err) {
        isError = true;
        if (err instanceof ToolTimeoutError) {
          toolResult = `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round(err.timeoutMs / 1000)}s. The operation may still be running in the background.`;
          log.warn(`Tool timeout: ${toolUse.name} after ${err.timeoutMs}ms [${nousId}]`);
        } else {
          toolResult = err instanceof Error ? err.message : String(err);
          if (services.competence) {
            const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
            services.competence.recordCorrection(nousId, domain);
          }
        }
      }
      const toolDuration = Date.now() - toolStart;

      yield {
        type: "tool_result",
        toolName: toolUse.name,
        toolId: toolUse.id,
        result: toolResult.slice(0, 2000),
        isError,
        durationMs: toolDuration,
      };

      if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
      services.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
      eventBus.emit(isError ? "tool:failed" : "tool:called", {
        nousId, sessionId, tool: toolUse.name, durationMs: toolDuration,
        ...(isError ? { error: toolResult.slice(0, 200) } : {}),
      });

      trace.addToolCall({
        name: toolUse.name, input: toolUse.input as Record<string, unknown>,
        output: toolResult.slice(0, 500), durationMs: toolDuration, isError,
        ...(reversibility !== "reversible" ? { reversibility } : {}),
        ...(needsSim ? { simulationRequired: true } : {}),
      });

      toolResults.push({
        type: "tool_result", tool_use_id: toolUse.id, content: toolResult,
        ...(isError ? { is_error: true } : {}),
      });

      services.store.appendMessage(sessionId, "tool_result", toolResult, {
        toolCallId: toolUse.id, toolName: toolUse.name, tokenEstimate: estimateTokens(toolResult),
      });

      // Loop detection
      const loopCheck = loopDetector.record(toolUse.name, toolUse.input, isError);
      if (loopCheck.verdict === "halt") {
        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id,
          content: `[LOOP DETECTED — HALTING] ${loopCheck.reason}`,
          is_error: true,
        });
        currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
        yield { type: "error", message: loopCheck.reason ?? "Tool loop detected" };
        state.totalToolCalls = totalToolCalls;
        state.currentMessages = currentMessages;
        return state;
      }
      if (loopCheck.verdict === "warn") {
        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id,
          content: `[WARNING: Possible loop detected] ${loopCheck.reason}`,
        });
      }
    }

    currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
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

  for (let loop = 0; ; loop++) {
    const result = await services.router.complete({
      model,
      system: systemPrompt,
      messages: currentMessages,
      ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
      maxTokens: services.config.agents.defaults.maxOutputTokens,
      ...(state.temperature !== undefined ? { temperature: state.temperature } : {}),
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
    for (let toolIdx = 0; toolIdx < toolUses.length; toolIdx++) {
      const toolUse = toolUses[toolIdx]!;
      totalToolCalls++;
      const reversibility = getReversibility(toolUse.name);
      const needsSim = requiresSimulation(toolUse.name, toolUse.input as Record<string, unknown>);
      log.debug(`Tool call: ${toolUse.name} (${reversibility}${needsSim ? ", SIMULATED" : ""})`);
      if (needsSim) log.warn(`Simulation required for ${toolUse.name} (${reversibility}) — logging to trace`);

      // Non-streaming approval gate — auto-deny destructive ops (no UI to approve)
      if (services.approvalMode && services.approvalMode !== "autonomous") {
        const nsApprovalCheck = checkApproval(
          toolUse.name,
          toolUse.input as Record<string, unknown>,
          services.approvalMode,
          services.approvalGate?.getSessionAllowList(sessionId),
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

      let toolResult: string;
      let isError = false;
      const toolStart = Date.now();
      try {
        const timeoutMs = resolveTimeout(toolUse.name, services.config.agents.defaults.toolTimeouts);
        toolResult = await executeWithTimeout(
          () => services.tools.execute(toolUse.name, toolUse.input, toolContext),
          timeoutMs,
          toolUse.name,
        );
      } catch (err) {
        isError = true;
        if (err instanceof ToolTimeoutError) {
          toolResult = `[TIMEOUT] Tool "${toolUse.name}" did not respond within ${Math.round(err.timeoutMs / 1000)}s. The operation may still be running in the background.`;
          log.warn(`Tool timeout: ${toolUse.name} after ${err.timeoutMs}ms [${nousId}]`);
        } else {
          toolResult = err instanceof Error ? err.message : String(err);
          log.warn(`Tool ${toolUse.name} failed: ${toolResult}`);
          if (services.competence) {
            const domain = sessionKey === "main" ? "general" : sessionKey.split(":")[0] ?? "general";
            services.competence.recordCorrection(nousId, domain);
          }
        }
      }
      const toolDuration = Date.now() - toolStart;

      if (!isError) turnToolCalls.push({ name: toolUse.name, input: toolUse.input as Record<string, unknown>, output: toolResult.slice(0, 500) });
      services.tools.recordToolUse(toolUse.name, sessionId, seq + loop);
      eventBus.emit(isError ? "tool:failed" : "tool:called", {
        nousId, sessionId, tool: toolUse.name, durationMs: toolDuration,
        ...(isError ? { error: toolResult.slice(0, 200) } : {}),
      });

      trace.addToolCall({
        name: toolUse.name, input: toolUse.input as Record<string, unknown>,
        output: toolResult.slice(0, 500), durationMs: toolDuration, isError,
        ...(reversibility !== "reversible" ? { reversibility } : {}),
        ...(needsSim ? { simulationRequired: true } : {}),
      });

      toolResults.push({
        type: "tool_result", tool_use_id: toolUse.id, content: toolResult,
        ...(isError ? { is_error: true } : {}),
      });

      services.store.appendMessage(sessionId, "tool_result", toolResult, {
        toolCallId: toolUse.id, toolName: toolUse.name, tokenEstimate: estimateTokens(toolResult),
      });

      const loopCheck = loopDetector.record(toolUse.name, toolUse.input, isError);
      if (loopCheck.verdict === "halt") {
        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id,
          content: `[LOOP DETECTED — HALTING] ${loopCheck.reason}`,
          is_error: true,
        });
        throw new Error(loopCheck.reason ?? "Tool loop detected");
      }
      if (loopCheck.verdict === "warn") {
        toolResults.push({
          type: "tool_result", tool_use_id: toolUse.id,
          content: `[WARNING: Possible loop detected] ${loopCheck.reason}`,
        });
      }
    }

    currentMessages = [...currentMessages, { role: "user" as const, content: toolResults }];
  }
}
