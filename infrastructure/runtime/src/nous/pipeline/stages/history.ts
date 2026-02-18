// History stage — budget calculation, history retrieval, message building
import { createLogger } from "../../../koina/logger.js";
import { estimateTokens } from "../../../hermeneus/token-counter.js";
import { buildMessages } from "../utils/build-messages.js";
import type { TurnState, RuntimeServices } from "../types.js";

const log = createLogger("pipeline:history");

export async function prepareHistory(
  state: TurnState,
  services: RuntimeServices,
): Promise<TurnState> {
  const { nousId, sessionId, msg, nous } = state;
  const historyBudget = (state as TurnState & { _historyBudget?: number })._historyBudget ?? 0;

  const history = services.store.getHistoryWithBudget(sessionId, historyBudget);

  // Surface unsurfaced cross-agent messages
  let crossAgentNotice: string | null = null;
  const unsurfaced = services.store.getUnsurfacedMessages(nousId);
  if (unsurfaced.length > 0) {
    const lines = unsurfaced.map((m) => {
      const from = m.sourceNousId ?? "unknown";
      const summary = m.response ? `\n  Response: ${m.response.slice(0, 500)}` : "";
      return `[From ${from}, ${m.kind}] ${m.content}${summary}`;
    });
    crossAgentNotice =
      `While you were in another conversation, you received cross-agent messages:\n\n` +
      lines.join("\n\n") +
      `\n\nThe user may not be aware of these. Mention them if relevant.`;

    services.store.appendMessage(sessionId, "user", crossAgentNotice, {
      tokenEstimate: estimateTokens(crossAgentNotice),
    });
    services.store.markMessagesSurfaced(
      unsurfaced.map((m) => m.id),
      sessionId,
    );
    log.info(`Surfaced ${unsurfaced.length} cross-agent messages into session ${sessionId}`);
  }

  const seq = services.store.appendMessage(sessionId, "user", msg.text, {
    tokenEstimate: estimateTokens(msg.text),
  });

  const currentText = crossAgentNotice
    ? crossAgentNotice + "\n\n" + msg.text
    : msg.text;

  const tz = nous["userTimezone"] as string | undefined;
  const messages = buildMessages(history, currentText, msg.media, tz);

  state.seq = seq;
  state.messages = messages;
  state.currentMessages = messages;

  // Dispatch plugin beforeTurn
  if (services.plugins) {
    await services.plugins.dispatchBeforeTurn({
      nousId,
      sessionId,
      messageText: msg.text,
      ...(msg.media ? { media: msg.media } : {}),
    });
  }

  return state;
}
