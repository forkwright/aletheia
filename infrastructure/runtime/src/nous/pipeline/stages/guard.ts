// Guard stage — circuit breakers, depth limits, drain check
import { createLogger } from "../../../koina/logger.js";
import { estimateTokens } from "../../../hermeneus/token-counter.js";
import { checkInputCircuitBreakers } from "../../circuit-breaker.js";
import type { RuntimeServices, TurnOutcome, TurnState } from "../types.js";

const log = createLogger("pipeline:guard");

/** Short-circuit result when guard rejects. Caller handles yielding/returning this. */
export interface GuardRefusal {
  refusal: true;
  outcome: TurnOutcome;
  text: string;
}

export function checkGuards(
  state: TurnState,
  services: RuntimeServices,
): GuardRefusal | null {
  const inputCheck = checkInputCircuitBreakers(state.msg.text);
  if (!inputCheck.triggered) return null;

  const { nousId, sessionId } = state;
  log.warn(`Circuit breaker (${inputCheck.severity}): ${inputCheck.reason} [${nousId}]`);

  services.store.appendMessage(sessionId, "user", state.msg.text, {
    tokenEstimate: estimateTokens(state.msg.text),
  });
  const text = `I can't process that request. ${inputCheck.reason}`;
  services.store.appendMessage(sessionId, "assistant", text, {
    tokenEstimate: estimateTokens(text),
  });

  return {
    refusal: true,
    text,
    outcome: {
      text,
      nousId,
      sessionId,
      toolCalls: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    },
  };
}
