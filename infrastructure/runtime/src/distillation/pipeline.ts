// Distillation pipeline — multi-pass context compression with hardening
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { SessionStore } from "../mneme/store.js";
import { extractFromMessages } from "./extract.js";
import { summarizeMessages } from "./summarize.js";
import { flushToMemory, type MemoryFlushTarget } from "./hooks.js";
import { sanitizeToolResults, summarizeInStages } from "./chunked-summarize.js";
import { pruneBySimilarity } from "./similarity-pruning.js";
import type { PluginRegistry } from "../prostheke/registry.js";
import { eventBus } from "../koina/event-bus.js";
import { flushToWorkspace } from "./workspace-flush.js";

const log = createLogger("distillation");

// Prevent concurrent distillation of the same session
const activeDistillations = new Set<string>();

export interface DistillationOpts {
  triggerThreshold: number;
  minMessages: number;
  extractionModel: string;
  summaryModel: string;
  memoryTarget?: MemoryFlushTarget;
  plugins?: PluginRegistry;
  preserveRecentMessages?: number;
  preserveRecentMaxTokens?: number;
  workspace?: string;
  /** Called after successful distillation to update the thread-level running summary. */
  onThreadSummaryUpdate?: (summary: string, keyFacts: string[]) => void;
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
  distillationNumber: number;
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
  if (activeDistillations.has(sessionId)) {
    log.info(
      `Distillation already in progress for session ${sessionId}, skipping`,
    );
    throw new Error(
      `Distillation already in progress for session ${sessionId}`,
    );
  }

  activeDistillations.add(sessionId);
  try {
    return await runDistillation(store, router, sessionId, nousId, opts);
  } finally {
    activeDistillations.delete(sessionId);
  }
}

