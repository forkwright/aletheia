// Distillation pipeline — multi-pass context compression
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { SessionStore } from "../mneme/store.js";
import { extractFromMessages } from "./extract.js";
import { summarizeMessages } from "./summarize.js";
import { flushToMemory, type MemoryFlushTarget } from "./hooks.js";
import type { PluginRegistry } from "../prostheke/registry.js";

const log = createLogger("distillation");

export interface DistillationOpts {
  triggerThreshold: number;
  minMessages: number;
  extractionModel: string;
  summaryModel: string;
  memoryTarget?: MemoryFlushTarget;
  plugins?: PluginRegistry;
}

export interface DistillationResult {
  sessionId: string;
  nousId: string;
  messagesBefore: number;
  messagesAfter: number;
  tokensBefore: number;
  tokensAfter: number;
  factsExtracted: number;
  summary: string;
}

export async function shouldDistill(
  store: SessionStore,
  sessionId: string,
  opts: { threshold: number; minMessages: number },
): Promise<boolean> {
  const session = store.findSessionById(sessionId);
  if (!session) return false;

  if (session.messageCount < opts.minMessages) return false;

  return session.tokenCountEstimate >= opts.threshold;
}

export async function distillSession(
  store: SessionStore,
  router: ProviderRouter,
  sessionId: string,
  nousId: string,
  opts: DistillationOpts,
): Promise<DistillationResult> {
  log.info(`Starting distillation for session ${sessionId}`);

  const allMessages = store.getHistory(sessionId, {});
  const undistilled = allMessages.filter((m) => !m.isDistilled);

  if (undistilled.length < opts.minMessages) {
    throw new Error(
      `Not enough messages to distill: ${undistilled.length} < ${opts.minMessages}`,
    );
  }

  const tokensBefore = undistilled.reduce(
    (sum, m) => sum + (m.tokenEstimate ?? 0),
    0,
  );

  if (opts.plugins) {
    await opts.plugins.dispatchBeforeDistill({
      nousId,
      sessionId,
      messageCount: undistilled.length,
      tokenCount: tokensBefore,
    });
  }

  const simpleMessages = undistilled
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => ({ role: m.role, content: m.content }));

  log.info(`Extraction pass: ${simpleMessages.length} messages`);
  const extraction = await extractFromMessages(
    router,
    simpleMessages,
    opts.extractionModel,
  );

  log.info(
    `Extracted: ${extraction.facts.length} facts, ${extraction.decisions.length} decisions, ${extraction.openItems.length} open items`,
  );

  if (opts.memoryTarget) {
    await flushToMemory(opts.memoryTarget, nousId, extraction);
  }

  log.info("Summary pass");
  const summary = await summarizeMessages(
    router,
    simpleMessages,
    extraction,
    opts.summaryModel,
  );

  const summaryTokens = estimateTokens(summary);

  store.appendMessage(sessionId, "assistant", summary, {
    tokenEstimate: summaryTokens,
    isDistilled: true,
  });

  store.markMessagesDistilled(sessionId, undistilled.map((m) => m.seq));

  store.recordDistillation({
    sessionId,
    messagesBefore: undistilled.length,
    messagesAfter: 1,
    tokensBefore,
    tokensAfter: summaryTokens,
    factsExtracted: extraction.facts.length + extraction.decisions.length,
    model: opts.extractionModel,
  });

  const result: DistillationResult = {
    sessionId,
    nousId,
    messagesBefore: undistilled.length,
    messagesAfter: 1,
    tokensBefore,
    tokensAfter: summaryTokens,
    factsExtracted: extraction.facts.length + extraction.decisions.length,
    summary,
  };

  if (opts.plugins) {
    await opts.plugins.dispatchAfterDistill({
      nousId,
      sessionId,
      factsExtracted: result.factsExtracted,
      tokensBefore: result.tokensBefore,
      tokensAfter: result.tokensAfter,
    });
  }

  log.info(
    `Distillation complete: ${result.tokensBefore} → ${result.tokensAfter} tokens (${Math.round((1 - result.tokensAfter / result.tokensBefore) * 100)}% reduction)`,
  );

  return result;
}
