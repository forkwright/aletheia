// Pipeline runner — composes stages for streaming and non-streaming turn execution
import { createLogger } from "../../koina/logger.js";
import { eventBus } from "../../koina/event-bus.js";
import { resolveStage } from "./stages/resolve.js";
import { checkGuards } from "./stages/guard.js";
import { buildContext } from "./stages/context.js";
import { prepareHistory } from "./stages/history.js";
import { executeStreaming, executeBuffered } from "./stages/execute.js";
import { finalize } from "./stages/finalize.js";
import type {
  InboundMessage,
  TurnOutcome,
  TurnState,
  TurnStreamEvent,
  RuntimeServices,
} from "./types.js";

const log = createLogger("pipeline:runner");

export interface StreamingPipelineOpts {
  abortSignal?: AbortSignal;
  turnId?: string;
}

function identifyFailedStage(state: Partial<TurnState>): string {
  if (!state.systemPrompt) return "context";
  if (!state.messages) return "history";
  if (!state.outcome) return "execute";
  return "finalize";
}

export async function* runStreamingPipeline(
  msg: InboundMessage,
  services: RuntimeServices,
  opts?: StreamingPipelineOpts,
): AsyncGenerator<TurnStreamEvent, TurnOutcome | undefined> {
  // Stage 1: Resolve
  const state = resolveStage(msg, services, opts?.abortSignal);
  if (!state) {
    yield { type: "error", message: `Unknown nous: ${msg.nousId ?? "default"}` };
    return undefined;
  }

  const turnId = opts?.turnId ?? `${state.nousId}:${state.sessionId}:${Date.now()}`;
  yield { type: "turn_start", sessionId: state.sessionId, nousId: state.nousId, turnId };

  // Stage 2: Guard
  const refusal = checkGuards(state, services);
  if (refusal) {
    yield { type: "text_delta", text: refusal.text };
    yield { type: "turn_complete", outcome: refusal.outcome };
    return refusal.outcome;
  }

  try {
    // Stage 3: Context
    await buildContext(state, services);

    // Stage 4: History
    await prepareHistory(state, services);

    // Stage 5: Execute (streaming)
    const finalState = yield* executeStreaming(state, services);

    // Stage 6: Finalize
    if (finalState.outcome) {
      await finalize(finalState, services);
      yield { type: "turn_complete", outcome: finalState.outcome };
    }

    return finalState.outcome;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    const stage = identifyFailedStage(state);
    log.error(`Pipeline failed at ${stage}: ${message}`, { nousId: state.nousId, sessionId: state.sessionId, stage });
    if (err instanceof Error && err.stack) log.error(err.stack);
    eventBus.emit("pipeline:error", { nousId: state.nousId, sessionId: state.sessionId, stage, error: message });

    yield { type: "error", message: `Turn failed at ${stage}: ${message}` };
    return undefined;
  }
}

export async function runBufferedPipeline(
  msg: InboundMessage,
  services: RuntimeServices,
): Promise<TurnOutcome> {
  // Stage 1: Resolve
  const state = resolveStage(msg, services);
  if (!state) {
    throw new Error(`Unknown nous: ${msg.nousId ?? "default"}`);
  }

  // Stage 2: Guard
  const refusal = checkGuards(state, services);
  if (refusal) {
    return refusal.outcome;
  }

  try {
    // Stage 3: Context
    await buildContext(state, services);

    // Stage 4: History
    await prepareHistory(state, services);

    // Stage 5: Execute (buffered)
    const finalState = await executeBuffered(state, services);

    // Stage 6: Finalize
    if (finalState.outcome) {
      await finalize(finalState, services);
    }

    if (!finalState.outcome) {
      throw new Error("Turn produced no outcome");
    }

    return finalState.outcome;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    const stage = identifyFailedStage(state);
    log.error(`Pipeline failed at ${stage}: ${message}`, { nousId: state.nousId, sessionId: state.sessionId, stage });
    if (err instanceof Error && err.stack) log.error(err.stack);
    eventBus.emit("pipeline:error", { nousId: state.nousId, sessionId: state.sessionId, stage, error: message });

    return {
      text: "",
      nousId: state.nousId,
      sessionId: state.sessionId,
      toolCalls: state.totalToolCalls,
      inputTokens: state.totalInputTokens,
      outputTokens: state.totalOutputTokens,
      cacheReadTokens: state.totalCacheReadTokens ?? 0,
      cacheWriteTokens: state.totalCacheWriteTokens ?? 0,
      error: message,
    };
  }
}