async function runDistillation(
  store: SessionStore,
  router: ProviderRouter,
  sessionId: string,
  nousId: string,
  opts: DistillationOpts,
): Promise<DistillationResult> {
  const distillationNumber = store.incrementDistillationCount(sessionId);
  eventBus.emit("distill:before", { sessionId, nousId, distillationNumber });
  log.info(
    `Starting distillation #${distillationNumber} for session ${sessionId}`,
  );

  if (distillationNumber > 3) {
    log.warn(
      `Session ${sessionId} has been distilled ${distillationNumber} times — consider archiving`,
    );
  }

  const allMessages = store.getHistory(sessionId, {});

  // Guard: reject distillation if history has orphaned tool_use blocks
  const lastAssistant = [...allMessages].reverse().find(m => !m.isDistilled && m.role === "assistant");
  if (lastAssistant) {
    try {
      const parsed = JSON.parse(lastAssistant.content);
      if (Array.isArray(parsed)) {
        const toolUses = parsed.filter((b: { type: string }) => b.type === "tool_use");
        if (toolUses.length > 0) {
          const answeredIds = new Set(
            allMessages
              .filter(m => m.role === "tool_result" && m.seq > lastAssistant.seq)
              .map(m => m.toolCallId)
              .filter(Boolean),
          );
          const unanswered = toolUses.filter((b: { id: string }) => !answeredIds.has(b.id));
          if (unanswered.length > 0) {
            log.warn(`Distillation blocked: ${unanswered.length} unanswered tool_use blocks at seq ${lastAssistant.seq}`);
            throw new Error(`Cannot distill: incomplete message sequence (${unanswered.length} orphaned tool_use blocks)`);
          }
        }
      }
    } catch (e) {
      if (e instanceof Error && e.message.startsWith("Cannot distill")) throw e;
    }
  }

  const undistilled = allMessages.filter((m) => !m.isDistilled);

  if (undistilled.length < opts.minMessages) {
    throw new Error(
      `Not enough messages to distill: ${undistilled.length} < ${opts.minMessages}`,
    );
  }

  // Split into messages to distill vs recent messages to preserve as raw context
  const preserveCount = opts.preserveRecentMessages ?? 0;
  const preserveMaxTokens = opts.preserveRecentMaxTokens ?? 4000;
  let toPreserve: typeof undistilled = [];
  let toDistill = undistilled;

  if (preserveCount > 0 && undistilled.length > preserveCount + opts.minMessages) {
    let preserveTokens = 0;
    const candidates = undistilled.slice(-preserveCount);
    for (let i = candidates.length - 1; i >= 0; i--) {
      if (preserveTokens + (candidates[i]!.tokenEstimate ?? 0) > preserveMaxTokens) break;
      preserveTokens += candidates[i]!.tokenEstimate ?? 0;
      toPreserve.unshift(candidates[i]!);
    }
    if (undistilled.length - toPreserve.length >= opts.minMessages) {
      toDistill = undistilled.slice(0, undistilled.length - toPreserve.length);
    } else {
      toPreserve = [];
    }
  }

  if (toPreserve.length > 0) {
    log.info(`Preserving ${toPreserve.length} recent messages alongside summary`);
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

  // Build simple messages from toDistill (preserved messages stay as raw context)
  const rawMessages = toDistill
    .filter(
      (m) =>
        m.role === "user" || m.role === "assistant" || m.role === "tool_result",
    )
    .map((m) => {
      if (m.role === "tool_result") {
        const label = m.toolName ? `[tool:${m.toolName}]` : "[tool_result]";
        return { role: "user" as const, content: `${label} ${m.content}` };
      }
      return { role: m.role, content: m.content };
    });

  // Sanitize tool results — truncate verbose payloads before LLM-facing operations
  const sanitized = sanitizeToolResults(rawMessages);

  // Prune low-information segments using word-overlap similarity
  const { prunedMessages: simpleMessages, removedCount: pruneCount } = pruneBySimilarity(sanitized);
  if (pruneCount > 0) log.info(`Pruned ${pruneCount} low-information messages before distillation`);

  // Pass 1: Extraction
  log.info(`Extraction pass: ${simpleMessages.length} messages`);
  const extraction = await extractFromMessages(
    router,
    simpleMessages,
    opts.extractionModel,
  );

  log.info(
    `Extracted: ${extraction.facts.length} facts, ${extraction.decisions.length} decisions, ` +
      `${extraction.openItems.length} open items, ${extraction.contradictions.length} contradictions`,
  );

  // Memory flush with retry — non-blocking, don't fail distillation on flush failure
  if (opts.memoryTarget) {
    const flushResult = await flushToMemory(
      opts.memoryTarget,
      nousId,
      extraction,
    );
    if (flushResult.errors > 0) {
      log.warn(
        `Memory flush had ${flushResult.errors} errors — some facts may be lost`,
      );
    }
  }

  // Pass 2: Summarization — multi-stage for large conversations, single-pass for small
  log.info("Summary pass");
  let summary = await summarizeInStages(
    router,
    simpleMessages,
    extraction,
    opts.summaryModel,
    nousId,
  );

  let summaryTokens = estimateTokens(summary);

  // Compression ratio check — if summary > 50% of input, run a tighter second pass
  const compressionRatio = tokensBefore > 0 ? summaryTokens / tokensBefore : 0;
  if (compressionRatio > 0.5 && tokensBefore > 5000) {
    log.warn(
      `Compression ratio ${Math.round(compressionRatio * 100)}% exceeds 50% — running second pass`,
    );
    const emptyExtraction = {
      facts: [],
      decisions: [],
      openItems: [],
      keyEntities: [],
      contradictions: [],
    };
    summary = await summarizeMessages(
      router,
      [{ role: "assistant", content: summary }],
      emptyExtraction,
      opts.summaryModel,
      nousId,
    );
    summaryTokens = estimateTokens(summary);
  }

  // Tag repeated distillations so agents can see compression history
  const markedSummary =
    distillationNumber > 1
      ? `[Distillation #${distillationNumber}]\n\n${summary}`
      : summary;
  const markedTokens = estimateTokens(markedSummary);

  // The summary replaces old messages and must remain visible in future history.
  // isDistilled=false (default) keeps it in getHistoryWithBudget; markMessagesDistilled
  // only marks the OLD messages, not this one.
  store.appendMessage(sessionId, "assistant", markedSummary, {
    tokenEstimate: markedTokens,
  });

  store.markMessagesDistilled(
    sessionId,
    toDistill.map((m) => m.seq),
  );

  const preservedTokens = toPreserve.reduce((sum, m) => sum + (m.tokenEstimate ?? 0), 0);
  store.recordDistillation({
    sessionId,
    messagesBefore: undistilled.length,
    messagesAfter: 1 + toPreserve.length,
    tokensBefore,
    tokensAfter: markedTokens + preservedTokens,
    factsExtracted: extraction.facts.length + extraction.decisions.length,
    model: opts.extractionModel,
  });

  if (opts.workspace) {
    const flushResult = flushToWorkspace({
      workspace: opts.workspace,
      nousId,
      sessionId,
      distillationNumber,
      summary: markedSummary,
      extraction,
    });
    if (!flushResult.written) {
      log.warn(`Workspace memory flush failed: ${flushResult.error}`);
    }
  }

  const result: DistillationResult = {
    sessionId,
    nousId,
    messagesBefore: undistilled.length,
    messagesAfter: 1 + toPreserve.length,
    tokensBefore,
    tokensAfter: markedTokens + preservedTokens,
    factsExtracted: extraction.facts.length + extraction.decisions.length,
    summary: markedSummary,
    distillationNumber,
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

  // Update thread-level running summary if callback provided
  if (opts.onThreadSummaryUpdate) {
    const keyFacts = [
      ...extraction.facts.slice(0, 30),
      ...extraction.decisions.map((d) => `Decision: ${d}`).slice(0, 10),
    ];
    opts.onThreadSummaryUpdate(markedSummary, keyFacts);
  }

  eventBus.emit("distill:after", { sessionId, nousId, distillationNumber, tokensBefore: result.tokensBefore, tokensAfter: result.tokensAfter, factsExtracted: result.factsExtracted });

  log.info(
    `Distillation #${distillationNumber} complete: ${result.tokensBefore} → ${result.tokensAfter} tokens ` +
      `(${Math.round((1 - result.tokensAfter / result.tokensBefore) * 100)}% reduction)`,
  );

  return result;
}
